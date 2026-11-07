import { describe, expect, it, vi } from "vitest";
import {
  GITHUB_RECONNECT,
  githubUserToken,
  type StoredGrant,
  type UserTokenDeps,
} from "../../supabase/functions/integration-proxy/github-user-token.ts";

const NOW = Date.parse("2026-11-08T00:00:00.000Z");
const iso = (offsetMs: number) => new Date(NOW + offsetMs).toISOString();
const HOUR = 3_600_000;

function stored(overrides: Partial<StoredGrant> = {}): StoredGrant {
  return {
    accessToken: "ghu_current",
    refreshToken: "ghr_current",
    expiresAt: iso(4 * HOUR),
    refreshExpiresAt: iso(1000 * HOUR),
    updatedAt: "2026-11-07T20:00:00.000Z",
    ...overrides,
  };
}

function setup(opts: {
  grants: (StoredGrant | null)[];
  refresh?: () => Response;
  client?: { id: string; secret: string } | null;
}) {
  const loads = [...opts.grants];
  const fetch = vi.fn(async () =>
    (opts.refresh ?? (() => new Response("{}")))(),
  );
  const deps: UserTokenDeps & {
    store: { load: ReturnType<typeof vi.fn>; save: ReturnType<typeof vi.fn> };
  } = {
    store: {
      load: vi.fn(
        async () => (loads.length > 1 ? loads.shift() : loads[0]) ?? null,
      ),
      save: vi.fn(async () => {}),
    },
    fetch: fetch as unknown as typeof globalThis.fetch,
    appClient:
      opts.client === undefined
        ? { id: "Iv1.client", secret: "s" }
        : opts.client,
    now: () => NOW,
  };
  return { deps, fetch };
}

const refreshed = () =>
  new Response(
    JSON.stringify({
      access_token: "ghu_new",
      expires_in: 28800,
      refresh_token: "ghr_new",
      refresh_token_expires_in: 15638400,
    }),
  );

describe("githubUserToken", () => {
  it("returns a stored token with time left, without calling GitHub", async () => {
    const { deps, fetch } = setup({ grants: [stored()] });
    expect(await githubUserToken(deps)).toEqual({
      status: 200,
      body: { token: "ghu_current", expires_at: iso(4 * HOUR) },
    });
    expect(fetch).not.toHaveBeenCalled();
  });

  it("returns a token that never expires as is", async () => {
    const { deps } = setup({
      grants: [stored({ expiresAt: null, refreshToken: null })],
    });
    expect(await githubUserToken(deps)).toEqual({
      status: 200,
      body: { token: "ghu_current", expires_at: null },
    });
  });

  it("refreshes a token within 10 minutes of expiry and saves the rotated grant", async () => {
    const { deps, fetch } = setup({
      grants: [stored({ expiresAt: iso(5 * 60_000) })],
      refresh: refreshed,
    });
    const result = await githubUserToken(deps);
    expect(result).toEqual({
      status: 200,
      body: { token: "ghu_new", expires_at: iso(8 * HOUR) },
    });

    const [url, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("https://github.com/login/oauth/access_token");
    expect(Object.fromEntries(new URLSearchParams(String(init.body)))).toEqual({
      client_id: "Iv1.client",
      client_secret: "s",
      grant_type: "refresh_token",
      refresh_token: "ghr_current",
    });
    expect(deps.store.save).toHaveBeenCalledWith({
      accessToken: "ghu_new",
      refreshToken: "ghr_new",
      expiresAt: iso(8 * HOUR),
      refreshExpiresAt: new Date(NOW + 15638400 * 1000).toISOString(),
    });
  });

  it("still returns the refreshed token when saving it fails", async () => {
    const { deps } = setup({
      grants: [stored({ expiresAt: iso(60_000) })],
      refresh: refreshed,
    });
    deps.store.save.mockRejectedValueOnce(new Error("vault down"));
    expect(await githubUserToken(deps)).toEqual({
      status: 200,
      body: { token: "ghu_new", expires_at: iso(8 * HOUR) },
    });
  });

  it("uses a concurrent request's refresh when GitHub refuses the spent refresh token", async () => {
    const { deps } = setup({
      grants: [
        stored({ expiresAt: iso(60_000) }),
        stored({
          accessToken: "ghu_other",
          expiresAt: iso(8 * HOUR),
          updatedAt: "2026-11-08T00:00:00.000Z",
        }),
      ],
      refresh: () =>
        new Response(JSON.stringify({ error: "bad_refresh_token" })),
    });
    expect(await githubUserToken(deps)).toEqual({
      status: 200,
      body: { token: "ghu_other", expires_at: iso(8 * HOUR) },
    });
    expect(deps.store.save).not.toHaveBeenCalled();
  });

  it("asks the user to reconnect when nothing is stored or the grant cannot be refreshed", async () => {
    const reconnect = {
      status: 401,
      body: { error: GITHUB_RECONNECT, code: "github_reconnect" },
    };
    const cases = [
      setup({ grants: [null] }),
      setup({
        grants: [stored({ expiresAt: iso(60_000), refreshToken: null })],
      }),
      setup({
        grants: [stored({ expiresAt: iso(60_000), refreshExpiresAt: iso(-1) })],
      }),
      setup({
        grants: [stored({ expiresAt: iso(60_000) })],
        refresh: () =>
          new Response(JSON.stringify({ error: "bad_refresh_token" })),
      }),
      setup({ grants: [stored({ expiresAt: iso(60_000) })], client: null }),
    ];
    for (const { deps } of cases) {
      expect(await githubUserToken(deps)).toEqual(reconnect);
    }
  });

  it("answers 502 when GitHub cannot be reached", async () => {
    const { deps } = setup({
      grants: [stored({ expiresAt: iso(60_000) })],
      refresh: () => new Response("upstream", { status: 503 }),
    });
    expect((await githubUserToken(deps)).status).toBe(502);
  });
});
