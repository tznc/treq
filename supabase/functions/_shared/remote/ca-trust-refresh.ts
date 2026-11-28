// Rewrites the trusted SSH CAs on every running machine, for a CA roll
// (remote-admin `refresh_ca_trust`). A stopped machine gets them when it
// wakes (reensureServicesAfterWake); a running one keeps its CA file until
// this runs. Each machine is audited as `ca_trust_installed` with the
// fingerprints it now trusts, which the verify script reads.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { recordAuditEvent } from "./audit.ts";
import { must } from "./db.ts";
import { scrubErrorText } from "./error-scrub.ts";
import { installCaTrust } from "./machine-setup.ts";
import type { ProviderKind, SandboxProvider } from "./provider.ts";
import { errorMessage } from "../error-message.ts";

export interface RunningMachine {
  instanceId: string;
  ownerUserId: string;
  endpointId: string | null;
  placementId: string;
  providerKind: ProviderKind;
  providerResourceId: string;
}

export interface CaTrustRefreshStore {
  /** Running machines by instance id, after `after` when given. */
  runningMachines(limit: number, after: string | null): Promise<RunningMachine[]>;
  recordInstalled(machine: RunningMachine, fingerprints: string[]): Promise<void>;
  recordFailed(machine: RunningMachine, error: string): Promise<void>;
}

export async function refreshCaTrust(
  store: CaTrustRefreshStore,
  providers: (kind: ProviderKind, placementId?: string | null) => SandboxProvider,
  limit: number,
  after: string | null = null,
): Promise<{ refreshed: number; failed: Array<{ instance_id: string; error: string }>; next_after: string | null }> {
  let refreshed = 0;
  const failed: Array<{ instance_id: string; error: string }> = [];
  const machines = await store.runningMachines(limit, after);
  for (const machine of machines) {
    try {
      const provider = providers(machine.providerKind, machine.placementId);
      await store.recordInstalled(machine, await installCaTrust(provider, machine.providerResourceId));
      refreshed++;
    } catch (err) {
      const error = scrubErrorText(errorMessage(err));
      await store.recordFailed(machine, error);
      failed.push({ instance_id: machine.instanceId, error });
    }
  }
  // A full page may have more after it.
  const next_after = machines.length === limit ? machines.at(-1)?.instanceId ?? null : null;
  return { refreshed, failed, next_after };
}

export function supabaseCaTrustRefreshStore(supabase: SupabaseClient, correlationId: string): CaTrustRefreshStore {
  const audit = (m: RunningMachine, eventType: "ca_trust_installed" | "ca_trust_install_failed", detail: object) =>
    recordAuditEvent(supabase, {
      ownerUserId: m.ownerUserId,
      instanceId: m.instanceId,
      endpointId: m.endpointId,
      eventType,
      detail: { ...detail, source: "refresh_ca_trust" },
      correlationId,
    });
  return {
    async runningMachines(limit, after) {
      let query = supabase
        .from("remote_instances")
        .select("id, owner_user_id, endpoint_id, active_placement_id")
        .eq("status", "ready")
        .not("active_placement_id", "is", null);
      if (after) query = query.gt("id", after);
      const instances = must(
        await query.order("id").limit(limit),
        "read running instances",
      ) ?? [];
      if (instances.length === 0) return [];
      const placements = must(
        await supabase
          .from("remote_instance_placements")
          .select("id, provider_kind, provider_resource_id")
          .in("id", instances.map((i) => i.active_placement_id)),
        "read active placements",
      ) ?? [];
      return instances.flatMap((i) => {
        const p = placements.find((row) => row.id === i.active_placement_id);
        if (!p?.provider_resource_id) return [];
        return [{
          instanceId: i.id,
          ownerUserId: i.owner_user_id,
          endpointId: i.endpoint_id,
          placementId: p.id,
          providerKind: p.provider_kind,
          providerResourceId: p.provider_resource_id,
        }];
      });
    },
    recordInstalled: (m, fingerprints) => audit(m, "ca_trust_installed", { ca_fingerprints: fingerprints }),
    recordFailed: (m, error) => audit(m, "ca_trust_install_failed", { error }),
  };
}
