import { describe, expect, it, vi } from "vitest";
import {
  githubRepoToken,
  mintSandboxToken,
  resolveCaller,
  SANDBOX_TOKEN_MAX_TTL_SEC,
} from "../../supabase/functions/integration-proxy/lib.ts";

const NOW = 1_800_000_000;
const PLACEMENT = "11111111-1111-1111-1111-111111111111";
const SECRET = "a".repeat(64);

const deps = {
  userFromJwt: async (jwt: string) => (jwt === "good-jwt" ? "user-1" : null),
  placement: async (id: string) =>
    id === PLACEMENT ? { secret: SECRET, ownerUserId: "owner-1" } : null,
  nowSec: () => NOW,
};

describe("resolveCaller", () => {
  it("accepts a Supabase JWT", async () => {
    expect(await resolveCaller("Bearer good-jwt", deps)).toEqual({ kind: "user", userId: "user-1" });
    expect(await resolveCaller("Bearer bad-jwt", deps)).toBeNull();
    expect(await resolveCaller(undefined, deps)).toBeNull();
  });

  it("accepts a sandbox token as the placement owner", async () => {
    const { token } = await mintSandboxToken(SECRET, PLACEMENT, 600, NOW);
    expect(await resolveCaller(`Bearer ${token}`, deps)).toEqual({
      kind: "sandbox",
      userId: "owner-1",
      placementId: PLACEMENT,
    });
  });

  it("rejects expired, tampered, foreign and too-long-lived tokens", async () => {
    const expired = await mintSandboxToken(SECRET, PLACEMENT, 60, NOW - 120);
    const wrongKey = await mintSandboxToken("b".repeat(64), PLACEMENT, 600, NOW);
    const unknown = await mintSandboxToken(SECRET, "22222222-2222-2222-2222-222222222222", 600, NOW);
    const { token } = await mintSandboxToken(SECRET, PLACEMENT, 600, NOW);
    const tooLong = token.replace(`.${NOW + 600}.`, `.${NOW + SANDBOX_TOKEN_MAX_TTL_SEC + 1}.`);
    for (const t of [expired.token, wrongKey.token, unknown.token, tooLong, "it1.x.y"]) {
      expect(await resolveCaller(`Bearer ${t}`, deps)).toBeNull();
    }
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
