import { createHmac } from "node:crypto";
import { describe, expect, it, vi } from "vitest";
import {
  exactIlike,
  githubRepoToken,
  mintSandboxJwt,
  resolveCaller,
} from "../../supabase/functions/integration-proxy/lib.ts";

const NOW = 1_800_000_000;
const PLACEMENT = "11111111-1111-1111-1111-111111111111";
const SECRET = "integration-proxy-secret-with-at-least-32-characters";

const deps = {
  sandboxSecret: SECRET,
  userFromJwt: vi.fn(async (jwt: string) => (jwt === "a.good.jwt" ? "user-1" : null)),
  placementOwner: async (id: string) => (id === PLACEMENT ? "owner-1" : null),
  nowSec: () => NOW,
};

function decode(segment: string) {
  return JSON.parse(Buffer.from(segment, "base64url").toString());
}

// Signs a payload, to test claim checks.
function forge(payload: Record<string, unknown>, secret = SECRET): string {
  const enc = (v: unknown) => Buffer.from(JSON.stringify(v)).toString("base64url");
  const input = `${enc({ alg: "HS256", typ: "JWT" })}.${enc(payload)}`;
  return `${input}.${createHmac("sha256", secret).update(input).digest("base64url")}`;
}

describe("mintSandboxJwt", () => {
  it("names the placement, not a user, for this function only", async () => {
    const { token, expiresAt } = await mintSandboxJwt(SECRET, PLACEMENT, 600, NOW);
    const [h, p, sig] = token.split(".");
    expect(decode(h)).toEqual({ alg: "HS256", typ: "JWT" });
    expect(decode(p)).toEqual({ sub: PLACEMENT, aud: "treq-integration-proxy", iat: NOW, exp: NOW + 600 });
    expect(sig).toBe(createHmac("sha256", SECRET).update(`${h}.${p}`).digest("base64url"));
    expect(expiresAt).toBe(NOW + 600);
  });

  it("caps the lifetime at an hour", async () => {
    const { expiresAt } = await mintSandboxJwt(SECRET, PLACEMENT, 99_999, NOW);
    expect(expiresAt).toBe(NOW + 3600);
  });
});

describe("resolveCaller", () => {
  const claims = { sub: PLACEMENT, aud: "treq-integration-proxy", iat: NOW, exp: NOW + 600 };

  it("passes a user's JWT to Auth", async () => {
    expect(await resolveCaller("Bearer a.good.jwt", deps)).toEqual({ kind: "user", userId: "user-1" });
    expect(await resolveCaller("Bearer a.bad.jwt", deps)).toBeNull();
    expect(await resolveCaller(undefined, deps)).toBeNull();
  });

  it("accepts a sandbox token as the placement owner", async () => {
    const { token } = await mintSandboxJwt(SECRET, PLACEMENT, 600, NOW);
    expect(await resolveCaller(`Bearer ${token}`, deps)).toEqual({
      kind: "sandbox",
      userId: "owner-1",
      placementId: PLACEMENT,
    });
  });

  it("rejects bad signatures, expired, too-long-lived and orphaned sandbox tokens", async () => {
    const tokens = [
      forge(claims, "another-secret-another-secret-another-secret"),
      forge({ ...claims, iat: NOW - 700, exp: NOW - 100 }),
      forge({ ...claims, exp: NOW + 3601 }),
      forge({ ...claims, sub: "22222222-2222-2222-2222-222222222222" }),
    ];
    for (const t of tokens) expect(await resolveCaller(`Bearer ${t}`, deps)).toBeNull();
  });

  it("never sends a sandbox token to Auth", async () => {
    deps.userFromJwt.mockClear();
    await resolveCaller(`Bearer ${forge(claims, "wrong-secret-wrong-secret-wrong-secret")}`, deps);
    expect(deps.userFromJwt).not.toHaveBeenCalled();
  });
});

describe("exactIlike", () => {
  it("escapes ILIKE wildcards so owner/tre_ cannot match owner/treq", () => {
    expect(exactIlike("tznc/tre_")).toBe("tznc/tre\\_");
    expect(exactIlike("a%b*c\\d")).toBe("a\\%b\\*c\\\\d");
    expect(exactIlike("tznc/treq")).toBe("tznc/treq");
  });
});

describe("githubRepoToken", () => {
  type Opts = {
    installation?: number | null;
    identity?: { githubUserId: number; login: string } | null;
    permission?: { status: number; body?: unknown };
    mintStatus?: number;
  };
  const ghDeps = (o: Opts = {}) => {
    const fetch = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.includes("/collaborators/")) {
        const p = o.permission ?? { status: 200, body: { permission: "write", user: { id: 7 } } };
        return new Response(JSON.stringify(p.body ?? {}), { status: p.status });
      }
      const perms = JSON.parse(String(init?.body)).permissions;
      const token = perms.contents ? "ghs_agent" : "ghs_check";
      return new Response(JSON.stringify({ token, expires_at: "2027-01-01T00:00:00Z" }), {
        status: o.mintStatus ?? 201,
      });
    });
    return {
      installationFor: vi.fn(async () => (o.installation === undefined ? 42 : o.installation)),
      identityFor: vi.fn(async () =>
        o.identity === undefined ? { githubUserId: 7, login: "octo" } : o.identity
      ),
      appJwt: async () => "app-jwt",
      fetch: fetch as unknown as typeof globalThis.fetch,
      calls: fetch.mock.calls as unknown as [string, RequestInit][],
    };
  };

  it("checks the user's permission, then mints a token limited to the repository", async () => {
    const d = ghDeps();
    const result = await githubRepoToken("owner-1", "tznc/treq", d);
    expect(result).toEqual({ status: 200, body: { token: "ghs_agent", expires_at: "2027-01-01T00:00:00Z" } });

    const [check, permission, mint] = d.calls;
    expect(JSON.parse(String(check[1].body))).toEqual({
      repositories: ["treq"],
      permissions: { metadata: "read" },
    });
    expect(permission[0]).toBe("https://api.github.com/repos/tznc/treq/collaborators/octo/permission");
    expect((permission[1].headers as Record<string, string>).Authorization).toBe("Bearer ghs_check");
    expect(mint[0]).toBe("https://api.github.com/app/installations/42/access_tokens");
    expect(JSON.parse(String(mint[1].body))).toEqual({
      repositories: ["treq"],
      permissions: {
        contents: "write",
        pull_requests: "write",
        issues: "write",
        metadata: "read",
        actions: "read",
        checks: "read",
        statuses: "read",
      },
    });
  });

  it("refuses a user who cannot push, without minting an agent token", async () => {
    for (const permission of ["read", "triage", "none"]) {
      const d = ghDeps({ permission: { status: 200, body: { permission, user: { id: 7 } } } });
      const result = await githubRepoToken("owner-1", "tznc/treq", d);
      expect(result).toMatchObject({ status: 403, body: { code: "github_permission_denied" } });
      expect(d.calls).toHaveLength(2);
    }
  });

  it("asks the user to reconnect when the GitHub account is unknown or changed", async () => {
    const cases: Opts[] = [
      { identity: null },
      { permission: { status: 404 } },
      // The stored login now belongs to another account.
      { permission: { status: 200, body: { permission: "admin", user: { id: 8 } } } },
    ];
    for (const o of cases) {
      const result = await githubRepoToken("owner-1", "tznc/treq", ghDeps(o));
      expect(result).toMatchObject({ status: 403, body: { code: "github_identity_required" } });
    }
  });

  it("refuses bad input, unlinked repos and GitHub errors", async () => {
    expect((await githubRepoToken("u", "not-a-repo", ghDeps())).status).toBe(400);
    expect((await githubRepoToken("u", "a/b", ghDeps({ installation: null }))).status).toBe(403);
    expect((await githubRepoToken("u", "a/b", ghDeps({ mintStatus: 500 }))).status).toBe(502);
    expect((await githubRepoToken("u", "a/b", ghDeps({ permission: { status: 500 } }))).status).toBe(502);
  });
});
