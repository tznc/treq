// Unit tests for the provider-switch migration engine
// (_shared/remote/migration-engine.ts) against an in-memory store and
// provider. The fake store enforces the same invariants as the database:
// advance is a compare-and-swap, cutover checks the fence, and an instance
// never has two active placements.
//
// Run with: `deno test supabase/functions/tests/remote_migration_engine.test.ts`

import { defined } from "./remote_fakes.ts";
import { assert, assertEquals, assertFalse } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  type EngineDeps,
  type MachineSetup,
  type MigrationRow,
  type MigrationStore,
  type PlacementRow,
  runMigration,
  stepMigration,
  TERMINAL_STATES,
} from "../_shared/remote/migration-engine.ts";
import { resumeParkedMigration } from "../_shared/remote/migration-resume.ts";
import type { PreflightReport } from "../_shared/remote/preflight.ts";
import {
  type CreateInstanceParams,
  type MachineExecResult,
  type ProviderCapabilities,
  ProviderError,
  type ProviderInstance,
  type ProviderKind,
  type SandboxProvider,
} from "../_shared/remote/provider.ts";
import { DAYTONA_CAPABILITIES } from "../_shared/remote/daytona-adapter.ts";
import { migrationRow, placementRow, providerInstance } from "./remote_fakes.ts";

class Crash extends Error {}

interface Machine {
  id: string;
  state: "ready" | "suspended";
  key: string;
}

// One world: database rows plus every provider's machines.
class World {
  writes = 0;
  crashAt = Number.POSITIVE_INFINITY;
  crashBefore = false;
  failCreate: ProviderError | null = null;
  // A buggy provider whose create hands back the source machine.
  createReturnsSource = false;
  failBootstrap = false;
  failExecTransient = 0;
  failProbe = false;
  failStopTransient = false;
  spendCapped = false;
  // Runs inside the provider's createInstance, before it returns: a
  // concurrent delete or sweep acting while the worker waits on the vendor.
  onCreate: (() => void) | null = null;
  // Lease renewals the engine asked for: [state, seconds].
  renewals: Array<[string, number]> = [];
  leaseLost = false;
  afterWakes: string[] = [];
  placementStates: Array<[string, string]> = [];
  // Commits the source's preflight reports as not pushed, per repo.
  unpushed = 0;
  machines = new Map<string, Machine>();
  migration: MigrationRow;
  placements = new Map<string, PlacementRow>();
  instance = { id: "inst", owner_user_id: "u1", region: "us_east", fence: 0, active_placement_id: "p-src" };
  repos = [
    { id: "r1", remote_path: "/home/treq/a", remote_url: "https://example.invalid/a.git" },
    { id: "r2", remote_path: "/home/treq/b", remote_url: null as string | null },
  ];
  steps: Array<{ from: string; to: string; detail: unknown }> = [];
  execs: string[][] = [];
  private nextId = 0;

  constructor(opts: { preflight?: PreflightReport | null; ack?: boolean } = {}) {
    this.machines.set("m-src", { id: "m-src", state: "ready", key: "src" });
    this.placements.set("p-src", placementRow("p-src", { provider_resource_id: "m-src", state: "ready", role: "active" }));
    this.migration = migrationRow({
      source_placement_id: "p-src",
      target_placement_id: null,
      target_machine_type_id: "boat:small",
      state: "requested",
      preflight: opts.preflight === undefined ? { repos: [], requires_ack: false } : opts.preflight,
      acknowledged_data_loss: opts.ack ?? false,
      expected_fence: 0,
    });
  }

  // Every durable write goes through here, so a test can crash the worker
  // just before or just after any of them.
  write<T>(apply: () => T): T {
    this.writes++;
    if (this.writes === this.crashAt && this.crashBefore) throw new Crash("crash before write");
    const out = apply();
    this.checkInvariants();
    if (this.writes === this.crashAt && !this.crashBefore) throw new Crash("crash after write");
    return out;
  }

  checkInvariants() {
    const active = [...this.placements.values()].filter((p) => p.role === "active");
    if (active.length > 1) throw new Error("invariant: two active placements");
    if (active[0] && active[0].id !== this.instance.active_placement_id) {
      throw new Error("invariant: active placement not mirrored");
    }
  }

  id(prefix: string) {
    return `${prefix}-${++this.nextId}`;
  }

  liveMachines() {
    return [...this.machines.keys()];
  }

  activePlacement() {
    return defined([...this.placements.values()].find((p) => p.role === "active"));
  }
}

function storeFor(w: World): MigrationStore {
  const clone = <T>(v: T): T => structuredClone(v);
  return {
    getMigration: (id) => Promise.resolve(id === w.migration.id ? clone(w.migration) : null),
    advance: (id, expected, next, detail) =>
      Promise.resolve(w.write(() => {
        if (id !== w.migration.id || w.migration.state !== expected) return false;
        w.migration.state = next;
        w.steps.push({ from: expected, to: next, detail: detail ?? null });
        return true;
      })),
    setMigrationFields: (_id, fields) =>
      Promise.resolve(w.write(() => {
        Object.assign(w.migration, fields);
      })),
    getInstance: () => Promise.resolve(clone(w.instance)),
    getPlacement: (id) => Promise.resolve(w.placements.has(id) ? clone(defined(w.placements.get(id))) : null),
    findCandidatePlacement: (instanceId) =>
      Promise.resolve(
        clone([...w.placements.values()].find((p) => p.instance_id === instanceId && p.role === "candidate") ??
          null),
      ),
    createCandidatePlacement: (p) =>
      Promise.resolve(w.write(() => {
        const row: PlacementRow = {
          id: w.id("p"),
          instance_id: p.instanceId,
          owner_user_id: p.ownerUserId,
          provider_kind: p.providerKind,
          machine_type_id: p.machineTypeId,
          region: p.region,
          provider_resource_id: null,
          state: "provisioning",
          role: "candidate",
          credential_source: p.credentialSource ?? "managed",
          credential_id: p.credentialId ?? null,
        };
        w.placements.set(row.id, row);
        return clone(row);
      })),
    setPlacementResource: (id, rid) =>
      Promise.resolve(w.write(() => {
        defined(w.placements.get(id)).provider_resource_id = rid;
      })),
    setPlacementState: (id, state) =>
      Promise.resolve(w.write(() => {
        defined(w.placements.get(id)).state = state;
        w.placementStates.push([id, state]);
      })),
    setPlacementRole: (id, role) =>
      Promise.resolve(w.write(() => {
        const p = defined(w.placements.get(id));
        if (p.role === "active") throw new Error("cannot change the active placement's role");
        p.role = role;
      })),
    cutover: (m, targetId) =>
      Promise.resolve(w.write(() => {
        const instanceId = m.instance_id;
        if (w.instance.fence !== (m.expected_fence ?? w.instance.fence)) throw new Error("fence_mismatch");
        const target = w.placements.get(targetId);
        if (!target || target.role !== "candidate") throw new Error("target_not_candidate");
        if (w.instance.active_placement_id !== m.source_placement_id) throw new Error("source_not_active");
        for (const p of w.placements.values()) {
          if (p.instance_id === instanceId && p.role === "active") p.role = "retiring";
        }
        target.role = "active";
        w.instance.active_placement_id = target.id;
        w.instance.fence++;
        return w.instance.fence;
      })),
    getMachineType: (id) =>
      Promise.resolve(
        id === "boat:small"
          ? { id, provider_kind: "boat" as ProviderKind, vendor_type: "small", tier_id: "s" }
          : { id, provider_kind: "daytona" as ProviderKind, vendor_type: "s", tier_id: "s" },
      ),
    listRepositories: () => Promise.resolve(clone(w.repos)),
  };
}

function providerFor(w: World, kind: ProviderKind): SandboxProvider {
  const instance = (m: Machine): ProviderInstance => providerInstance(m.id, m.state);
  const get = (id: string) => {
    const m = w.machines.get(id);
    if (!m) throw new ProviderError("not_found", "no machine");
    return m;
  };
  // Provider calls are side effects the worker can crash around too.
  const effect = <T>(apply: () => T) => Promise.resolve().then(() => w.write(apply));
  return {
    kind,
    capabilities: (): ProviderCapabilities => ({ ...DAYTONA_CAPABILITIES }),
    createInstance: (p: CreateInstanceParams) =>
      effect(() => {
        if (w.failCreate) throw w.failCreate;
        w.onCreate?.();
        if (w.createReturnsSource) return instance(defined(w.machines.get("m-src")));
        const existing = [...w.machines.values()].find((m) => m.key === p.idempotencyKey);
        if (existing) return instance(existing);
        const m: Machine = { id: w.id("m"), state: "ready", key: p.idempotencyKey };
        w.machines.set(m.id, m);
        return instance(m);
      }),
    getInstance: (id) => Promise.resolve(instance(get(id))),
    wakeInstance: (id) =>
      effect(() => {
        get(id).state = "ready";
      }),
    stopInstance: (id) =>
      effect(() => {
        if (w.failStopTransient) throw new ProviderError("unavailable", "flaky stop");
        get(id).state = "suspended";
      }),
    replaceInstance: () => Promise.reject(new Error("unused")),
    deleteInstance: (id) => effect(() => void w.machines.delete(id)),
    execOnMachine: (id, cmd): Promise<MachineExecResult> => {
      get(id);
      w.execs.push(cmd);
      if (w.failExecTransient > 0) {
        w.failExecTransient--;
        return Promise.reject(new ProviderError("unavailable", "flaky"));
      }
      if (cmd[3] === "preflight") {
        const out = cmd.slice(4).map((p) => `REPO\t${p}\nVCS\tjj\nDIRTY\t0\nUNPUSHED\t${w.unpushed}\nAGENTS\t0\n`).join("");
        return Promise.resolve({ exitCode: 0, stdout: out, stderr: "" });
      }
      if (w.failProbe && cmd.join(" ").includes("2222")) {
        return Promise.resolve({ exitCode: 1, stdout: "", stderr: "connection refused" });
      }
      return Promise.resolve({ exitCode: 0, stdout: "", stderr: "" });
    },
    ensureService: () => Promise.resolve(),
    gatewayEndpoint: () => Promise.reject(new Error("unused")),
  };
}

function depsFor(w: World): EngineDeps {
  const setup: MachineSetup = {
    bootstrap: (_provider, _id) => {
      if (w.failBootstrap) return Promise.reject(new ProviderError("other", "bootstrap exited 1"));
      return Promise.resolve();
    },
    afterCutover: () => Promise.resolve(),
    afterWake: (_provider, id) => {
      w.afterWakes.push(id);
      return Promise.resolve();
    },
  };
  return {
    store: storeFor(w),
    providers: (kind) => providerFor(w, kind),
    setup,
    now: () => 0,
    log: () => {},
    renewLease: (_id, seconds) => {
      w.renewals.push([w.migration.state, seconds]);
      return Promise.resolve(!w.leaseLost);
    },
    spendCapped: () => Promise.resolve(w.spendCapped),
  };
}

async function runToEnd(w: World): Promise<string> {
  const deps = depsFor(w);
  for (let i = 0; i < 20; i++) {
    const r = await runMigration(deps, w.migration.id, { deadlineMs: 1, maxSteps: 50 });
    if (TERMINAL_STATES.has(r.state) || r.state === "awaiting_ack") return r.state;
  }
  return w.migration.state;
}

Deno.test("a clean migration runs every state in order and ends on the target", async () => {
  const w = new World();
  assertEquals(await runToEnd(w), "completed");
  assertEquals(w.steps.map((s) => s.to), [
    "preflight",
    "target_provisioning",
    "target_bootstrapped",
    "verifying",
    "cutover",
    "source_retiring",
    "completed",
  ]);
  const active = w.activePlacement();
  assertEquals(active.provider_kind, "boat");
  assertEquals(active.machine_type_id, "boat:small");
  assertEquals(defined(w.placements.get("p-src")).role, "retiring");
  // The source is stopped, not destroyed, so a reverse cutover is possible.
  assertEquals(defined(w.machines.get("m-src")).state, "suspended");
  assertEquals(w.instance.fence, 1);
});

Deno.test("repos are re-cloned and repos without a remote url need attention", async () => {
  const w = new World();
  await runToEnd(w);
  const reclones = w.execs.filter((c) => c[3] === "reclone");
  assertEquals(reclones.map((c) => c[5]), ["/home/treq/a"]);
  const bootstrapped = defined(w.steps.find((s) => s.to === "verifying")).detail as {
    repos: Array<{ path: string; status: string }>;
  };
  assertEquals(bootstrapped.repos, [
    { path: "/home/treq/a", status: "cloned" },
    { path: "/home/treq/b", status: "needs_attention", reason: "no_remote_url" },
  ] as unknown);
});

Deno.test("unpushed work without acknowledgement parks the migration", async () => {
  const w = new World({ preflight: { repos: [], requires_ack: true } });
  assertEquals(await runToEnd(w), "awaiting_ack");
  assertEquals(w.liveMachines(), ["m-src"]);
});

Deno.test("acknowledged unpushed work proceeds", async () => {
  const w = new World({ preflight: { repos: [], requires_ack: true }, ack: true });
  assertEquals(await runToEnd(w), "completed");
});

Deno.test("a migration without a stored preflight runs one on the source", async () => {
  const w = new World({ preflight: null });
  assertEquals(await runToEnd(w), "completed");
  assert(w.execs.some((c) => c[3] === "preflight"));
  assertEquals(w.migration.preflight?.repos.length, 2);
});

Deno.test("a target that cannot be created rolls back and leaves the source active", async () => {
  const w = new World();
  w.failCreate = new ProviderError("quota_exceeded", "no capacity");
  assertEquals(await runToEnd(w), "rolled_back");
  assertEquals(w.activePlacement().id, "p-src");
  assertEquals(w.liveMachines(), ["m-src"]);
  assertEquals(defined(w.machines.get("m-src")).state, "ready");
});

Deno.test("a target that is the source machine rolls back without deleting it", async () => {
  const w = new World();
  w.createReturnsSource = true;
  assertEquals(await runToEnd(w), "rolled_back");
  assertEquals(w.activePlacement().id, "p-src");
  assertEquals(w.liveMachines(), ["m-src"]);
  assertEquals(defined(w.machines.get("m-src")).state, "ready");
});

Deno.test("a failed bootstrap destroys the target and rolls back", async () => {
  const w = new World();
  w.failBootstrap = true;
  assertEquals(await runToEnd(w), "rolled_back");
  assertEquals(w.liveMachines(), ["m-src"]);
  const dead = [...w.placements.values()].filter((p) => p.role === "dead");
  assertEquals(dead.length, 1);
  assertEquals(dead[0].state, "deleted");
});

Deno.test("transient provider errors are retried, not rolled back", async () => {
  const w = new World();
  w.failExecTransient = 1;
  assertEquals(await runToEnd(w), "completed");
});

Deno.test("a lost compare-and-swap does no work", async () => {
  const w = new World();
  const deps = depsFor(w);
  const store = deps.store;
  deps.store = { ...store, advance: () => Promise.resolve(false) };
  const r = await stepMigration(deps, "mig");
  assertFalse(r.advanced);
  assertEquals(w.steps, []);
});

Deno.test("a crash at every write boundary converges and never has two active placements", async () => {
  // Count the writes of a clean run, then crash at each one, before and
  // after it lands, and also with a failing bootstrap (the rollback path).
  for (const failBootstrap of [false, true]) {
    const probe = new World();
    probe.failBootstrap = failBootstrap;
    await runToEnd(probe);
    const total = probe.writes;
    assert(total > 5);
    for (let at = 1; at <= total; at++) {
      for (const before of [true, false]) {
        const w = new World();
        w.failBootstrap = failBootstrap;
        w.crashAt = at;
        w.crashBefore = before;
        let crashed = false;
        try {
          await runToEnd(w);
        } catch (err) {
          if (!(err instanceof Crash)) throw err;
          crashed = true;
        }
        assert(crashed, `expected a crash at write ${at} (bootstrap fails: ${failBootstrap}, before: ${before}, probe ${total})`);
        w.crashAt = Number.POSITIVE_INFINITY;
        const end = await runToEnd(w);
        const label = `bootstrap=${!failBootstrap} crash ${before ? "before" : "after"} write ${at}`;
        assertEquals(end, failBootstrap ? "rolled_back" : "completed", label);
        w.checkInvariants();
        if (failBootstrap) {
          assertEquals(w.activePlacement().id, "p-src", label);
          assertEquals(w.liveMachines(), ["m-src"], label);
        } else {
          assertEquals(w.activePlacement().provider_kind, "boat", label);
          assertEquals(w.liveMachines().length, 2, label);
          assertEquals(defined(w.machines.get("m-src")).state, "suspended", label);
        }
      }
    }
  }
});

// Resuming a parked migration (acknowledge_migration, force_migration).

async function parked(unpushed: number): Promise<World> {
  const w = new World({ preflight: null });
  w.unpushed = unpushed;
  assertEquals(await runToEnd(w), "awaiting_ack");
  return w;
}

Deno.test("an acknowledged parked migration re-runs preflight, records the ack and completes", async () => {
  const w = await parked(2);
  const preflights = w.execs.filter((c) => c[3] === "preflight").length;
  const r = await resumeParkedMigration(depsFor(w), "mig", { acknowledge: true });
  assertEquals(r.resumed, true);
  assertEquals(r.acknowledged, true);
  assertEquals(r.preflight?.requires_ack, true);
  assertEquals(w.execs.filter((c) => c[3] === "preflight").length, preflights + 1);
  assert(w.migration.acknowledged_data_loss);
  assertEquals(await runToEnd(w), "completed");
});

Deno.test("a parked migration whose work was pushed since resumes without an ack", async () => {
  const w = await parked(2);
  w.unpushed = 0;
  const r = await resumeParkedMigration(depsFor(w), "mig", { acknowledge: false });
  assertEquals(r.resumed, true);
  assertEquals(r.acknowledged, false);
  assertFalse(w.migration.acknowledged_data_loss);
  assertEquals(w.migration.preflight?.requires_ack, false);
  assertEquals(await runToEnd(w), "completed");
});

Deno.test("a parked migration with work still unpushed stays parked without an ack", async () => {
  const w = await parked(1);
  const r = await resumeParkedMigration(depsFor(w), "mig", { acknowledge: false });
  assertEquals(r.resumed, false);
  assertEquals(r.state, "awaiting_ack");
  assertEquals(w.migration.state, "awaiting_ack");
  assertFalse(w.migration.acknowledged_data_loss);
});

Deno.test("resuming a migration that is not parked does nothing", async () => {
  const w = new World();
  assertEquals(await runToEnd(w), "completed");
  const writes = w.writes;
  const r = await resumeParkedMigration(depsFor(w), "mig", { acknowledge: true });
  assertEquals(r, { resumed: false, state: "completed", acknowledged: false, preflight: null });
  assertEquals(w.writes, writes);
});

// ── BYOS (bring your own sandbox) ───────────────────────────────────────

function byosWorld(target: string) {
  const w = new World();
  const src = defined(w.placements.get("p-src"));
  src.credential_source = "byos";
  src.credential_id = "cred-1";
  w.migration.target_machine_type_id = target;
  return w;
}

Deno.test("a BYOS resize keeps the user's credential on the new machine", async () => {
  const w = byosWorld("daytona:m");
  const seen: Array<[ProviderKind, string | null | undefined]> = [];
  const deps = depsFor(w);
  const r = await runMigration(
    { ...deps, providers: (kind, placementId) => {
        seen.push([kind, placementId]);
        return providerFor(w, kind);
      },
    },
    "mig",
    { deadlineMs: 1, maxSteps: 20 },
  );
  assertEquals(r.state, "completed");
  const active = w.activePlacement();
  assertEquals([active.credential_source, active.credential_id], ["byos", "cred-1"]);
  // Every provider call names its placement, so the key can be loaded.
  assert(seen.every(([, id]) => typeof id === "string"));
});

Deno.test("a BYOS workspace never moves to another provider", async () => {
  const w = byosWorld("boat:small");
  const r = await runMigration(depsFor(w), "mig", { deadlineMs: 1, maxSteps: 20 });
  assertEquals(r.state, "rolled_back");
  assertEquals(w.activePlacement().id, "p-src");
});

Deno.test("an invalid BYOS key rolls back at once instead of retrying", async () => {
  const w = byosWorld("daytona:m");
  w.failCreate = new ProviderError("unauthorized", "revoked");
  const r = await runMigration(depsFor(w), "mig", { deadlineMs: 1, maxSteps: 20 });
  assertEquals(r.state, "rolled_back");
  assertEquals(w.migration.attempt, 1);
});
