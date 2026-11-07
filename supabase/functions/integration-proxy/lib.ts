// Request logic for the integration-proxy Edge Function, kept free of Deno
// and Supabase imports so it runs under the repo's unit tests.
//
// Callers are either a signed-in user or an agent inside a cloud sandbox.
// Both send a Supabase JWT. A sandbox JWT is minted here, HS256 with the
// project's JWT secret, so the Data API and RLS accept it as the sandbox's
// owner (`role: authenticated`, `sub` = owner). Its `treq_placement_id`
// claim marks it as a sandbox token, for RLS and for refusing it on
// /sandbox-token. It lives at most an hour and is not tied to an Auth
// session, so it is checked locally here rather than through Auth.

export const SANDBOX_CLAIM = "treq_placement_id";
export const SANDBOX_TOKEN_MAX_TTL_SEC = 3600;

export type Caller =
  | { kind: "user"; userId: string }
  | { kind: "sandbox"; userId: string; placementId: string };

export type Result = { status: number; body: unknown };

export interface AuthDeps {
  /** The project's JWT secret, used for sandbox JWTs. */
  jwtSecret: string;
  /** The user id for a user's Supabase JWT, or null when it is not valid. */
  userFromJwt(jwt: string): Promise<string | null>;
  /** The live placement's owner, or null when there is none. */
  placementOwner(placementId: string): Promise<string | null>;
  nowSec?: () => number;
}

function base64url(bytes: Uint8Array): string {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function decodeSegment(segment: string): Record<string, unknown> | null {
  try {
    const b64 = segment.replace(/-/g, "+").replace(/_/g, "/");
    const binary = atob(b64 + "=".repeat((4 - (b64.length % 4)) % 4));
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    const value = JSON.parse(new TextDecoder().decode(bytes));
    return value && typeof value === "object" ? value : null;
  } catch {
    return null;
  }
}

async function hs256(secret: string, input: string): Promise<string> {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  return base64url(new Uint8Array(await crypto.subtle.sign("HMAC", key, enc.encode(input))));
}

function constantTimeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

export async function mintSandboxJwt(
  secret: string,
  ownerUserId: string,
  placementId: string,
  ttlSec = SANDBOX_TOKEN_MAX_TTL_SEC,
  nowSec = Math.floor(Date.now() / 1000),
): Promise<{ token: string; expiresAt: number }> {
  const exp = nowSec + Math.min(ttlSec, SANDBOX_TOKEN_MAX_TTL_SEC);
  const enc = new TextEncoder();
  const header = base64url(enc.encode(JSON.stringify({ alg: "HS256", typ: "JWT" })));
  const payload = base64url(enc.encode(JSON.stringify({
    sub: ownerUserId,
    role: "authenticated",
    aud: "authenticated",
    iat: nowSec,
    exp,
    is_anonymous: false,
    [SANDBOX_CLAIM]: placementId,
  })));
  const input = `${header}.${payload}`;
  return { token: `${input}.${await hs256(secret, input)}`, expiresAt: exp };
}

async function verifySandboxJwt(
  jwt: string,
  deps: AuthDeps,
): Promise<Caller | null> {
  const [header, payload, sig] = jwt.split(".");
  const h = decodeSegment(header);
  const p = decodeSegment(payload);
  if (!deps.jwtSecret || h?.alg !== "HS256" || !p || !sig) return null;
  if (!constantTimeEqual(await hs256(deps.jwtSecret, `${header}.${payload}`), sig)) return null;

  const now = (deps.nowSec ?? (() => Math.floor(Date.now() / 1000)))();
  const { sub, exp, iat, role } = p;
  const placementId = p[SANDBOX_CLAIM];
  if (
    typeof sub !== "string" || typeof placementId !== "string" ||
    typeof exp !== "number" || typeof iat !== "number" || role !== "authenticated" ||
    exp < now || exp - iat > SANDBOX_TOKEN_MAX_TTL_SEC
  ) return null;

  // A sandbox token dies with its placement, or if the placement changes hands.
  if ((await deps.placementOwner(placementId)) !== sub) return null;
  return { kind: "sandbox", userId: sub, placementId };
}

/** Resolves the Authorization header to a caller, or null when it is not valid. */
export async function resolveCaller(
  authorization: string | undefined,
  deps: AuthDeps,
): Promise<Caller | null> {
  const jwt = (authorization ?? "").replace(/^Bearer\s+/i, "");
  if (jwt.split(".").length !== 3) return null;
  if (decodeSegment(jwt.split(".")[1])?.[SANDBOX_CLAIM] !== undefined) {
    return verifySandboxJwt(jwt, deps);
  }
  const userId = await deps.userFromJwt(jwt);
  return userId ? { kind: "user", userId } : null;
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
