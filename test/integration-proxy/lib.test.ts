import { createHmac } from "node:crypto";
import { describe, expect, it, vi } from "vitest";
import {
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

describe("githubRepoToken", () => {
  const ghDeps = (installation: number | null, ok = true) => ({
    installationFor: vi.fn(async () => installation),
    appJwt: async () => "app-jwt",
    fetch: vi.fn(async () =>
      new Response(JSON.stringify({ token: "ghs_x", expires_at: "2027-01-01T00:00:00Z" }), {
        status: ok ? 201 : 500,
      })
    ) as unknown as typeof fetch,
  });

  it("mints a token limited to the repository", async () => {
    const d = ghDeps(42);
    const result = await githubRepoToken("owner-1", "tznc/treq", d);
    expect(result).toEqual({ status: 200, body: { token: "ghs_x", expires_at: "2027-01-01T00:00:00Z" } });
    const [url, init] = (d.fetch as unknown as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(url).toBe("https://api.github.com/app/installations/42/access_tokens");
    expect(JSON.parse(init.body)).toEqual({ repositories: ["treq"] });
  });

  it("refuses bad input, unlinked repos and GitHub errors", async () => {
    expect((await githubRepoToken("u", "not-a-repo", ghDeps(42))).status).toBe(400);
    expect((await githubRepoToken("u", "a/b", ghDeps(null))).status).toBe(403);
    expect((await githubRepoToken("u", "a/b", ghDeps(42, false))).status).toBe(502);
  });
});
