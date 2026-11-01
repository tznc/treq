// Provider-switch migration engine (docs/design/sandbox-provider-adapter.md
// §6, §7, §10). A migration moves an instance to a fresh machine, possibly
// on another provider, while the instance keeps its id and endpoint:
//
//   requested → preflight → target_provisioning → target_bootstrapped
//     → verifying → cutover → source_retiring → completed
//   preflight → awaiting_ack            (unpushed work, no acknowledgement)
//   target_* / verifying → rolling_back → rolled_back
//
// The state names the step to run next. Each step does its work, then moves
// on through `advance` (the `advance_migration` compare-and-swap), so a
// worker that lost a race does nothing and a crashed worker's step is simply
// run again. Every step is therefore idempotent: the target placement is
// found before it is created, provider creates reuse one idempotency key,
// and cutover is skipped once the target is already active. Nothing rolls
// back after cutover; the source is only stopped, and an operator can
// reverse the cutover within the retention window.
//
// The engine holds no Supabase or vendor code. The store and providers are
// injected, so tests run it against fakes (tests/remote_migration_engine.test.ts).

import { type PreflightReport, parsePreflightOutput, preflightCommand, recloneCommand } from "./preflight.ts";
import { ProviderError, type ProviderKind, type SandboxProvider } from "./provider.ts";
import { CURRENT_MANIFEST_VERSION } from "./boot-manifest.ts";
import { GATEWAY_LISTEN, MANAGED_SSHD_PORT } from "./ssh-vm-config.ts";
import { wakeIfStopped } from "./wake.ts";
import { ignoreNotFound } from "./placement-store.ts";

export type MigrationState =
  | "requested"
  | "preflight"
  | "awaiting_ack"
  | "target_provisioning"
  | "target_bootstrapped"
  | "verifying"
  | "cutover"
  | "source_retiring"
  | "completed"
  | "rolling_back"
  | "rolled_back"
  | "failed"
  | "cancelled";

export const TERMINAL_STATES: ReadonlySet<string> = new Set(["completed", "rolled_back", "failed", "cancelled"]);

// States the engine moves forward on its own. `awaiting_ack` waits for the user.
export const RUNNABLE_STATES: readonly MigrationState[] = [
  "requested",
  "preflight",
  "target_provisioning",
  "target_bootstrapped",
  "verifying",
  "cutover",
  "source_retiring",
  "rolling_back",
];

// States in which a failure can no longer roll back (the target may be active).
const POST_CUTOVER_STATES: ReadonlySet<string> = new Set(["cutover", "source_retiring"]);

const ROLLBACK_ALLOWED: ReadonlySet<string> = new Set([
  "preflight",
  "cutover",
  "target_provisioning",
  "target_bootstrapped",
  "verifying",
]);

// Transient provider errors are retried this many times before rolling back.
export const MAX_ATTEMPTS = 3;
// After cutover nothing rolls back, so a failing step is retried; past this
// many attempts the migration is marked failed for an operator.
export const MAX_POST_CUTOVER_ATTEMPTS = 10;

// Seconds a step may take at worst, so the lease is extended to cover it
// before the step starts (a second worker must not take an expired lease
// while the step still runs). Bootstrap execs run up to 300 s each; a
// re-clone runs up to 600 s per repo.
const STEP_LEASE_SEC: Record<string, number> = {
  preflight: 300,
  target_provisioning: 1200,
  verifying: 300,
};
const DEFAULT_STEP_LEASE_SEC = 120;
const RECLONE_TIMEOUT_SEC = 600;

export interface MigrationRow {
  id: string;
  instance_id: string;
  owner_user_id: string;
  source_placement_id: string;
  target_placement_id: string | null;
  target_machine_type_id: string;
  reason: string;
  state: string;
  preflight: PreflightReport | null;
  acknowledged_data_loss: boolean;
  attempt: number;
  deadline?: string | null;
  /** The instance's fence when the migration was created; cutover requires it unchanged. */
  expected_fence?: number | null;
}

export interface PlacementRow {
  id: string;
  instance_id: string;
  owner_user_id: string;
  provider_kind: ProviderKind;
  machine_type_id: string;
  region: string;
  provider_resource_id: string | null;
  state: string;
  role: "active" | "candidate" | "retiring" | "dead";
  /** 'byos': the machine runs on the owner's own provider account (byos.ts). */
  credential_source?: "managed" | "byos";
  credential_id?: string | null;
}

export interface MachineTypeRow {
  id: string;
  provider_kind: ProviderKind;
  vendor_type: string;
  tier_id: string;
}

export interface RepositoryRow {
  id: string;
  remote_path: string;
  remote_url: string | null;
}

export interface MigrationInstance {
  id: string;
  owner_user_id: string;
  region: string;
  fence: number;
  active_placement_id: string | null;
}

export interface MigrationStore {
  getMigration(id: string): Promise<MigrationRow | null>;
  /** `advance_migration`: false when the row was not in `expected`. */
  advance(id: string, expected: string, next: string, detail?: Record<string, unknown>): Promise<boolean>;
  setMigrationFields(
    id: string,
    fields: Partial<Pick<MigrationRow, "target_placement_id" | "preflight" | "attempt" | "acknowledged_data_loss">> & {
      error?: Record<string, unknown> | null;
    },
  ): Promise<void>;
  getInstance(id: string): Promise<MigrationInstance | null>;
  getPlacement(id: string): Promise<PlacementRow | null>;
  /** The instance's candidate placement. At most one migration is open per instance, so it is this one's. */
  findCandidatePlacement(instanceId: string): Promise<PlacementRow | null>;
  createCandidatePlacement(p: {
    instanceId: string;
    ownerUserId: string;
    providerKind: ProviderKind;
    machineTypeId: string;
    region: string;
    credentialSource?: "managed" | "byos";
    credentialId?: string | null;
  }): Promise<PlacementRow>;
  setPlacementResource(placementId: string, providerResourceId: string): Promise<void>;
  /** `set_placement_state`, which also keeps billing in step. */
  setPlacementState(placementId: string, state: string): Promise<void>;
  setPlacementRole(placementId: string, role: "dead" | "candidate"): Promise<void>;
  /**
   * `cutover_migration`: cuts over to the target only while the instance's
   * fence is the one captured at creation and the source is still active.
   * Returns the new fence.
   */
  cutover(migration: MigrationRow, targetPlacementId: string): Promise<number>;
  getMachineType(id: string): Promise<MachineTypeRow | null>;
  listRepositories(instanceId: string): Promise<RepositoryRow[]>;
}

/** Machine setup shared with `ensure` (see machine-setup.ts). */
export interface MachineSetup {
  /** sshd, our host key and CA trust, services. Throws on failure. */
  bootstrap(provider: SandboxProvider, providerResourceId: string, placementId?: string): Promise<void>;
  /** Runs once the target is active: records its host key on the endpoint. Idempotent. */
  afterCutover(migration: MigrationRow, target: PlacementRow): Promise<void>;
  /** Restarts sshd and the gateway after a wake (`reensureServicesAfterWake`). */
  afterWake(provider: SandboxProvider, providerResourceId: string, placementId: string | null): Promise<void>;
}

export interface EngineDeps {
  store: MigrationStore;
  providers: (kind: ProviderKind, placementId?: string | null) => SandboxProvider;
  setup: MachineSetup;
  now: () => number;
  log?: (fields: Record<string, unknown>) => void;
  /**
   * Extends this worker's lease to `seconds` from now. False when the lease
   * is held by another worker, and the step is not run.
   */
  renewLease?: (migrationId: string, seconds: number) => Promise<boolean>;
  /** True when the owner is at their spend cap: cutover is refused. */
  spendCapped?: (ownerUserId: string) => Promise<boolean>;
}

export interface StepResult {
  state: string;
  advanced: boolean;
}

/**
 * Config sync hook (§6 "pull configs"). Config sync is a separate slice;
 * until it lands there is nothing to restore, so this returns at once.
 */
export function pullUserConfigOntoTarget(
  _provider: SandboxProvider,
  _providerResourceId: string,
  _migration: MigrationRow,
): Promise<{ restored: boolean }> {
  return Promise.resolve({ restored: false });
}

class StepFailure extends Error {}

function isTransient(err: unknown): boolean {
  return err instanceof ProviderError && (err.kind === "unavailable" || err.kind === "timeout");
}

function targetIdempotencyKey(migrationId: string): string {
  return `migration:${migrationId}:target`;
}

/**
 * Asks the source machine what a fresh start would lose (§6). A stopped
 * source is woken, recorded through set_placement_state; `woke` says so,
 * so a caller that parks the migration can stop it again.
 */
export async function runPreflightOnSource(
  deps: Pick<EngineDeps, "store" | "providers" | "setup">,
  m: MigrationRow,
): Promise<{ report: PreflightReport; woke: boolean }> {
  const source = await deps.store.getPlacement(m.source_placement_id);
  const repos = await deps.store.listRepositories(m.instance_id);
  const paths = repos.map((r) => r.remote_path);
  if (!source?.provider_resource_id || paths.length === 0) return { report: parsePreflightOutput("", paths), woke: false };
  const provider = deps.providers(source.provider_kind, source.id);
  const woke = await wakeIfStopped(provider, source.provider_resource_id, source.id, {
    setState: (id, state) => deps.store.setPlacementState(id, state),
    afterWake: (p, rid, pid) => deps.setup.afterWake(p, rid, pid),
  });
  const out = await provider.execOnMachine(source.provider_resource_id, preflightCommand(paths), 120);
  return { report: parsePreflightOutput(out.exitCode === 0 ? out.stdout : "", paths), woke };
}

export async function runPreflight(
  deps: Pick<EngineDeps, "store" | "providers" | "setup">,
  m: MigrationRow,
): Promise<PreflightReport> {
  return (await runPreflightOnSource(deps, m)).report;
}

/** Stops a source that preflight woke, once the migration parks. */
export async function stopSourceAgain(deps: Pick<EngineDeps, "store" | "providers">, m: MigrationRow): Promise<void> {
  const source = await deps.store.getPlacement(m.source_placement_id);
  if (!source?.provider_resource_id) return;
  await deps.providers(source.provider_kind, source.id).stopInstance(source.provider_resource_id);
  await deps.store.setPlacementState(source.id, "suspended");
}

/**
 * Checks that sshd (127.0.0.1:2222) and the gateway accept connections on
 * the target, so cutover never lands on a machine the relay cannot reach.
 */
export function listenProbeCommand(gatewayListen: string = GATEWAY_LISTEN): string[] {
  const port = gatewayListen.slice(gatewayListen.lastIndexOf(":") + 1);
  const script = `exec 3<>/dev/tcp/127.0.0.1/${MANAGED_SSHD_PORT} && exec 4<>/dev/tcp/127.0.0.1/${port}`;
  return ["bash", "-c", script];
}

async function stepLeaseSeconds(deps: EngineDeps, m: MigrationRow): Promise<number> {
  if (m.state === "target_bootstrapped") {
    const repos = await deps.store.listRepositories(m.instance_id);
    return DEFAULT_STEP_LEASE_SEC + RECLONE_TIMEOUT_SEC * repos.length;
  }
  return STEP_LEASE_SEC[m.state] ?? DEFAULT_STEP_LEASE_SEC;
}

/**
 * A migration cancelled (instance deleted or lapsed) while this worker
 * created its target: the new machine is ours to delete, since the
 * canceller listed placements before it existed.
 */
async function abandonIfCancelled(
  deps: EngineDeps,
  m: MigrationRow,
  target: PlacementRow,
  provider: SandboxProvider,
  resourceId: string,
): Promise<StepResult | null> {
  const now = await deps.store.getMigration(m.id);
  if (now?.state !== "cancelled") return null;
  await ignoreNotFound(provider.deleteInstance(resourceId, { placementId: target.id }));
  await deps.store.setPlacementState(target.id, "deleted");
  if (target.role !== "dead") await deps.store.setPlacementRole(target.id, "dead");
  deps.log?.({ migration_id: m.id, abandoned_target: target.id, provider_resource_id: resourceId });
  return { state: "cancelled", advanced: false };
}

async function requireTarget(deps: EngineDeps, m: MigrationRow): Promise<PlacementRow> {
  const id = m.target_placement_id;
  const target = id ? await deps.store.getPlacement(id) : null;
  if (!target) throw new StepFailure("target placement missing");
  return target;
}

// One step of the state machine. Returns the state after the step.
export async function stepMigration(deps: EngineDeps, migrationId: string): Promise<StepResult> {
  const { store } = deps;
  const m = await store.getMigration(migrationId);
  if (!m) throw new Error(`migration ${migrationId} not found`);
  const advance = async (next: MigrationState, detail?: Record<string, unknown>): Promise<StepResult> => {
    const ok = await store.advance(m.id, m.state, next, detail);
    deps.log?.({ migration_id: m.id, from: m.state, to: next, advanced: ok });
    return { state: ok ? next : m.state, advanced: ok };
  };

  if (deps.renewLease && RUNNABLE_STATES.includes(m.state as MigrationState)) {
    if (!(await deps.renewLease(m.id, await stepLeaseSeconds(deps, m)))) {
      deps.log?.({ migration_id: m.id, lease_lost: true });
      return { state: m.state, advanced: false };
    }
  }

  try {
    switch (m.state) {
      case "requested":
        return await advance("preflight");

      case "preflight": {
        let report = m.preflight;
        let woke = false;
        if (!report) {
          ({ report, woke } = await runPreflightOnSource(deps, m));
          await store.setMigrationFields(m.id, { preflight: report });
        }
        if (report.requires_ack && !m.acknowledged_data_loss) {
          const r = await advance("awaiting_ack", { requires_ack: true });
          // The source was stopped before preflight; parked, it stops again.
          if (r.advanced && woke) await stopSourceAgain(deps, m);
          return r;
        }
        return await advance("target_provisioning");
      }

      case "target_provisioning": {
        const type = await store.getMachineType(m.target_machine_type_id);
        if (!type) throw new StepFailure(`unknown machine type ${m.target_machine_type_id}`);
        const instance = await store.getInstance(m.instance_id);
        if (!instance) throw new StepFailure("instance missing");
        let target = m.target_placement_id ? await store.getPlacement(m.target_placement_id) : null;
        if (!target) {
          // A BYOS workspace stays on its owner's account: same provider,
          // same credential.
          const source = await store.getPlacement(m.source_placement_id);
          const byos = source?.credential_source === "byos";
          if (byos && source.provider_kind !== type.provider_kind) {
            throw new StepFailure("a BYOS workspace cannot move to another provider");
          }
          target = (await store.findCandidatePlacement(m.instance_id)) ??
            await store.createCandidatePlacement({
              instanceId: m.instance_id,
              ownerUserId: m.owner_user_id,
              providerKind: type.provider_kind,
              machineTypeId: type.id,
              region: instance.region,
              credentialSource: byos ? "byos" : "managed",
              credentialId: byos ? source.credential_id ?? null : null,
            });
          await store.setMigrationFields(m.id, { target_placement_id: target.id });
        }
        const provider = deps.providers(target.provider_kind, target.id);
        const sourceResource = (await store.getPlacement(m.source_placement_id))?.provider_resource_id ?? null;
        let resourceId = target.provider_resource_id;
        if (!resourceId) {
          const created = await provider.createInstance({
            ownerUserId: m.owner_user_id,
            region: instance.region as never,
            sizePreset: "small",
            manifestVersion: CURRENT_MANIFEST_VERSION,
            idempotencyKey: targetIdempotencyKey(m.id),
            machineType: type.vendor_type,
            placementId: target.id,
          });
          resourceId = created.providerResourceId;
          // Defence in depth: a provider that hands back the source machine
          // would turn the resize into a no-op, and the rollback would then
          // delete the user's only disk. Refuse before recording it, so the
          // rollback has nothing to delete.
          if (sourceResource && resourceId === sourceResource) {
            throw new StepFailure("provider returned the source machine as the target");
          }
          await store.setPlacementResource(target.id, resourceId);
          const abandoned = await abandonIfCancelled(deps, m, target, provider, resourceId);
          if (abandoned) return abandoned;
        }
        if (target.state !== "bootstrapping") await store.setPlacementState(target.id, "bootstrapping");
        await deps.setup.bootstrap(provider, resourceId, target.id);
        return await advance("target_bootstrapped", { target_placement_id: target.id, provider_resource_id: resourceId });
      }

      case "target_bootstrapped": {
        const target = await requireTarget(deps, m);
        const provider = deps.providers(target.provider_kind, target.id);
        const resourceId = target.provider_resource_id;
        if (!resourceId) throw new StepFailure("the target placement has no machine");
        const repos: Array<Record<string, unknown>> = [];
        for (const repo of await store.listRepositories(m.instance_id)) {
          if (!repo.remote_url) {
            repos.push({ path: repo.remote_path, status: "needs_attention", reason: "no_remote_url" });
            continue;
          }
          try {
            const out = await provider.execOnMachine(
              resourceId,
              recloneCommand(repo.remote_url, repo.remote_path),
              RECLONE_TIMEOUT_SEC,
            );
            repos.push(
              out.exitCode === 0
                ? { path: repo.remote_path, status: "cloned" }
                : { path: repo.remote_path, status: "needs_attention", reason: "clone_failed" },
            );
          } catch (err) {
            // A transient provider error retries the whole step (clones that
            // finished are kept by the script). Anything else marks the repo.
            if (isTransient(err)) throw err;
            repos.push({ path: repo.remote_path, status: "needs_attention", reason: "clone_failed" });
          }
        }
        const config = await pullUserConfigOntoTarget(provider, resourceId, m);
        return await advance("verifying", { repos, config_restored: config.restored });
      }

      case "verifying": {
        const target = await requireTarget(deps, m);
        const provider = deps.providers(target.provider_kind, target.id);
        const resourceId = target.provider_resource_id;
        if (!resourceId) throw new StepFailure("the target placement has no machine");
        let observed = await provider.getInstance(resourceId);
        if (observed.state === "suspended") {
          // A candidate bills nothing, so its wake needs no state change.
          await provider.wakeInstance(resourceId);
          await deps.setup.afterWake(provider, resourceId, target.id);
          observed = await provider.getInstance(resourceId);
        }
        if (observed.state !== "ready") throw new ProviderError("unavailable", `target is ${observed.state}`);
        const probe = await provider.execOnMachine(resourceId, listenProbeCommand(provider.gatewayListenAddress), 30);
        if (probe.exitCode !== 0) {
          throw new StepFailure(`target sshd or gateway is not listening (exit ${probe.exitCode})`);
        }
        if (target.state !== "ready") await store.setPlacementState(target.id, "ready");
        return await advance("cutover");
      }

      case "cutover": {
        const target = await requireTarget(deps, m);
        let fence: number | null = null;
        if (target.role === "candidate") {
          if (deps.spendCapped && await deps.spendCapped(m.owner_user_id)) {
            throw new StepFailure("owner is over the spend cap");
          }
          try {
            fence = await store.cutover(m, target.id);
          } catch (err) {
            // Lost to a concurrent cutover of this same target: fine.
            // Any other fence, source or role conflict is not ours to resolve.
            const msg = (err as Error).message ?? "";
            if (!/fence_mismatch|target_not_candidate|source_not_active/.test(msg)) throw err;
            const now = await store.getPlacement(target.id);
            if (now?.role !== "active") throw new StepFailure(msg);
          }
        } else if (target.role !== "active") {
          throw new StepFailure(`target placement is ${target.role}`);
        }
        await deps.setup.afterCutover(m, { ...target, role: "active" });
        return await advance("source_retiring", fence === null ? {} : { fence });
      }

      case "source_retiring": {
        const source = await store.getPlacement(m.source_placement_id);
        if (source?.provider_resource_id && source.state !== "suspended") {
          await ignoreNotFound(deps.providers(source.provider_kind, source.id).stopInstance(source.provider_resource_id));
        }
        if (source && source.state !== "suspended") await store.setPlacementState(source.id, "suspended");
        return await advance("completed");
      }

      case "rolling_back": {
        const target = m.target_placement_id
          ? await store.getPlacement(m.target_placement_id)
          : await store.findCandidatePlacement(m.instance_id);
        if (target && target.role === "candidate") {
          const source = await store.getPlacement(m.source_placement_id);
          // Never delete a machine the source placement still points at.
          if (target.provider_resource_id && target.provider_resource_id !== source?.provider_resource_id) {
            await ignoreNotFound(deps.providers(target.provider_kind, target.id).deleteInstance(target.provider_resource_id, {
                placementId: target.id,
              }));
          }
          await store.setPlacementState(target.id, "deleted");
          await store.setPlacementRole(target.id, "dead");
        }
        return await advance("rolled_back", { reason: "rollback" });
      }

      default:
        return { state: m.state, advanced: false };
    }
  } catch (err) {
    if (!(err instanceof ProviderError) && !(err instanceof StepFailure)) throw err;
    const detail = {
      step: m.state,
      kind: err instanceof ProviderError ? err.kind : "step_failed",
      message: err.message,
    };
    deps.log?.({ migration_id: m.id, error: detail });
    const attempt = m.attempt + 1;
    let mayRollBack = ROLLBACK_ALLOWED.has(m.state);
    if (mayRollBack && m.state === "cutover") {
      // Only while the target never became active.
      const target = m.target_placement_id ? await store.getPlacement(m.target_placement_id) : null;
      mayRollBack = target?.role === "candidate";
    }
    if (mayRollBack && (!isTransient(err) || attempt >= MAX_ATTEMPTS)) {
      await store.setMigrationFields(m.id, { attempt, error: detail });
      return await advance("rolling_back", detail);
    }
    await store.setMigrationFields(m.id, { attempt, error: detail });
    if (!mayRollBack && attempt >= MAX_POST_CUTOVER_ATTEMPTS && POST_CUTOVER_STATES.has(m.state)) {
      return await advance("failed", { ...detail, attempts: attempt });
    }
    // After cutover (or on a transient error) the step is retried.
    return { state: m.state, advanced: false };
  }
}

/**
 * Runs steps until the migration ends, parks, stops making progress, or the
 * time budget runs out. The caller must hold the migration's lease.
 */
export async function runMigration(
  deps: EngineDeps,
  migrationId: string,
  opts: { deadlineMs: number; maxSteps: number },
): Promise<{ state: string; steps: number }> {
  let state = "";
  let steps = 0;
  while (steps < opts.maxSteps && deps.now() < opts.deadlineMs) {
    const r = await stepMigration(deps, migrationId);
    state = r.state;
    steps++;
    if (TERMINAL_STATES.has(state) || state === "awaiting_ack" || !r.advanced) break;
  }
  if (!state) state = (await deps.store.getMigration(migrationId))?.state ?? "";
  return { state, steps };
}
