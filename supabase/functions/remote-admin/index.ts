// Edge function: administrative key revocation and instance recovery
// (prds/remote-development.md, Phase 7: "Add administrative key revocation and
// instance recovery procedures").
//
// This function is deliberately NOT reachable with a normal end user's
// Supabase session token. This codebase has no notion of an "admin" role on
// a Supabase user (see e.g. 005_merge_queue_pgmq.sql's pattern of
// service-role-only SECURITY DEFINER wrappers instead of a user-role check) -
// every other privileged operation in this codebase is gated by holding the
// service-role key, a server-side secret, rather than by a claim on a user
// JWT. This function follows the same convention for its own privilege
// gate: the caller must present a valid `x-admin-api-key` header matching
// the `REMOTE_ADMIN_API_KEY` Edge Function secret. That secret is held only
// by operators (e.g. invoked from an internal script or a scheduler), never
// shipped to the desktop client, and is a completely separate credential
// from any user's Supabase access token - a compromised or replayed user JWT
// can never reach this endpoint.
//
// POST body: { action, ...action-specific fields }
// action:
//   "revoke_client_key"     - revoke a specific user's client key by id, regardless of who owns it
//   "recover_instance"      - force-transition a stuck instance out of a
//                             non-terminal state (e.g. stuck in
//                             "provisioning"/"reprovisioning"/"waking" long
//                             past a reasonable readiness window) back to
//                             "degraded" so the owner's next reconcile/retry
//                             is not blocked forever, or to "failed" if
//                             recovery is not possible
//   "list_recent_failures"  - reads `remote_recent_failures` (see migration
//                             017_remote_ssh_observability.sql)
//   "prune_audit_events"    - runs the retention cleanup function
//   "sweep_lapsed_instances" - stops the cloud workspace of every owner
//                             whose Pro ended and deletes it 30 days later
//                             (prds/billing-and-teams.md, open decision
//                             B02; see `_shared/remote/lapse-sweep.ts`).
//                             Schedule it hourly from the same scheduler
//                             that runs `prune_audit_events`.
//
// Every admin action writes its own audit trail entry (event types
// `admin_client_key_revoked` / `admin_instance_recovered`) distinct from a
// user's own self-service actions, recording the operator action rather than
// attributing it to the affected user.

import { createClient, type SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { recordAuditEvent } from "../_shared/remote/audit.ts";
import { correlationIdFromRequest, logWithCorrelation } from "../_shared/remote/correlation.ts";
import { supabaseLapseSweepStore, sweepLapsedInstances } from "../_shared/remote/lapse-sweep.ts";
import { SpritesProvider, spritesConfigFromEnv } from "../_shared/remote/sprites-adapter.ts";
import { isSpritesStubEnabled, StubSpritesProvider } from "../_shared/remote/stub-sprites-adapter.ts";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "x-admin-api-key, x-client-info, content-type",
};

// Instance statuses this endpoint treats as "non-terminal" and therefore
// eligible for a forced recovery transition. Matches the state machine in
// prds/remote-development.md "Instance lifecycle" minus "ready"/"suspended"/
// "deleted", which are not stuck states.
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

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...corsHeaders, "Content-Type": "application/json" },
  });
}

class AdminValidationError extends Error {
  constructor(message: string, public readonly status = 400) {
    super(message);
  }
}

function requireString(body: Record<string, unknown>, field: string): string {
  const value = body[field];
  if (typeof value !== "string" || value.length === 0) throw new AdminValidationError(`${field} is required`);
  return value;
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") return new Response(null, { headers: corsHeaders, status: 204 });
  if (req.method !== "POST") return json({ error: "Method not allowed" }, 405);

  // Admin gate: a server-side-only secret, never a user Supabase JWT. No
  // fallback and no "also accept a user token with some claim" branch - the
  // whole point is that a normal end user's session can never reach this
  // regardless of what is in their JWT.
  const configuredKey = Deno.env.get("REMOTE_ADMIN_API_KEY") ?? "";
  const presentedKey = req.headers.get("x-admin-api-key") ?? "";
  if (!configuredKey || presentedKey.length === 0 || !timingSafeEqual(configuredKey, presentedKey)) {
    return json({ error: "Unauthorized" }, 401);
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const supabaseServiceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "";
  const supabase = createClient(supabaseUrl, supabaseServiceKey);
  const correlationId = correlationIdFromRequest(req);

  // deno-lint-ignore no-explicit-any
  let body: Record<string, any>;
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  const action = body.action;
  const operatorId = typeof body.operator_id === "string" && body.operator_id.length > 0 ? body.operator_id : "unknown-operator";

  try {
    switch (action) {
      case "revoke_client_key":
        return await handleAdminRevokeClientKey(supabase, body, operatorId, correlationId);
      case "recover_instance":
        return await handleAdminRecoverInstance(supabase, body, operatorId, correlationId);
      case "list_recent_failures":
        return await handleListRecentFailures(supabase, body);
      case "prune_audit_events":
        return await handlePruneAuditEvents(supabase);
      case "sweep_lapsed_instances":
        return await handleSweepLapsedInstances(supabase, correlationId);
      default:
        return json({ error: `Unknown action '${action}'` }, 400);
    }
  } catch (err) {
    if (err instanceof AdminValidationError) return json({ error: err.message }, err.status);
    logWithCorrelation(correlationId, "error", `remote-admin action=${action} failed: ${(err as Error).message}`);
    return json({ error: "Internal error" }, 500);
  }
});

// Constant-time comparison so the admin key check does not leak timing
// information about how many leading characters matched.
function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return diff === 0;
}

async function handleAdminRevokeClientKey(
  supabase: SupabaseClient,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  operatorId: string,
  correlationId: string,
): Promise<Response> {
  const keyId = requireString(body, "key_id");
  const reason = typeof body.reason === "string" ? body.reason : null;

  const { data: key, error: fetchError } = await supabase
    .from("remote_client_keys")
    .select("id, owner_user_id, fingerprint_sha256, revoked_at")
    .eq("id", keyId)
    .maybeSingle();
  if (fetchError) throw new Error(`failed to read client key: ${fetchError.message}`);
  if (!key) throw new AdminValidationError("Key not found", 404);

  if (!key.revoked_at) {
    const { error: updateError } = await supabase
      .from("remote_client_keys")
      .update({ revoked_at: new Date().toISOString() })
      .eq("id", keyId);
    if (updateError) throw new Error(`failed to revoke client key: ${updateError.message}`);
  }

  await recordAuditEvent(supabase, {
    ownerUserId: key.owner_user_id,
    eventType: "admin_client_key_revoked",
    detail: {
      key_id: keyId,
      fingerprint: key.fingerprint_sha256,
      operator_id: operatorId,
      reason,
      already_revoked: Boolean(key.revoked_at),
    },
    correlationId,
    severity: "warning",
  });

  return json({ status: "succeeded", key_id: keyId });
}

async function handleAdminRecoverInstance(
  supabase: SupabaseClient,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  operatorId: string,
  correlationId: string,
): Promise<Response> {
  const instanceId = requireString(body, "instance_id");
  const targetStatus = typeof body.target_status === "string" ? body.target_status : "degraded";
  const reason = typeof body.reason === "string" ? body.reason : "manual admin recovery";

  if (!["degraded", "failed"].includes(targetStatus)) {
    throw new AdminValidationError("target_status must be 'degraded' or 'failed'");
  }

  const { data: instance, error: fetchError } = await supabase
    .from("remote_instances")
    .select("id, owner_user_id, status, generation")
    .eq("id", instanceId)
    .maybeSingle();
  if (fetchError) throw new Error(`failed to read instance: ${fetchError.message}`);
  if (!instance) throw new AdminValidationError("Instance not found", 404);

  if (!RECOVERABLE_STATUSES.includes(instance.status)) {
    throw new AdminValidationError(
      `Instance is in status '${instance.status}', which is not treated as a stuck/recoverable state`,
      409,
    );
  }

  const { error: updateError } = await supabase
    .from("remote_instances")
    .update({ status: targetStatus, updated_at: new Date().toISOString() })
    .eq("id", instanceId);
  if (updateError) throw new Error(`failed to update instance status: ${updateError.message}`);

  await recordAuditEvent(supabase, {
    ownerUserId: instance.owner_user_id,
    instanceId: instance.id,
    eventType: "admin_instance_recovered",
    detail: {
      from_status: instance.status,
      to_status: targetStatus,
      generation: instance.generation,
      operator_id: operatorId,
      reason,
    },
    correlationId,
    severity: "warning",
  });

  return json({ status: "succeeded", instance_id: instanceId, from_status: instance.status, to_status: targetStatus });
}

async function handleListRecentFailures(
  supabase: SupabaseClient,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
): Promise<Response> {
  const limit = typeof body.limit === "number" && body.limit > 0 && body.limit <= 500 ? body.limit : 100;
  const { data, error } = await supabase
    .from("remote_recent_failures")
    .select("*")
    .order("created_at", { ascending: false })
    .limit(limit);
  if (error) throw new Error(`failed to read remote_recent_failures: ${error.message}`);
  return json({ failures: data ?? [] });
}

async function handlePruneAuditEvents(supabase: SupabaseClient): Promise<Response> {
  const { data, error } = await supabase.rpc("prune_remote_audit_events");
  if (error) throw new Error(`failed to prune audit events: ${error.message}`);
  return json({ deleted_count: data });
}

async function handleSweepLapsedInstances(supabase: SupabaseClient, correlationId: string): Promise<Response> {
  const result = await sweepLapsedInstances({
    store: supabaseLapseSweepStore(supabase, correlationId),
    provider: isSpritesStubEnabled() ? new StubSpritesProvider() : new SpritesProvider(spritesConfigFromEnv()),
    now: () => new Date(),
    log: (fields) => logWithCorrelation(correlationId, "log", JSON.stringify(fields)),
  });
  return json(result);
}
