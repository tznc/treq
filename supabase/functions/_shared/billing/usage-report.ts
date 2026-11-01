// Hourly usage report to Stripe Billing Meters
// (docs/design/sandbox-provider-adapter.md, section 5 and D7). Run by
// remote-admin's `report_usage` action.
//
// 1. build_usage_reports(now) turns closed hours of usage_intervals into one
//    usage_reports row per (owner, hour). It is idempotent.
// 2. Each unsent row becomes one meter event. The row's identifier is the
//    Stripe event identifier, so a retry after a timeout cannot bill twice.
//
// Meter unit: micro-USD of billed amount (provider cost x markup). The value
// is round(billed_nanousd / 1000), rounded once per (owner, hour). The Stripe
// price is 0.0001 cents per unit, so 1,000,000 units bill US$1.
//
// A row that cannot be sent ends in a terminal stripe_status so it never
// blocks newer rows: `no_customer` (the owner has no Stripe customer) or
// `dead` (a non-retryable 4xx, or MAX_USAGE_REPORT_ATTEMPTS failures). Both
// log an error with `alert: true`. pendingReports() selects only rows that
// can still be sent.
//
// When `enabled` is false (shadow mode) the reports are built but nothing is
// sent, so the ledger can be checked before customers are charged.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { StripeApiError, type StripeClient } from "./stripe-api.ts";
import { must, rpc } from "../remote/db.ts";

export const DEFAULT_USAGE_METER_EVENT = "treq_sandbox_usage";
export const MAX_USAGE_REPORT_ATTEMPTS = 10;
/** A row still unsent this long after its hour ended misses the invoice it belongs to. */
const LATE_AFTER_MS = 60 * 60_000;

/** An unsent usage_reports row with its owner's Stripe customer, if any. */
export interface PendingUsageReport {
  identifier: string;
  owner_user_id: string;
  hour: string;
  billed_nanousd: number;
  stripe_customer_id: string | null;
  /** Failed send attempts so far. */
  attempts: number;
}

export type TerminalStatus = "no_customer" | "dead";

export interface UsageReportStore {
  buildReports(until: Date): Promise<number>;
  pendingReports(): Promise<PendingUsageReport[]>;
  markSent(identifier: string, stripeStatus: string): Promise<void>;
  markFailed(identifier: string, error: string, attempts: number): Promise<void>;
  markTerminal(identifier: string, status: TerminalStatus, error: string): Promise<void>;
}

export interface UsageReportDeps {
  store: UsageReportStore;
  stripe: StripeClient;
  now: () => Date;
  enabled: boolean;
  log: (fields: Record<string, unknown>) => void;
  eventName?: string;
}

export interface UsageReportResult {
  built: number;
  pending: number;
  sent: number;
  failed: number;
  skipped: number;
  dead: number;
  late: number;
  shadow: boolean;
}

export function microUsd(billedNanousd: number): number {
  return Math.round(billedNanousd / 1000);
}

export async function reportUsage(deps: UsageReportDeps): Promise<UsageReportResult> {
  const eventName = deps.eventName ?? DEFAULT_USAGE_METER_EVENT;
  const built = await deps.store.buildReports(deps.now());
  const pending = await deps.store.pendingReports();
  const result: UsageReportResult = {
    built,
    pending: pending.length,
    sent: 0,
    failed: 0,
    skipped: 0,
    dead: 0,
    late: 0,
    shadow: !deps.enabled,
  };
  const nowMs = deps.now().getTime();
  for (const row of pending) {
    if (nowMs - (Date.parse(row.hour) + 60 * 60_000) > LATE_AFTER_MS) {
      result.late++;
      deps.log({ level: "warn", msg: "usage report late", identifier: row.identifier, hour: row.hour });
    }
  }
  if (!deps.enabled) {
    deps.log({ level: "info", msg: "usage report shadow mode", ...result });
    return result;
  }

  for (const row of pending) {
    const value = microUsd(row.billed_nanousd);
    if (value <= 0) {
      await deps.store.markSent(row.identifier, "zero");
      continue;
    }
    if (!row.stripe_customer_id) {
      result.skipped++;
      await deps.store.markTerminal(row.identifier, "no_customer", "owner has no Stripe customer");
      deps.log({
        level: "error",
        alert: true,
        msg: "usage report owner has no Stripe customer",
        identifier: row.identifier,
        owner_user_id: row.owner_user_id,
      });
      continue;
    }
    try {
      await deps.stripe.request(
        "POST",
        "/v1/billing/meter_events",
        {
          event_name: eventName,
          identifier: row.identifier,
          timestamp: Math.floor(Date.parse(row.hour) / 1000),
          "payload[stripe_customer_id]": row.stripe_customer_id,
          "payload[value]": String(value),
        },
        { idempotencyKey: `treq-usage-${row.identifier}` },
      );
      await deps.store.markSent(row.identifier, "accepted");
      result.sent++;
    } catch (err) {
      const message = (err as Error).message;
      const attempts = row.attempts + 1;
      if (!retryable(err) || attempts >= MAX_USAGE_REPORT_ATTEMPTS) {
        await deps.store.markTerminal(row.identifier, "dead", message);
        result.dead++;
        deps.log({
          level: "error",
          alert: true,
          msg: "usage report dead",
          identifier: row.identifier,
          attempts,
          error: message,
        });
        continue;
      }
      await deps.store.markFailed(row.identifier, message, attempts);
      result.failed++;
      deps.log({ level: "error", msg: "usage report send failed", identifier: row.identifier, attempts, error: message });
    }
  }
  deps.log({ level: "info", msg: "usage report done", ...result });
  return result;
}

// A 4xx other than 409 (in-flight idempotent retry) and 429 (rate limit)
// will fail the same way next time. Network errors and 5xx are retried.
function retryable(err: unknown): boolean {
  if (!(err instanceof StripeApiError)) return true;
  return err.status >= 500 || err.status === 409 || err.status === 429 || err.status < 400;
}

/** UsageReportStore over the service-role Supabase client. */
export function supabaseUsageReportStore(supabase: SupabaseClient): UsageReportStore {
  return {
    async buildReports(until) {
      const data = await rpc(supabase, "build_usage_reports", {
        p_until: until.toISOString(),
      });
      return Number(data ?? 0);
    },
    async pendingReports() {
      const rows = must(await supabase
        .from("usage_reports")
        .select("identifier, owner_user_id, hour, billed_nanousd, attempts")
        .is("sent_at", null)
        .or("stripe_status.is.null,stripe_status.eq.error")
        .order("created_at")
        .limit(1000), "list usage reports");
      if (!rows?.length) return [];
      const owners = [...new Set(rows.map((r) => r.owner_user_id as string))];
      const customers = must(await supabase
        .from("billing_customers")
        .select("owner_id, stripe_customer_id")
        .eq("owner_type", "user")
        .in("owner_id", owners), "read billing customers");
      const byOwner = new Map(
        (customers ?? []).map((c) => [c.owner_id as string, c.stripe_customer_id as string]),
      );
      return rows.map((r) => ({
        identifier: r.identifier as string,
        owner_user_id: r.owner_user_id as string,
        hour: r.hour as string,
        billed_nanousd: Number(r.billed_nanousd),
        stripe_customer_id: byOwner.get(r.owner_user_id as string) ?? null,
        attempts: Number(r.attempts ?? 0),
      }));
    },
    async markSent(identifier, stripeStatus) {
      must(await supabase
        .from("usage_reports")
        .update({ sent_at: new Date().toISOString(), stripe_status: stripeStatus, error: null })
        .eq("identifier", identifier), "mark usage report sent");
    },
    async markFailed(identifier, message, attempts) {
      must(await supabase
        .from("usage_reports")
        .update({ stripe_status: "error", error: message, attempts })
        .eq("identifier", identifier), "mark usage report failed");
    },
    async markTerminal(identifier, status, message) {
      const { error } = await supabase
        .from("usage_reports")
        .update({ stripe_status: status, error: message })
        .eq("identifier", identifier);
      if (error) throw new Error(`failed to mark usage report ${status}: ${error.message}`);
    },
  };
}
