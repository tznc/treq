import { describe, expect, it } from "vitest";
import { isServiceRoleRequest } from "../../supabase/functions/_shared/auth/service-role.ts";

const SERVICE_KEY = "service-role-key";

function request(authorization?: string): Request {
  const headers = authorization ? { authorization } : undefined;
  return new Request("https://example.test/functions/v1/merge-queue-worker", {
    method: "POST",
    headers,
  });
}

describe("isServiceRoleRequest", () => {
  it("accepts the service-role key as a bearer token", () => {
    expect(
      isServiceRoleRequest(request(`Bearer ${SERVICE_KEY}`), SERVICE_KEY),
    ).toBe(true);
  });

  it("rejects a signed-in user's session JWT", () => {
    expect(
      isServiceRoleRequest(request("Bearer user-session-jwt"), SERVICE_KEY),
    ).toBe(false);
  });

  it("rejects a missing or malformed Authorization header", () => {
    expect(isServiceRoleRequest(request(), SERVICE_KEY)).toBe(false);
    expect(isServiceRoleRequest(request(SERVICE_KEY), SERVICE_KEY)).toBe(false);
  });

  it("rejects everything when the function has no service-role key", () => {
    expect(isServiceRoleRequest(request("Bearer "), "")).toBe(false);
  });
});
