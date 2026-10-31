// Unit tests for the remote-instance `resize` request: body validation and
// the decision on what to do with it. No network.
//
// Run with: `deno test supabase/functions/tests/remote_resize.test.ts`

import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { decideResize, parseResizeRequest } from "../_shared/remote/resize.ts";

Deno.test("a valid resize request is parsed", () => {
  assertEquals(
    parseResizeRequest({ tier_id: "m", idempotency_key: "k", instance_id: "i", acknowledged_data_loss: true }),
    { ok: true, value: { tierId: "m", idempotencyKey: "k", instanceId: "i", acknowledgedDataLoss: true } },
  );
});

Deno.test("acknowledgement defaults to false and instance_id is optional", () => {
  assertEquals(parseResizeRequest({ tier_id: "l", idempotency_key: "k" }), {
    ok: true,
    value: { tierId: "l", idempotencyKey: "k", instanceId: null, acknowledgedDataLoss: false },
  });
});

Deno.test("bad resize requests are rejected with a reason", () => {
  const cases: Array<[Record<string, unknown>, string]> = [
    [{ tier_id: "m" }, "idempotency_key is required"],
    [{ idempotency_key: "k" }, "tier_id is required"],
    [{ tier_id: "", idempotency_key: "k" }, "tier_id is required"],
    [{ tier_id: "M; drop", idempotency_key: "k" }, "tier_id is invalid"],
    [{ tier_id: "m", idempotency_key: "k", acknowledged_data_loss: "yes" }, "acknowledged_data_loss must be a boolean"],
    [{ tier_id: "m", idempotency_key: "k", instance_id: 3 }, "instance_id must be a string"],
  ];
  for (const [body, error] of cases) {
    assertEquals(parseResizeRequest(body), { ok: false, error }, JSON.stringify(body));
  }
});

const clean = { repos: [], requires_ack: false };
const dirty = { repos: [], requires_ack: true };

Deno.test("resizing to the current machine type is a no-op", () => {
  assertEquals(
    decideResize({ current: "boat:small", target: "boat:small", openMigrationId: null, preflight: null, ack: false }),
    { kind: "unchanged" },
  );
});

Deno.test("an open migration blocks a new resize", () => {
  assertEquals(
    decideResize({ current: "boat:small", target: "boat:default", openMigrationId: "m1", preflight: clean, ack: true }),
    { kind: "in_progress", migrationId: "m1" },
  );
});

Deno.test("unpushed work needs acknowledgement", () => {
  assertEquals(
    decideResize({ current: "a", target: "b", openMigrationId: null, preflight: dirty, ack: false }),
    { kind: "needs_ack" },
  );
  assertEquals(
    decideResize({ current: "a", target: "b", openMigrationId: null, preflight: dirty, ack: true }),
    { kind: "migrate" },
  );
  assertEquals(
    decideResize({ current: "a", target: "b", openMigrationId: null, preflight: clean, ack: false }),
    { kind: "migrate" },
  );
});
