// Supabase access for placements, the machine catalog and sandbox
// migrations (044_sandbox_catalog.sql, 047_sandbox_migrations.sql).
// Status changes on a placement always go through `set_placement_state`,
// which also mirrors an active placement onto remote_instances and opens or
// closes its usage intervals, so billing follows what we did.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import type {
  MachineTypeRow,
  MigrationInstance,
  MigrationRow,
  MigrationStore,
  PlacementRow,
  RepositoryRow,
} from "./migration-engine.ts";
import { ProviderError, type ProviderKind, type SandboxProvider } from "./provider.ts";
import { must } from "./db.ts";

function fail(what: string, error: { message: string } | null): void {
  must({ data: null, error }, what);
}

export async function setPlacementState(
  supabase: SupabaseClient,
  placementId: string,
  state: string,
  diskGb?: number,
  at?: string,
): Promise<void> {
  must(await supabase.rpc("set_placement_state", {
    p_placement_id: placementId,
    p_state: state,
    p_disk_gb: diskGb ?? null,
    p_at: at ?? null,
  }), "set placement state");
}

export async function getPlacement(supabase: SupabaseClient, id: string): Promise<PlacementRow | null> {
  return must(await supabase.from("remote_instance_placements").select("*").eq("id", id).maybeSingle(), "read placement") as PlacementRow | null;
}

export async function listLivePlacements(supabase: SupabaseClient, instanceId: string): Promise<PlacementRow[]> {
  return (must(await supabase
    .from("remote_instance_placements")
    .select("*")
    .eq("instance_id", instanceId)
    .neq("role", "dead"), "list placements") ?? []) as PlacementRow[];
}

export async function updatePlacement(
  supabase: SupabaseClient,
  id: string,
  fields: Partial<Pick<PlacementRow, "provider_resource_id" | "role">> & { retired_at?: string },
): Promise<void> {
  must(await supabase.from("remote_instance_placements").update(fields).eq("id", id), "update placement");
}

/** Creates the instance's active placement and points the instance at it. */
export async function createActivePlacement(
  supabase: SupabaseClient,
  p: {
    instanceId: string;
    ownerUserId: string;
    providerKind: ProviderKind;
    machineTypeId: string;
    region: string;
    /** Set for BYOS: the owner's credential for this provider. */
    credentialId?: string | null;
  },
): Promise<PlacementRow> {
  const data = must(await supabase
    .from("remote_instance_placements")
    .insert({
      instance_id: p.instanceId,
      owner_user_id: p.ownerUserId,
      provider_kind: p.providerKind,
      machine_type_id: p.machineTypeId,
      region: p.region,
      state: "provisioning",
      role: "active",
      credential_source: p.credentialId ? "byos" : "managed",
      credential_id: p.credentialId ?? null,
    })
    .select()
    .single(), "create placement");
  must(
    await supabase.from("remote_instances").update({ active_placement_id: data.id }).eq("id", p.instanceId),
    "link placement",
  );
  return data as PlacementRow;
}

export async function getMachineType(supabase: SupabaseClient, id: string): Promise<MachineTypeRow | null> {
  return must(await supabase
    .from("provider_machine_types")
    .select("id, provider_kind, vendor_type, tier_id")
    .eq("id", id)
    .maybeSingle(), "read machine type") as MachineTypeRow | null;
}

/** The Pro default machine type for a tier and region, or null when none is admitted. */
export async function resolveDefaultMachineType(
  supabase: SupabaseClient,
  tierId: string,
  region: string,
): Promise<MachineTypeRow | null> {
  const data = must(await supabase
    .from("pro_provider_defaults")
    .select("machine_type_id")
    .eq("tier_id", tierId)
    .eq("region", region)
    .maybeSingle(), "read provider defaults");
  if (!data) return null;
  return await getMachineType(supabase, data.machine_type_id as string);
}

export async function getOpenMigration(
  supabase: SupabaseClient,
  instanceId: string,
): Promise<MigrationRow | null> {
  return must(await supabase
    .from("sandbox_migrations")
    .select("*")
    .eq("instance_id", instanceId)
    .not("state", "in", "(completed,rolled_back,failed,cancelled)")
    .maybeSingle(), "read open migration") as MigrationRow | null;
}

export async function createMigration(
  supabase: SupabaseClient,
  p: {
    instanceId: string;
    ownerUserId: string;
    sourcePlacementId: string;
    targetMachineTypeId: string;
    reason: "user_resize" | "drain" | "retire" | "cheaper_default" | "restart";
    preflight?: unknown;
    acknowledgedDataLoss?: boolean;
    requestedBy?: string | null;
    deadline?: string | null;
  },
): Promise<MigrationRow | null> {
  const { data, error } = await supabase
    .from("sandbox_migrations")
    .insert({
      instance_id: p.instanceId,
      owner_user_id: p.ownerUserId,
      source_placement_id: p.sourcePlacementId,
      target_machine_type_id: p.targetMachineTypeId,
      reason: p.reason,
      preflight: p.preflight ?? null,
      acknowledged_data_loss: p.acknowledgedDataLoss ?? false,
      requested_by: p.requestedBy ?? null,
      deadline: p.deadline ?? null,
    })
    .select()
    .single();
  // One open migration per instance: losing that race is not an error.
  if (error?.code === "23505") return null;
  fail("create migration", error);
  return data as MigrationRow;
}

export async function listInstanceRepositories(
  supabase: SupabaseClient,
  instanceId: string,
): Promise<RepositoryRow[]> {
  const endpoints = must(
    await supabase.from("remote_endpoints").select("id").eq("instance_id", instanceId),
    "list endpoints",
  );
  const ids = (endpoints ?? []).map((e) => e.id as string);
  if (ids.length === 0) return [];
  return (must(await supabase
    .from("remote_repositories")
    .select("id, remote_path, remote_url")
    .in("endpoint_id", ids)
    .order("remote_path"), "list repositories") ?? []) as RepositoryRow[];
}

export async function claimMigrations(
  supabase: SupabaseClient,
  p: { owner: string; leaseSec: number; limit: number; migrationId?: string },
): Promise<string[]> {
  const data = must(await supabase.rpc("claim_sandbox_migrations", {
    p_owner: p.owner,
    p_lease_sec: p.leaseSec,
    p_limit: p.limit,
    p_migration_id: p.migrationId ?? null,
  }), "claim migrations");
  return ((data ?? []) as Array<{ migration_id: string }>).map((r) => r.migration_id);
}

export async function releaseMigration(supabase: SupabaseClient, migrationId: string, owner: string): Promise<void> {
  must(await supabase.rpc("release_sandbox_migration", { p_migration_id: migrationId, p_owner: owner }), "release migration");
}

/** Extends `owner`'s lease to `seconds` from now. False when another worker holds it. */
export async function renewMigrationLease(
  supabase: SupabaseClient,
  migrationId: string,
  owner: string,
  seconds: number,
): Promise<boolean> {
  const data = must(await supabase.rpc("renew_sandbox_migration_lease", {
    p_migration_id: migrationId,
    p_owner: owner,
    p_lease_sec: seconds,
  }), "renew migration lease");
  return data === true;
}

/**
 * Cancels the instance's open migration, whatever state it is in, in one
 * statement (cancel_open_migration), so a worker advancing it concurrently
 * cannot make the cancel miss. Returns the cancelled id, or null.
 */
export async function cancelOpenMigration(
  supabase: SupabaseClient,
  instanceId: string,
  reason: string,
): Promise<string | null> {
  const data = must(await supabase.rpc("cancel_open_migration", {
    p_instance_id: instanceId,
    p_detail: { reason },
  }), "cancel open migration");
  return (data as string | null) ?? null;
}

export async function advanceMigration(
  supabase: SupabaseClient,
  id: string,
  expected: string,
  next: string,
  detail?: Record<string, unknown>,
): Promise<boolean> {
  const data = must(await supabase.rpc("advance_migration", {
    p_migration_id: id,
    p_expected_state: expected,
    p_next_state: next,
    p_detail: detail ?? null,
  }), "advance migration");
  return data === true;
}

export async function cutoverMigration(
  supabase: SupabaseClient,
  migrationId: string,
  targetPlacementId: string,
): Promise<number> {
  const data = must(await supabase.rpc("cutover_migration", {
    p_migration_id: migrationId,
    p_target_placement_id: targetPlacementId,
  }), "cut over");
  return Number(data);
}

/**
 * The engine's store. With `leaseOwner`, every state change also requires
 * that worker to still hold the migration's lease (advance_migration_leased).
 */
export function supabaseMigrationStore(supabase: SupabaseClient, opts: { leaseOwner?: string } = {}): MigrationStore {
  const findCandidate = async (instanceId: string) => {
    return must(await supabase
      .from("remote_instance_placements")
      .select("*")
      .eq("instance_id", instanceId)
      .eq("role", "candidate")
      .order("created_at", { ascending: false })
      .limit(1)
      .maybeSingle(), "read candidate placement") as PlacementRow | null;
  };
  return {
    async getMigration(id) {
      return must(await supabase.from("sandbox_migrations").select("*").eq("id", id).maybeSingle(), "read migration") as MigrationRow | null;
    },
    async advance(id, expected, next, detail) {
      if (!opts.leaseOwner) return await advanceMigration(supabase, id, expected, next, detail);
      const data = must(await supabase.rpc("advance_migration_leased", {
        p_migration_id: id,
        p_expected_state: expected,
        p_next_state: next,
        p_detail: detail ?? null,
        p_owner: opts.leaseOwner,
      }), "advance migration");
      return data === true;
    },
    async setMigrationFields(id, fields) {
      must(await supabase
        .from("sandbox_migrations")
        .update({ ...fields, updated_at: new Date().toISOString() })
        .eq("id", id), "update migration");
    },
    async getInstance(id) {
      const data = must(await supabase
        .from("remote_instances")
        .select("id, owner_user_id, region, fence, active_placement_id")
        .eq("id", id)
        .maybeSingle(), "read instance");
      return data ? { ...(data as MigrationInstance), fence: Number(data.fence) } : null;
    },
    getPlacement: (id) => getPlacement(supabase, id),
    findCandidatePlacement: findCandidate,
    async createCandidatePlacement(p) {
      const { data, error } = await supabase
        .from("remote_instance_placements")
        .insert({
          instance_id: p.instanceId,
          owner_user_id: p.ownerUserId,
          provider_kind: p.providerKind,
          machine_type_id: p.machineTypeId,
          region: p.region,
          state: "provisioning",
          role: "candidate",
          credential_source: p.credentialSource ?? "managed",
          credential_id: p.credentialId ?? null,
        })
        .select()
        .single();
      // One candidate per instance (047_sandbox_migrations.sql): another
      // worker made it first, so use theirs.
      if (error?.code === "23505") {
        const existing = await findCandidate(p.instanceId);
        if (existing) return existing;
      }
      fail("create candidate placement", error);
      return data as PlacementRow;
    },
    setPlacementResource: (id, rid) => updatePlacement(supabase, id, { provider_resource_id: rid }),
    setPlacementState: (id, state) => setPlacementState(supabase, id, state),
    setPlacementRole: (id, role) =>
      updatePlacement(supabase, id, role === "dead" ? { role, retired_at: new Date().toISOString() } : { role }),
    cutover: (m, targetId) => cutoverMigration(supabase, m.id, targetId),
    getMachineType: (id) => getMachineType(supabase, id),
    listRepositories: (instanceId) => listInstanceRepositories(supabase, instanceId),
  };
}

/** Runs a provider call on a machine that may already be gone. */
export async function ignoreNotFound(call: Promise<void>): Promise<void> {
  try {
    await call;
  } catch (err) {
    if (!(err instanceof ProviderError && err.kind === "not_found")) throw err;
  }
}

/**
 * Deletes every machine behind an instance: the active one, a switch target,
 * and a source kept for reverse cutover. The instance row may name a machine
 * no live placement does, so it is included too.
 */
export async function deleteInstanceMachines(
  providers: (kind: ProviderKind, placementId?: string | null) => Pick<SandboxProvider, "deleteInstance">,
  instance: { provider_kind: string; provider_resource_id: string | null; active_placement_id?: string | null },
  placements: Pick<PlacementRow, "id" | "provider_kind" | "provider_resource_id">[],
): Promise<void> {
  const resources = new Map<string, [ProviderKind, string | null]>();
  if (instance.provider_resource_id) {
    resources.set(instance.provider_resource_id, [
      instance.provider_kind as ProviderKind,
      instance.active_placement_id ?? null,
    ]);
  }
  for (const p of placements) if (p.provider_resource_id) resources.set(p.provider_resource_id, [p.provider_kind, p.id]);
  for (const [resourceId, [kind, placementId]] of resources) {
    await ignoreNotFound(providers(kind, placementId).deleteInstance(resourceId, { placementId }));
  }
}

/** Stops a workspace's machine, then records it stopped so billing closes. */
export async function stopWorkspace(
  providers: (kind: ProviderKind, placementId?: string | null) => Pick<SandboxProvider, "stopInstance">,
  w: {
    instance_id: string;
    provider_kind: ProviderKind;
    provider_resource_id: string | null;
    active_placement_id?: string | null;
  },
  markStopped: (instanceId: string, activePlacementId: string | null) => Promise<void>,
): Promise<void> {
  const placementId = w.active_placement_id ?? null;
  if (w.provider_resource_id) await providers(w.provider_kind, placementId).stopInstance(w.provider_resource_id);
  await markStopped(w.instance_id, placementId);
}
