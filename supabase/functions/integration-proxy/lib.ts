// Request logic for the integration-proxy Edge Function, kept free of Deno
// and Supabase imports so it runs under the repo's unit tests.
//
// Callers are either a signed-in user (Supabase JWT, checked with Auth) or
// an agent inside a cloud sandbox holding a sandbox token. A sandbox token
// is an HS256 JWT signed with INTEGRATION_PROXY_SECRET, not the project's
// JWT secret, so no other Supabase service accepts it: `sub` is the
// placement id and `aud` is "treq-integration-proxy". It acts for the
// placement's owner, lives at most an hour, dies with its placement, and
// cannot mint further tokens.

export const SANDBOX_AUD = "treq-integration-proxy";
export const SANDBOX_TOKEN_MAX_TTL_SEC = 3600;

export type Caller =
  | { kind: "user"; userId: string }
  | { kind: "sandbox"; userId: string; placementId: string };

export type Result = { status: number; body: unknown };

export interface AuthDeps {
  /** INTEGRATION_PROXY_SECRET, which signs sandbox tokens. */
  sandboxSecret: string;
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
  placementId: string,
  ttlSec = SANDBOX_TOKEN_MAX_TTL_SEC,
  nowSec = Math.floor(Date.now() / 1000),
): Promise<{ token: string; expiresAt: number }> {
  const exp = nowSec + Math.min(ttlSec, SANDBOX_TOKEN_MAX_TTL_SEC);
  const enc = new TextEncoder();
  const header = base64url(enc.encode(JSON.stringify({ alg: "HS256", typ: "JWT" })));
  const payload = base64url(enc.encode(JSON.stringify({
    sub: placementId,
    aud: SANDBOX_AUD,
    iat: nowSec,
    exp,
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
  if (!deps.sandboxSecret || h?.alg !== "HS256" || !p || !sig) return null;
  if (!constantTimeEqual(await hs256(deps.sandboxSecret, `${header}.${payload}`), sig)) return null;

  const now = (deps.nowSec ?? (() => Math.floor(Date.now() / 1000)))();
  const { sub: placementId, exp, iat } = p;
  if (
    typeof placementId !== "string" || typeof exp !== "number" || typeof iat !== "number" ||
    exp < now || exp - iat > SANDBOX_TOKEN_MAX_TTL_SEC
  ) return null;

  // A sandbox token dies with its placement.
  const owner = await deps.placementOwner(placementId);
  return owner ? { kind: "sandbox", userId: owner, placementId } : null;
}

/** Resolves the Authorization header to a caller, or null when it is not valid. */
export async function resolveCaller(
  authorization: string | undefined,
  deps: AuthDeps,
): Promise<Caller | null> {
  const jwt = (authorization ?? "").replace(/^Bearer\s+/i, "");
  if (jwt.split(".").length !== 3) return null;
  if (decodeSegment(jwt.split(".")[1])?.aud === SANDBOX_AUD) {
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

// What an agent may do with a minted token, narrower than the App's own
// permissions. Workflow files are read through `contents`; GitHub's
// `workflows` permission only grants write, so it is left out.
export const TOKEN_PERMISSIONS = {
  contents: "write",
  pull_requests: "write",
  metadata: "read",
  actions: "read",
} as const;

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
      body: JSON.stringify({
        repositories: [repository.split("/")[1]],
        permissions: TOKEN_PERMISSIONS,
      }),
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
