// Unit tests for the hourly Stripe usage report (docs/design/sandbox-provider-adapter.md,
// section 5 and D7). A fake store and a fake Stripe client; no network.
//
// Run with: `deno test --allow-env supabase/functions/tests/usage_report.test.ts`

import { defined } from "./remote_fakes.ts";
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { StripeApiError, type StripeClient, type StripeParams } from "../_shared/billing/stripe-api.ts";
import {
  MAX_USAGE_REPORT_ATTEMPTS,
  type PendingUsageReport,
  reportUsage,
  type UsageReportStore,
} from "../_shared/billing/usage-report.ts";

const NOW = new Date("2026-10-28T12:05:00Z");
const USER = "6f1c1f4e-6a8f-4a39-9d55-2b1f0b3f9a10";

function row(overrides: Partial<PendingUsageReport> = {}): PendingUsageReport {
  return {
    identifier: `${USER}:2026-10-28T10`,
    owner_user_id: USER,
    hour: "2026-10-28T10:00:00+00:00",
    billed_nanousd: 21_600_400, // $0.0216004 -> 21600 micro-USD
    stripe_customer_id: "cus_1",
    attempts: 0,
    ...overrides,
  };
}

function fakeStore(initial: PendingUsageReport[]) {
  const rows = initial.map((r) => ({
    ...r,
    sent: false,
    error: null as string | null,
    terminal: null as string | null,
  }));
  const built: Date[] = [];
  const store: UsageReportStore = {
    buildReports: async (until) => {
      built.push(until);
      return rows.length;
    },
    pendingReports: async () => rows.filter((r) => !r.sent && !r.terminal),
    markSent: async (identifier) => {
      const r = defined(rows.find((x) => x.identifier === identifier));
      r.sent = true;
      r.error = null;
    },
    markFailed: async (identifier, error, attempts) => {
      const r = defined(rows.find((x) => x.identifier === identifier));
      r.error = error;
      r.attempts = attempts;
    },
    markTerminal: async (identifier, status, error) => {
      const r = defined(rows.find((x) => x.identifier === identifier));
      r.terminal = status;
      r.error = error;
    },
  };
  return { store, rows, built };
}

type Call = { method: string; path: string; params?: StripeParams; idempotencyKey?: string };

function fakeStripe(failTimes = 0, failWith: () => Error = () => new Error("stripe down")) {
  const calls: Call[] = [];
  let failures = failTimes;
  const stripe = {
    request: async (
      method: string,
      path: string,
      params?: StripeParams,
      options?: { idempotencyKey?: string },
    ) => {
      calls.push({ method, path, params, idempotencyKey: options?.idempotencyKey });
      if (failures > 0) {
        failures--;
        throw failWith();
      }
      return { object: "billing.meter_event" };
    },
  } as unknown as StripeClient;
  return { stripe, calls };
}

const logs: Record<string, unknown>[] = [];
const log = (f: Record<string, unknown>) => {
  logs.push(f);
};

Deno.test("shadow mode builds reports but sends nothing", async () => {
  const { store, rows, built } = fakeStore([row()]);
  const { stripe, calls } = fakeStripe();
  const result = await reportUsage({ store, stripe, now: () => NOW, enabled: false, log });
  assertEquals(built, [NOW]);
  assertEquals(calls.length, 0);
  assertEquals(rows[0].sent, false);
  assertEquals(result.sent, 0);
});

Deno.test("enabled sends one meter event per row", async () => {
  const second = row({
    identifier: `${USER}:2026-10-28T11`,
    hour: "2026-10-28T11:00:00Z",
    billed_nanousd: 1_499,
  });
  const { store, rows } = fakeStore([row(), second]);
  const { stripe, calls } = fakeStripe();
  const result = await reportUsage({ store, stripe, now: () => NOW, enabled: true, log });
  assertEquals(calls.length, 2);
  assertEquals(calls[0], {
    method: "POST",
    path: "/v1/billing/meter_events",
    params: {
      event_name: "treq_sandbox_usage",
      identifier: `${USER}:2026-10-28T10`,
      timestamp: Date.parse("2026-10-28T10:00:00Z") / 1000,
      "payload[stripe_customer_id]": "cus_1",
      "payload[value]": "21600",
    },
    idempotencyKey: `treq-usage-${USER}:2026-10-28T10`,
  });
  assertEquals(calls[1].params?.["payload[value]"], "1");
  assertEquals(rows.every((r) => r.sent), true);
  assertEquals(result.sent, 2);
});

Deno.test("uses the configured event name", async () => {
  const { store } = fakeStore([row()]);
  const { stripe, calls } = fakeStripe();
  await reportUsage({ store, stripe, now: () => NOW, enabled: true, log, eventName: "custom" });
  assertEquals(calls[0].params?.event_name, "custom");
});

Deno.test("a failure is recorded and retried with the same identifier", async () => {
  const { store, rows } = fakeStore([row()]);
  const { stripe, calls } = fakeStripe(1);
  const first = await reportUsage({ store, stripe, now: () => NOW, enabled: true, log });
  assertEquals(first.failed, 1);
  assertEquals(rows[0].sent, false);
  assertEquals(rows[0].error, "stripe down");

  const second = await reportUsage({ store, stripe, now: () => NOW, enabled: true, log });
  assertEquals(second.sent, 1);
  assertEquals(rows[0].sent, true);
  assertEquals(calls.length, 2);
  assertEquals(calls[0].params?.identifier, calls[1].params?.identifier);
  assertEquals(calls[0].idempotencyKey, calls[1].idempotencyKey);
});

Deno.test("a row without a Stripe customer becomes terminal and alerts", async () => {
  logs.length = 0;
  const { store, rows } = fakeStore([row({ stripe_customer_id: null })]);
  const { stripe, calls } = fakeStripe();
  const result = await reportUsage({ store, stripe, now: () => NOW, enabled: true, log });
  assertEquals(calls.length, 0);
  assertEquals(rows[0].sent, false);
  assertEquals(rows[0].terminal, "no_customer");
  assertEquals(result.skipped, 1);
  assertEquals(logs.some((l) => l.level === "error" && l.alert === true && l.identifier === row().identifier), true);
  // It is not picked up again.
  assertEquals((await store.pendingReports()).length, 0);
});

Deno.test("a non-retryable 4xx marks the row dead", async () => {
  logs.length = 0;
  const { store, rows } = fakeStore([row()]);
  const { stripe } = fakeStripe(1, () => new StripeApiError(400, "No such customer"));
  const result = await reportUsage({ store, stripe, now: () => NOW, enabled: true, log });
  assertEquals(rows[0].terminal, "dead");
  assertEquals(result.dead, 1);
  assertEquals(logs.some((l) => l.level === "error" && l.alert === true), true);
});

Deno.test("a 429 is retried and counts an attempt", async () => {
  const { store, rows } = fakeStore([row()]);
  const { stripe } = fakeStripe(1, () => new StripeApiError(429, "rate limited"));
  const result = await reportUsage({ store, stripe, now: () => NOW, enabled: true, log });
  assertEquals(rows[0].terminal, null);
  assertEquals(rows[0].attempts, 1);
  assertEquals(result.failed, 1);
});

Deno.test("the last allowed attempt failing marks the row dead", async () => {
  const { store, rows } = fakeStore([row({ attempts: MAX_USAGE_REPORT_ATTEMPTS - 1 })]);
  const { stripe } = fakeStripe(1);
  await reportUsage({ store, stripe, now: () => NOW, enabled: true, log });
  assertEquals(rows[0].terminal, "dead");
});

Deno.test("a row unsent more than an hour after its hour ends is logged as late", async () => {
  logs.length = 0;
  const { store } = fakeStore([
    row(), // hour 10:00, ended 11:00, now 12:05
    row({ identifier: `${USER}:2026-10-28T11`, hour: "2026-10-28T11:00:00Z" }),
  ]);
  const { stripe } = fakeStripe();
  await reportUsage({ store, stripe, now: () => NOW, enabled: true, log });
  const late = logs.filter((l) => l.msg === "usage report late");
  assertEquals(late.length, 1);
  assertEquals(late[0].level, "warn");
  assertEquals(late[0].identifier, row().identifier);
});

Deno.test("a row that rounds to zero is marked sent without an event", async () => {
  const { store, rows } = fakeStore([row({ billed_nanousd: 400 })]);
  const { stripe, calls } = fakeStripe();
  await reportUsage({ store, stripe, now: () => NOW, enabled: true, log });
  assertEquals(calls.length, 0);
  assertEquals(rows[0].sent, true);
});
