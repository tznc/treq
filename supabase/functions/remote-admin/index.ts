// Edge function: operator actions for managed instances, billing jobs and
// provider migrations (prds/remote-development.md Phase 7,
// docs/design/sandbox-provider-adapter.md §9).
//
// A normal user's Supabase session can never reach it. There is no admin
// role on a Supabase user; like every other privileged path here, the gate
// is a server-side secret: the `x-admin-api-key` header must match the
// `REMOTE_ADMIN_API_KEY` secret, held only by operators and the scheduler
// (.github/workflows/remote-maintenance.yml).
//
// POST body: { action, operator_id?, ...action-specific fields }
//   "revoke_client_key" {key_id, reason?}      - revoke any user's client key
//   "recover_instance" {instance_id, target_status?, reason?}
//                                              - force a stuck instance to degraded/failed
//   "list_recent_failures" {limit?}            - read remote_recent_failures
//   "prune_audit_events"                       - audit retention cleanup
//   "sweep_lapsed_instances"                   - stop workspaces whose owner's Pro ended,
//                                                delete them 7 days later (lapse-sweep.ts)
//   "report_usage"                             - send hourly usage to the Stripe meter
//                                                (usage-report.ts; only with TREQ_USAGE_BILLING_ENABLED=1)
//   "enforce_idle"                             - stop idle workspaces, reconcile state (idle-controller.ts)
//   "enforce_spend_caps"                       - warn at 80%, stop at 100% (spend-cap.ts)
//   "recompute_defaults"                       - recompute_pro_defaults()
//   "drain_provider" / "retire_provider" {kind, limit?}
//   "set_price" {machine_type_id, running_nanousd_per_sec, stopped_nanousd_per_gb_sec?, effective_from?}
//   "advance_migrations" {limit?, migration_id?}
//   "destroy_retired_placements" {limit?}
//   "reverse_cutover" {migration_id}
//   "force_migration" {migration_id, reason}
//
// Operator actions are audited as `admin_*` events, not as the affected
// user's own actions.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { correlationIdFromRequest, logWithCorrelation } from "../_shared/remote/correlation.ts";
import { supabaseLapseSweepStore, sweepLapsedInstances } from "../_shared/remote/lapse-sweep.ts";
import { placementProviders } from "../_shared/remote/byos.ts";
import { enforceIdle, idleTimeoutMinutes, supabaseIdleStore } from "../_shared/remote/idle-controller.ts";
import { reportUsage, supabaseUsageReportStore } from "../_shared/billing/usage-report.ts";
import { enforceSpendCaps, supabaseSpendCapStore } from "../_shared/billing/spend-cap.ts";
import { createStripeClient } from "../_shared/billing/stripe-api.ts";
import * as ops from "../_shared/remote/operator-controls.ts";
import { isProviderStubEnabled } from "../_shared/remote/local-provider.ts";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "x-admin-api-key, x-client-info, content-type",
};

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...corsHeaders, "Content-Type": "application/json" },
  });
}

type Ctx = ops.OperatorContext & { log: (fields: Record<string, unknown>) => void };

const ACTIONS: Record<string, (ctx: Ctx, body: Record<string, unknown>) => Promise<unknown>> = {
  revoke_client_key: ops.revokeClientKey,
  recover_instance: ops.recoverInstance,
  list_recent_failures: ops.listRecentFailures,
  prune_audit_events: ops.pruneAuditEvents,
  sweep_lapsed_instances: (ctx) =>
    sweepLapsedInstances({
      store: supabaseLapseSweepStore(ctx.supabase, ctx.correlationId),
      providers: ctx.providers,
      now: () => new Date(),
      log: ctx.log,
    }),
  report_usage: (ctx) => {
    const enabled = Deno.env.get("TREQ_USAGE_BILLING_ENABLED") === "1";
    const stripeKey = Deno.env.get("STRIPE_SECRET_KEY") ?? "";
    if (enabled && !stripeKey) throw new Error("STRIPE_SECRET_KEY is not configured");
    return reportUsage({
      store: supabaseUsageReportStore(ctx.supabase),
      stripe: createStripeClient(stripeKey),
      now: () => new Date(),
      enabled,
      eventName: Deno.env.get("TREQ_STRIPE_USAGE_METER_EVENT") || undefined,
      log: ctx.log,
    });
  },
  // No machine to probe under the stub.
  enforce_idle: async (ctx) =>
    ctx.stub ? { skipped: "stub" } : await enforceIdle({
      store: supabaseIdleStore(ctx.supabase),
      providers: ctx.providers,
      now: () => new Date(),
      log: ctx.log,
      idleTimeoutMin: idleTimeoutMinutes((k) => Deno.env.get(k)),
    }),
  enforce_spend_caps: (ctx) =>
    enforceSpendCaps({
      store: supabaseSpendCapStore(ctx.supabase),
      providers: ctx.providers,
      now: () => new Date(),
      log: ctx.log,
    }),
  recompute_defaults: (ctx) => ops.recomputeDefaults(ctx),
  drain_provider: (ctx, body) => ops.drainProvider(ctx, body, "drain"),
  retire_provider: (ctx, body) => ops.drainProvider(ctx, body, "retire"),
  set_price: ops.setPrice,
  advance_migrations: ops.advanceMigrations,
  destroy_retired_placements: ops.destroyRetiredPlacements,
  reverse_cutover: ops.reverseCutover,
  force_migration: ops.forceMigration,
};

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") return new Response(null, { headers: corsHeaders, status: 204 });
  if (req.method !== "POST") return json({ error: "Method not allowed" }, 405);

  // No fallback to a user token with some claim: a user session must never
  // reach this, whatever its JWT holds.
  const configuredKey = Deno.env.get("REMOTE_ADMIN_API_KEY") ?? "";
  const presentedKey = req.headers.get("x-admin-api-key") ?? "";
  if (!configuredKey || presentedKey.length === 0 || !timingSafeEqual(configuredKey, presentedKey)) {
    return json({ error: "Unauthorized" }, 401);
  }

  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  const supabase = createClient(Deno.env.get("SUPABASE_URL") ?? "", Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "");
  const correlationId = correlationIdFromRequest(req);
  const action = body.action;
  const handler = typeof action === "string" && Object.hasOwn(ACTIONS, action) ? ACTIONS[action] : null;
  if (!handler) return json({ error: `Unknown action '${action}'` }, 400);

  try {
    return json(await handler({
      supabase,
      providers: placementProviders(supabase),
      stub: isProviderStubEnabled(),
      correlationId,
      operatorId: typeof body.operator_id === "string" && body.operator_id.length > 0
        ? body.operator_id
        : "unknown-operator",
      log: (fields) => logWithCorrelation(correlationId, "log", JSON.stringify(fields)),
    }, body));
  } catch (err) {
    if (err instanceof ops.OperatorInputError) return json({ error: err.message }, err.status);
    logWithCorrelation(correlationId, "error", `remote-admin action=${action} failed: ${(err as Error).message}`);
    return json({ error: "Internal error" }, 500);
  }
});

// Constant time, so the check leaks nothing about how many leading
// characters matched.
function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}
