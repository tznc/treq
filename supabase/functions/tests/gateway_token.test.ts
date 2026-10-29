// Gateway token mint/verify. The vectors are shared with
// crates/treq-gateway/src/token.rs so both sides agree byte for byte.

import { assertEquals, assertMatch, assertNotEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  GATEWAY_TOKEN_MAX_TTL_SEC,
  generateGatewaySecret,
  mintGatewayToken,
  verifyGatewayToken,
} from "../_shared/remote/gateway-token.ts";

const SECRET = "test-secret-0123456789abcdef";
const PLACEMENT = "11111111-2222-3333-4444-555555555555";
const VECTOR =
  "v1.11111111-2222-3333-4444-555555555555.1700000120.de3bbc5d0ee1ddca9929f572086391dca3b8e6897df5ec790edbcfd993475d9f";

Deno.test("mints the shared test vector", async () => {
  assertEquals(await mintGatewayToken(SECRET, PLACEMENT, 1_700_000_120), VECTOR);
});

Deno.test("verifies the shared test vector, and rejects it once expired or too far out", async () => {
  assertEquals(await verifyGatewayToken(SECRET, PLACEMENT, VECTOR, 1_700_000_000), "ok");
  assertEquals(await verifyGatewayToken(SECRET, PLACEMENT, VECTOR, 1_700_000_121), "expired");
  assertEquals(await verifyGatewayToken(SECRET, PLACEMENT, VECTOR, 1_699_999_999), "too_far_in_future");
});

Deno.test("rejects a forged, misplaced or malformed token", async () => {
  const forged = await mintGatewayToken("wrong", PLACEMENT, 1_700_000_120);
  assertEquals(await verifyGatewayToken(SECRET, PLACEMENT, forged, 1_700_000_000), "bad_signature");
  const other = await mintGatewayToken(SECRET, "other", 1_700_000_120);
  assertEquals(await verifyGatewayToken(SECRET, PLACEMENT, other, 1_700_000_000), "wrong_placement");
  assertEquals(await verifyGatewayToken(SECRET, PLACEMENT, "garbage", 0), "malformed");
});

Deno.test("the token never contains the secret, and the TTL cap is 120 s", async () => {
  const token = await mintGatewayToken(SECRET, PLACEMENT, 1_700_000_120);
  assertEquals(token.includes(SECRET), false);
  assertEquals(GATEWAY_TOKEN_MAX_TTL_SEC, 120);
});

Deno.test("generated secrets are 64 hex chars and distinct", () => {
  const a = generateGatewaySecret();
  assertMatch(a, /^[0-9a-f]{64}$/);
  assertNotEquals(a, generateGatewaySecret());
});
