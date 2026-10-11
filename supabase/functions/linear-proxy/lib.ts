// Request logic for the linear-proxy Edge Function, kept free of Deno and
// Supabase imports so it runs under the repo's unit tests. The caller
// supplies the user's token store, `fetch`, the Linear OAuth client, and the
// user's Pro check.

import {
  PRO_REQUIRED_MESSAGES,
  proRequired,
} from "../_shared/billing/entitlement.ts";
import type { ServiceClientLike } from "../_shared/billing/store.ts";

export type StoredToken = {
  access_token: string;
  refresh_token: string | null;
  expires_at: string | null;
  updated_at: string;
};

export type TokenUpdate = Omit<StoredToken, "updated_at">;

/** The signed-in user's row in linear_oauth_tokens. */
export interface TokenStore {
  load(): Promise<StoredToken | null>;
  save(update: TokenUpdate): Promise<void>;
}

/** TokenStore through a service-role client. */
export function linearTokenStore(
  client: ServiceClientLike,
  userId: string,
): TokenStore {
  return {
    load: async () => {
      const { data, error } = await client
        .from("linear_oauth_tokens")
        .select("access_token, refresh_token, expires_at, updated_at")
        .eq("user_id", userId)
        .maybeSingle();
      if (error) throw new Error(error.message);
      return (data as StoredToken | null) ?? null;
    },
    save: async (update) => {
      const { error } = await client
        .from("linear_oauth_tokens")
        .update({ ...update, updated_at: new Date().toISOString() })
        .eq("user_id", userId);
      if (error) throw new Error(error.message);
    },
  };
}

export interface ProxyDeps {
  store: TokenStore;
  fetch: typeof fetch;
  linearClient: { id: string; secret: string } | null;
  /** has_pro for the signed-in user. Linear OAuth is a Pro feature. */
  hasPro: () => Promise<boolean>;
  now?: () => number;
}

export type ProxyResult = { status: number; body: string };

export const RECONNECT_MESSAGE =
  "Linear authorization expired. Reconnect Linear in treq settings.";

const LINEAR_GRAPHQL_URL = "https://api.linear.app/graphql";
const LINEAR_TOKEN_URL = "https://api.linear.app/oauth/token";

// Refresh a little early so the token cannot expire mid-request.
const REFRESH_MARGIN_MS = 60_000;

function json(body: unknown, status: number): ProxyResult {
  return { status, body: JSON.stringify(body) };
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function needsRefresh(token: StoredToken, now: number): boolean {
  if (!token.expires_at) return false;
  return Date.parse(token.expires_at) - now < REFRESH_MARGIN_MS;
}

// Returns a usable access token, or null when the user must reconnect.
async function refreshToken(
  token: StoredToken,
  deps: ProxyDeps,
  now: number,
): Promise<string | null> {
  if (!token.refresh_token || !deps.linearClient) return null;

  const res = await deps.fetch(LINEAR_TOKEN_URL, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      client_id: deps.linearClient.id,
      client_secret: deps.linearClient.secret,
      refresh_token: token.refresh_token,
      grant_type: "refresh_token",
    }).toString(),
  });

  if (!res.ok) {
    console.error("[linear-proxy] token refresh failed:", res.status);
    // A concurrent request may have refreshed first, which spends the
    // refresh token this request used. Use its result if there is one.
    const latest = await deps.store.load();
    if (
      latest && latest.updated_at !== token.updated_at &&
      !needsRefresh(latest, now)
    ) {
      return latest.access_token;
    }
    return null;
  }

  const grant = await res.json();
  if (!grant.access_token) return null;
  try {
    await deps.store.save({
      access_token: grant.access_token,
      refresh_token: grant.refresh_token ?? token.refresh_token,
      expires_at: grant.expires_in
        ? new Date(now + grant.expires_in * 1000).toISOString()
        : null,
    });
  } catch (err) {
    console.error(
      "[linear-proxy] storing refreshed token failed:",
      errorMessage(err),
    );
  }
  return grant.access_token as string;
}

export async function proxyLinearRequest(
  request: { query: string; variables?: unknown },
  deps: ProxyDeps,
): Promise<ProxyResult> {
  const now = (deps.now ?? Date.now)();

  // A Free user gets 402 and a message pointing at the personal API key,
  // which the desktop app uses directly without this proxy.
  try {
    if (!(await deps.hasPro())) {
      const refused = proRequired(PRO_REQUIRED_MESSAGES.linearOAuth);
      return json(refused.body, refused.status);
    }
  } catch (err) {
    console.error("[linear-proxy] entitlement check failed:", errorMessage(err));
    return json({ error: "Failed to check your plan" }, 500);
  }

  let token: StoredToken | null;
  try {
    token = await deps.store.load();
  } catch (err) {
    console.error("[linear-proxy] token lookup failed:", errorMessage(err));
    return json({ error: "Failed to retrieve Linear token" }, 500);
  }
  if (!token) return json({ error: "Linear account not linked" }, 403);

  try {
    let accessToken = token.access_token;
    if (needsRefresh(token, now)) {
      const refreshed = await refreshToken(token, deps, now);
      if (!refreshed) return json({ error: RECONNECT_MESSAGE }, 401);
      accessToken = refreshed;
    }

    const response = await deps.fetch(LINEAR_GRAPHQL_URL, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        // OAuth tokens need the Bearer scheme; only personal API keys are
        // sent bare.
        "Authorization": `Bearer ${accessToken}`,
      },
      body: JSON.stringify({
        query: request.query,
        variables: request.variables,
      }),
    });

    if (response.status === 401) {
      return json({ error: RECONNECT_MESSAGE }, 401);
    }
    if (!response.ok) {
      console.error("[linear-proxy] Linear API error:", response.status);
    }
    // Pass Linear's body through, including GraphQL `errors` on a 4xx, so
    // the desktop app can show Linear's own message.
    return { status: response.status, body: await response.text() };
  } catch (err) {
    console.error("[linear-proxy] request failed:", errorMessage(err));
    return json({ error: "Failed to proxy Linear request" }, 502);
  }
}
