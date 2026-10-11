import { describe, expect, it, vi } from "vitest";
import {
  proxyLinearRequest,
  RECONNECT_MESSAGE,
  type ProxyDeps,
  type StoredToken,
} from "../../supabase/functions/linear-proxy/lib.ts";

const NOW = Date.parse("2026-09-28T12:00:00Z");
const QUERY = { query: "query { viewer { id } }" };
const VIEWER = { data: { viewer: { id: "u1" } } };

function token(overrides: Partial<StoredToken> = {}): StoredToken {
  return {
    access_token: "lin_oauth_current",
    refresh_token: "lin_refresh",
    expires_at: new Date(NOW + 60 * 60 * 1000).toISOString(),
    updated_at: "2026-09-28T11:00:00Z",
    ...overrides,
  };
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

type Route = (init: RequestInit) => Response;

function setup(opts: {
  stored: StoredToken | null | (() => StoredToken | null);
  graphql?: Route;
  oauth?: Route;
  pro?: boolean;
}) {
  const fetch = vi.fn(
    async (url: string | URL | Request, init?: RequestInit) => {
      const target = String(url);
      if (target === "https://api.linear.app/graphql" && opts.graphql) {
        return opts.graphql(init!);
      }
      if (target === "https://api.linear.app/oauth/token" && opts.oauth) {
        return opts.oauth(init!);
      }
      throw new Error(`unexpected fetch ${target}`);
    },
  );
  const save = vi.fn(async () => {});
  const load = vi.fn(async () =>
    typeof opts.stored === "function" ? opts.stored() : opts.stored,
  );
  const deps: ProxyDeps = {
    store: { load, save },
    fetch: fetch as unknown as typeof globalThis.fetch,
    linearClient: { id: "client-id", secret: "client-secret" },
    hasPro: vi.fn(async () => opts.pro ?? true),
    now: () => NOW,
  };
  const graphqlAuth = () =>
    fetch.mock.calls
      .filter(([url]) => String(url) === "https://api.linear.app/graphql")
      .map(([, init]) => new Headers(init?.headers).get("authorization"));
  return { deps, fetch, save, graphqlAuth };
}

async function run(deps: ProxyDeps) {
  const result = await proxyLinearRequest(QUERY, deps);
  return { status: result.status, body: JSON.parse(result.body) };
}

describe("proxyLinearRequest", () => {
  it("answers 402 pro_required for a user without Pro, before reading the token or calling Linear", async () => {
    const { deps, fetch } = setup({ stored: token(), pro: false });
    const result = await run(deps);
    expect(result.status).toBe(402);
    expect(result.body.code).toBe("pro_required");
    // The desktop app shows this message as is, so it names the free path.
    expect(result.body.error).toMatch(/personal Linear API key/);
    expect(deps.store.load).not.toHaveBeenCalled();
    expect(fetch).not.toHaveBeenCalled();
  });

  it("fails closed when the entitlement check fails", async () => {
    const { deps, fetch } = setup({ stored: token() });
    vi.mocked(deps.hasPro).mockRejectedValueOnce(new Error("db down"));
    expect((await run(deps)).status).toBe(500);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("reports an unlinked Linear account", async () => {
    const { deps } = setup({ stored: null });
    expect(await run(deps)).toEqual({
      status: 403,
      body: { error: "Linear account not linked" },
    });
  });

  it("sends the OAuth token with the Bearer scheme", async () => {
    const { deps, graphqlAuth } = setup({
      stored: token(),
      graphql: () => jsonResponse(VIEWER),
    });
    expect(await run(deps)).toEqual({ status: 200, body: VIEWER });
    expect(graphqlAuth()).toEqual(["Bearer lin_oauth_current"]);
  });

  it("passes Linear's GraphQL errors through with their status", async () => {
    const errors = { errors: [{ message: "Argument Validation Error" }] };
    const { deps } = setup({
      stored: token(),
      graphql: () => jsonResponse(errors, 400),
    });
    expect(await run(deps)).toEqual({ status: 400, body: errors });
  });

  it("asks the user to reconnect when Linear rejects the token", async () => {
    const { deps } = setup({
      stored: token(),
      graphql: () => jsonResponse({ errors: [{ message: "revoked" }] }, 401),
    });
    expect(await run(deps)).toEqual({
      status: 401,
      body: { error: RECONNECT_MESSAGE },
    });
  });

  it("refreshes a token close to expiry, stores it, and uses it", async () => {
    let oauthBody: URLSearchParams | undefined;
    const { deps, save, graphqlAuth } = setup({
      stored: token({ expires_at: new Date(NOW + 30_000).toISOString() }),
      oauth: (init) => {
        oauthBody = new URLSearchParams(String(init.body));
        return jsonResponse({
          access_token: "lin_oauth_new",
          refresh_token: "lin_refresh_new",
          expires_in: 3600,
        });
      },
      graphql: () => jsonResponse(VIEWER),
    });

    expect((await run(deps)).status).toBe(200);
    expect(oauthBody?.get("grant_type")).toBe("refresh_token");
    expect(oauthBody?.get("refresh_token")).toBe("lin_refresh");
    expect(save).toHaveBeenCalledWith({
      access_token: "lin_oauth_new",
      refresh_token: "lin_refresh_new",
      expires_at: new Date(NOW + 3600 * 1000).toISOString(),
    });
    expect(graphqlAuth()).toEqual(["Bearer lin_oauth_new"]);
  });

  it("does not refresh a token that is still valid", async () => {
    const { deps, fetch } = setup({
      stored: token(),
      graphql: () => jsonResponse(VIEWER),
    });
    await run(deps);
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("asks to reconnect when an expired token has no refresh token", async () => {
    const { deps, fetch } = setup({
      stored: token({
        refresh_token: null,
        expires_at: new Date(NOW - 1000).toISOString(),
      }),
      graphql: () => jsonResponse(VIEWER),
    });
    expect(await run(deps)).toEqual({
      status: 401,
      body: { error: RECONNECT_MESSAGE },
    });
    expect(fetch).not.toHaveBeenCalled();
  });

  it("uses a token another request refreshed first", async () => {
    const stale = token({ expires_at: new Date(NOW - 1000).toISOString() });
    const fresh = token({
      access_token: "lin_oauth_by_other_request",
      updated_at: "2026-09-28T11:59:59Z",
    });
    let loads = 0;
    const { deps, save, graphqlAuth } = setup({
      stored: () => (loads++ === 0 ? stale : fresh),
      oauth: () => jsonResponse({ error: "invalid_grant" }, 400),
      graphql: () => jsonResponse(VIEWER),
    });

    expect((await run(deps)).status).toBe(200);
    expect(save).not.toHaveBeenCalled();
    expect(graphqlAuth()).toEqual(["Bearer lin_oauth_by_other_request"]);
  });
});
