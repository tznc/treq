// Request logic for the integration-proxy Edge Function, kept free of Deno
// and Supabase imports so it runs under the repo's unit tests.
//
// Callers are either a signed-in user (Supabase JWT) or an agent inside a
// cloud sandbox holding a sandbox token. A sandbox token is
// `it1.<placement_id>.<expiry>.<mac>`: `mac` is lowercase hex HMAC-SHA256
// over `it1.<placement_id>.<expiry>`, keyed with the placement's gateway
// secret (051_gateway_secrets.sql). The `it1` prefix keeps it from being
// accepted as a gateway token and the other way round. It acts as the
// placement's owner and cannot mint further tokens.

export const SANDBOX_TOKEN_PREFIX = "it1";
export const SANDBOX_TOKEN_MAX_TTL_SEC = 3600;

export type Caller =
  | { kind: "user"; userId: string }
  | { kind: "sandbox"; userId: string; placementId: string };

export type Result = { status: number; body: unknown };

export interface AuthDeps {
  /** The user id for a Supabase JWT, or null when it is not valid. */
  userFromJwt(jwt: string): Promise<string | null>;
  /** The placement's gateway secret and owner, or null when there is none. */
  placement(
    placementId: string,
  ): Promise<{ secret: string; ownerUserId: string } | null>;
  nowSec?: () => number;
}

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

async function macHex(
  secret: string,
  placementId: string,
  expiry: number,
): Promise<string> {
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

export async function mintSandboxToken(
  secret: string,
  placementId: string,
  ttlSec = SANDBOX_TOKEN_MAX_TTL_SEC,
  nowSec = Math.floor(Date.now() / 1000),
): Promise<{ token: string; expiresAt: number }> {
  const expiry = nowSec + Math.min(ttlSec, SANDBOX_TOKEN_MAX_TTL_SEC);
  const mac = await macHex(secret, placementId, expiry);
  return {
    token: `${SANDBOX_TOKEN_PREFIX}.${placementId}.${expiry}.${mac}`,
    expiresAt: expiry,
  };
}

/** Resolves the Authorization header to a caller, or null when it is not valid. */
export async function resolveCaller(
  authorization: string | undefined,
  deps: AuthDeps,
): Promise<Caller | null> {
  const bearer = (authorization ?? "").replace(/^Bearer\s+/i, "");
  if (!bearer) return null;

  if (!bearer.startsWith(`${SANDBOX_TOKEN_PREFIX}.`)) {
    const userId = await deps.userFromJwt(bearer);
    return userId ? { kind: "user", userId } : null;
  }

  const parts = bearer.split(".");
  if (parts.length !== 4) return null;
  const [, placementId, expiryText, mac] = parts;
  if (mac.length !== 64 || !/^\d+$/.test(expiryText)) return null;
  const expiry = Number(expiryText);
  const now = (deps.nowSec ?? (() => Math.floor(Date.now() / 1000)))();
  if (expiry < now || expiry > now + SANDBOX_TOKEN_MAX_TTL_SEC) return null;

  const placement = await deps.placement(placementId);
  if (!placement) return null;
  const expected = await macHex(placement.secret, placementId, expiry);
  if (!constantTimeEqual(expected, mac)) return null;
  return { kind: "sandbox", userId: placement.ownerUserId, placementId };
}

// ── GitHub ──────────────────────────────────────────────────────────────

export interface GitHubDeps {
  /** The installation linked to `userId` that can see `owner/name`, if any. */
  installationFor(userId: string, fullName: string): Promise<number | null>;
  appJwt(): Promise<string>;
  fetch: typeof fetch;
}

const REPO_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;

/**
 * Mints a GitHub App installation token limited to one repository the
 * caller's linked installation can see. GitHub expires it after an hour.
 */
export async function githubRepoToken(
  userId: string,
  repository: unknown,
  deps: GitHubDeps,
): Promise<Result> {
  if (typeof repository !== "string" || !REPO_PATTERN.test(repository)) {
    return { status: 400, body: { error: "repository must be owner/name" } };
  }
  const installationId = await deps.installationFor(userId, repository);
  if (installationId === null) {
    return {
      status: 403,
      body: { error: "The treq GitHub App is not installed for this repository" },
    };
  }

  const res = await deps.fetch(
    `https://api.github.com/app/installations/${installationId}/access_tokens`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${await deps.appJwt()}`,
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "treq-github-app/1.0",
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ repositories: [repository.split("/")[1]] }),
    },
  );
  if (!res.ok) {
    console.error("[integration-proxy] GitHub token mint failed:", res.status);
    return { status: 502, body: { error: "Failed to mint GitHub token" } };
  }
  const grant = await res.json();
  return {
    status: 200,
    body: { token: grant.token, expires_at: grant.expires_at },
  };
}
