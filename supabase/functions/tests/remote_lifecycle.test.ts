// Unit tests for placement lifecycle under concurrency: recorded wakes
// (wake.ts), destroying retired and orphaned machines, and reverse cutover
// (operator-controls.ts). In-memory store and provider; no network.
//
// Run with: `deno test supabase/functions/tests/remote_lifecycle.test.ts`

import { assert, assertEquals, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { wakeIfStopped, wakeRecorded } from "../_shared/remote/wake.ts";
import {
  destroyRetired,
  type RetireStore,
  type ReverseCutoverDeps,
  runReverseCutover,
} from "../_shared/remote/operator-controls.ts";
import type { PlacementRow } from "../_shared/remote/migration-engine.ts";
import { reensureServicesAfterWake } from "../_shared/remote/machine-setup.ts";
import { deleteInstanceMachines } from "../_shared/remote/placement-store.ts";
import { DAYTONA_CAPABILITIES } from "../_shared/remote/daytona-adapter.ts";
import { ProviderError, type SandboxProvider } from "../_shared/remote/provider.ts";
import { migrationRow, placementRow as placement, providerInstance } from "./remote_fakes.ts";

class FakeProvider {
  calls: string[] = [];
  machines = new Map<string, "ready" | "suspended">();
  failWake = false;
  kind = "daytona" as const;
  capabilities() {
    return { ...DAYTONA_CAPABILITIES, serviceSupervisor: false };
  }
  getInstance(id: string) {
    const state = this.machines.get(id);
    if (!state) return Promise.reject(new ProviderError("not_found", id));
    return Promise.resolve(providerInstance(id, state));
  }
  wakeInstance(id: string) {
    this.calls.push(`wake:${id}`);
    if (this.failWake) return Promise.reject(new ProviderError("unavailable", "no"));
    this.machines.set(id, "ready");
    return Promise.resolve();
  }
  stopInstance(id: string) {
    this.calls.push(`stop:${id}`);
    this.machines.set(id, "suspended");
    return Promise.resolve();
  }
  deleteInstance(id: string) {
    this.calls.push(`delete:${id}`);
    if (!this.machines.delete(id)) return Promise.reject(new ProviderError("not_found", id));
    return Promise.resolve();
  }
  ensureService(id: string, name: string) {
    this.calls.push(`service:${id}:${name}`);
    return Promise.resolve();
  }
}

const asProvider = (p: FakeProvider) => p as unknown as SandboxProvider;

// ── Recorded wakes ──────────────────────────────────────────────────────

Deno.test("a recorded wake marks the placement waking, restarts services, then ready", async () => {
  const p = new FakeProvider();
  p.machines.set("m1", "suspended");
  const states: Array<[string, string]> = [];
  await wakeRecorded(asProvider(p), "m1", "pl1", {
    setState: (id, s) => (states.push([id, s]), Promise.resolve()),
    afterWake: reensureServicesAfterWake,
  });
  assertEquals(states, [["pl1", "waking"], ["pl1", "ready"]]);
  assertEquals(p.calls[0], "wake:m1");
  assert(p.calls.some((c) => c.startsWith("service:m1:")), p.calls.join(","));
});

Deno.test("wakeIfStopped leaves a running machine alone", async () => {
  const p = new FakeProvider();
  p.machines.set("m1", "ready");
  const states: string[] = [];
  const woke = await wakeIfStopped(asProvider(p), "m1", "pl1", {
    setState: (_id, s) => (states.push(s), Promise.resolve()),
    afterWake: () => Promise.resolve(),
  });
  assertEquals(woke, false);
  assertEquals(states, []);
  assertEquals(p.calls, []);
});

// ── Destroying retired and orphaned machines ────────────────────────────

class FakeRetireStore implements RetireStore {
  rows = new Map<string, PlacementRow>();
  orphans: string[] = [];
  events: string[] = [];
  // Runs between the listing and the claim: a reverse cutover winning the race.
  beforeClaim: ((id: string) => void) | null = null;
  listRetiring() {
    return Promise.resolve([...this.rows.values()].filter((p) => p.role === "retiring").map((p) => ({ ...p })));
  }
  claimRetiring(id: string) {
    this.beforeClaim?.(id);
    const p = this.rows.get(id)!;
    if (p.role !== "retiring") return Promise.resolve(false);
    p.role = "dead";
    return Promise.resolve(true);
  }
  listOrphans() {
    return Promise.resolve(this.orphans.map((id) => ({ ...this.rows.get(id)! })));
  }
  setPlacementState(id: string, state: string) {
    this.rows.get(id)!.state = state;
    return Promise.resolve();
  }
  markDead(id: string) {
    this.rows.get(id)!.role = "dead";
    return Promise.resolve();
  }
  audit(eventType: string) {
    this.events.push(eventType);
    return Promise.resolve();
  }
}

Deno.test("a retiring placement is claimed before its machine is deleted", async () => {
  const store = new FakeRetireStore();
  const p = new FakeProvider();
  store.rows.set("a", placement("a"));
  p.machines.set("m-a", "suspended");
  const r = await destroyRetired({ store, providers: () => asProvider(p) }, { cutoff: new Date(), limit: 10 });
  assertEquals(r.destroyed, ["a"]);
  assertEquals(store.rows.get("a")!.role, "dead");
  assertEquals(store.rows.get("a")!.state, "deleted");
  assertEquals(p.calls, ["delete:m-a"]);
});

Deno.test("a placement a reverse cutover took back is not destroyed", async () => {
  const store = new FakeRetireStore();
  const p = new FakeProvider();
  store.rows.set("a", placement("a"));
  p.machines.set("m-a", "ready");
  store.beforeClaim = (id) => void (store.rows.get(id)!.role = "active");
  const r = await destroyRetired({ store, providers: () => asProvider(p) }, { cutoff: new Date(), limit: 10 });
  assertEquals(r.destroyed, []);
  assertEquals(p.calls, []);
  assertEquals(store.rows.get("a")!.role, "active");
});

Deno.test("dead and abandoned placements that still hold a machine are swept", async () => {
  const store = new FakeRetireStore();
  const p = new FakeProvider();
  store.rows.set("dead", placement("dead", { role: "dead", state: "bootstrapping" }));
  store.rows.set("cand", placement("cand", { role: "candidate", state: "provisioning" }));
  store.rows.set("gone", placement("gone", { role: "dead", state: "ready" }));
  store.orphans = ["dead", "cand", "gone"];
  p.machines.set("m-dead", "ready");
  p.machines.set("m-cand", "ready");
  // m-gone is already deleted at the provider: not_found is success.
  const r = await destroyRetired({ store, providers: () => asProvider(p) }, { cutoff: new Date(), limit: 10 });
  assertEquals(r.orphans_destroyed, ["dead", "cand", "gone"]);
  assertEquals(p.machines.size, 0);
  for (const id of ["dead", "cand", "gone"]) {
    assertEquals([store.rows.get(id)!.role, store.rows.get(id)!.state], ["dead", "deleted"], id);
  }
});

// ── Reverse cutover ─────────────────────────────────────────────────────

const migration = migrationRow();

function reverseWorld(opts: { open?: boolean; reverseFails?: boolean } = {}) {
  const p = new FakeProvider();
  p.machines.set("m-src", "suspended");
  p.machines.set("m-tgt", "ready");
  const states: Array<[string, string]> = [];
  const wakes: string[] = [];
  let reversed = 0;
  const deps: ReverseCutoverDeps = {
    openMigration: () => Promise.resolve(opts.open ? { id: "other" } : null),
    reverse: () => {
      reversed++;
      return opts.reverseFails ? Promise.reject(new Error("source_not_retiring")) : Promise.resolve(3);
    },
    setPlacementState: (id, s) => (states.push([id, s]), Promise.resolve()),
    providers: () => asProvider(p),
    afterWake: (_p, id) => (wakes.push(id), Promise.resolve()),
    afterCutover: () => Promise.resolve(),
  };
  return { p, deps, states, wakes, reversed: () => reversed };
}

const src = placement("src", { provider_resource_id: "m-src", role: "retiring" });
const tgt = placement("tgt", { provider_resource_id: "m-tgt", role: "active", state: "ready" });

Deno.test("reverse cutover wakes the source, restarts its services, swaps atomically and stops the target", async () => {
  const w = reverseWorld();
  const fence = await runReverseCutover(w.deps, migration, src, tgt);
  assertEquals(fence, 3);
  assertEquals(w.reversed(), 1);
  assertEquals(w.wakes, ["m-src"]);
  assertEquals(w.p.machines.get("m-src"), "ready");
  assertEquals(w.p.machines.get("m-tgt"), "suspended");
  assertEquals(w.states, [["src", "waking"], ["tgt", "suspended"]]);
});

Deno.test("reverse cutover is refused while another migration is open", async () => {
  const w = reverseWorld({ open: true });
  await assertRejects(() => runReverseCutover(w.deps, migration, src, tgt), Error, "another migration is open");
  assertEquals(w.reversed(), 0);
  assertEquals(w.p.calls, []);
});

Deno.test("a failed reverse cutover stops the source again and leaves it retiring", async () => {
  const w = reverseWorld({ reverseFails: true });
  await assertRejects(() => runReverseCutover(w.deps, migration, src, tgt), Error, "source_not_retiring");
  assertEquals(w.p.machines.get("m-src"), "suspended");
  assertEquals(w.p.machines.get("m-tgt"), "ready");
  assertEquals(w.states, [["src", "waking"], ["src", "suspended"]]);
});

Deno.test("operator controls accept every provider kind, including e2b and fly_machines", async () => {
  const { requireKind } = await import("../_shared/remote/operator-controls.ts");
  for (const kind of ["boat", "daytona", "e2b", "fly_machines", "local"]) {
    assertEquals(requireKind(kind), kind);
  }
  let threw = false;
  try {
    requireKind("fly_sprites");
  } catch {
    threw = true;
  }
  assertEquals(threw, true);
});

// ── Deleting an instance's machines ─────────────────────────────────────

Deno.test("deleteInstanceMachines deletes each machine once and tolerates ones already gone", async () => {
  const p = new FakeProvider();
  p.machines.set("m-a", "ready");
  p.machines.set("m-b", "suspended");
  // m-a is both the instance's machine and its active placement's; m-gone was deleted earlier.
  await deleteInstanceMachines(
    () => asProvider(p),
    { provider_kind: "daytona", provider_resource_id: "m-a", active_placement_id: "a" },
    [placement("a", { provider_resource_id: "m-a" }), placement("b"), placement("gone")],
  );
  assertEquals(p.calls, ["delete:m-a", "delete:m-b", "delete:m-gone"]);
  assertEquals(p.machines.size, 0);
});

Deno.test("deleteInstanceMachines stops at a provider error other than not_found", async () => {
  const failing = { deleteInstance: () => Promise.reject(new ProviderError("unavailable", "down")) };
  await assertRejects(
    () => deleteInstanceMachines(() => failing, { provider_kind: "daytona", provider_resource_id: "m-a" }, []),
    ProviderError,
    "down",
  );
});
