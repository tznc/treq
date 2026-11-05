// Unit tests for the remote-instance `cancel_migration` request
// (_shared/remote/migration-cancel.ts): the owner declines a migration
// parked in `awaiting_ack`. No network.
//
// Run with: `deno test supabase/functions/tests/remote_migration_cancel.test.ts`

import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { type CancelDeps, cancelParkedMigration } from "../_shared/remote/migration-cancel.ts";
import { PRO_GATED_ACTIONS } from "../remote-instance/lib.ts";

type Row = { id: string; instance_id: string; owner_user_id: string; state: string; reason: string };

function world(state = "awaiting_ack") {
  const row: Row = { id: "m1", instance_id: "i1", owner_user_id: "u1", state, reason: "drain" };
  const audits: Array<Record<string, unknown>> = [];
  const deps: CancelDeps = {
    ownerUserId: "u1",
    correlationId: "c1",
    getMigration: (id) => Promise.resolve(id === row.id ? { ...row } : null),
    advance: (id, expected, next) => {
      if (id !== row.id || row.state !== expected) return Promise.resolve(false);
      row.state = next;
      return Promise.resolve(true);
    },
    audit: (e) => {
      audits.push(e);
      return Promise.resolve();
    },
  };
  return { row, audits, deps };
}

const body = { migration_id: "m1", idempotency_key: "k1" };

Deno.test("the owner cancels a parked migration", async () => {
  const w = world();
  const r = await cancelParkedMigration(w.deps, body);
  assertEquals(r.status, 200);
  assertEquals(r.body.migration_state, "cancelled");
  assertEquals(w.row.state, "cancelled");
  assertEquals(w.audits.map((a) => a.eventType), ["migration_cancelled"]);
});

Deno.test("re-sending the cancel is harmless", async () => {
  const w = world();
  await cancelParkedMigration(w.deps, body);
  const again = await cancelParkedMigration(w.deps, body);
  assertEquals(again.status, 200);
  assertEquals(again.body.migration_state, "cancelled");
  assertEquals(w.audits.length, 1);
});

Deno.test("another user's migration is not found", async () => {
  const w = world();
  const r = await cancelParkedMigration({ ...w.deps, ownerUserId: "u2" }, body);
  assertEquals(r.status, 404);
  assertEquals(w.row.state, "awaiting_ack");
});

Deno.test("a running migration is not cancelled", async () => {
  const w = world("verifying");
  const r = await cancelParkedMigration(w.deps, body);
  assertEquals(r.status, 409);
  assertEquals(r.body.code, "migration_not_parked");
  assertEquals(r.body.migration_state, "verifying");
  assertEquals(w.row.state, "verifying");
});

Deno.test("a migration that moves on during the cancel is reported as it stands", async () => {
  const w = world();
  const deps = { ...w.deps, advance: () => { w.row.state = "target_provisioning"; return Promise.resolve(false); } };
  const r = await cancelParkedMigration(deps, body);
  assertEquals(r.status, 409);
  assertEquals(r.body.migration_state, "target_provisioning");
});

Deno.test("migration_id and idempotency_key are required", async () => {
  const w = world();
  assertEquals((await cancelParkedMigration(w.deps, { idempotency_key: "k" })).status, 400);
  assertEquals((await cancelParkedMigration(w.deps, { migration_id: "m1" })).status, 400);
});

Deno.test("declining a migration is open to Free users", () => {
  assertEquals(PRO_GATED_ACTIONS.has("cancel_migration"), false);
});
