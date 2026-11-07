import { describe, expect, it, vi } from "vitest";
import {
  MAX_REPOSITORIES,
  sandboxGithubTokens,
  TOKEN_PERMISSIONS,
} from "../../supabase/functions/integration-proxy/github-token.ts";

const INSTALLATIONS: Record<string, number> = {
  "tznc/treq": 1,
  "tznc/biz-tools": 1,
  "acme/app": 2,
};

function setup(
  o: {
    userToken?: string | null;
    push?: Record<string, boolean | 404>;
    mintStatus?: number;
  } = {},
) {
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    const repo = url.match(/\/repos\/([^/]+\/[^/]+)$/)?.[1];
    if (repo) {
      const push = o.push?.[repo] ?? true;
      if (push === 404) return new Response("{}", { status: 404 });
      return new Response(
        JSON.stringify({ full_name: repo, permissions: { push } }),
      );
    }
    const installation = url.match(/installations\/(\d+)\/access_tokens/)?.[1];
    const names = JSON.parse(String(init?.body)).repositories.join(",");
    return new Response(
      JSON.stringify({
        token: `ghs_${installation}_${names}`,
        expires_at: "2027-01-01T00:00:00Z",
      }),
      { status: o.mintStatus ?? 201 },
    );
  });
  const deps = {
    installationFor: vi.fn(
      async (fullName: string) => INSTALLATIONS[fullName.toLowerCase()] ?? null,
    ),
    userToken: vi.fn(async () =>
      o.userToken === undefined ? "ghu_owner" : o.userToken,
    ),
    appJwt: async () => "app-jwt",
    fetch: fetch as unknown as typeof globalThis.fetch,
  };
  return {
    deps,
    calls: fetch.mock.calls as unknown as [string, RequestInit][],
  };
}

describe("sandboxGithubTokens", () => {
  it("mints one scoped token per installation for repositories the owner can push to", async () => {
    const { deps, calls } = setup();
    const result = await sandboxGithubTokens(
      ["tznc/treq", "acme/app", "tznc/biz-tools"],
      deps,
    );
    expect(result).toEqual({
      status: 200,
      body: {
        tokens: [
          {
            repositories: ["tznc/treq", "tznc/biz-tools"],
            token: "ghs_1_treq,biz-tools",
            expires_at: "2027-01-01T00:00:00Z",
          },
          {
            repositories: ["acme/app"],
            token: "ghs_2_app",
            expires_at: "2027-01-01T00:00:00Z",
          },
        ],
        errors: [],
      },
    });
    const checks = calls.filter(([url]) => url.includes("/repos/"));
    expect(checks.map(([url]) => url)).toEqual([
      "https://api.github.com/repos/tznc/treq",
      "https://api.github.com/repos/acme/app",
      "https://api.github.com/repos/tznc/biz-tools",
    ]);
    for (const [, init] of checks) {
      expect((init.headers as Record<string, string>).Authorization).toBe(
        "Bearer ghu_owner",
      );
    }
    const mints = calls.filter(([url]) => url.includes("/access_tokens"));
    expect(JSON.parse(String(mints[0][1].body)).permissions).toEqual(
      TOKEN_PERMISSIONS,
    );
    expect((mints[0][1].headers as Record<string, string>).Authorization).toBe(
      "Bearer app-jwt",
    );
  });

  it("lists repositories it refused, and still mints for the rest", async () => {
    const { deps } = setup({
      push: { "tznc/biz-tools": false, "acme/app": 404 },
    });
    const result = await sandboxGithubTokens(
      ["tznc/treq", "tznc/biz-tools", "acme/app", "nope/none"],
      deps,
    );
    expect(result.status).toBe(200);
    expect(result.body).toMatchObject({
      tokens: [{ repositories: ["tznc/treq"] }],
      errors: [
        { repository: "tznc/biz-tools", code: "github_permission_denied" },
        { repository: "acme/app", code: "github_permission_denied" },
        { repository: "nope/none", code: "not_installed" },
      ],
    });
  });

  it("answers 403 when no repository can get a token", async () => {
    const { deps } = setup({ push: { "tznc/treq": false } });
    const result = await sandboxGithubTokens(["tznc/treq"], deps);
    expect(result).toMatchObject({ status: 403, body: { tokens: [] } });
  });

  it("asks the owner to reconnect when there is no usable user token", async () => {
    const { deps, calls } = setup({ userToken: null });
    const result = await sandboxGithubTokens(["tznc/treq"], deps);
    expect(result).toMatchObject({
      status: 401,
      body: { code: "github_reconnect" },
    });
    expect(calls).toHaveLength(0);
  });

  it("refuses malformed input before calling GitHub", async () => {
    const bad = [
      undefined,
      [],
      "tznc/treq",
      ["not a repo"],
      Array(MAX_REPOSITORIES + 1).fill("a/b"),
    ];
    for (const input of bad) {
      const { deps, calls } = setup();
      expect((await sandboxGithubTokens(input, deps)).status).toBe(400);
      expect(calls).toHaveLength(0);
    }
  });

  it("answers 502 when GitHub will not mint", async () => {
    const { deps } = setup({ mintStatus: 500 });
    expect((await sandboxGithubTokens(["tznc/treq"], deps)).status).toBe(502);
  });
});
