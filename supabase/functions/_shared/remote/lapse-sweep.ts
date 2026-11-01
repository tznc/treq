// Stops and later deletes the cloud workspace of an owner whose Pro ended
// (prds/billing-and-teams.md, "Enforcement" and open decision B02: "7 days,
// stopped, then deleted"). Run by remote-admin's `sweep_lapsed_instances`
// action from a scheduler.
//
// For each live instance whose owner has no Pro, or that still carries a
// lapse:
//
// * Pro is back: the lapse is cleared.
// * The lapse is new: lapsed_at is recorded and the instance is stopped.
// * 7 days have passed: the instance is deleted.
//
// The database decides every write against its own clock and the owner's
// entitlement at that moment (031_retention_windows.sql), so this clock
// only chooses which instances to ask about. A clock running ahead cannot
// delete early, and an owner who subscribes again after the listing keeps
// their instance.
//
// The sweep works on any provider: each instance's machine is reached
// through `providers(instance.provider_kind)`, stops go through
// set_placement_state so billing intervals close, and a deletion removes
// every live placement's machine, as remote-instance `delete` does.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { recordAuditEvent, type RemoteAuditEventType } from "./audit.ts";
import {
  cancelOpenMigration,
  deleteInstanceMachines,
  stopWorkspace,
  setPlacementState,
} from "./placement-store.ts";
import type { ManagedComputeProvider, ProviderKind } from "./provider.ts";
import { must, rpc } from "./db.ts";

export const LAPSE_GRACE_DAYS = 7;
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
  /** From remote_instances. */
  provider_kind: ProviderKind;
  active_placement_id?: string | null;
}

/** A placement of the instance whose role is not 'dead'. */
export interface LapsePlacement {
  id: string;
  instance_id: string;
  provider_kind: ProviderKind;
  provider_resource_id: string | null;
  role: string;
}

export interface LapseSweepStore {
  candidates(): Promise<LapseCandidate[]>;
  /** remote_sync_instance_lapse: lapsed_at after the write, null if entitled. */
  syncLapse(instanceId: string): Promise<string | null>;
  /** remote_claim_lapsed_deletion: true once the instance is 'deleting'. */
  claimDeletion(instanceId: string): Promise<boolean>;
  /** Shows a running instance as suspended, through its active placement when it has one. */
  markStopped(instanceId: string, activePlacementId: string | null): Promise<void>;
  /** Cancels an open switch, then lists the placements whose role is not 'dead'. */
  livePlacements(instanceId: string): Promise<LapsePlacement[]>;
  /** Marks the instance deleted and the placements deleted and dead, closing billing. */
  markDeleted(instanceId: string, placementIds: string[]): Promise<void>;
  audit(event: {
    ownerUserId: string;
    instanceId: string;
    eventType: RemoteAuditEventType;
    detail?: Record<string, unknown>;
  }): Promise<void>;
}

export interface LapseSweepDeps {
  store: LapseSweepStore;
  providers: (
    kind: ProviderKind,
    placementId?: string | null,
  ) => Pick<ManagedComputeProvider, "stopInstance" | "deleteInstance">;
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
        const placements = await deps.store.livePlacements(id);
        await deleteInstanceMachines(deps.providers, instance, placements);
        await deps.store.markDeleted(id, placements.map((p) => p.id));
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

const stop = (instance: LapseCandidate, deps: LapseSweepDeps) =>
  stopWorkspace(deps.providers, instance, deps.store.markStopped);

/** The store above, through the service-role client. */
export function supabaseLapseSweepStore(
  supabase: SupabaseClient,
  correlationId: string,
): LapseSweepStore {
  return {
    async candidates() {
      const data = await rpc(supabase, "remote_lapse_candidates");
      const rows = (data ?? []) as LapseCandidate[];
      if (rows.length === 0) return rows;
      // The candidates function predates placements; add each instance's
      // provider and active placement.
      const instances = must(await supabase
        .from("remote_instances")
        .select("id, provider_kind, active_placement_id")
        .in("id", rows.map((r) => r.instance_id)), "read lapse candidates' providers");
      const byId = new Map((instances ?? []).map((i) => [i.id as string, i]));
      return rows.map((r) => ({
        ...r,
        provider_kind: byId.get(r.instance_id)?.provider_kind as ProviderKind,
        active_placement_id: (byId.get(r.instance_id)?.active_placement_id as string | null | undefined) ?? null,
      }));
    },
    async syncLapse(instanceId) {
      return ((await rpc(supabase, "remote_sync_instance_lapse", {
        p_instance_id: instanceId,
      })) as string | null) ?? null;
    },
    async claimDeletion(instanceId) {
      return (await rpc(supabase, "remote_claim_lapsed_deletion", {
        p_instance_id: instanceId,
      })) === true;
    },
    async markStopped(instanceId, activePlacementId) {
      if (activePlacementId) {
        const data = must(await supabase
          .from("remote_instance_placements")
          .select("state")
          .eq("id", activePlacementId)
          .maybeSingle(), "read placement");
        if (data && RUNNING_STATES.has(data.state as string)) {
          await setPlacementState(supabase, activePlacementId, "suspended");
        }
        return;
      }
      must(await supabase
        .from("remote_instances")
        .update({ status: "suspended", updated_at: new Date().toISOString() })
        .eq("id", instanceId)
        .in("status", [...RUNNING_STATES]), "mark instance stopped");
    },
    async livePlacements(instanceId) {
      // Cancel an open switch first, in one statement so a worker advancing
      // it cannot make the cancel miss. A worker mid-create sees the cancel
      // and deletes its own target (migration-engine.ts).
      await cancelOpenMigration(supabase, instanceId, "instance_lapse_deleted");
      return (must(await supabase
        .from("remote_instance_placements")
        .select("id, instance_id, provider_kind, provider_resource_id, role")
        .eq("instance_id", instanceId)
        .neq("role", "dead"), "list placements") ?? []) as LapsePlacement[];
    },
    async markDeleted(instanceId, placementIds) {
      must(await supabase
        .from("remote_instances")
        .update({
          status: "deleted",
          endpoint_id: null,
          updated_at: new Date().toISOString(),
        })
        .eq("id", instanceId)
        .eq("status", "deleting"), "mark instance deleted");
      // set_placement_state closes each placement's billing interval.
      const retiredAt = new Date().toISOString();
      for (const placementId of placementIds) {
        await setPlacementState(supabase, placementId, "deleted");
        must(await supabase
          .from("remote_instance_placements")
          .update({ role: "dead", retired_at: retiredAt })
          .eq("id", placementId), "retire placement");
      }
    },
    async audit(event) {
      await recordAuditEvent(supabase, { ...event, correlationId });
    },
  };
}
