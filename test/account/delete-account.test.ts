import { describe, expect, it } from "vitest";
import {
  createStripeClient,
  StripeApiError,
  type StripeClient,
} from "../../supabase/functions/_shared/billing/stripe-api.ts";
import type { ManagedComputeProvider } from "../../supabase/functions/_shared/remote/sprites-adapter.ts";
import {
  type DeleteAccountDeps,
  type DeleteAccountStore,
  deleteAccount,
} from "../../supabase/functions/delete-account/lib.ts";

const ME = "11111111-1111-4111-8111-111111111111";
const OTHER = "22222222-2222-4222-8222-222222222222";

type Instance = { id: string; provider_resource_id: string | null };

function fakeStore(
  seed: {
    subscriptions?: string[];
    instances?: Instance[];
    linearToken?: string | null;
  } = {},
) {
  const log: string[] = [];
  const instances = [...(seed.instances ?? [])];
  const store: DeleteAccountStore = {
    async subscriptionsToCancel() {
      return seed.subscriptions ?? [];
    },
    async liveInstances() {
      return instances.filter((i) => !log.includes(`instance-deleted:${i.id}`));
    },
    async markInstanceDeleted(id) {
      log.push(`instance-deleted:${id}`);
    },
    async linearAccessToken() {
      return seed.linearToken ?? null;
    },
    async deletePersonalInstallations() {
      log.push("installations-deleted");
    },
    async deleteUser() {
      log.push("user-deleted");
    },
  };
  return { store, log };
}

type StripeSub = { status: string } | "missing";

function fakeStripe(subs: Record<string, StripeSub>, failDelete = false) {
  const calls: string[] = [];
  const stripe: StripeClient = {
    async request<T>(method: string, path: string): Promise<T> {
      calls.push(`${method} ${path}`);
      const id = path.replace("/v1/subscriptions/", "");
      const sub = subs[id];
      if (sub === undefined || sub === "missing") {
        throw new StripeApiError(404, `No such subscription: '${id}'`);
      }
      if (method === "GET") return { id, status: sub.status } as T;
      if (method === "DELETE") {
        if (failDelete) throw new StripeApiError(500, "Stripe is down");
        sub.status = "canceled";
        return { id, status: "canceled" } as T;
      }
      throw new Error(`unexpected Stripe call ${method} ${path}`);
    },
  };
  return { stripe, calls };
}

function fakeProvider(failDelete = false) {
  const deleted: string[] = [];
  const provider = {
    async deleteInstance(providerId: string) {
      if (failDelete) throw new Error("provider unavailable");
      deleted.push(providerId);
    },
  } as unknown as ManagedComputeProvider;
  return { provider, deleted };
}

function deps(
  overrides: Partial<DeleteAccountDeps> & { store: DeleteAccountStore },
): DeleteAccountDeps {
  return {
    method: "POST",
    authorization: "Bearer valid-token",
    verifyUser: async (token) => (token === "valid-token" ? { id: ME } : null),
    storeFor: () => overrides.store,
    stripe: fakeStripe({}).stripe,
    provider: () => fakeProvider().provider,
    revokeLinearToken: async () => {},
    ...overrides,
  };
}

describe("deleteAccount authorization", () => {
  it("rejects a request without a bearer token", async () => {
    const { store, log } = fakeStore();
    const result = await deleteAccount(deps({ store, authorization: null }));
    expect(result.status).toBe(401);
    expect(log).toEqual([]);
  });

  it("rejects a token that does not verify", async () => {
    const { store, log } = fakeStore();
    const result = await deleteAccount(
      deps({ store, authorization: "Bearer forged" }),
    );
    expect(result.status).toBe(401);
    expect(log).toEqual([]);
  });

  it("rejects methods other than POST", async () => {
    const { store, log } = fakeStore();
    const result = await deleteAccount(deps({ store, method: "GET" }));
    expect(result.status).toBe(405);
    expect(log).toEqual([]);
  });

  it("deletes the user the token names, whatever the body says", async () => {
    const { store, log } = fakeStore();
    const storesFor: string[] = [];
    const result = await deleteAccount(
      deps({
        store,
        body: { user_id: OTHER },
        storeFor: (userId) => {
          storesFor.push(userId);
          return store;
        },
      }),
    );
    expect(result).toEqual({ status: 200, body: { deleted: true } });
    expect(storesFor).toEqual([ME]);
    expect(log).toContain("user-deleted");
  });
});

describe("deleteAccount cleanup", () => {
  it("cancels live subscriptions immediately before deleting the user", async () => {
    const { store, log } = fakeStore({ subscriptions: ["sub_a", "sub_b"] });
    const { stripe, calls } = fakeStripe({
      sub_a: { status: "active" },
      sub_b: { status: "trialing" },
    });
    const result = await deleteAccount(deps({ store, stripe }));
    expect(result.status).toBe(200);
    expect(calls).toEqual([
      "GET /v1/subscriptions/sub_a",
      "DELETE /v1/subscriptions/sub_a",
      "GET /v1/subscriptions/sub_b",
      "DELETE /v1/subscriptions/sub_b",
    ]);
    expect(log.at(-1)).toBe("user-deleted");
  });

  it("skips subscriptions Stripe already ended or no longer has", async () => {
    const { store } = fakeStore({ subscriptions: ["sub_done", "sub_gone"] });
    const { stripe, calls } = fakeStripe({
      sub_done: { status: "canceled" },
      sub_gone: "missing",
    });
    const result = await deleteAccount(deps({ store, stripe }));
    expect(result.status).toBe(200);
    expect(calls).toEqual([
      "GET /v1/subscriptions/sub_done",
      "GET /v1/subscriptions/sub_gone",
    ]);
  });

  it("keeps the account when Stripe fails to cancel", async () => {
    const { store, log } = fakeStore({ subscriptions: ["sub_a"] });
    const { stripe } = fakeStripe({ sub_a: { status: "active" } }, true);
    const { provider, deleted } = fakeProvider();
    const result = await deleteAccount(
      deps({ store, stripe, provider: () => provider }),
    );
    expect(result.status).toBe(502);
    expect(log).toEqual([]);
    expect(deleted).toEqual([]);
  });

  it("keeps the account when it has a subscription and Stripe is not configured", async () => {
    const { store, log } = fakeStore({ subscriptions: ["sub_a"] });
    const result = await deleteAccount(deps({ store, stripe: null }));
    expect(result.status).toBe(500);
    expect(log).toEqual([]);
  });

  it("deletes the account without Stripe when there is nothing to cancel", async () => {
    const { store, log } = fakeStore();
    const result = await deleteAccount(deps({ store, stripe: null }));
    expect(result.status).toBe(200);
    expect(log).toContain("user-deleted");
  });

  it("destroys cloud workspace machines and marks them deleted", async () => {
    const { store, log } = fakeStore({
      instances: [
        { id: "inst-1", provider_resource_id: "dev-treq-1" },
        { id: "inst-2", provider_resource_id: null },
      ],
    });
    const { provider, deleted } = fakeProvider();
    const result = await deleteAccount(deps({ store, provider: () => provider }));
    expect(result.status).toBe(200);
    expect(deleted).toEqual(["dev-treq-1"]);
    expect(log).toEqual([
      "instance-deleted:inst-1",
      "instance-deleted:inst-2",
      "installations-deleted",
      "user-deleted",
    ]);
  });

  it("keeps the account when a machine cannot be destroyed", async () => {
    const { store, log } = fakeStore({
      instances: [{ id: "inst-1", provider_resource_id: "dev-treq-1" }],
    });
    const { provider } = fakeProvider(true);
    const result = await deleteAccount(deps({ store, provider: () => provider }));
    expect(result.status).toBe(502);
    expect(log).toEqual([]);
  });

  it("revokes the Linear token before deleting the user", async () => {
    const { store, log } = fakeStore({ linearToken: "lin_oauth_abc" });
    const revoked: string[] = [];
    const result = await deleteAccount(
      deps({
        store,
        revokeLinearToken: async (token) => {
          revoked.push(token);
          log.push("linear-revoked");
        },
      }),
    );
    expect(result.status).toBe(200);
    expect(revoked).toEqual(["lin_oauth_abc"]);
    expect(log).toEqual([
      "linear-revoked",
      "installations-deleted",
      "user-deleted",
    ]);
  });

  it("still deletes the account when Linear revocation fails", async () => {
    const { store, log } = fakeStore({ linearToken: "lin_oauth_abc" });
    const result = await deleteAccount(
      deps({
        store,
        revokeLinearToken: async () => {
          throw new Error("linear down");
        },
      }),
    );
    expect(result.status).toBe(200);
    expect(log).toContain("user-deleted");
  });

  it("finishes the deletion on a retry after an earlier failure", async () => {
    const { store, log } = fakeStore({ subscriptions: ["sub_a"] });
    const subs = { sub_a: { status: "active" } };
    const failing = fakeStripe(subs, true);
    expect((await deleteAccount(deps({ store, stripe: failing.stripe }))).status)
      .toBe(502);

    const working = fakeStripe(subs);
    const retry = await deleteAccount(deps({ store, stripe: working.stripe }));
    expect(retry.status).toBe(200);
    expect(log).toEqual(["installations-deleted", "user-deleted"]);
  });
});

describe("createStripeClient DELETE", () => {
  it("sends DELETE without a body", async () => {
    const seen: { url: string; init: RequestInit }[] = [];
    const fetchImpl = (async (url: string, init: RequestInit) => {
      seen.push({ url, init });
      return new Response(JSON.stringify({ id: "sub_a", status: "canceled" }));
    }) as unknown as typeof fetch;
    const client = createStripeClient("sk_test", fetchImpl);
    await client.request("DELETE", "/v1/subscriptions/sub_a");
    expect(seen[0].url).toBe("https://api.stripe.com/v1/subscriptions/sub_a");
    expect(seen[0].init.method).toBe("DELETE");
    expect(seen[0].init.body).toBeUndefined();
  });
});
