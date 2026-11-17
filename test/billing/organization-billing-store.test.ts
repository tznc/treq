import { describe, expect, it } from "vitest";
import {
  organizationBillingFor,
  type ServiceClientLike,
} from "../../supabase/functions/_shared/billing/store.ts";

function clientWithRole(role: string | null): ServiceClientLike {
  const query = {
    select: () => query,
    eq: () => query,
    maybeSingle: async () => ({
      data: role === null ? null : { role },
      error: null,
    }),
  };
  return { from: () => query } as unknown as ServiceClientLike;
}

describe("organizationBillingFor.canManageBilling", () => {
  it.each([
    ["owner", true],
    ["admin", true],
    ["member", false],
    [null, false],
  ])("%s manages the team's billing account: %s", async (role, expected) => {
    const billing = organizationBillingFor(clientWithRole(role), "user-1");
    expect(await billing.canManageBilling("org-1")).toBe(expected);
  });
});
