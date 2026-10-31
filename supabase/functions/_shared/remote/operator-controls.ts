// Operator controls for providers, prices and migrations
// (docs/design/sandbox-provider-adapter.md §9). remote-admin dispatches
// here; every action is audited in remote_audit_events. Actions that touch
// no single user's instance are audited with no owner.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { recordAuditEvent } from "./audit.ts";
import { must, rpc } from "./db.ts";
import { isOperatorMovable } from "./byos.ts";
import { supabaseMachineSetup } from "./machine-setup.ts";
import { drainAllowance, type MigrationCaps, migrationCaps } from "./drain.ts";
import { engineDeps, INLINE_BUDGET_MS, runLeasedMigrations } from "./migration-runner.ts";
import { isOverdueParked, resumeParkedMigration } from "./migration-resume.ts";
import {
  createMigration,
  getOpenMigration,
  getPlacement,
  resolveDefaultMachineType,
  ignoreNotFound,
  setPlacementState,
  updatePlacement,
} from "./placement-store.ts";
import type { MigrationRow, PlacementRow } from "./migration-engine.ts";
import type { ProviderKind, SandboxProvider } from "./provider.ts";

export const RETIRING_TTL_HOURS = 24;
const DEFAULT_BATCH = 25;
const DRAIN_NOTICE_DAYS = 7;
const PROVIDER_KINDS: readonly ProviderKind[] = ["boat", "local", "daytona", "e2b", "fly_machines"];

export class OperatorInputError extends Error {
  constructor(message: string, public readonly status = 400) {
    super(message);
  }
}

export interface OperatorContext {
  supabase: SupabaseClient;
  providers: (kind: ProviderKind, placementId?: string | null) => SandboxProvider;
  stub: boolean;
  correlationId: string;
  operatorId: string;
  /** Concurrency caps on open migrations; read from the environment when absent. */
  caps?: MigrationCaps;
}

const fail = (what: string, error: { message: string } | null) => must({ data: null, error }, what);

export function requireKind(value: unknown): ProviderKind {
  if (typeof value !== "string" || !PROVIDER_KINDS.includes(value as ProviderKind)) {
    throw new OperatorInputError(`kind must be one of ${PROVIDER_KINDS.join(", ")}`);
  }
  return value as ProviderKind;
}

function batch(value: unknown): number {
  return typeof value === "number" && Number.isInteger(value) && value > 0 && value <= 200 ? value : DEFAULT_BATCH;
}

async function recompute(supabase: SupabaseClient): Promise<number> {
  const data = must(await supabase.rpc("recompute_pro_defaults"), "recompute defaults");
  return Number(data);
}

export async function recomputeDefaults(ctx: OperatorContext): Promise<{ defaults: number }> {
  const defaults = await recompute(ctx.supabase);
  await recordAuditEvent(ctx.supabase, {
    ownerUserId: null,
    eventType: "admin_defaults_recomputed",
    detail: { operator_id: ctx.operatorId, defaults },
    correlationId: ctx.correlationId,
  });
  return { defaults };
}

/**
 * Stops new placements on a provider and moves its live instances off it.
 * Creates the first batch of migrations at once (at most `limit`, within
 * the concurrency caps); each hourly `advance_migrations` run creates the
 * next. Instances with no admitted default elsewhere are reported as
 * skipped. A retiring provider becomes 'retired' once nothing is left on it.
 */
export async function drainProvider(
  ctx: OperatorContext,
  body: Record<string, unknown>,
  mode: "drain" | "retire" = "drain",
): Promise<Record<string, unknown>> {
  const kind = requireKind(body.kind);
  const limit = batch(body.limit);
  const { supabase } = ctx;
  const status = mode === "drain" ? "draining" : "retiring";

  must(await supabase
    .from("sandbox_providers")
    .update({ status, updated_at: new Date().toISOString() })
    .eq("kind", kind), "update provider");
  must(await supabase
    .from("provider_machine_types")
    .update({ selectable: false })
    .eq("provider_kind", kind), "hide machine types");
  const defaults = await recompute(supabase);

  const { created, skipped } = await createDrainBatch(ctx, kind, mode, limit);

  await recordAuditEvent(supabase, {
    ownerUserId: null,
    eventType: mode === "drain" ? "admin_provider_drained" : "admin_provider_retired",
    detail: { operator_id: ctx.operatorId, kind, migrations_created: created.length, skipped: skipped.length },
    correlationId: ctx.correlationId,
    severity: "warning",
  });
  return { kind, status, defaults, migrations_created: created, skipped };
}

/**
 * Creates up to `limit` drain or retire migrations for live instances still
 * on `kind`, skipping instances with an open migration, within the
 * per-provider and global caps on open migrations.
 */
async function createDrainBatch(
  ctx: OperatorContext,
  kind: ProviderKind,
  mode: "drain" | "retire",
  limit: number,
): Promise<{ created: string[]; skipped: Array<{ instance_id: string; reason: string }> }> {
  const { supabase } = ctx;
  const open = must(await supabase
    .from("sandbox_migrations")
    .select(
      "instance_id, source:remote_instance_placements!sandbox_migrations_source_placement_id_fkey(provider_kind)",
    )
    .not("state", "in", "(completed,rolled_back,failed,cancelled)"), "list open migrations");
  const busy = new Set((open ?? []).map((r) => r.instance_id as string));
  const openForProvider = (open ?? []).filter((r) =>
    (r.source as unknown as { provider_kind: string } | null)?.provider_kind === kind
  ).length;
  const allowance = drainAllowance({
    batch: limit,
    openForProvider,
    openTotal: busy.size,
    caps: ctx.caps ?? migrationCaps(),
  });
  const created: string[] = [];
  const skipped: Array<{ instance_id: string; reason: string }> = [];
  if (allowance === 0) return { created, skipped };

  const placements = must(await supabase
    .from("remote_instance_placements")
    .select(
      "id, instance_id, owner_user_id, region, credential_source, remote_instances!remote_instance_placements_instance_id_fkey(tier_id)",
    )
    .eq("provider_kind", kind)
    // BYOS workspaces run on the user's own account; operators never move them.
    .eq("credential_source", "managed")
    .eq("role", "active")
    .neq("state", "deleted")
    .order("created_at")
    .limit(limit + busy.size), "list placements");

  const deadline = new Date(Date.now() + DRAIN_NOTICE_DAYS * 86_400_000).toISOString();
  for (const p of placements ?? []) {
    if (created.length >= allowance) break;
    if (busy.has(p.instance_id) || !isOperatorMovable(p)) continue;
    const joined = p.remote_instances as unknown as { tier_id: string | null } | null;
    const tier = joined?.tier_id ?? "s";
    const target = await resolveDefaultMachineType(supabase, tier, p.region);
    if (!target || target.provider_kind === kind) {
      skipped.push({ instance_id: p.instance_id, reason: "no_admitted_target" });
      continue;
    }
    const m = await createMigration(supabase, {
      instanceId: p.instance_id,
      ownerUserId: p.owner_user_id,
      sourcePlacementId: p.id,
      targetMachineTypeId: target.id,
      reason: mode,
      deadline,
    });
    if (!m) continue;
    created.push(m.id);
    await recordAuditEvent(supabase, {
      ownerUserId: p.owner_user_id,
      instanceId: p.instance_id,
      eventType: "migration_requested",
      detail: { migration_id: m.id, reason: mode, from_provider: kind, target_machine_type_id: target.id, operator_id: ctx.operatorId },
      correlationId: ctx.correlationId,
    });
  }
  return { created, skipped };
}

/**
 * Continues every draining or retiring provider: the next batch of
 * migrations, and 'retired' for a retiring provider with no live placement.
 */
async function continueDrains(ctx: OperatorContext, limit: number): Promise<{
  drains: Array<{ kind: string; status: string; migrations_created: string[]; skipped: number }>;
  retired: string[];
}> {
  const { supabase } = ctx;
  const providers = must(await supabase
    .from("sandbox_providers")
    .select("kind, status")
    .in("status", ["draining", "retiring"])
    .order("kind"), "list draining providers");
  const drains: Array<{ kind: string; status: string; migrations_created: string[]; skipped: number }> = [];
  const retired: string[] = [];
  for (const p of providers ?? []) {
    const kind = p.kind as ProviderKind;
    const mode = p.status === "retiring" ? "retire" : "drain";
    const { created, skipped } = await createDrainBatch(ctx, kind, mode, limit);
    drains.push({ kind, status: p.status as string, migrations_created: created, skipped: skipped.length });
    if (mode !== "retire") continue;
    const { count, error: countError } = await supabase
      .from("remote_instance_placements")
      .select("id", { count: "exact", head: true })
      .eq("provider_kind", kind)
      .eq("credential_source", "managed")
      .neq("role", "dead");
    fail("count live placements", countError);
    if ((count ?? 0) > 0) continue;
    must(await supabase
      .from("sandbox_providers")
      .update({ status: "retired", updated_at: new Date().toISOString() })
      .eq("kind", kind)
      .eq("status", "retiring"), "retire provider");
    retired.push(kind);
    await recordAuditEvent(supabase, {
      ownerUserId: null,
      eventType: "provider_retire_completed",
      detail: { kind },
      correlationId: ctx.correlationId,
      severity: "warning",
    });
  }
  return { drains, retired };
}

export async function setPrice(ctx: OperatorContext, body: Record<string, unknown>): Promise<Record<string, unknown>> {
  const machineTypeId = body.machine_type_id;
  if (typeof machineTypeId !== "string" || machineTypeId.length === 0) {
    throw new OperatorInputError("machine_type_id is required");
  }
  const running = body.running_nanousd_per_sec;
  if (typeof running !== "number" || !Number.isInteger(running) || running < 0) {
    throw new OperatorInputError("running_nanousd_per_sec must be a non-negative integer");
  }
  const stopped = body.stopped_nanousd_per_gb_sec ?? 0;
  if (typeof stopped !== "number" || !Number.isInteger(stopped) || stopped < 0) {
    throw new OperatorInputError("stopped_nanousd_per_gb_sec must be a non-negative integer");
  }
  const from = body.effective_from ?? new Date().toISOString();
  if (typeof from !== "string" || Number.isNaN(Date.parse(from))) {
    throw new OperatorInputError("effective_from must be an ISO timestamp");
  }
  const { data, error } = await ctx.supabase.rpc("set_machine_type_price", {
    p_machine_type_id: machineTypeId,
    p_running_nanousd_per_sec: running,
    p_stopped_nanousd_per_gb_sec: stopped,
    p_effective_from: from,
  });
  if (error?.message.includes("effective_from must be after")) throw new OperatorInputError(error.message, 409);
  fail("set price", error);
  await recordAuditEvent(ctx.supabase, {
    ownerUserId: null,
    eventType: "admin_price_set",
    detail: {
      operator_id: ctx.operatorId,
      machine_type_id: machineTypeId,
      running_nanousd_per_sec: running,
      stopped_nanousd_per_gb_sec: stopped,
      effective_from: from,
      price_id: data,
    },
    correlationId: ctx.correlationId,
    severity: "warning",
  });
  return { price_id: data, machine_type_id: machineTypeId, effective_from: from };
}

/**
 * The hourly run: continues drains, advances runnable migrations, and
 * reports parked drains past their deadline as `overdue` (never forced).
 */
export async function advanceMigrations(
  ctx: OperatorContext,
  body: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  const single = typeof body.migration_id === "string";
  const drains = single ? { drains: [], retired: [] } : await continueDrains(ctx, batch(body.limit));
  const result = await runLeasedMigrations({
    supabase: ctx.supabase,
    providers: ctx.providers,
    stub: ctx.stub,
    correlationId: ctx.correlationId,
    leaseOwner: `remote-admin:${ctx.correlationId}`,
    budgetMs: INLINE_BUDGET_MS * 4,
    limit: batch(body.limit),
    migrationId: typeof body.migration_id === "string" ? body.migration_id : undefined,
  });
  const overdue = single ? [] : await listOverdueParked(ctx.supabase);
  return { ...result, ...drains, overdue };
}

async function listOverdueParked(supabase: SupabaseClient): Promise<
  Array<{ id: string; instance_id: string; owner_user_id: string; reason: string; deadline: string }>
> {
  const data = must(await supabase
    .from("sandbox_migrations")
    .select("id, instance_id, owner_user_id, state, reason, deadline")
    .eq("state", "awaiting_ack")
    .in("reason", ["drain", "retire"])
    .lt("deadline", new Date().toISOString())
    .order("deadline")
    .limit(200), "list overdue migrations");
  const now = Date.now();
  return (data ?? [])
    .filter((m) => isOverdueParked(m as { state: string; reason: string; deadline: string | null }, now))
    .map((m) => ({
      id: m.id as string,
      instance_id: m.instance_id as string,
      owner_user_id: m.owner_user_id as string,
      reason: m.reason as string,
      deadline: m.deadline as string,
    }));
}

/**
 * Moves a parked migration on for the user (§9 `force(migration, reason)`):
 * re-runs preflight, records the acknowledgement if work is still unpushed,
 * and runs the engine inline. Audited with the operator's reason.
 */
export async function forceMigration(
  ctx: OperatorContext,
  body: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  const migrationId = body.migration_id;
  if (typeof migrationId !== "string" || migrationId.length === 0) {
    throw new OperatorInputError("migration_id is required");
  }
  const reason = typeof body.reason === "string" ? body.reason.trim() : "";
  if (reason.length === 0) throw new OperatorInputError("reason is required");
  const { supabase } = ctx;
  const m = must(await supabase
    .from("sandbox_migrations")
    .select("id, instance_id, owner_user_id, state")
    .eq("id", migrationId)
    .maybeSingle(), "read migration");
  if (!m) throw new OperatorInputError("migration not found", 404);
  if (m.state !== "awaiting_ack") throw new OperatorInputError(`migration is ${m.state}`, 409);

  const resumed = await resumeParkedMigration(engineDeps(ctx), migrationId, { acknowledge: true });
  await recordAuditEvent(supabase, {
    ownerUserId: m.owner_user_id as string,
    instanceId: m.instance_id as string,
    eventType: "admin_migration_forced",
    detail: {
      migration_id: migrationId,
      operator_id: ctx.operatorId,
      reason,
      resumed: resumed.resumed,
      acknowledged_data_loss: resumed.acknowledged,
    },
    correlationId: ctx.correlationId,
    severity: "warning",
  });
  let state = resumed.state;
  if (resumed.resumed) {
    const run = await runLeasedMigrations({
      supabase,
      providers: ctx.providers,
      stub: ctx.stub,
      correlationId: ctx.correlationId,
      leaseOwner: `remote-admin:${ctx.correlationId}`,
      budgetMs: INLINE_BUDGET_MS,
      limit: 1,
      migrationId,
    });
    state = run.migrations[0]?.state ?? state;
  }
  return {
    migration_id: migrationId,
    resumed: resumed.resumed,
    migration_state: state,
    acknowledged_data_loss: resumed.acknowledged,
    preflight: resumed.preflight,
  };
}

/** What destroying retired and orphaned machines reads and writes. */
export interface RetireStore {
  /** Retiring placements retired before `cutoff`. */
  listRetiring(cutoff: Date, limit: number): Promise<PlacementRow[]>;
  /**
   * Marks a retiring placement dead, only while it is still retiring. False
   * when a reverse cutover took it back first.
   */
  claimRetiring(placementId: string): Promise<boolean>;
  /** Placements that still hold a machine nobody uses (orphaned_placements). */
  listOrphans(limit: number): Promise<PlacementRow[]>;
  setPlacementState(placementId: string, state: string): Promise<void>;
  markDead(placementId: string): Promise<void>;
  audit(eventType: string, p: PlacementRow, detail: Record<string, unknown>): Promise<void>;
}

export interface RetireDeps {
  store: RetireStore;
  providers: (kind: ProviderKind, placementId?: string | null) => Pick<SandboxProvider, "deleteInstance">;
}

async function deleteMachine(deps: RetireDeps, p: PlacementRow): Promise<void> {
  if (!p.provider_resource_id) return;
  await ignoreNotFound(deps.providers(p.provider_kind, p.id).deleteInstance(p.provider_resource_id, { placementId: p.id }));
}

/**
 * Destroys retiring placements older than `cutoff`, then sweeps orphans:
 * dead placements whose machine was never deleted (a destroy or cancel that
 * failed half way) and candidates no open migration owns. A retiring
 * placement is claimed (role 'dead') before its machine is deleted, so a
 * reverse cutover racing it at the window's edge wins or loses cleanly.
 */
export async function destroyRetired(
  deps: RetireDeps,
  opts: { cutoff: Date; limit: number },
): Promise<{ destroyed: string[]; failed: string[]; orphans_destroyed: string[] }> {
  const { store } = deps;
  const destroyed: string[] = [];
  const failed: string[] = [];
  const orphans: string[] = [];
  for (const p of await store.listRetiring(opts.cutoff, opts.limit)) {
    try {
      if (!(await store.claimRetiring(p.id))) continue;
      await deleteMachine(deps, p);
      await store.setPlacementState(p.id, "deleted");
      destroyed.push(p.id);
      await store.audit("placement_destroyed", p, {
        placement_id: p.id,
        provider_kind: p.provider_kind,
        provider_resource_id: p.provider_resource_id,
      });
    } catch (err) {
      // Claimed but not deleted: the orphan sweep retries it.
      failed.push(p.id);
      await store.audit("placement_destroy_failed", p, { placement_id: p.id, error: (err as Error).message });
    }
  }
  for (const p of await store.listOrphans(opts.limit)) {
    try {
      await deleteMachine(deps, p);
      await store.setPlacementState(p.id, "deleted");
      if (p.role !== "dead") await store.markDead(p.id);
      orphans.push(p.id);
      await store.audit("placement_orphan_destroyed", p, {
        placement_id: p.id,
        role: p.role,
        state: p.state,
        provider_kind: p.provider_kind,
        provider_resource_id: p.provider_resource_id,
      });
    } catch (err) {
      failed.push(p.id);
      await store.audit("placement_destroy_failed", p, { placement_id: p.id, error: (err as Error).message });
    }
  }
  return { destroyed, failed, orphans_destroyed: orphans };
}

function supabaseRetireStore(ctx: OperatorContext): RetireStore {
  const { supabase } = ctx;
  return {
    async listRetiring(cutoff, limit) {
      return (must(await supabase
        .from("remote_instance_placements")
        .select("*")
        .eq("role", "retiring")
        .lt("retired_at", cutoff.toISOString())
        .order("retired_at")
        .limit(limit), "list retiring placements") ?? []) as PlacementRow[];
    },
    async claimRetiring(id) {
      const data = must(await supabase
        .from("remote_instance_placements")
        .update({ role: "dead" })
        .eq("id", id)
        .eq("role", "retiring")
        .select("id"), "claim retiring placement");
      return (data ?? []).length > 0;
    },
    async listOrphans(limit) {
      return (must(await supabase.rpc("orphaned_placements", { p_limit: limit }), "list orphaned placements") ?? []) as PlacementRow[];
    },
    setPlacementState: (id, state) => setPlacementState(supabase, id, state),
    markDead: (id) => updatePlacement(supabase, id, { role: "dead", retired_at: new Date().toISOString() }),
    async audit(eventType, p, detail) {
      await recordAuditEvent(supabase, {
        ownerUserId: p.owner_user_id,
        instanceId: p.instance_id,
        eventType: eventType as "placement_destroyed",
        detail,
        correlationId: ctx.correlationId,
      });
    },
  };
}

/** Destroys retiring placements older than the reverse-cutover window, and orphaned machines. */
export async function destroyRetiredPlacements(
  ctx: OperatorContext,
  body: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  return await destroyRetired(
    { store: supabaseRetireStore(ctx), providers: ctx.providers },
    { cutoff: new Date(Date.now() - RETIRING_TTL_HOURS * 3_600_000), limit: batch(body.limit) },
  );
}

/** What a reverse cutover reads and writes. */
export interface ReverseCutoverDeps {
  /** The instance's open migration, if any. */
  openMigration(instanceId: string): Promise<{ id: string } | null>;
  /** `reverse_cutover`: the atomic role swap and fence bump. Returns the new fence. */
  reverse(instanceId: string, migrationId: string): Promise<number>;
  setPlacementState(placementId: string, state: string): Promise<void>;
  providers: (kind: ProviderKind, placementId?: string | null) => SandboxProvider;
  afterWake(provider: SandboxProvider, providerResourceId: string, placementId: string | null): Promise<void>;
  afterCutover(migration: MigrationRow, placement: PlacementRow): Promise<void>;
}

/**
 * Wakes the source, swaps it back in with `reverse_cutover` (which checks
 * in one transaction that no migration is open, the source is still
 * retiring and the target active), then stops the target. If the swap
 * fails the source is stopped again and stays retiring.
 */
export async function runReverseCutover(
  deps: ReverseCutoverDeps,
  migration: MigrationRow,
  source: PlacementRow,
  target: PlacementRow,
): Promise<number> {
  if (await deps.openMigration(migration.instance_id)) {
    throw new OperatorInputError("another migration is open on this instance", 409);
  }
  const resourceId = source.provider_resource_id;
  if (!resourceId) throw new OperatorInputError("the source placement has no machine", 409);
  const sourceProvider = deps.providers(source.provider_kind, source.id);
  let fence: number;
  try {
    // The source is not active, so `waking` bills nothing; reverse_cutover
    // marks it ready once it is active.
    await deps.setPlacementState(source.id, "waking");
    await sourceProvider.wakeInstance(resourceId);
    await deps.afterWake(sourceProvider, resourceId, source.id);
    fence = await deps.reverse(migration.instance_id, migration.id);
  } catch (err) {
    await ignoreNotFound(sourceProvider.stopInstance(resourceId));
    await deps.setPlacementState(source.id, "suspended");
    throw err;
  }
  await deps.afterCutover(migration, { ...source, role: "active" });
  if (target.provider_resource_id) {
    await ignoreNotFound(deps.providers(target.provider_kind, target.id).stopInstance(target.provider_resource_id));
  }
  await deps.setPlacementState(target.id, "suspended");
  return fence;
}

/**
 * Moves a completed migration back to its source while the source is still
 * kept (within RETIRING_TTL_HOURS). The source must start again; the
 * switched-to machine is stopped and retires in turn.
 */
export async function reverseCutover(
  ctx: OperatorContext,
  body: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  const migrationId = body.migration_id;
  if (typeof migrationId !== "string" || migrationId.length === 0) {
    throw new OperatorInputError("migration_id is required");
  }
  const { supabase } = ctx;
  const m = must(await supabase.from("sandbox_migrations").select("*").eq("id", migrationId).maybeSingle(), "read migration");
  if (!m) throw new OperatorInputError("migration not found", 404);
  const migration = m as MigrationRow;
  if (migration.state !== "completed") throw new OperatorInputError(`migration is ${migration.state}`, 409);
  const source = await getPlacement(supabase, migration.source_placement_id);
  const target = migration.target_placement_id ? await getPlacement(supabase, migration.target_placement_id) : null;
  if (!source || source.role !== "retiring" || !source.provider_resource_id) {
    throw new OperatorInputError("source placement is no longer kept", 409);
  }
  const retiredAt = (source as PlacementRow & { retired_at?: string }).retired_at;
  if (!retiredAt || Date.parse(retiredAt) < Date.now() - RETIRING_TTL_HOURS * 3_600_000) {
    throw new OperatorInputError("the reverse-cutover window has passed", 409);
  }
  if (!target || target.role !== "active") throw new OperatorInputError("target is not the active placement", 409);

  const setup = supabaseMachineSetup(supabase, { stub: ctx.stub, correlationId: ctx.correlationId, providers: ctx.providers });
  const fence = await runReverseCutover(
    {
      openMigration: (instanceId) => getOpenMigration(supabase, instanceId),
      async reverse(instanceId, id) {
        const data = must(await supabase.rpc("reverse_cutover", {
          p_instance_id: instanceId,
          p_migration_id: id,
          p_detail: { reversed: true, operator_id: ctx.operatorId },
        }), "reverse cutover");
        return Number(data);
      },
      setPlacementState: (id, state) => setPlacementState(supabase, id, state),
      providers: ctx.providers,
      afterWake: (p, rid, pid) => setup.afterWake(p, rid, pid),
      afterCutover: (mig, p) => setup.afterCutover(mig, p),
    },
    migration,
    source,
    target,
  );
  await recordAuditEvent(supabase, {
    ownerUserId: migration.owner_user_id,
    instanceId: migration.instance_id,
    eventType: "admin_cutover_reversed",
    detail: { migration_id: migration.id, operator_id: ctx.operatorId, fence },
    correlationId: ctx.correlationId,
    severity: "warning",
  });
  return { migration_id: migration.id, state: "rolled_back", active_placement_id: source.id, fence };
}

// Instance statuses an operator may force out of: the lifecycle minus
// ready, suspended and deleted, which are not stuck.
const RECOVERABLE_STATUSES = [
  "provisioning",
  "bootstrapping",
  "installing_access",
  "verifying",
  "waking",
  "reprovisioning",
  "degraded",
  "deleting",
];

function requireString(body: Record<string, unknown>, field: string): string {
  const value = body[field];
  if (typeof value !== "string" || value.length === 0) throw new OperatorInputError(`${field} is required`);
  return value;
}

/** Revokes any user's client key by id. */
export async function revokeClientKey(ctx: OperatorContext, body: Record<string, unknown>) {
  const keyId = requireString(body, "key_id");
  const key = must(
    await ctx.supabase
      .from("remote_client_keys")
      .select("id, owner_user_id, fingerprint_sha256, revoked_at")
      .eq("id", keyId)
      .maybeSingle(),
    "read client key",
  );
  if (!key) throw new OperatorInputError("Key not found", 404);
  if (!key.revoked_at) {
    must(
      await ctx.supabase.from("remote_client_keys").update({ revoked_at: new Date().toISOString() }).eq("id", keyId),
      "revoke client key",
    );
  }
  await recordAuditEvent(ctx.supabase, {
    ownerUserId: key.owner_user_id,
    eventType: "admin_client_key_revoked",
    detail: {
      key_id: keyId,
      fingerprint: key.fingerprint_sha256,
      operator_id: ctx.operatorId,
      reason: typeof body.reason === "string" ? body.reason : null,
      already_revoked: Boolean(key.revoked_at),
    },
    correlationId: ctx.correlationId,
    severity: "warning",
  });
  return { status: "succeeded", key_id: keyId };
}

/**
 * Forces a stuck instance to 'degraded' (the owner's next retry is no longer
 * blocked) or 'failed' (recovery is not possible).
 */
export async function recoverInstance(ctx: OperatorContext, body: Record<string, unknown>) {
  const instanceId = requireString(body, "instance_id");
  const targetStatus = typeof body.target_status === "string" ? body.target_status : "degraded";
  if (!["degraded", "failed"].includes(targetStatus)) {
    throw new OperatorInputError("target_status must be 'degraded' or 'failed'");
  }
  const instance = must(
    await ctx.supabase
      .from("remote_instances")
      .select("id, owner_user_id, status, generation")
      .eq("id", instanceId)
      .maybeSingle(),
    "read instance",
  );
  if (!instance) throw new OperatorInputError("Instance not found", 404);
  if (!RECOVERABLE_STATUSES.includes(instance.status)) {
    throw new OperatorInputError(
      `Instance is in status '${instance.status}', which is not treated as a stuck/recoverable state`,
      409,
    );
  }
  must(
    await ctx.supabase
      .from("remote_instances")
      .update({ status: targetStatus, updated_at: new Date().toISOString() })
      .eq("id", instanceId),
    "update instance status",
  );
  await recordAuditEvent(ctx.supabase, {
    ownerUserId: instance.owner_user_id,
    instanceId: instance.id,
    eventType: "admin_instance_recovered",
    detail: {
      from_status: instance.status,
      to_status: targetStatus,
      generation: instance.generation,
      operator_id: ctx.operatorId,
      reason: typeof body.reason === "string" ? body.reason : "manual admin recovery",
    },
    correlationId: ctx.correlationId,
    severity: "warning",
  });
  return { status: "succeeded", instance_id: instanceId, from_status: instance.status, to_status: targetStatus };
}

/** Reads remote_recent_failures (017_remote_ssh_observability.sql). */
export async function listRecentFailures(ctx: OperatorContext, body: Record<string, unknown>) {
  const limit = typeof body.limit === "number" && body.limit > 0 && body.limit <= 500 ? body.limit : 100;
  const failures = must(
    await ctx.supabase
      .from("remote_recent_failures")
      .select("*")
      .order("created_at", { ascending: false })
      .limit(limit),
    "read remote_recent_failures",
  );
  return { failures: failures ?? [] };
}

export async function pruneAuditEvents(ctx: OperatorContext) {
  return { deleted_count: await rpc(ctx.supabase, "prune_remote_audit_events") };
}
