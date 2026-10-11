// Audit event helper for remote_audit_events, per the PRD's "Observability
// and audit" section. `detail` must never contain a provider token, CA key
// material, or repository content — callers pass only normalized lifecycle
// fields (region, size, generation, manifest version, provider request ids,
// readiness stage).

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";

export type RemoteAuditEventType =
  | "instance_create_requested"
  | "instance_create_succeeded"
  | "instance_create_failed"
  | "instance_wake_requested"
  | "instance_wake_succeeded"
  | "instance_wake_failed"
  | "instance_replace_requested"
  | "instance_replace_succeeded"
  | "instance_replace_failed"
  | "instance_delete_requested"
  | "instance_delete_succeeded"
  | "instance_delete_failed"
  | "host_key_registered"
  | "host_key_rotated"
  | "host_keyscan_failed"
  | "readiness_stage_failed"
  | "client_key_registered"
  | "client_key_revoked"
  | "certificate_issued"
  | "certificate_issue_failed"
  // Silent renewal while the session is active (PRD "SSH identity and
  // certificates" > "Silent renewal while the session is active"): the same
  // signing path as the initial issuance, distinguished here so a renewal
  // shows up distinctly from first issuance in the audit trail.
  | "certificate_renewed"
  | "certificate_renewal_failed"
  // Hard cutoff on revocation or expiry (PRD "Hard cutoff on revocation or
  // expiry"): the desktop client reports a forced cutoff so it is
  // correlatable in the same audit trail as the revocation or lapse that
  // caused it, even though the cutoff itself happens client-side.
  | "certificate_cutoff_forced"
  | "authorized_key_installed"
  | "authorized_key_removed"
  | "ca_trust_installed"
  | "ca_trust_install_failed"
  | "admin_client_key_revoked"
  | "admin_instance_recovered"
  // The owner's Pro ended (prds/billing-and-teams.md, open decision B02):
  // the lapse sweep stops the instance, deletes it after 30 days, or clears
  // the lapse when Pro returns. See `lapse-sweep.ts`.
  | "instance_lapse_stopped"
  | "instance_lapse_deleted"
  | "instance_lapse_cleared"
  | "instance_lapse_sweep_failed";

export type RemoteAuditSeverity = "info" | "warning" | "error";

// Event types whose name alone marks them as a failure, so callers do not
// need to remember to pass severity: "error" for every one of these -
// forgetting it would silently drop the row out of `remote_recent_failures`.
const FAILURE_EVENT_SUFFIXES = ["_failed"];

function inferSeverity(eventType: RemoteAuditEventType): RemoteAuditSeverity {
  return FAILURE_EVENT_SUFFIXES.some((suffix) => eventType.endsWith(suffix)) ? "error" : "info";
}

export async function recordAuditEvent(
  supabase: SupabaseClient,
  params: {
    ownerUserId: string;
    instanceId?: string | null;
    endpointId?: string | null;
    eventType: RemoteAuditEventType;
    // deno-lint-ignore no-explicit-any
    detail?: Record<string, any>;
    /** Threads the desktop request / Edge Function operation / provider call
     * together. See `_shared/remote/correlation.ts`. Optional only for call
     * sites (background reconciliation, admin actions with their own
     * identifier scheme) that do not have an inbound HTTP request. */
    correlationId?: string | null;
    providerRequestId?: string | null;
    idempotencyKey?: string | null;
    durationMs?: number | null;
    severity?: RemoteAuditSeverity;
  },
): Promise<void> {
  const severity = params.severity ?? inferSeverity(params.eventType);
  const { error } = await supabase.from("remote_audit_events").insert({
    owner_user_id: params.ownerUserId,
    instance_id: params.instanceId ?? null,
    endpoint_id: params.endpointId ?? null,
    event_type: params.eventType,
    detail: params.detail ?? {},
    correlation_id: params.correlationId ?? null,
    provider_request_id: params.providerRequestId ?? null,
    idempotency_key: params.idempotencyKey ?? null,
    duration_ms: params.durationMs ?? null,
    severity,
  });
  // Audit recording failures must not silently vanish, but they also must
  // not fail the underlying lifecycle operation the caller already
  // committed to the provider — log and continue.
  if (error) {
    const correlationSuffix = params.correlationId ? ` correlation_id=${params.correlationId}` : "";
    console.error(`failed to record audit event ${params.eventType}${correlationSuffix}: ${error.message}`);
  }
}

/** Small timing helper so call sites recording an operation's duration don't
 * each hand-roll `Date.now()` bookkeeping (PRD: "idempotency key and
 * operation duration"). */
export function startTimer(): () => number {
  const start = Date.now();
  return () => Date.now() - start;
}
