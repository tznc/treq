import { describe, expect, it, vi } from "vitest";
import {
  appOnly,
  exactIlike,
  mintSandboxToken,
  resolveCaller,
  SANDBOX_TOKEN_MAX_TTL_SEC,
} from "../../supabase/functions/integration-proxy/lib.ts";

const NOW = 1_800_000_000;
const PLACEMENT = "11111111-1111-1111-1111-111111111111";
const SECRET = "a".repeat(64);
// The treq CLI signs the same way (src-tauri/src/integration.rs checks this vector).
const VECTOR =
  "it1.11111111-1111-1111-1111-111111111111.1800000300.f02f52f8f67b6774a7f2ec2bc93e2211b6f76df76e5be3af643f60b4d0380535";

function deps() {
  return {
    userFromJwt: vi.fn(async (jwt: string) =>
      jwt === "a.good.jwt" ? "user-1" : null,
    ),
    placementSecret: vi.fn(async (id: string) =>
      id === PLACEMENT ? { secret: SECRET, ownerUserId: "owner-1" } : null,
    ),
    nowSec: () => NOW,
  };
}

describe("resolveCaller", () => {
  it("accepts a signed-in user's Supabase JWT", async () => {
    expect(await resolveCaller("Bearer a.good.jwt", deps())).toEqual({
      kind: "user",
      userId: "user-1",
    });
  });

  it("refuses a missing, malformed or rejected token", async () => {
    const d = deps();
    expect(await resolveCaller(undefined, d)).toBeNull();
    expect(await resolveCaller("Bearer not-a-jwt", d)).toBeNull();
    expect(await resolveCaller("Bearer a.bad.jwt", d)).toBeNull();
  });

  it("accepts a sandbox token signed with its placement's secret, as the owner", async () => {
    const d = deps();
    expect(await resolveCaller(`Bearer ${VECTOR}`, d)).toEqual({
      kind: "sandbox",
      userId: "owner-1",
      placementId: PLACEMENT,
    });
    expect(d.userFromJwt).not.toHaveBeenCalled();
  });

  it("mints the same token the treq CLI does", async () => {
    expect(await mintSandboxToken(SECRET, PLACEMENT, 300, NOW)).toBe(VECTOR);
  });

  it("refuses a sandbox token that is forged, expired, too long-lived or for a dead placement", async () => {
    const tokens = [
      await mintSandboxToken("b".repeat(64), PLACEMENT, 300, NOW),
      await mintSandboxToken(SECRET, PLACEMENT, 60, NOW - 120),
      await mintSandboxToken(
        SECRET,
        "22222222-2222-2222-2222-222222222222",
        300,
        NOW,
      ),
      VECTOR.replace(
        `.${NOW + 300}.`,
        `.${NOW + SANDBOX_TOKEN_MAX_TTL_SEC + 1}.`,
      ),
      "it1.x.y",
    ];
    for (const t of tokens)
      expect(await resolveCaller(`Bearer ${t}`, deps())).toBeNull();
  });

  it("caps a minted token at five minutes", async () => {
    const token = await mintSandboxToken(SECRET, PLACEMENT, 99_999, NOW);
    expect(token.split(".")[2]).toBe(String(NOW + SANDBOX_TOKEN_MAX_TTL_SEC));
    expect(SANDBOX_TOKEN_MAX_TTL_SEC).toBe(300);
  });
});

describe("appOnly", () => {
  it("lets signed-in users through and refuses sandboxes", () => {
    expect(appOnly({ kind: "user", userId: "u" })).toBeNull();
    expect(appOnly({ kind: "sandbox", userId: "u", placementId: "p" })).toEqual(
      {
        status: 403,
        body: {
          error: "Not available from sandboxes",
          code: "sandbox_not_allowed",
        },
      },
    );
  });
});

describe("exactIlike", () => {
  it("escapes ILIKE wildcards so owner/tre_ cannot match owner/treq", () => {
    expect(exactIlike("tznc/tre_")).toBe("tznc/tre\\_");
    expect(exactIlike("a%b*c\\d")).toBe("a\\%b\\*c\\\\d");
    expect(exactIlike("tznc/treq")).toBe("tznc/treq");
  });
});
