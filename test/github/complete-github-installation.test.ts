import { describe, expect, it, vi } from "vitest";
import {
  githubInstallationAccess,
  MAX_INSTALLATION_PAGES,
} from "../../supabase/functions/complete-github-installation/github.ts";
import {
  completeInstallation,
  type CompleteInstallationDeps,
  type GitHubInstallation,
  type GitHubInstallationAccess,
  type InstallationLinkStore,
} from "../../supabase/functions/complete-github-installation/lib.ts";

const USER_ID = "6f1c1f4e-6a8f-4a39-9d55-2b1f0b3f9a10";
const INSTALLATION_ID = 4242;
const STATE = "f".repeat(64);
const CODE = "0123456789abcdef0123";
const USER_TOKEN = "ghu_user_token";

const installation: GitHubInstallation = {
  id: INSTALLATION_ID,
  account: { login: "acme", type: "Organization", avatar_url: null },
  app_id: 1,
};

function fakeDeps(
  github: Partial<GitHubInstallationAccess> = {},
): CompleteInstallationDeps & {
  store: InstallationLinkStore & {
    consumeIntent: ReturnType<typeof vi.fn>;
    linkInstallation: ReturnType<typeof vi.fn>;
  };
  github: { [K in keyof GitHubInstallationAccess]: ReturnType<typeof vi.fn> };
} {
  return {
    userId: USER_ID,
    hasPro: vi.fn(async () => true),
    store: {
      consumeIntent: vi.fn(async () => ({ id: "intent-1" })),
      linkInstallation: vi.fn(async () => ({
        result: "linked" as const,
        organization_id: null,
      })),
    },
    github: {
      exchangeCode: vi.fn(github.exchangeCode ?? (async () => USER_TOKEN)),
      userCanAccessInstallation: vi.fn(
        github.userCanAccessInstallation ?? (async () => true),
      ),
      getInstallation: vi.fn(
        github.getInstallation ?? (async () => installation),
      ),
    },
  };
}

const body = { installation_id: INSTALLATION_ID, state: STATE, code: CODE };

describe("completeInstallation verifies the installer on GitHub", () => {
  it("links an installation the GitHub user can access", async () => {
    const deps = fakeDeps();
    const result = await completeInstallation(body, deps);

    expect(result).toEqual({
      status: 200,
      body: {
        ok: true,
        account_login: "acme",
        account_type: "Organization",
        organization_id: null,
      },
    });
    expect(deps.github.exchangeCode).toHaveBeenCalledWith(CODE);
    expect(deps.github.userCanAccessInstallation).toHaveBeenCalledWith(
      USER_TOKEN,
      INSTALLATION_ID,
    );
    expect(deps.store.linkInstallation).toHaveBeenCalledWith(
      installation,
      USER_ID,
    );
  });

  it("refuses an installation the GitHub user cannot access", async () => {
    const deps = fakeDeps({ userCanAccessInstallation: async () => false });
    const result = await completeInstallation(body, deps);

    expect(result).toEqual({
      status: 403,
      body: {
        error: expect.stringContaining(
          "cannot access this GitHub App installation",
        ),
        code: "installation_not_accessible",
      },
    });
    expect(deps.store.linkInstallation).not.toHaveBeenCalled();
    // The intent is spent, so a forged callback cannot be retried with it.
    expect(deps.store.consumeIntent).toHaveBeenCalledOnce();
  });

  it.each([
    [{}],
    [{ code: "" }],
    [{ code: null }],
    [{ code: 42 }],
  ])("answers 400 github_authorization_required without a code: %j", async (extra) => {
    const deps = fakeDeps();
    const { code: _omitted, ...withoutCode } = body;
    const result = await completeInstallation(
      { ...withoutCode, ...extra },
      deps,
    );

    expect(result).toEqual({
      status: 400,
      body: {
        error: expect.stringContaining("authorization"),
        code: "github_authorization_required",
      },
    });
    expect(deps.hasPro).not.toHaveBeenCalled();
    expect(deps.store.consumeIntent).not.toHaveBeenCalled();
    expect(deps.github.exchangeCode).not.toHaveBeenCalled();
  });

  it("answers 502 when GitHub does not exchange the code", async () => {
    const deps = fakeDeps({
      exchangeCode: async () => {
        throw new Error("GitHub token exchange refused: bad_verification_code");
      },
    });
    const result = await completeInstallation(body, deps);

    expect(result).toEqual({
      status: 502,
      body: {
        error: expect.stringContaining("GitHub"),
        code: "github_verification_failed",
      },
    });
    expect(deps.github.userCanAccessInstallation).not.toHaveBeenCalled();
    expect(deps.store.linkInstallation).not.toHaveBeenCalled();
  });

  it("answers 502 when GitHub cannot list the user's installations", async () => {
    const deps = fakeDeps({
      userCanAccessInstallation: async () => {
        throw new Error("GitHub GET /user/installations → 500");
      },
    });
    const result = await completeInstallation(body, deps);

    expect(result.status).toBe(502);
    expect(result.body.code).toBe("github_verification_failed");
    expect(deps.store.linkInstallation).not.toHaveBeenCalled();
  });

  it("keeps the intent check: a bad state is refused before GitHub is asked", async () => {
    const deps = fakeDeps();
    deps.store.consumeIntent.mockResolvedValueOnce(null);
    const result = await completeInstallation(body, deps);

    expect(result.status).toBe(403);
    expect(deps.github.exchangeCode).not.toHaveBeenCalled();
  });
});

type FetchCall = { url: string; init: RequestInit };

function fakeFetch(responses: Array<() => Response>) {
  const calls: FetchCall[] = [];
  const impl = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(input), init: init ?? {} });
    const next = responses.shift();
    if (!next) throw new Error(`unexpected fetch ${String(input)}`);
    return next();
  });
  return { calls, impl: impl as unknown as typeof fetch };
}

const jsonResponse =
  (value: unknown, status = 200) =>
  () =>
    new Response(JSON.stringify(value), {
      status,
      headers: { "Content-Type": "application/json" },
    });

const credentials = { clientId: "Iv1.client", clientSecret: "secret-value" };

function installationsPage(ids: number[], totalCount: number) {
  return jsonResponse({
    total_count: totalCount,
    installations: ids.map((id) => ({ id })),
  });
}

describe("githubInstallationAccess", () => {
  it("exchanges the code with the App's client credentials", async () => {
    const { calls, impl } = fakeFetch([
      jsonResponse({ access_token: USER_TOKEN, token_type: "bearer" }),
    ]);
    const access = githubInstallationAccess(credentials, impl);

    await expect(access.exchangeCode(CODE)).resolves.toBe(USER_TOKEN);
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toBe("https://github.com/login/oauth/access_token");
    expect(calls[0].init.method).toBe("POST");
    expect(new Headers(calls[0].init.headers).get("accept")).toBe(
      "application/json",
    );
    expect(
      Object.fromEntries(new URLSearchParams(String(calls[0].init.body))),
    ).toEqual({
      client_id: "Iv1.client",
      client_secret: "secret-value",
      code: CODE,
    });
  });

  it("throws when GitHub refuses the code, answers an error, or credentials are missing", async () => {
    const refused = fakeFetch([
      jsonResponse({
        error: "bad_verification_code",
        error_description: "expired",
      }),
    ]);
    await expect(
      githubInstallationAccess(credentials, refused.impl).exchangeCode(CODE),
    ).rejects.toThrow(/bad_verification_code/);

    const failing = fakeFetch([jsonResponse({ message: "boom" }, 503)]);
    await expect(
      githubInstallationAccess(credentials, failing.impl).exchangeCode(CODE),
    ).rejects.toThrow(/503/);

    const unused = fakeFetch([]);
    await expect(
      githubInstallationAccess(
        { clientId: "", clientSecret: "" },
        unused.impl,
      ).exchangeCode(CODE),
    ).rejects.toThrow(/GITHUB_APP_CLIENT_ID/);
    expect(unused.calls).toEqual([]);
  });

  it("pages through /user/installations with the user's token until it finds the id", async () => {
    const firstPage = Array.from({ length: 100 }, (_, i) => i + 1);
    const { calls, impl } = fakeFetch([
      installationsPage(firstPage, 150),
      installationsPage([INSTALLATION_ID, 7], 150),
    ]);
    const access = githubInstallationAccess(credentials, impl);

    await expect(
      access.userCanAccessInstallation(USER_TOKEN, INSTALLATION_ID),
    ).resolves.toBe(true);
    expect(calls.map((c) => c.url)).toEqual([
      "https://api.github.com/user/installations?per_page=100&page=1",
      "https://api.github.com/user/installations?per_page=100&page=2",
    ]);
    expect(new Headers(calls[0].init.headers).get("authorization")).toBe(
      `Bearer ${USER_TOKEN}`,
    );
  });

  it("answers false after the last page without the id", async () => {
    const { calls, impl } = fakeFetch([installationsPage([1, 2, 3], 3)]);
    await expect(
      githubInstallationAccess(credentials, impl).userCanAccessInstallation(
        USER_TOKEN,
        INSTALLATION_ID,
      ),
    ).resolves.toBe(false);
    expect(calls).toHaveLength(1);
  });

  it("stops after the page cap and answers false", async () => {
    const full = Array.from({ length: 100 }, (_, i) => i + 100_000);
    const { calls, impl } = fakeFetch(
      Array.from({ length: MAX_INSTALLATION_PAGES + 1 }, () =>
        installationsPage(full, 1_000_000),
      ),
    );
    await expect(
      githubInstallationAccess(credentials, impl).userCanAccessInstallation(
        USER_TOKEN,
        INSTALLATION_ID,
      ),
    ).resolves.toBe(false);
    expect(calls).toHaveLength(MAX_INSTALLATION_PAGES);
  });

  it("throws when GitHub answers an error for the installations list", async () => {
    const { impl } = fakeFetch([
      jsonResponse({ message: "Bad credentials" }, 401),
    ]);
    await expect(
      githubInstallationAccess(credentials, impl).userCanAccessInstallation(
        USER_TOKEN,
        INSTALLATION_ID,
      ),
    ).rejects.toThrow(/401/);
  });
});
