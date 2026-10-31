// The remote-instance actions that change a user's managed instance:
// status, ensure, wake, reprovision, delete and resize. remote-instance/lib.ts
// gates and dispatches; each action here returns a Reply.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { isRegionCode, isSizePreset, type RegionCode, type SizePreset } from "./catalog.ts";
import { CURRENT_MANIFEST_VERSION } from "./boot-manifest.ts";
import type { ProviderKind, SandboxProvider } from "./provider.ts";
import {
  type ByosActionDeps,
  type ByosCredentialStore,
  type ByosPlanStore,
  parsePlacementChoice,
  planByosPlacement,
} from "./byos.ts";
import type { MachineTypeRow } from "./migration-engine.ts";
import { establishSshTrust, reensureServicesAfterWake, runBootManifest } from "./machine-setup.ts";
import {
  cancelOpenMigration,
  createActivePlacement,
  createMigration,
  deleteInstanceMachines,
  getMachineType,
  getOpenMigration,
  listInstanceRepositories,
  listLivePlacements,
  resolveDefaultMachineType,
  setPlacementState,
  updatePlacement,
} from "./placement-store.ts";
import { parsePreflightOutput, type PreflightReport, preflightCommand } from "./preflight.ts";
import { decideResize, parseResizeRequest } from "./resize.ts";
import { wakeIfStopped } from "./wake.ts";
import { INLINE_BUDGET_MS, runLeasedMigrations } from "./migration-runner.ts";
import { migrationSummary } from "./migration-resume.ts";
import { auditFor, type RemoteAuditEventType, startTimer } from "./audit.ts";
import type { SpendStatus } from "../billing/spend-cap.ts";
import {
  beginOperation,
  completeOperation,
  createProvisioningInstance,
  findExistingOperation,
  getInstanceForOwner,
  type InstanceRow,
  type OperationRow,
  type OperationType,
  previousHostKeyFingerprint,
  recordManagedEndpoint,
  updateInstance,
} from "./instance-store.ts";
import { logWithCorrelation } from "./correlation.ts";
import { MANAGED_SSHD_HOST, MANAGED_SSHD_PORT } from "./ssh-vm-config.ts";

const MANAGED_SSH_USERNAME = "treq";
const DEFAULT_TIER = "s";

export interface Reply {
  status: number;
  body: unknown;
}

const ok = (body: unknown): Reply => ({ status: 200, body });

/** Thrown for a request the caller got wrong; lib.ts answers it with `status`. */
export class HttpError extends Error {
  constructor(message: string, readonly status: number, readonly code: string | undefined = "validation_error") {
    super(message);
  }
}

export interface RemoteInstanceDeps {
  /** Service-role client. */
  supabase: SupabaseClient;
  ownerUserId: string;
  correlationId: string;
  /** The adapter for a placement (`placementProviders` in production). */
  provider: (kind: ProviderKind, placementId?: string | null) => SandboxProvider;
  /** The in-memory stub: there is no machine to exec on. */
  stub: boolean;
  /** has_pro for the caller. */
  hasPro: () => Promise<boolean>;
  /** The caller's monthly spend (get_spend_status_for); ensure/wake/resize get 402 when capped. */
  spendStatus?: () => Promise<SpendStatus | null>;
  /** Bring your own sandbox (byos.ts). Absent: every BYOS action answers byos_disabled. */
  byos?: ByosActionDeps & { store: ByosCredentialStore & ByosPlanStore };
}

export const BYOS_DISABLED: Reply = {
  status: 404,
  body: { error: "Bring your own sandbox is not available", code: "byos_disabled" },
};

// Clients (desktop's and mobile's ManagedInstanceRecord) read the id as
// `instance_id`, but the row's primary key is `id`.
function serializeInstance(row: InstanceRow | null) {
  if (!row) return null;
  const { id, ...rest } = row;
  return { instance_id: id, ...rest };
}

const kindOf = (instance: InstanceRow) => instance.provider_kind as ProviderKind;
const providerOf = (deps: RemoteInstanceDeps, instance: InstanceRow) =>
  deps.provider(kindOf(instance), instance.active_placement_id);

// `size_preset` predates tiers; it stays filled for existing readers.
function sizePresetForTier(tierId: string): SizePreset {
  return tierId === "s" ? "small" : tierId === "m" ? "medium" : "large";
}

// Records a status on the instance. With a placement, it goes through
// set_placement_state so billing follows and the instance row mirrors it.
async function setInstanceStatus(
  supabase: SupabaseClient,
  instance: InstanceRow,
  status: string,
  fields: Record<string, unknown> = {},
): Promise<void> {
  if (instance.active_placement_id) {
    await setPlacementState(supabase, instance.active_placement_id, status);
    if (Object.keys(fields).length > 0) await updateInstance(supabase, instance.id, fields);
  } else {
    await updateInstance(supabase, instance.id, { status, ...fields });
  }
}

function requireIdempotencyKey(body: Record<string, unknown>): string {
  const key = body.idempotency_key;
  if (typeof key !== "string" || key.length === 0) {
    throw new HttpError("idempotency_key is required", 400, undefined);
  }
  return key;
}

// Verifies ownership server-side rather than trusting a client-supplied id.
// instance_id may be omitted: a user has at most one managed instance.
async function requireOwnedInstance(deps: RemoteInstanceDeps, suppliedInstanceId: unknown): Promise<InstanceRow> {
  const instance = await getInstanceForOwner(deps.supabase, deps.ownerUserId);
  if (!instance) throw new HttpError("No managed instance for this user", 404);
  if (typeof suppliedInstanceId === "string" && suppliedInstanceId !== instance.id) {
    throw new HttpError("Instance does not belong to this user", 403);
  }
  return instance;
}

function requireProvisioned(instance: InstanceRow): string {
  if (!instance.provider_resource_id) throw new HttpError("Instance has no provider resource yet", 409, undefined);
  return instance.provider_resource_id;
}

const replay = (op: OperationRow): Reply => ok({ operation_id: op.id, status: op.status });

function tierUnavailable(tierId: string, region: string): Reply {
  return {
    status: 400,
    body: { error: `No machine type is available for tier '${tierId}' in ${region}`, code: "tier_unavailable" },
  };
}

function byosPlanDeps(byos: NonNullable<RemoteInstanceDeps["byos"]>) {
  return {
    canByos: byos.canByos,
    credentialFor: (kind: ProviderKind) => byos.store.credentialFor(kind),
    machineTypes: (kind: ProviderKind) => byos.store.machineTypes(kind),
  };
}

/**
 * Records an operation and its `<event>_requested`, `_succeeded` and
 * `_failed` audit events around `work`. On failure, `onFailure` runs before
 * the operation is marked failed, then the error rethrows.
 */
async function tracked(
  deps: RemoteInstanceDeps,
  instance: InstanceRow,
  idempotencyKey: string,
  op: { type: OperationType; event: string; detail?: Record<string, unknown> },
  work: (opId: string) => Promise<{ providerRequestId?: string; detail?: Record<string, unknown> }>,
  onFailure?: () => Promise<void>,
): Promise<string> {
  const { supabase, ownerUserId, correlationId } = deps;
  const elapsed = startTimer();
  const row = await beginOperation(supabase, {
    ownerUserId,
    instanceId: instance.id,
    operationType: op.type,
    idempotencyKey,
  });
  const audit = auditFor(supabase, { ownerUserId, instanceId: instance.id, correlationId });
  const event = (suffix: string) => `${op.event}_${suffix}` as RemoteAuditEventType;
  await audit(event("requested"), { detail: op.detail, idempotencyKey });
  try {
    const done = await work(row.id);
    await completeOperation(supabase, row.id, { status: "succeeded", providerRequestId: done.providerRequestId });
    await audit(event("succeeded"), {
      detail: done.detail,
      providerRequestId: done.providerRequestId,
      durationMs: elapsed(),
    });
    return row.id;
  } catch (err) {
    await onFailure?.();
    const errorMessage = (err as Error).message;
    await completeOperation(supabase, row.id, { status: "failed", errorMessage });
    await audit(event("failed"), { detail: { error: errorMessage }, durationMs: elapsed() });
    throw err;
  }
}

export async function handleStatus(_body: Record<string, unknown>, deps: RemoteInstanceDeps): Promise<Reply> {
  const { supabase, ownerUserId } = deps;
  let instance = await getInstanceForOwner(supabase, ownerUserId);
  if (!instance) return ok({ instance: null, endpoint: null });

  if (instance.provider_resource_id && instance.status !== "deleted") {
    try {
      const observed = await providerOf(deps, instance).getInstance(instance.provider_resource_id);
      await setInstanceStatus(supabase, instance, observed.state, {
        ready_at: observed.state === "ready" ? (instance.ready_at ?? new Date().toISOString()) : instance.ready_at,
      });
      instance = (await getInstanceForOwner(supabase, ownerUserId)) ?? instance;
    } catch (err) {
      logWithCorrelation(deps.correlationId, "warn", `could not refresh machine status: ${(err as Error).message}`);
    }
  }

  let endpoint = null;
  if (instance.endpoint_id) {
    const { data } = await supabase
      .from("remote_endpoints")
      .select("id, hostname, port, username, source")
      .eq("id", instance.endpoint_id)
      .maybeSingle();
    endpoint = data ?? null;
  }
  // An open switch, so the app can prompt for a parked one.
  const migration = migrationSummary(await getOpenMigration(supabase, instance.id));
  return ok({ instance: serializeInstance(instance), endpoint, migration });
}

export async function handleEnsure(body: Record<string, unknown>, deps: RemoteInstanceDeps): Promise<Reply> {
  const { supabase, ownerUserId, correlationId } = deps;
  const idempotencyKey = requireIdempotencyKey(body);
  const region: RegionCode = isRegionCode(body.region) ? body.region : "us_east";
  if (body.tier_id !== undefined && (typeof body.tier_id !== "string" || body.tier_id.length === 0)) {
    throw new HttpError("tier_id must be a string", 400);
  }
  const tierId = (body.tier_id as string | undefined) ?? DEFAULT_TIER;
  const choice = parsePlacementChoice(body);
  if (!choice.ok) throw new HttpError(choice.error, 400);

  const existingOp = await findExistingOperation(supabase, ownerUserId, idempotencyKey);
  if (existingOp) {
    // Repeated request with the same key: never create a second instance.
    const instance = await getInstanceForOwner(supabase, ownerUserId);
    return ok({ operation_id: existingOp.id, status: existingOp.status, instance: serializeInstance(instance) });
  }

  const existingInstance = await getInstanceForOwner(supabase, ownerUserId);
  if (existingInstance && existingInstance.status !== "failed") {
    // One managed instance per user: ensure is a no-op once provisioned,
    // whatever the key, so a second "first open" never makes a second VM.
    const op = await beginOperation(supabase, {
      ownerUserId,
      instanceId: existingInstance.id,
      operationType: "provision",
      idempotencyKey,
    });
    await completeOperation(supabase, op.id, { status: "succeeded" });
    return ok({ operation_id: op.id, status: "succeeded", instance: serializeInstance(existingInstance) });
  }

  // A retry of a failed instance keeps its machine type; a new one gets the
  // cheapest admitted type for the tier and region, or in BYOS mode the
  // chosen provider's type for the tier on the user's own account.
  let machineType: MachineTypeRow | null;
  let byosCredentialId: string | null = null;
  if (existingInstance?.machine_type_id) {
    machineType = await getMachineType(supabase, existingInstance.machine_type_id);
  } else if (choice.value.mode === "byos") {
    if (!deps.byos) return BYOS_DISABLED;
    const plan = await planByosPlacement(byosPlanDeps(deps.byos), choice.value, tierId, region);
    if (!plan.ok) return { status: plan.status, body: { error: plan.code, code: plan.code } };
    machineType = plan.machineType;
    byosCredentialId = plan.credentialId;
  } else {
    machineType = await resolveDefaultMachineType(supabase, tierId, region);
  }
  if (!machineType) return tierUnavailable(tierId, region);
  const mt = machineType;
  const sizePreset: SizePreset = isSizePreset(body.size_preset) ? body.size_preset : sizePresetForTier(mt.tier_id);

  let instance = existingInstance ?? await createProvisioningInstance(supabase, {
    ownerUserId,
    region,
    sizePreset,
    manifestVersion: CURRENT_MANIFEST_VERSION,
    providerKind: mt.provider_kind,
    tierId: mt.tier_id,
    machineTypeId: mt.id,
  });
  if (!instance.active_placement_id) {
    const placement = await createActivePlacement(supabase, {
      instanceId: instance.id,
      ownerUserId,
      providerKind: kindOf(instance),
      machineTypeId: mt.id,
      region: instance.region,
      credentialId: byosCredentialId,
    });
    instance = { ...instance, active_placement_id: placement.id };
  }
  if (instance.status === "failed") await setInstanceStatus(supabase, instance, "provisioning");

  const target = instance;
  const opId = await tracked(
    deps,
    target,
    idempotencyKey,
    {
      type: "provision",
      event: "instance_create",
      detail: {
        region,
        size_preset: sizePreset,
        tier_id: mt.tier_id,
        machine_type_id: mt.id,
        provider_kind: mt.provider_kind,
        credential_source: byosCredentialId ? "byos" : "managed",
        manifest_version: CURRENT_MANIFEST_VERSION,
      },
    },
    async () => {
      const provider = providerOf(deps, target);
      const created = await provider.createInstance({
        ownerUserId,
        region,
        sizePreset,
        manifestVersion: CURRENT_MANIFEST_VERSION,
        idempotencyKey,
        machineType: mt.vendor_type,
        placementId: target.active_placement_id ?? undefined,
      });
      const resourceId = created.providerResourceId;
      if (target.active_placement_id) {
        await updatePlacement(supabase, target.active_placement_id, { provider_resource_id: resourceId });
      }
      await updateInstance(supabase, target.id, { provider_resource_id: resourceId });
      await setInstanceStatus(supabase, target, "bootstrapping");
      await runBootManifest(provider, resourceId);

      const observed = await provider.getInstance(resourceId);
      await setInstanceStatus(supabase, target, observed.state, {
        ready_at: observed.state === "ready" ? new Date().toISOString() : null,
      });

      // The endpoint names sshd as the relay sees it inside the machine
      // (loopback). Clients never dial it: they go through the relay
      // transport that `issue_certificate` returns.
      const endpointId = await recordManagedEndpoint(supabase, {
        ownerUserId,
        instanceId: target.id,
        hostname: MANAGED_SSHD_HOST,
        port: MANAGED_SSHD_PORT,
        username: MANAGED_SSH_USERNAME,
        existingEndpointId: null,
      });
      await updateInstance(supabase, target.id, { endpoint_id: endpointId });
      await establishSshTrust(supabase, provider, {
        ownerUserId,
        instanceId: target.id,
        endpointId,
        providerResourceId: resourceId,
        generation: 0,
        correlationId,
        stub: deps.stub,
        placementId: target.active_placement_id,
      });
      return {
        providerRequestId: resourceId,
        detail: { provider_resource_id: resourceId, observed_state: observed.state },
      };
    },
    () => setInstanceStatus(supabase, target, "failed"),
  );
  const refreshed = await getInstanceForOwner(supabase, ownerUserId);
  return ok({ operation_id: opId, status: "succeeded", instance: serializeInstance(refreshed) });
}

export async function handleWake(body: Record<string, unknown>, deps: RemoteInstanceDeps): Promise<Reply> {
  const { supabase, ownerUserId } = deps;
  const idempotencyKey = requireIdempotencyKey(body);
  const instance = await requireOwnedInstance(deps, body.instance_id);
  const resourceId = requireProvisioned(instance);
  const existingOp = await findExistingOperation(supabase, ownerUserId, idempotencyKey);
  if (existingOp) return replay(existingOp);

  const opId = await tracked(deps, instance, idempotencyKey, { type: "wake", event: "instance_wake" }, async () => {
    await setInstanceStatus(supabase, instance, "waking");
    const provider = providerOf(deps, instance);
    await provider.wakeInstance(resourceId);
    if (!deps.stub) await reensureServicesAfterWake(provider, resourceId, instance.active_placement_id ?? null);
    const observed = await provider.getInstance(resourceId);
    await setInstanceStatus(supabase, instance, observed.state, {
      ready_at: observed.state === "ready" ? new Date().toISOString() : instance.ready_at,
    });
    return { detail: { observed_state: observed.state } };
  });
  return ok({ operation_id: opId, status: "succeeded" });
}

export async function handleReprovision(body: Record<string, unknown>, deps: RemoteInstanceDeps): Promise<Reply> {
  const { supabase, ownerUserId, correlationId } = deps;
  const idempotencyKey = requireIdempotencyKey(body);
  const instance = await requireOwnedInstance(deps, body.instance_id);
  const resourceId = requireProvisioned(instance);
  const region: RegionCode = isRegionCode(body.region) ? body.region : instance.region;
  const sizePreset: SizePreset = isSizePreset(body.size_preset) ? body.size_preset : instance.size_preset;
  const existingOp = await findExistingOperation(supabase, ownerUserId, idempotencyKey);
  if (existingOp) return replay(existingOp);

  // Repair is in place. The generation identifies the durable provider
  // resource and changes only when that identity changes.
  const generation = instance.generation;
  const opId = await tracked(
    deps,
    instance,
    idempotencyKey,
    {
      type: "reprovision",
      event: "instance_replace",
      detail: { region, size_preset: sizePreset, from_generation: generation, to_generation: generation },
    },
    async () => {
      await setInstanceStatus(supabase, instance, "reprovisioning");
      const provider = providerOf(deps, instance);
      const replaced = await provider.replaceInstance({
        providerResourceId: resourceId,
        region,
        sizePreset,
        manifestVersion: CURRENT_MANIFEST_VERSION,
        idempotencyKey,
      });
      const newResourceId = replaced.providerResourceId;
      await runBootManifest(provider, newResourceId, "repair");
      const observed = await provider.getInstance(newResourceId);
      await setInstanceStatus(supabase, instance, observed.state, {
        generation,
        size_preset: sizePreset,
        image_manifest_version: CURRENT_MANIFEST_VERSION,
        ready_at: observed.state === "ready" ? new Date().toISOString() : null,
      });

      const endpointId = await recordManagedEndpoint(supabase, {
        ownerUserId,
        instanceId: instance.id,
        hostname: MANAGED_SSHD_HOST,
        port: MANAGED_SSHD_PORT,
        username: MANAGED_SSH_USERNAME,
        existingEndpointId: instance.endpoint_id,
      });
      if (!instance.endpoint_id) await updateInstance(supabase, instance.id, { endpoint_id: endpointId });
      const previousFingerprint = instance.endpoint_id
        ? await previousHostKeyFingerprint(supabase, instance.endpoint_id)
        : null;

      // A replaced VM starts from the base image, so read its host key and
      // reinstall CA trust. The host_key_rotated event is the rotation record
      // the PRD asks for: old fingerprint, generation and resource id.
      await establishSshTrust(supabase, provider, {
        ownerUserId,
        instanceId: instance.id,
        endpointId,
        providerResourceId: newResourceId,
        generation,
        correlationId,
        stub: deps.stub,
        placementId: instance.active_placement_id,
      });
      await auditFor(supabase, { ownerUserId, instanceId: instance.id, correlationId })("host_key_rotated", {
        endpointId,
        detail: {
          previous_fingerprint: previousFingerprint,
          generation,
          provider_resource_id: newResourceId,
          initiating_principal: ownerUserId,
        },
      });
      return { providerRequestId: newResourceId, detail: { generation, observed_state: observed.state } };
    },
  );
  const refreshed = await getInstanceForOwner(supabase, ownerUserId);
  return ok({ operation_id: opId, status: "succeeded", instance: serializeInstance(refreshed) });
}

export async function handleDelete(body: Record<string, unknown>, deps: RemoteInstanceDeps): Promise<Reply> {
  const { supabase, ownerUserId } = deps;
  const idempotencyKey = requireIdempotencyKey(body);
  // Replay before the ownership lookup: once the delete succeeded the owner
  // has no live instance, and a retry must still return the stored result.
  const existingOp = await findExistingOperation(supabase, ownerUserId, idempotencyKey);
  if (existingOp) return replay(existingOp);
  const instance = await requireOwnedInstance(deps, body.instance_id);

  const opId = await tracked(
    deps,
    instance,
    idempotencyKey,
    { type: "delete", event: "instance_delete" },
    async () => {
      await setInstanceStatus(supabase, instance, "deleting");
      // Cancel an open switch first, in one statement whatever its state, so
      // no worker creates a target after us. A worker mid-create sees the
      // cancel and deletes its own target (migration-engine.ts).
      await cancelOpenMigration(supabase, instance.id, "instance_deleted");
      const placements = await listLivePlacements(supabase, instance.id);
      await deleteInstanceMachines(deps.provider, instance, placements);
      // Closes billing on the active placement, then retires them all.
      await setInstanceStatus(supabase, instance, "deleted", { endpoint_id: null });
      for (const p of placements) {
        if (p.id !== instance.active_placement_id) await setPlacementState(supabase, p.id, "deleted");
        await updatePlacement(supabase, p.id, { role: "dead", retired_at: new Date().toISOString() });
      }
      return {};
    },
    // 'deleting' keeps billing open (046_placements_and_usage.sql). The machine is
    // still there, so the instance goes back to the state it was in.
    () => setInstanceStatus(supabase, instance, instance.status).catch(() => {}),
  );
  return ok({ operation_id: opId, status: "succeeded" });
}

// Asks the source machine what a fresh start would lose (§6 preflight).
async function runSourcePreflight(deps: RemoteInstanceDeps, instance: InstanceRow): Promise<PreflightReport> {
  const repos = await listInstanceRepositories(deps.supabase, instance.id);
  const paths = repos.map((r) => r.remote_path);
  if (paths.length === 0 || !instance.provider_resource_id) return parsePreflightOutput("", paths);
  const provider = providerOf(deps, instance);
  await wakeIfStopped(provider, instance.provider_resource_id, instance.active_placement_id ?? null, {
    setState: (id, state) => setPlacementState(deps.supabase, id, state),
    afterWake: reensureServicesAfterWake,
  });
  const out = await provider.execOnMachine(instance.provider_resource_id, preflightCommand(paths), 120);
  return parsePreflightOutput(out.exitCode === 0 ? out.stdout : "", paths);
}

// Moves the instance to the tier's default machine type with a fresh
// migration (docs/design/sandbox-provider-adapter.md §6). Runs the engine
// inline for up to INLINE_BUDGET_MS; the hourly admin job finishes it.
export async function handleResize(body: Record<string, unknown>, deps: RemoteInstanceDeps): Promise<Reply> {
  const { supabase, ownerUserId, correlationId } = deps;
  const parsed = parseResizeRequest(body);
  if (!parsed.ok) throw new HttpError(parsed.error, 400);
  const req = parsed.value;

  const existingOp = await findExistingOperation(supabase, ownerUserId, req.idempotencyKey);
  if (existingOp) {
    return ok({ operation_id: existingOp.id, status: existingOp.status, migration_id: existingOp.provider_request_id });
  }

  const instance = await requireOwnedInstance(deps, req.instanceId);
  if (!instance.active_placement_id || !instance.provider_resource_id) {
    throw new HttpError("Instance is not provisioned yet", 409, "not_ready");
  }
  // A BYOS workspace resizes on its own provider account.
  let resolved: MachineTypeRow | null;
  if (instance.credential_source === "byos") {
    if (!deps.byos) return BYOS_DISABLED;
    const plan = await planByosPlacement(
      byosPlanDeps(deps.byos),
      { mode: "byos", kind: kindOf(instance) },
      req.tierId,
      instance.region,
    );
    if (!plan.ok) return { status: plan.status, body: { error: plan.code, code: plan.code } };
    resolved = plan.machineType;
  } else {
    resolved = await resolveDefaultMachineType(supabase, req.tierId, instance.region);
  }
  if (!resolved) return tierUnavailable(req.tierId, instance.region);
  const target = resolved;

  const open = await getOpenMigration(supabase, instance.id);
  const decide = (openMigrationId: string | null, preflight: PreflightReport | null) =>
    decideResize({
      current: instance.machine_type_id,
      target: target.id,
      openMigrationId,
      preflight,
      ack: req.acknowledgedDataLoss,
    });
  const early = decide(open?.id ?? null, null);
  if (early.kind === "in_progress") {
    return {
      status: 409,
      body: {
        error: "A switch is already running",
        code: "migration_in_progress",
        migration_id: early.migrationId,
        migration: migrationSummary(open),
      },
    };
  }
  if (early.kind === "unchanged") return ok({ status: "unchanged", instance: serializeInstance(instance) });

  const preflight = await runSourcePreflight(deps, instance);
  if (decide(null, preflight).kind === "needs_ack") {
    return { status: 409, body: { error: "The sandbox has work that is not pushed", code: "unpushed_work", preflight } };
  }

  const op = await beginOperation(supabase, {
    ownerUserId,
    instanceId: instance.id,
    operationType: "resize",
    idempotencyKey: req.idempotencyKey,
  });
  const migration = await createMigration(supabase, {
    instanceId: instance.id,
    ownerUserId,
    sourcePlacementId: instance.active_placement_id,
    targetMachineTypeId: target.id,
    reason: "user_resize",
    preflight,
    acknowledgedDataLoss: req.acknowledgedDataLoss,
    requestedBy: ownerUserId,
  });
  if (!migration) {
    await completeOperation(supabase, op.id, { status: "failed", errorMessage: "migration_in_progress" });
    return { status: 409, body: { error: "A switch is already running", code: "migration_in_progress" } };
  }
  await completeOperation(supabase, op.id, { status: "succeeded", providerRequestId: migration.id });
  await auditFor(supabase, { ownerUserId, instanceId: instance.id, correlationId })("migration_requested", {
    detail: {
      migration_id: migration.id,
      reason: "user_resize",
      from_machine_type_id: instance.machine_type_id,
      target_machine_type_id: target.id,
      acknowledged_data_loss: req.acknowledgedDataLoss,
    },
    idempotencyKey: req.idempotencyKey,
  });

  const run = await runLeasedMigrations({
    supabase,
    providers: deps.provider,
    stub: deps.stub,
    correlationId,
    leaseOwner: `resize:${correlationId}`,
    budgetMs: INLINE_BUDGET_MS,
    limit: 1,
    migrationId: migration.id,
  });
  const state = run.migrations[0]?.state ?? migration.state;
  const refreshed = await getInstanceForOwner(supabase, ownerUserId);
  return {
    status: state === "completed" ? 200 : 202,
    body: {
      operation_id: op.id,
      status: "accepted",
      migration_id: migration.id,
      migration_state: state,
      preflight,
      instance: serializeInstance(refreshed),
    },
  };
}
