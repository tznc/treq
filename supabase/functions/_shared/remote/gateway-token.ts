// Tokens for treq-gateway, the WSS-to-TCP gateway that runs inside every
// gateway-provider machine (crates/treq-gateway). The relay sends one in the
// `x-treq-gateway-token` header on every connect.
//
// Format: `v1.<placement_id>.<expiry>.<mac>`, where `expiry` is Unix seconds
// and `mac` is lowercase hex HMAC-SHA256 over `v1.<placement_id>.<expiry>`,
// keyed with the placement's gateway secret. The gateway accepts a token only
// for its own placement and only while `now <= expiry <= now + 120`. It is
// checked once, at connect.
//
// A token is safe to hand to a client: it names the placement and expiry in
// the clear, and the MAC does not reveal the secret. Kept free of the relay
// so a ticket-minting action can reuse it. The Rust side
// (crates/treq-gateway/src/token.rs) checks the same test vectors.

export const GATEWAY_TOKEN_HEADER = "x-treq-gateway-token";
export const GATEWAY_TOKEN_MAX_TTL_SEC = 120;
/** Lifetime of a token the relay mints for its own connect. */
const GATEWAY_TOKEN_DEFAULT_TTL_SEC = 60;

export type GatewayTokenCheck =
  | "ok"
  | "malformed"
  | "wrong_placement"
  | "expired"
  | "too_far_in_future"
  | "bad_signature";

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

async function macHex(secret: string, placementId: string, expiry: number): Promise<string> {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(`v1.${placementId}.${expiry}`));
  return hex(new Uint8Array(sig));
}

/** A fresh per-placement secret: 32 random bytes as hex. */
export function generateGatewaySecret(): string {
  return hex(crypto.getRandomValues(new Uint8Array(32)));
}

export async function mintGatewayToken(
  secret: string,
  placementId: string,
  expiryUnixSec: number,
): Promise<string> {
  return `v1.${placementId}.${expiryUnixSec}.${await macHex(secret, placementId, expiryUnixSec)}`;
}

/** Mints a token valid for `ttlSec` (capped at the gateway's maximum) from now. */
export function mintGatewayTokenFor(
  secret: string,
  placementId: string,
  ttlSec = GATEWAY_TOKEN_DEFAULT_TTL_SEC,
  nowSec = Math.floor(Date.now() / 1000),
): Promise<string> {
  return mintGatewayToken(secret, placementId, nowSec + Math.min(ttlSec, GATEWAY_TOKEN_MAX_TTL_SEC));
}

function constantTimeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/** The same checks as the gateway, in the same order. */
export async function verifyGatewayToken(
  secret: string,
  placementId: string,
  token: string,
  nowSec: number,
  maxTtlSec = GATEWAY_TOKEN_MAX_TTL_SEC,
): Promise<GatewayTokenCheck> {
  const parts = token.split(".");
  if (parts.length !== 4) return "malformed";
  const [version, placement, expiryText, mac] = parts;
  if (version !== "v1" || mac.length !== 64 || !/^\d+$/.test(expiryText)) return "malformed";
  const expiry = Number(expiryText);
  if (!constantTimeEqual(await macHex(secret, placement, expiry), mac)) return "bad_signature";
  if (!constantTimeEqual(placement, placementId)) return "wrong_placement";
  if (expiry < nowSec) return "expired";
  if (expiry > nowSec + maxTtlSec) return "too_far_in_future";
  return "ok";
}
