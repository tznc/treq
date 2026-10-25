// Request logic for delete-account, free of Deno imports so it runs under the
// unit tests and service-qa. index.ts verifies the user's JWT and supplies
// the service-role store, the Stripe client and the Sprites provider.
//
// Deleting an account (App Store guideline 5.1.1(v), GDPR) runs in this
// order, and stops at the first failure with the account still in place, so
// a retry picks up where it stopped. Every step is safe to run twice.
//
// 1. Refuse while the user is the only owner of an organization that has
//    other members: they hand it over or remove the members first. An
//    organization they are alone in goes with them (the trigger in
//    029_organization_owners.sql deletes it).
// 2. Cancel, at once and without a refund, every live Stripe subscription of
//    the user and of the organizations that go with them. Stripe keeps the
//    customers for their invoices.
// 3. Destroy the user's Treq Cloud machines. Their rows cascade with the
//    user, but the Sprites would keep running and billing.
// 4. Delete the auth user. Foreign keys cascade or clear the rest, and the
//    trigger in 032_linear_tokens_vault.sql deletes the Linear tokens from
//    Vault.

import {
  StripeApiError,
  type StripeClient,
} from "../_shared/billing/stripe-api.ts";
import type { ServiceClientLike } from "../_shared/billing/store.ts";

/** One of the user's organization memberships, with its head counts. */
export type Membership = {
  org_id: string;
  org_name: string;
  role: "owner" | "member";
  member_count: number;
  owner_count: number;
};

export type BillingOwner = { type: "user" | "organization"; id: string };

/** A remote_instances row that is not deleted yet. */
export type LiveInstance = { id: string; provider_resource_id: string | null };

export interface AccountDeletionStore {
  memberships(): Promise<Membership[]>;
  /** The Stripe customer ids of these owners, where they have one. */
  customers(owners: BillingOwner[]): Promise<string[]>;
  liveInstances(): Promise<LiveInstance[]>;
  markInstance(id: string, status: "deleting" | "deleted"): Promise<void>;
  deleteUser(): Promise<void>;
}

export type AccountDeletionDeps = {
  userId: string;
  store: AccountDeletionStore;
  /** Null when billing is not configured. */
  stripe: StripeClient | null;
  provider: { deleteInstance(providerResourceId: string): Promise<void> };
};

type DeletionResult = { status: number; body: Record<string, unknown> };

const FAILED: DeletionResult = {
  status: 500,
  body: { error: "Your account could not be deleted. Try again in a moment." },
};

/** Statuses Stripe has finished with: nothing left to cancel. */
const ENDED_STATUSES = new Set(["canceled", "incomplete_expired"]);

function listNames(names: string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

export async function deleteAccount(
  deps: AccountDeletionDeps,
): Promise<DeletionResult> {
  try {
    const memberships = await deps.store.memberships();
    const blocking = memberships.filter(
      (m) => m.role === "owner" && m.owner_count <= 1 && m.member_count > 1,
    );
    if (blocking.length > 0) {
      const names = listNames(blocking.map((m) => m.org_name));
      const which = blocking.length === 1 ? "which has" : "which each have";
      return {
        status: 409,
        body: {
          error: `You are the only owner of ${names}, ${which} other members. Make another member an owner or remove the other members, then delete your account.`,
          code: "sole_owner",
        },
      };
    }

    const owners: BillingOwner[] = [
      { type: "user", id: deps.userId },
      ...memberships
        .filter((m) => m.member_count <= 1)
        .map((m) => ({ type: "organization" as const, id: m.org_id })),
    ];
    await cancelSubscriptions(deps, await deps.store.customers(owners));

    for (const instance of await deps.store.liveInstances()) {
      await deps.store.markInstance(instance.id, "deleting");
      if (instance.provider_resource_id) {
        await deps.provider.deleteInstance(instance.provider_resource_id);
      }
      await deps.store.markInstance(instance.id, "deleted");
    }

    await deps.store.deleteUser();
    return { status: 200, body: { deleted: true } };
  } catch (err) {
    console.error(
      "[delete-account] failed:",
      err instanceof Error ? err.message : String(err),
    );
    return FAILED;
  }
}

type StripeSubscriptionList = {
  data: Array<{ id: string; status: string }>;
  has_more: boolean;
};

// Stripe is asked rather than billing_subscriptions, which lags behind its
// webhooks: a subscription started a moment ago is cancelled too.
async function cancelSubscriptions(
  deps: AccountDeletionDeps,
  customers: string[],
): Promise<void> {
  if (customers.length === 0) return;
  const stripe = deps.stripe;
  if (!stripe) throw new Error("STRIPE_SECRET_KEY is not configured");
  for (const customer of customers) {
    let startingAfter: string | undefined;
    for (;;) {
      const page = await stripe.request<StripeSubscriptionList>(
        "GET",
        "/v1/subscriptions",
        {
          customer,
          status: "all",
          limit: 100,
          ...(startingAfter ? { starting_after: startingAfter } : {}),
        },
      );
      for (const subscription of page.data) {
        if (ENDED_STATUSES.has(subscription.status)) continue;
        await cancelSubscription(stripe, subscription.id);
      }
      if (!page.has_more || page.data.length === 0) break;
      startingAfter = page.data[page.data.length - 1].id;
    }
  }
}

async function cancelSubscription(
  stripe: StripeClient,
  id: string,
): Promise<void> {
  try {
    // No `prorate`: Stripe's default credits nothing for the unused time.
    await stripe.request(
      "DELETE",
      `/v1/subscriptions/${encodeURIComponent(id)}`,
    );
  } catch (err) {
    // Gone already, from an earlier attempt or the customer's own cancel.
    if (err instanceof StripeApiError && err.status === 404) return;
    throw err;
  }
}

/** The organization_members rows of `orgIds`, as the counts need them. */
type MemberRow = { org_id: string; role: "owner" | "member" };

type MembershipRow = MemberRow & {
  organizations: { name: string } | null;
};

/** The store above, through a service-role client. */
export function accountDeletionStore(
  client: ServiceClientLike & {
    auth: {
      admin: {
        deleteUser(
          id: string,
        ): PromiseLike<{ error: { message: string; status?: number } | null }>;
      };
    };
  },
  userId: string,
): AccountDeletionStore {
  return {
    async memberships() {
      const { data, error } = await client
        .from("organization_members")
        .select("org_id, role, organizations(name)")
        .eq("user_id", userId);
      if (error) throw new Error(`membership lookup failed: ${error.message}`);
      const mine = (data ?? []) as MembershipRow[];
      if (mine.length === 0) return [];

      const { data: all, error: countError } = await client
        .from("organization_members")
        .select("org_id, role")
        .in(
          "org_id",
          mine.map((m) => m.org_id),
        );
      if (countError) {
        throw new Error(`member count failed: ${countError.message}`);
      }
      const rows = (all ?? []) as MemberRow[];
      return mine.map((m) => {
        const members = rows.filter((r) => r.org_id === m.org_id);
        return {
          org_id: m.org_id,
          org_name: m.organizations?.name ?? "an organization",
          role: m.role,
          member_count: members.length,
          owner_count: members.filter((r) => r.role === "owner").length,
        };
      });
    },

    async customers(owners) {
      const ids: string[] = [];
      for (const type of ["user", "organization"] as const) {
        const ownerIds = owners.filter((o) => o.type === type).map((o) => o.id);
        if (ownerIds.length === 0) continue;
        const { data, error } = await client
          .from("billing_customers")
          .select("stripe_customer_id")
          .eq("owner_type", type)
          .in("owner_id", ownerIds);
        if (error) {
          throw new Error(`billing customer lookup failed: ${error.message}`);
        }
        for (const row of (data ?? []) as Array<{
          stripe_customer_id: string;
        }>) {
          ids.push(row.stripe_customer_id);
        }
      }
      return ids;
    },

    async liveInstances() {
      const { data, error } = await client
        .from("remote_instances")
        .select("id, provider_resource_id")
        .eq("owner_user_id", userId)
        .neq("status", "deleted");
      if (error) throw new Error(`instance lookup failed: ${error.message}`);
      return (data ?? []) as LiveInstance[];
    },

    async markInstance(id, status) {
      const { error } = await client
        .from("remote_instances")
        .update({
          status,
          ...(status === "deleted" ? { endpoint_id: null } : {}),
          updated_at: new Date().toISOString(),
        })
        .eq("id", id)
        .eq("owner_user_id", userId);
      if (error) {
        throw new Error(`failed to mark instance ${status}: ${error.message}`);
      }
    },

    async deleteUser() {
      const { error } = await client.auth.admin.deleteUser(userId);
      // Already gone: an earlier attempt got this far.
      if (error && error.status !== 404) {
        throw new Error(`deleteUser failed: ${error.message}`);
      }
    },
  };
}
