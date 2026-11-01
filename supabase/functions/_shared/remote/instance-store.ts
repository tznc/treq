// Storage helpers for remote_instances / remote_instance_operations /
// remote_endpoints / remote_endpoint_host_keys, shared by the remote-instance
// Edge Function's ensure/status/wake/reprovision/delete actions.
//
// Idempotency follows the PRD's "Idempotency" section: the operation record
// is written before the provider call, and a repeated request with the same
// (owner_user_id, idempotency_key) returns the existing operation rather
// than invoking the provider again.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import type { RegionCode, SizePreset } from "./catalog.ts";
import type { ProviderKind } from "./provider.ts";
import { must } from "./db.ts";

export type OperationType =
  | "provision"
  | "wake"
  | "reprovision"
  | "delete"
  | "resize"
  | "register_client_key"
  | "revoke_client_key"
  | "issue_certificate"
  | "register_endpoint"
  | "register_repository"
  | "install_authorized_key"
  | "remove_authorized_key"
  | "keyscan_host_key";
export type OperationStatus = "pending" | "in_progress" | "succeeded" | "failed";

export interface OperationRow {
  id: string;
  owner_user_id: string;
  instance_id: string | null;
  operation_type: OperationType;
  status: OperationStatus;
  idempotency_key: string;
  provider_request_id: string | null;
  error_message: string | null;
}

export interface InstanceRow {
  id: string;
  owner_user_id: string;
  provider_kind: string;
  provider_resource_id: string | null;
  region: RegionCode;
  size_preset: SizePreset;
  status: string;
  generation: number;
  endpoint_id: string | null;
  image_manifest_version: number;
  ready_at: string | null;
  // Enforced base resource allocation (PRD "Resource quotas"). Fixed values
  // in this delivery; not yet purchasable/adjustable.
  disk_quota_gb: number;
  vcpu_quota: number;
  ram_quota_gb: number;
  // Placement model (044_sandbox_catalog.sql). The instance mirrors its
  // active placement; `fence` increases on every cutover.
  tier_id: string | null;
  machine_type_id: string | null;
  active_placement_id: string | null;
  /** "byos" when the active placement runs on the owner's own provider account (045_provider_credentials.sql). */
  credential_source?: "managed" | "byos";
  fence: number;
}

// Looks up an existing operation for this idempotency key. When found, the
// caller must not invoke the provider again — this is what makes a repeated
// mutating request safe to retry (PRD "Idempotency").
export async function findExistingOperation(
  supabase: SupabaseClient,
  ownerUserId: string,
  idempotencyKey: string,
): Promise<OperationRow | null> {
  return must(await supabase
    .from("remote_instance_operations")
    .select("*")
    .eq("owner_user_id", ownerUserId)
    .eq("idempotency_key", idempotencyKey)
    .maybeSingle(), "look up operation") as OperationRow | null;
}

// Records the operation before the provider call, per the PRD's "The server
// records the operation before invoking the provider" requirement. A unique
// violation on (owner_user_id, idempotency_key) means a concurrent request
// already claimed it; the caller should re-read and treat it as existing.
export async function beginOperation(
  supabase: SupabaseClient,
  params: {
    ownerUserId: string;
    instanceId: string | null;
    operationType: OperationType;
    idempotencyKey: string;
  },
): Promise<OperationRow> {
  return must(await supabase
    .from("remote_instance_operations")
    .insert({
      owner_user_id: params.ownerUserId,
      instance_id: params.instanceId,
      operation_type: params.operationType,
      idempotency_key: params.idempotencyKey,
      status: "in_progress",
    })
    .select()
    .single(), "begin operation") as OperationRow;
}

export async function completeOperation(
  supabase: SupabaseClient,
  operationId: string,
  outcome: { status: "succeeded" | "failed"; providerRequestId?: string | null; errorMessage?: string | null },
): Promise<void> {
  must(await supabase
    .from("remote_instance_operations")
    .update({
      status: outcome.status,
      provider_request_id: outcome.providerRequestId ?? null,
      error_message: outcome.errorMessage ?? null,
      finished_at: new Date().toISOString(),
    })
    .eq("id", operationId), "complete operation");
}

// The owner's one live instance. Deleted rows are kept for history (a user
// may have several) and are never returned, so after a delete the owner has
// no instance and `ensure` creates a new row.
export async function getInstanceForOwner(
  supabase: SupabaseClient,
  ownerUserId: string,
): Promise<InstanceRow | null> {
  return must(await supabase
    .from("remote_instances")
    .select("*")
    .eq("owner_user_id", ownerUserId)
    .neq("status", "deleted")
    .maybeSingle(), "read instance") as InstanceRow | null;
}

export async function getInstanceById(
  supabase: SupabaseClient,
  ownerUserId: string,
  instanceId: string,
): Promise<InstanceRow | null> {
  return must(await supabase
    .from("remote_instances")
    .select("*")
    .eq("owner_user_id", ownerUserId)
    .eq("id", instanceId)
    .maybeSingle(), "read instance") as InstanceRow | null;
}

export async function createProvisioningInstance(
  supabase: SupabaseClient,
  params: {
    ownerUserId: string;
    region: RegionCode;
    sizePreset: SizePreset;
    manifestVersion: number;
    providerKind: ProviderKind;
    tierId?: string;
    machineTypeId?: string;
  },
): Promise<InstanceRow> {
  return must(await supabase
    .from("remote_instances")
    .insert({
      owner_user_id: params.ownerUserId,
      provider_kind: params.providerKind,
      tier_id: params.tierId ?? null,
      machine_type_id: params.machineTypeId ?? null,
      region: params.region,
      size_preset: params.sizePreset,
      status: "provisioning",
      generation: 0,
      image_manifest_version: params.manifestVersion,
      // disk_quota_gb is set from the machine type's disk_gb by a trigger
      // (046_placements_and_usage.sql); there is no free base allocation.
    })
    .select()
    .single(), "create instance record") as InstanceRow;
}

export async function updateInstance(
  supabase: SupabaseClient,
  instanceId: string,
  fields: Record<string, unknown>,
): Promise<void> {
  must(await supabase
    .from("remote_instances")
    .update({ ...fields, updated_at: new Date().toISOString() })
    .eq("id", instanceId), "update instance");
}

// Upserts the managed endpoint for an instance and records its host key
// under the instance's current generation. Reprovisioning calls this again
// with a new generation; a differing fingerprint at a higher generation is
// the explicit host-key rotation record required by the PRD's "Host-key
// verification" section (verification of the new key against a client is
// Phase 3's job — this only durably records the transition).
export async function recordManagedEndpoint(
  supabase: SupabaseClient,
  params: {
    ownerUserId: string;
    instanceId: string;
    hostname: string;
    port: number;
    username: string;
    existingEndpointId: string | null;
  },
): Promise<string> {
  if (params.existingEndpointId) {
    must(await supabase
      .from("remote_endpoints")
      .update({
        hostname: params.hostname,
        port: params.port,
        username: params.username,
        updated_at: new Date().toISOString(),
      })
      .eq("id", params.existingEndpointId), "update endpoint");
    return params.existingEndpointId;
  }

  const data = must(await supabase
    .from("remote_endpoints")
    .insert({
      owner_user_id: params.ownerUserId,
      instance_id: params.instanceId,
      source: "managed",
      display_name: "Treq-managed VM",
      hostname: params.hostname,
      port: params.port,
      username: params.username,
    })
    .select("id")
    .single(), "create endpoint");
  return (data as { id: string }).id;
}

// Records a host key fingerprint for an endpoint at a given generation.
// Called once a fingerprint is available (obtained through a trusted
// provisioning path); when unavailable at create time, callers should audit
// a `readiness_stage_failed` event for the host-key stage instead of
// fabricating a value.
export async function recordEndpointHostKey(
  supabase: SupabaseClient,
  params: {
    ownerUserId: string;
    endpointId: string;
    algorithm: string;
    fingerprintSha256: string;
    generation: number;
  },
): Promise<void> {
  must(await supabase.from("remote_endpoint_host_keys").upsert(
    {
      owner_user_id: params.ownerUserId,
      endpoint_id: params.endpointId,
      algorithm: params.algorithm,
      fingerprint_sha256: params.fingerprintSha256,
      generation: params.generation,
    },
    { onConflict: "endpoint_id,fingerprint_sha256" },
  ), "record host key");
}

export async function previousHostKeyFingerprint(
  supabase: SupabaseClient,
  endpointId: string,
): Promise<string | null> {
  const data = must(await supabase
    .from("remote_endpoint_host_keys")
    .select("fingerprint_sha256")
    .eq("endpoint_id", endpointId)
    .is("revoked_at", null)
    .order("generation", { ascending: false })
    .limit(1)
    .maybeSingle(), "read previous host key");
  return data?.fingerprint_sha256 ?? null;
}
