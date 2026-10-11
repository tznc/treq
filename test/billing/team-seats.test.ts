import { describe, expect, it, vi } from "vitest";
import type { StripeClient } from "../../supabase/functions/_shared/billing/stripe-api.ts";
import {
  syncTeamSeats,
  type TeamSeatsStore,
  teamQuantity,
} from "../../supabase/functions/_shared/billing/team-seats.ts";
import {
  handleOrganizationsRequest,
  type RpcResult,
} from "../../supabase/functions/organizations/lib.ts";

const ORG_ID = "0b8d3f0e-2c1a-4f5e-9d7b-6a4c3e2f1a00";
const USER_ID = "6f1c1f4e-6a8f-4a39-9d55-2b1f0b3f9a10";
const OTHER_ID = "11111111-2222-4333-8444-555555555555";

type Call = { method: string; path: string; params?: unknown };

function fakeStripe(quantity: number) {
  const calls: Call[] = [];
  const stripe = {
    request: async (method: string, path: string, params?: unknown) => {
      calls.push({ method, path, params });
      if (method === "GET") {
        return {
          items: {
            data: [
              { id: "si_addon", quantity: 1, price: { lookup_key: "other" } },
              {
                id: "si_team",
                quantity,
                price: { lookup_key: "treq_team_monthly" },
              },
            ],
          },
        };
      }
      return {};
    },
  } as StripeClient;
  return { stripe, calls };
}

function fakeStore(members: number, subscriptionId: string | null = "sub_1") {
  return {
    memberCount: vi.fn(async () => members),
    billingSubscriptionId: vi.fn(async () => subscriptionId),
  } satisfies TeamSeatsStore;
}

describe("teamQuantity", () => {
  it("is the member count, and never below 1", () => {
    expect(teamQuantity(0)).toBe(1);
    expect(teamQuantity(5)).toBe(5);
    expect(teamQuantity(8)).toBe(8);
  });
});

describe("syncTeamSeats", () => {
  it("sets the Team item's quantity to the member count, prorated", async () => {
    const { stripe, calls } = fakeStripe(5);
    const result = await syncTeamSeats(ORG_ID, {
      store: fakeStore(7),
      stripe,
    });
    expect(result).toBe("updated");
    expect(calls).toEqual([
      { method: "GET", path: "/v1/subscriptions/sub_1", params: undefined },
      {
        method: "POST",
        path: "/v1/subscription_items/si_team",
        params: { quantity: 7, proration_behavior: "create_prorations" },
      },
    ]);
  });

  it("writes nothing when the quantity already matches", async () => {
    const { stripe, calls } = fakeStripe(6);
    expect(await syncTeamSeats(ORG_ID, { store: fakeStore(6), stripe })).toBe(
      "unchanged",
    );
    expect(calls.map((c) => c.method)).toEqual(["GET"]);
  });

  it("does nothing for an organization without a billing Team", async () => {
    const { stripe, calls } = fakeStripe(1);
    expect(
      await syncTeamSeats(ORG_ID, { store: fakeStore(3, null), stripe }),
    ).toBe("no_subscription");
    expect(calls).toEqual([]);
  });
});

describe("organizations seat sync", () => {
  function run(body: Record<string, unknown>, data: unknown) {
    const syncSeats = vi.fn(async (_org: string) => "updated");
    const rpc = vi.fn(async (): Promise<RpcResult> => ({ data, error: null }));
    return {
      syncSeats,
      result: handleOrganizationsRequest(body, {
        userId: USER_ID,
        rpc,
        webUrl: "https://treq.dev",
        syncSeats,
      }),
    };
  }

  it.each([
    [
      { action: "remove_member", organization_id: ORG_ID, user_id: OTHER_ID },
      null,
    ],
    [{ action: "leave", organization_id: ORG_ID }, null],
    [
      { action: "accept", token: "a".repeat(64) },
      { organization_id: ORG_ID, name: "Acme", result: "joined" },
    ],
  ])("syncs seats after %o", async (body, data) => {
    const { syncSeats, result } = run(body, data);
    expect((await result).status).toBe(200);
    expect(syncSeats).toHaveBeenCalledWith(ORG_ID);
  });

  it.each([
    [
      { action: "accept", token: "a".repeat(64) },
      { organization_id: ORG_ID, name: "Acme", result: "already_member" },
    ],
    [
      { action: "invite", organization_id: ORG_ID, email: "a@example.com" },
      {
        invite_id: OTHER_ID,
        email: "a@example.com",
        expires_at: "x",
        token: "t",
      },
    ],
  ])("leaves seats alone after %o", async (body, data) => {
    const { syncSeats, result } = run(body, data);
    expect((await result).status).toBe(200);
    expect(syncSeats).not.toHaveBeenCalled();
  });

  it("still answers 200 when the sync fails, since the change committed", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const result = await handleOrganizationsRequest(
      { action: "leave", organization_id: ORG_ID },
      {
        userId: USER_ID,
        rpc: async () => ({ data: null, error: null }),
        webUrl: "https://treq.dev",
        syncSeats: async () => {
          throw new Error("Stripe down");
        },
      },
    );
    expect(result).toEqual({ status: 200, body: { ok: true } });
    expect(error).toHaveBeenCalledWith(
      expect.stringContaining("Team seat sync failed"),
    );
    error.mockRestore();
  });
});
