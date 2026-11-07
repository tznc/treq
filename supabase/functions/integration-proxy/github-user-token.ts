// POST /github/user-token: a GitHub App user access token for the signed-in
// user, so the desktop and mobile apps call GitHub as that user (their
// comments, reviews and approvals are theirs, and GitHub applies their own
// repository access). The grant lives in Vault (github_user_tokens); a
// token within 10 minutes of expiry is refreshed first and the rotated
// grant saved. Kept free of Deno and Supabase imports for the unit tests.

import { parseUserGrant, type UserGrant } from "../_shared/github/user-token.ts";
import type { Result } from "./lib.ts";

export type StoredGrant = UserGrant & { updatedAt: string };

export interface UserTokenDeps {
  store: {
    load(): Promise<StoredGrant | null>;
    save(grant: UserGrant): Promise<void>;
  };
  fetch: typeof fetch;
  /** The App's OAuth client, needed to refresh. */
  appClient: { id: string; secret: string } | null;
  now?: () => number;
}

export const GITHUB_RECONNECT =
  "GitHub authorization expired. Reconnect the treq GitHub App from the dashboard.";

const REFRESH_MARGIN_MS = 10 * 60_000;

const reconnect = (): Result => ({
  status: 401,
  body: { error: GITHUB_RECONNECT, code: "github_reconnect" },
});

const usable = (grant: UserGrant, now: number) =>
  grant.expiresAt === null || Date.parse(grant.expiresAt) - now > REFRESH_MARGIN_MS;

const issued = (grant: UserGrant): Result => ({
  status: 200,
  body: { token: grant.accessToken, expires_at: grant.expiresAt },
});

export async function githubUserToken(deps: UserTokenDeps): Promise<Result> {
  const now = (deps.now ?? Date.now)();
  const grant = await deps.store.load();
  if (!grant) return reconnect();
  if (usable(grant, now)) return issued(grant);

  const refreshable = grant.refreshToken &&
    (grant.refreshExpiresAt === null || Date.parse(grant.refreshExpiresAt) > now);
  if (!refreshable || !deps.appClient) return reconnect();

  const res = await deps.fetch("https://github.com/login/oauth/access_token", {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/x-www-form-urlencoded",
      "User-Agent": "treq-github-app/1.0",
    },
    body: new URLSearchParams({
      client_id: deps.appClient.id,
      client_secret: deps.appClient.secret,
      grant_type: "refresh_token",
      refresh_token: grant.refreshToken as string,
    }).toString(),
  });
  if (!res.ok) {
    console.error("[integration-proxy] GitHub user token refresh failed:", res.status);
    await res.body?.cancel();
    return { status: 502, body: { error: "Failed to refresh the GitHub token" } };
  }

  let fresh: UserGrant;
  try {
    fresh = parseUserGrant(await res.json(), now);
  } catch {
    // A concurrent request may have refreshed first, which spends the
    // refresh token this one sent. Use its result if there is one.
    const latest = await deps.store.load();
    if (latest && latest.updatedAt !== grant.updatedAt && usable(latest, now)) {
      return issued(latest);
    }
    return reconnect();
  }
  try {
    await deps.store.save(fresh);
  } catch (err) {
    // GitHub already spent the old refresh token, so the user will have to
    // reconnect once this token expires; this request still gets it.
    console.error(
      "[integration-proxy] storing the refreshed GitHub token failed:",
      err instanceof Error ? err.message : String(err),
    );
  }
  return issued(fresh);
}
