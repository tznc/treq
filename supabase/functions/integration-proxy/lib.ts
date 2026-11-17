// Request logic for the integration-proxy Edge Function, kept free of Deno
// and Supabase imports so it runs under the repo's unit tests.
//
// Callers are either a signed-in user (Supabase JWT, checked with Auth) or
// an agent in a cloud sandbox. A sandbox signs its own short-lived token
// with its placement's integration secret (machine setup writes it to
// /etc/treq/integration.json; Vault keeps it, 078_integration_secrets.sql):
// `it1.<placement_id>.<expiry>.<mac>`, where `mac` is lowercase hex
// HMAC-SHA256 over `it1.<placement_id>.<expiry>`. It acts for the
// placement's owner, lives at most five minutes, and stops working when the
// placement dies. The treq CLI mints the same token (src-tauri/src/integration.rs).

export const SANDBOX_TOKEN_PREFIX = "it1";
export const SANDBOX_TOKEN_MAX_TTL_SEC = 300;

export type Caller =
  | { kind: "user"; userId: string }
  | { kind: "sandbox"; userId: string; placementId: string };

export type Result = { status: number; body: unknown };

export interface AuthDeps {
  /** The user id for a user's Supabase JWT, or null when it is not valid. */
  userFromJwt(jwt: string): Promise<string | null>;
  /** A live placement's integration secret and owner, or null. */
  placementSecret(placementId: string): Promise<{ secret: string; ownerUserId: string } | null>;
  nowSec?: () => number;
}

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
  const sig = await crypto.subtle.sign(
    "HMAC",
    key,
    enc.encode(`${SANDBOX_TOKEN_PREFIX}.${placementId}.${expiry}`),
  );
  return hex(new Uint8Array(sig));
}

function constantTimeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/** A sandbox token, as the treq CLI mints it. */
export async function mintSandboxToken(
  secret: string,
  placementId: string,
  ttlSec = SANDBOX_TOKEN_MAX_TTL_SEC,
  nowSec = Math.floor(Date.now() / 1000),
): Promise<string> {
  const expiry = nowSec + Math.min(ttlSec, SANDBOX_TOKEN_MAX_TTL_SEC);
  return `${SANDBOX_TOKEN_PREFIX}.${placementId}.${expiry}.${await macHex(secret, placementId, expiry)}`;
}

async function verifySandboxToken(token: string, deps: AuthDeps): Promise<Caller | null> {
  const parts = token.split(".");
  if (parts.length !== 4) return null;
  const [, placementId, expiryText, mac] = parts;
  if (mac.length !== 64 || !/^\d+$/.test(expiryText)) return null;
  const expiry = Number(expiryText);
  const now = (deps.nowSec ?? (() => Math.floor(Date.now() / 1000)))();
  if (expiry < now || expiry > now + SANDBOX_TOKEN_MAX_TTL_SEC) return null;

  const placement = await deps.placementSecret(placementId);
  if (!placement) return null;
  if (!constantTimeEqual(await macHex(placement.secret, placementId, expiry), mac)) return null;
  return { kind: "sandbox", userId: placement.ownerUserId, placementId };
}

/** Resolves the Authorization header to a caller, or null when it is not valid. */
export async function resolveCaller(
  authorization: string | undefined,
  deps: AuthDeps,
): Promise<Caller | null> {
  const token = (authorization ?? "").replace(/^Bearer\s+/i, "");
  if (token.startsWith(`${SANDBOX_TOKEN_PREFIX}.`)) return verifySandboxToken(token, deps);
  if (token.split(".").length !== 3) return null;
  const userId = await deps.userFromJwt(token);
  return userId ? { kind: "user", userId } : null;
}

/**
 * Linear and the user's own GitHub token are for the desktop and mobile
 * apps only: a sandbox would hold the user's whole grant, so sandbox
 * tokens are refused. Null when the caller may proceed.
 */
export function appOnly(caller: Caller): Result | null {
  if (caller.kind === "user") return null;
  return {
    status: 403,
    body: { error: "Not available from sandboxes", code: "sandbox_not_allowed" },
  };
}

/**
 * `owner/name` as an exact, case-insensitive ILIKE pattern: `_` (allowed
 * in repository names) is otherwise a single-character wildcard.
 */
export function exactIlike(value: string): string {
  return value.replace(/[\\%_*]/g, "\\$&");
}
