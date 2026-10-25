import { describe, expect, it, vi } from "vitest";
import {
  type AccountDeletionStore,
  deleteAccount,
  type Membership,
} from "../../supabase/functions/delete-account/lib.ts";
import {
  StripeApiError,
  type StripeClient,
} from "../../supabase/functions/_shared/billing/stripe-api.ts";

const USER_ID = "6f1c1f4e-6a8f-4a39-9d55-2b1f0b3f9a10";
const ORG_ID = "0b8d3f0e-2c1a-4f5e-9d7b-6a4c3e2f1a00";

type Subscription = { id: string; status: string };

/** The database side, recording every write in `log`. */
function fakeStore(
  log: string[],
  db: {
    memberships?: Membership[];
    customers?: Record<string, string>;
    instances?: Array<{ id: string; provider_resource_id: string | null }>;
  } = {},
): AccountDeletionStore {
  return {
    memberships: vi.fn(async () => db.memberships ?? []),
    customers: vi.fn(async (owners) =>
      owners
        .map((owner) => db.customers?.[`${owner.type}:${owner.id}`])
        .filter((id): id is string => typeof id === "string"),
    ),
    liveInstances: vi.fn(async () => db.instances ?? []),
    markInstance: vi.fn(async (id, status) => {
      log.push(`instance ${id} ${status}`);
    }),
    deleteUser: vi.fn(async () => {
      log.push("delete user");
    }),
  };
}

/** Stripe, listing `subscriptions` by customer and cancelling on DELETE. */
function fakeStripe(
  log: string[],
  subscriptions: Record<string, Subscription[]>,
  fail?: (method: string, path: string) => StripeApiError | null,
): StripeClient {
  return {
    request: vi.fn(async (method, path, params) => {
      const error = fail?.(method, path);
      if (error) throw error;
      if (method === "GET") {
        const customer = String(params?.customer);
        return { data: subscriptions[customer] ?? [], has_more: false };
      }
      log.push(`stripe ${method} ${path}`);
      return {};
    }) as StripeClient["request"],
  };
}

function provider(log: string[], fail = false) {
  return {
    deleteInstance: vi.fn(async (id: string) => {
      if (fail) throw new Error("Sprites API unavailable");
      log.push(`provider delete ${id}`);
    }),
  };
}

function membership(overrides: Partial<Membership> = {}): Membership {
  return {
    org_id: ORG_ID,
    org_name: "Acme",
    role: "owner",
    member_count: 1,
    owner_count: 1,
    ...overrides,
  };
}

describe("deleteAccount", () => {
  it("refuses a sole owner of an organization that has other members", async () => {
    const log: string[] = [];
    const store = fakeStore(log, {
      memberships: [membership({ member_count: 3 })],
      customers: { [`user:${USER_ID}`]: "cus_user" },
      instances: [{ id: "inst-1", provider_resource_id: "sprite-1" }],
    });
    const stripe = fakeStripe(log, {
      cus_user: [{ id: "sub_pro", status: "active" }],
    });
    const result = await deleteAccount({
      userId: USER_ID,
      store,
      stripe,
      provider: provider(log),
    });
    expect(result.status).toBe(409);
    expect(result.body.code).toBe("sole_owner");
    expect(result.body.error).toBe(
      "You are the only owner of Acme, which has other members. Make another member an owner or remove the other members, then delete your account.",
    );
    expect(log).toEqual([]);
    expect(stripe.request).not.toHaveBeenCalled();
  });

  it("lets an owner go when another owner stays, and a member always", async () => {
    const log: string[] = [];
    const store = fakeStore(log, {
      memberships: [
        membership({ member_count: 4, owner_count: 2 }),
        membership({ org_id: "org-2", role: "member", member_count: 5 }),
      ],
    });
    const result = await deleteAccount({
      userId: USER_ID,
      store,
      stripe: fakeStripe(log, {}),
      provider: provider(log),
    });
    expect(result.status).toBe(200);
    expect(store.customers).toHaveBeenCalledWith([
      { type: "user", id: USER_ID },
    ]);
    expect(log).toEqual(["delete user"]);
  });

  it("cancels the Team of an organization the user is alone in", async () => {
    const log: string[] = [];
    const store = fakeStore(log, {
      memberships: [membership()],
      customers: {
        [`user:${USER_ID}`]: "cus_user",
        [`organization:${ORG_ID}`]: "cus_org",
      },
    });
    const result = await deleteAccount({
      userId: USER_ID,
      store,
      stripe: fakeStripe(log, {
        cus_user: [{ id: "sub_old", status: "canceled" }],
        cus_org: [
          { id: "sub_team", status: "past_due" },
          { id: "sub_gone", status: "incomplete_expired" },
        ],
      }),
      provider: provider(log),
    });
    expect(result.status).toBe(200);
    expect(store.customers).toHaveBeenCalledWith([
      { type: "user", id: USER_ID },
      { type: "organization", id: ORG_ID },
    ]);
    expect(log).toEqual([
      "stripe DELETE /v1/subscriptions/sub_team",
      "delete user",
    ]);
  });

  it("cancels Pro, destroys the cloud machines, then deletes the user", async () => {
    const log: string[] = [];
    const store = fakeStore(log, {
      customers: { [`user:${USER_ID}`]: "cus_user" },
      instances: [
        { id: "inst-1", provider_resource_id: "sprite-1" },
        { id: "inst-2", provider_resource_id: null },
      ],
    });
    const result = await deleteAccount({
      userId: USER_ID,
      store,
      stripe: fakeStripe(log, {
        cus_user: [{ id: "sub_pro", status: "trialing" }],
      }),
      provider: provider(log),
    });
    expect(result).toEqual({ status: 200, body: { deleted: true } });
    expect(log).toEqual([
      "stripe DELETE /v1/subscriptions/sub_pro",
      "instance inst-1 deleting",
      "provider delete sprite-1",
      "instance inst-1 deleted",
      "instance inst-2 deleting",
      "instance inst-2 deleted",
      "delete user",
    ]);
  });

  it("treats a subscription Stripe no longer has as cancelled", async () => {
    const log: string[] = [];
    const result = await deleteAccount({
      userId: USER_ID,
      store: fakeStore(log, { customers: { [`user:${USER_ID}`]: "cus_user" } }),
      stripe: fakeStripe(
        log,
        { cus_user: [{ id: "sub_pro", status: "active" }] },
        (method) =>
          method === "DELETE"
            ? new StripeApiError(404, "No such subscription")
            : null,
      ),
      provider: provider(log),
    });
    expect(result.status).toBe(200);
    expect(log).toEqual(["delete user"]);
  });

  it("keeps the account when Stripe fails, so a retry finishes the job", async () => {
    const log: string[] = [];
    const result = await deleteAccount({
      userId: USER_ID,
      store: fakeStore(log, {
        customers: { [`user:${USER_ID}`]: "cus_user" },
        instances: [{ id: "inst-1", provider_resource_id: "sprite-1" }],
      }),
      stripe: fakeStripe(
        log,
        { cus_user: [{ id: "sub_pro", status: "active" }] },
        (method) =>
          method === "DELETE"
            ? new StripeApiError(500, "Stripe is down")
            : null,
      ),
      provider: provider(log),
    });
    expect(result.status).toBe(500);
    expect(result.body.error).toBe(
      "Your account could not be deleted. Try again in a moment.",
    );
    expect(log).toEqual([]);
  });

  it("keeps the account when a cloud machine cannot be destroyed", async () => {
    const log: string[] = [];
    const store = fakeStore(log, {
      instances: [{ id: "inst-1", provider_resource_id: "sprite-1" }],
    });
    const result = await deleteAccount({
      userId: USER_ID,
      store,
      stripe: null,
      provider: provider(log, true),
    });
    expect(result.status).toBe(500);
    expect(log).toEqual(["instance inst-1 deleting"]);
    expect(store.deleteUser).not.toHaveBeenCalled();
  });

  it("needs Stripe only when there is a customer to cancel for", async () => {
    const log: string[] = [];
    const withoutCustomer = await deleteAccount({
      userId: USER_ID,
      store: fakeStore(log),
      stripe: null,
      provider: provider(log),
    });
    expect(withoutCustomer.status).toBe(200);

    const withCustomer = await deleteAccount({
      userId: USER_ID,
      store: fakeStore([], { customers: { [`user:${USER_ID}`]: "cus_user" } }),
      stripe: null,
      provider: provider([]),
    });
    expect(withCustomer.status).toBe(500);
  });
});
