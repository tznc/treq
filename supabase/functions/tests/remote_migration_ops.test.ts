// Unit tests for the drain scheduling and parked-migration helpers
// (_shared/remote/drain.ts, _shared/remote/migration-resume.ts). No network.
//
// Run with: `deno test supabase/functions/tests/remote_migration_ops.test.ts`

import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { drainAllowance, migrationCaps } from "../_shared/remote/drain.ts";
import { isOverdueParked, migrationSummary } from "../_shared/remote/migration-resume.ts";

Deno.test("migration caps default to 10 per provider and 50 overall", () => {
  assertEquals(migrationCaps(() => undefined), { perProvider: 10, total: 50 });
});

Deno.test("migration caps read the environment and ignore bad values", () => {
  const env: Record<string, string> = { TREQ_MIGRATION_MAX_OPEN_PER_PROVIDER: "3", TREQ_MIGRATION_MAX_OPEN: "x" };
  assertEquals(migrationCaps((k) => env[k]), { perProvider: 3, total: 50 });
  const zero: Record<string, string> = { TREQ_MIGRATION_MAX_OPEN: "0" };
  assertEquals(migrationCaps((k) => zero[k]), { perProvider: 10, total: 50 });
});

Deno.test("the drain allowance is the smallest of the batch and both caps", () => {
  const caps = { perProvider: 10, total: 50 };
  assertEquals(drainAllowance({ batch: 25, openForProvider: 0, openTotal: 0, caps }), 10);
  assertEquals(drainAllowance({ batch: 4, openForProvider: 0, openTotal: 0, caps }), 4);
  assertEquals(drainAllowance({ batch: 25, openForProvider: 7, openTotal: 7, caps }), 3);
  assertEquals(drainAllowance({ batch: 25, openForProvider: 2, openTotal: 48, caps }), 2);
  assertEquals(drainAllowance({ batch: 25, openForProvider: 12, openTotal: 12, caps }), 0);
  assertEquals(drainAllowance({ batch: 25, openForProvider: 0, openTotal: 60, caps }), 0);
});

const NOW = Date.parse("2026-10-28T12:00:00Z");

Deno.test("a parked drain or retire past its deadline is overdue", () => {
  const base = { state: "awaiting_ack", reason: "drain", deadline: "2026-10-27T00:00:00Z" };
  assertEquals(isOverdueParked(base, NOW), true);
  assertEquals(isOverdueParked({ ...base, reason: "retire" }, NOW), true);
  assertEquals(isOverdueParked({ ...base, deadline: "2026-10-29T00:00:00Z" }, NOW), false);
  assertEquals(isOverdueParked({ ...base, deadline: null }, NOW), false);
  assertEquals(isOverdueParked({ ...base, reason: "user_resize" }, NOW), false);
  assertEquals(isOverdueParked({ ...base, state: "verifying" }, NOW), false);
});

Deno.test("the migration summary carries what the app needs to prompt", () => {
  assertEquals(
    migrationSummary({
      id: "m1",
      state: "awaiting_ack",
      reason: "drain",
      deadline: "2026-11-04T00:00:00Z",
      preflight: { repos: [], requires_ack: true },
    }),
    {
      id: "m1",
      state: "awaiting_ack",
      reason: "drain",
      deadline: "2026-11-04T00:00:00Z",
      preflight: { repos: [], requires_ack: true },
    },
  );
  assertEquals(migrationSummary(null), null);
});
