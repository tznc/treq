import { describe, expect, it, vi } from "vitest";
import { resolveCaller } from "../../supabase/functions/integration-proxy/lib.ts";

describe("resolveCaller", () => {
  const userFromJwt = vi.fn(async (jwt: string) =>
    jwt === "a.good.jwt" ? "user-1" : null,
  );

  it("accepts a signed-in user's Supabase JWT", async () => {
    expect(await resolveCaller("Bearer a.good.jwt", { userFromJwt })).toEqual({
      kind: "user",
      userId: "user-1",
    });
  });

  it("refuses a missing, malformed or rejected token without guessing", async () => {
    userFromJwt.mockClear();
    expect(await resolveCaller(undefined, { userFromJwt })).toBeNull();
    expect(await resolveCaller("Bearer not-a-jwt", { userFromJwt })).toBeNull();
    expect(userFromJwt).not.toHaveBeenCalled();
    expect(await resolveCaller("Bearer a.bad.jwt", { userFromJwt })).toBeNull();
  });
});
