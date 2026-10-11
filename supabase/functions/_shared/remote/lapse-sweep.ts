// Stops and later deletes the cloud workspace of an owner whose Pro ended
// (prds/billing-and-teams.md, "Enforcement" and open decision B02: "30 days,
// stopped, then deleted"). Run by remote-admin's `sweep_lapsed_instances`
// action from a scheduler.
//
// For each live instance whose owner has no Pro, or that still carries a
// lapse:
//
// * Pro is back: the lapse is cleared.
// * The lapse is new: lapsed_at is recorded and the instance is stopped.
// * 30 days have passed: the instance is deleted.
//
// The database decides every write against its own clock and the owner's
// entitlement at that moment (027_billing_enforcement.sql), so this clock
// only chooses which instances to ask about. A clock running ahead cannot
// delete early, and an owner who subscribes again after the listing keeps
// their instance.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { recordAuditEvent, type RemoteAuditEventType } from "./audit.ts";
import type { ManagedComputeProvider } from "./sprites-adapter.ts";

export const LAPSE_GRACE_DAYS = 30;
const DAY_MS = 86_400_000;

// States in which the machine may be running. Other states (failed,
// degraded, provisioning) keep their status so the owner still sees them.
const RUNNING_STATES = new Set(["ready", "waking"]);

/** A row of public.remote_lapse_candidates(). */
export interface LapseCandidate {
  instance_id: string;
  owner_user_id: string;
  status: string;
  provider_resource_id: string | null;
  lapsed_at: string | null;
  owner_has_pro: boolean;
}

export interface LapseSweepStore {
  candidates(): Promise<LapseCandidate[]>;
  /** remote_sync_instance_lapse: lapsed_at after the write, null if entitled. */
  syncLapse(instanceId: string): Promise<string | null>;
  /** remote_claim_lapsed_deletion: true once the instance is 'deleting'. */
  claimDeletion(instanceId: string): Promise<boolean>;
  /** Shows a running instance as suspended. */
  markStopped(instanceId: string): Promise<void>;
  markDeleted(instanceId: string): Promise<void>;
  audit(event: {
    ownerUserId: string;
    instanceId: string;
    eventType: RemoteAuditEventType;
    detail?: Record<string, unknown>;
  }): Promise<void>;
}

export interface LapseSweepDeps {
  store: LapseSweepStore;
  provider: Pick<ManagedComputeProvider, "stopInstance" | "deleteInstance">;
  now: () => Date;
  log: (fields: Record<string, unknown>) => void;
}

export interface LapseSweepResult {
  stopped: string[];
  deleted: string[];
  restored: string[];
  kept: string[];
  failed: Array<{ instanceId: string; error: string }>;
}

export async function sweepLapsedInstances(
  deps: LapseSweepDeps,
): Promise<LapseSweepResult> {
  const result: LapseSweepResult = {
    stopped: [],
    deleted: [],
    restored: [],
    kept: [],
    failed: [],
  };
  const now = deps.now().getTime();

  for (const instance of await deps.store.candidates()) {
    const id = instance.instance_id;
    const audit = (
      eventType: RemoteAuditEventType,
      detail?: Record<string, unknown>,
    ) =>
      deps.store.audit({
        ownerUserId: instance.owner_user_id,
        instanceId: id,
        eventType,
        detail,
      });

    try {
      if (instance.owner_has_pro) {
        if ((await deps.store.syncLapse(id)) === null) {
          await audit("instance_lapse_cleared", { lapsed_at: instance.lapsed_at });
          result.restored.push(id);
        }
        continue;
      }

      if (instance.lapsed_at === null) {
        const lapsedAt = await deps.store.syncLapse(id);
        // Null: the owner subscribed after the listing.
        if (lapsedAt === null) continue;
        await stop(instance, deps);
        await audit("instance_lapse_stopped", { lapsed_at: lapsedAt });
        result.stopped.push(id);
        continue;
      }

      const lapsedFor = now - Date.parse(instance.lapsed_at);
      if (lapsedFor >= LAPSE_GRACE_DAYS * DAY_MS) {
        if (!(await deps.store.claimDeletion(id))) {
          result.kept.push(id);
          continue;
        }
        if (instance.provider_resource_id) {
          await deps.provider.deleteInstance(instance.provider_resource_id);
        }
        await deps.store.markDeleted(id);
        await audit("instance_lapse_deleted", {
          lapsed_at: instance.lapsed_at,
          provider_resource_id: instance.provider_resource_id,
        });
        result.deleted.push(id);
        continue;
      }

      if (RUNNING_STATES.has(instance.status)) {
        await stop(instance, deps);
        await audit("instance_lapse_stopped", { lapsed_at: instance.lapsed_at });
        result.stopped.push(id);
      } else {
        result.kept.push(id);
      }
    } catch (err) {
      const error = err instanceof Error ? err.message : String(err);
      result.failed.push({ instanceId: id, error });
      deps.log({ operation: "lapse_sweep_failed", instance_id: id, error });
      await audit("instance_lapse_sweep_failed", { error });
    }
  }

  deps.log({
    operation: "lapse_sweep_complete",
    stopped: result.stopped.length,
    deleted: result.deleted.length,
    restored: result.restored.length,
    kept: result.kept.length,
    failed: result.failed.length,
  });
  return result;
}

async function stop(instance: LapseCandidate, deps: LapseSweepDeps) {
  if (instance.provider_resource_id) {
    await deps.provider.stopInstance(instance.provider_resource_id);
  }
  await deps.store.markStopped(instance.instance_id);
}

/** The store above, through the service-role client. */
export function supabaseLapseSweepStore(
  supabase: SupabaseClient,
  correlationId: string,
): LapseSweepStore {
  return {
    async candidates() {
      const { data, error } = await supabase.rpc("remote_lapse_candidates");
      if (error) throw new Error(`remote_lapse_candidates failed: ${error.message}`);
      return (data ?? []) as LapseCandidate[];
    },
    async syncLapse(instanceId) {
      const { data, error } = await supabase.rpc("remote_sync_instance_lapse", {
        p_instance_id: instanceId,
      });
      if (error) throw new Error(`remote_sync_instance_lapse failed: ${error.message}`);
      return (data as string | null) ?? null;
    },
    async claimDeletion(instanceId) {
      const { data, error } = await supabase.rpc("remote_claim_lapsed_deletion", {
        p_instance_id: instanceId,
      });
      if (error) throw new Error(`remote_claim_lapsed_deletion failed: ${error.message}`);
      return data === true;
    },
    async markStopped(instanceId) {
      const { error } = await supabase
        .from("remote_instances")
        .update({ status: "suspended", updated_at: new Date().toISOString() })
        .eq("id", instanceId)
        .in("status", [...RUNNING_STATES]);
      if (error) throw new Error(`failed to mark instance stopped: ${error.message}`);
    },
    async markDeleted(instanceId) {
      const { error } = await supabase
        .from("remote_instances")
        .update({
          status: "deleted",
          endpoint_id: null,
          updated_at: new Date().toISOString(),
        })
        .eq("id", instanceId)
        .eq("status", "deleting");
      if (error) throw new Error(`failed to mark instance deleted: ${error.message}`);
    },
    async audit(event) {
      await recordAuditEvent(supabase, { ...event, correlationId });
    },
  };
}
