// Request logic for delete-account, free of Deno and Supabase imports so it
// runs under the unit tests and service-qa. index.ts supplies the JWT check,
// the service-role store, the Stripe client and the compute provider.
//
// The account deleted is always the one the bearer token names. The body is
// never read for a user id.
//
// Order matters. Everything that holds an external resource or a charge is
// released first, and any failure there stops the request before the auth
// user is deleted, so the user can retry:
//   1. Cancel live Stripe subscriptions immediately (the user's own, and the
//      Team subscription of any organization the user is the only member
//      of, since deleting the user deletes that organization).
//   2. Destroy cloud workspace machines at the provider. The provider id is
//      lost once the rows cascade away, so a machine left running here
//      could never be found again.
//   3. Revoke the Linear OAuth token. Best effort: the stored token is
//      deleted with the user either way.
//   4. Delete personal GitHub App installation rows. Their foreign key is
//      SET NULL, which would otherwise leave them ownerless.
//   5. Delete the auth user. Foreign keys cascade the remaining user rows,
//      and the organizations_on_account_delete trigger (029) hands
//      organizations to a successor or deletes them.
//
// billing_customers and billing_subscriptions have no foreign key to the
// user and are kept on purpose, for invoices, refunds and disputes.

import type { HandlerResult } from "../_shared/intent-state.ts";
import {
  StripeApiError,
  type StripeClient,
} from "../_shared/billing/stripe-api.ts";
import type { ManagedComputeProvider } from "../_shared/remote/sprites-adapter.ts";

export type LiveInstance = { id: string; provider_resource_id: string | null };

export interface DeleteAccountStore {
  /** Stripe subscription ids that may still bill once the user is gone. */
  subscriptionsToCancel(): Promise<string[]>;
  /** The user's remote instances not yet marked deleted. */
  liveInstances(): Promise<LiveInstance[]>;
  markInstanceDeleted(instanceId: string): Promise<void>;
  linearAccessToken(): Promise<string | null>;
  deletePersonalInstallations(): Promise<void>;
  /** Deletes the auth user. Succeeds when the user is already gone. */
  deleteUser(): Promise<void>;
}

export type DeleteAccountDeps = {
  method: string;
  authorization: string | null;
  /** Ignored. Present so tests can prove a body user id changes nothing. */
  body?: unknown;
  /** Resolves a Supabase access token to its user, or null. */
  verifyUser(token: string): Promise<{ id: string } | null>;
  storeFor(userId: string): DeleteAccountStore;
  /** Null when STRIPE_SECRET_KEY is not configured. */
  stripe: StripeClient | null;
  provider(): ManagedComputeProvider;
  revokeLinearToken(accessToken: string): Promise<void>;
};

// Stripe statuses after which a subscription can no longer charge.
const ENDED_STATUSES = new Set(["canceled", "incomplete_expired"]);

async function cancelSubscription(
  stripe: StripeClient,
  subscriptionId: string,
): Promise<void> {
  const path = `/v1/subscriptions/${encodeURIComponent(subscriptionId)}`;
  let current: { status?: string };
  try {
    current = await stripe.request<{ status?: string }>("GET", path);
  } catch (err) {
    if (err instanceof StripeApiError && err.status === 404) return;
    throw err;
  }
  if (current.status && ENDED_STATUSES.has(current.status)) return;
  await stripe.request("DELETE", path);
}

export async function deleteAccount(
  deps: DeleteAccountDeps,
): Promise<HandlerResult> {
  if (deps.method !== "POST") {
    return { status: 405, body: { error: "Method not allowed" } };
  }
  const token = (deps.authorization ?? "").replace(/^Bearer\s+/i, "").trim();
  if (!token) return { status: 401, body: { error: "Unauthorized" } };
  const user = await deps.verifyUser(token);
  if (!user) return { status: 401, body: { error: "Unauthorized" } };

  const store = deps.storeFor(user.id);

  const subscriptions = await store.subscriptionsToCancel();
  if (subscriptions.length > 0) {
    if (!deps.stripe) {
      console.error("[delete-account] STRIPE_SECRET_KEY is not configured");
      return {
        status: 500,
        body: {
          error:
            "Billing is not configured, so your subscription could not be canceled. Your account was not deleted.",
        },
      };
    }
    for (const subscriptionId of subscriptions) {
      try {
        await cancelSubscription(deps.stripe, subscriptionId);
      } catch (err) {
        console.error(
          `[delete-account] cancel ${subscriptionId} failed:`,
          err instanceof Error ? err.message : String(err),
        );
        return {
          status: 502,
          body: {
            error:
              "Your subscription could not be canceled. Your account was not deleted. Try again.",
          },
        };
      }
    }
  }

  const instances = await store.liveInstances();
  if (instances.length > 0) {
    const provider = deps.provider();
    for (const instance of instances) {
      try {
        if (instance.provider_resource_id) {
          await provider.deleteInstance(instance.provider_resource_id);
        }
        await store.markInstanceDeleted(instance.id);
      } catch (err) {
        console.error(
          `[delete-account] delete instance ${instance.id} failed:`,
          err instanceof Error ? err.message : String(err),
        );
        return {
          status: 502,
          body: {
            error:
              "Your cloud workspace could not be deleted. Your account was not deleted. Try again.",
          },
        };
      }
    }
  }

  const linearToken = await store.linearAccessToken();
  if (linearToken) {
    try {
      await deps.revokeLinearToken(linearToken);
    } catch (err) {
      console.warn(
        "[delete-account] Linear token revocation failed:",
        err instanceof Error ? err.message : String(err),
      );
    }
  }

  await store.deletePersonalInstallations();
  await store.deleteUser();
  return { status: 200, body: { deleted: true } };
}

/** Revokes a Linear OAuth access token. A token Linear already rejects counts as revoked. */
export async function revokeLinearToken(
  accessToken: string,
  fetchImpl: typeof fetch = fetch,
): Promise<void> {
  const response = await fetchImpl("https://api.linear.app/oauth/revoke", {
    method: "POST",
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  if (!response.ok && response.status !== 400 && response.status !== 401) {
    throw new Error(`Linear revoke returned HTTP ${response.status}`);
  }
}

type QueryResult = { data: unknown; error: { message: string } | null };

// supabase-js builders are too deeply generic to match a structural type, so
// `from` stays untyped, as in _shared/billing/store.ts.
export interface DeleteAccountClient {
  // deno-lint-ignore no-explicit-any
  from(table: string): any;
  auth: {
    admin: {
      deleteUser(
        id: string,
      ): PromiseLike<{ error: { message: string; status?: number } | null }>;
    };
  };
}

function check(result: QueryResult, what: string): unknown {
  if (result.error) throw new Error(`${what} failed: ${result.error.message}`);
  return result.data;
}

const ENDED_LIST = `(${[...ENDED_STATUSES].join(",")})`;

export function deleteAccountStoreFor(
  client: DeleteAccountClient,
  userId: string,
): DeleteAccountStore {
  return {
    async subscriptionsToCancel() {
      const ids = new Set<string>();

      const own = check(
        await client
          .from("billing_subscriptions")
          .select("stripe_subscription_id")
          .eq("owner_type", "user")
          .eq("owner_id", userId)
          .not("status", "in", ENDED_LIST),
        "user subscription lookup",
      ) as { stripe_subscription_id: string }[];
      for (const row of own) ids.add(row.stripe_subscription_id);

      // A subscription can arrive before its customer is mapped to an
      // owner, leaving owner_id null. Match those through the customer.
      const customer = check(
        await client
          .from("billing_customers")
          .select("stripe_customer_id")
          .eq("owner_type", "user")
          .eq("owner_id", userId)
          .maybeSingle(),
        "billing customer lookup",
      ) as { stripe_customer_id: string } | null;
      if (customer) {
        const byCustomer = check(
          await client
            .from("billing_subscriptions")
            .select("stripe_subscription_id")
            .eq("stripe_customer_id", customer.stripe_customer_id)
            .not("status", "in", ENDED_LIST),
          "customer subscription lookup",
        ) as { stripe_subscription_id: string }[];
        for (const row of byCustomer) ids.add(row.stripe_subscription_id);
      }

      const memberships = check(
        await client
          .from("organization_members")
          .select("org_id")
          .eq("user_id", userId),
        "membership lookup",
      ) as { org_id: string }[];
      const soleOrgs: string[] = [];
      for (const { org_id } of memberships) {
        const { count, error } = await client
          .from("organization_members")
          .select("user_id", { count: "exact", head: true })
          .eq("org_id", org_id);
        if (error) throw new Error(`member count failed: ${error.message}`);
        if (count === 1) soleOrgs.push(org_id);
      }
      if (soleOrgs.length > 0) {
        const team = check(
          await client
            .from("billing_subscriptions")
            .select("stripe_subscription_id")
            .eq("owner_type", "organization")
            .in("owner_id", soleOrgs)
            .not("status", "in", ENDED_LIST),
          "organization subscription lookup",
        ) as { stripe_subscription_id: string }[];
        for (const row of team) ids.add(row.stripe_subscription_id);
      }

      return [...ids];
    },

    async liveInstances() {
      return check(
        await client
          .from("remote_instances")
          .select("id, provider_resource_id")
          .eq("owner_user_id", userId)
          .neq("status", "deleted"),
        "remote instance lookup",
      ) as LiveInstance[];
    },

    async markInstanceDeleted(instanceId) {
      check(
        await client
          .from("remote_instances")
          .update({
            status: "deleted",
            endpoint_id: null,
            updated_at: new Date().toISOString(),
          })
          .eq("id", instanceId)
          .eq("owner_user_id", userId),
        "remote instance update",
      );
    },

    async linearAccessToken() {
      const row = check(
        await client
          .from("linear_oauth_tokens")
          .select("access_token")
          .eq("user_id", userId)
          .maybeSingle(),
        "Linear token lookup",
      ) as { access_token: string } | null;
      return row?.access_token ?? null;
    },

    async deletePersonalInstallations() {
      check(
        await client
          .from("github_app_installations")
          .delete()
          .eq("linked_user_id", userId)
          .is("organization_id", null),
        "GitHub installation delete",
      );
    },

    async deleteUser() {
      const { error } = await client.auth.admin.deleteUser(userId);
      if (error && error.status !== 404) {
        throw new Error(`auth user delete failed: ${error.message}`);
      }
    },
  };
}
