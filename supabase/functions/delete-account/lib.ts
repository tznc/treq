// Request logic for delete-account, free of Deno and Supabase imports so it
// runs under the unit tests and service-qa. index.ts supplies the JWT check,
// the service-role store, the Stripe client and the compute provider.
//
// The account deleted is always the one the bearer token names. The body is
// never read for a user id.
//
// A user who is the last owner of any organization is refused (409
// last_owner) before anything is released: every organization keeps at
// least one owner. They promote another member or delete the organization
// first. This includes an organization the user is the only member of, so
// a Team subscription is never ended as a side effect of account deletion.
//
// Order matters. Everything that holds an external resource or a charge is
// released first, and any failure there stops the request before the auth
// user is deleted, so the user can retry:
//   1. Cancel the user's live Stripe subscriptions immediately.
//   2. Destroy cloud workspace machines at the provider. The provider id is
//      lost once the rows cascade away, so a machine left running here
//      could never be found again.
//   3. Revoke the Linear OAuth token. Best effort: the stored token is
//      deleted with the user either way.
//   4. Delete personal GitHub App installation rows. Their foreign key is
//      SET NULL, which would otherwise leave them ownerless.
//   5. Delete the auth user. Foreign keys cascade the remaining user rows,
//      including the user's memberships in organizations that have another
//      owner.
//
// billing_customers and billing_subscriptions have no foreign key to the
// user and are kept on purpose, for invoices, refunds and disputes.

import type { HandlerResult } from "../_shared/intent-state.ts";
import {
  StripeApiError,
  type StripeClient,
} from "../_shared/billing/stripe-api.ts";
import { fetchWithTimeout } from "../_shared/http/fetch-with-timeout.ts";
import type { ManagedComputeProvider, ProviderKind } from "../_shared/remote/provider.ts";

export type LiveInstance = {
  id: string;
  provider_kind: string;
  provider_resource_id: string | null;
  active_placement_id?: string | null;
};

export type OrganizationRef = { id: string; name: string };

export interface DeleteAccountStore {
  /** Organizations in which the user is the only owner. */
  lastOwnedOrganizations(): Promise<OrganizationRef[]>;
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
  /** The adapter for a placement (`placementProviders` in production). */
  provider(kind: ProviderKind, placementId?: string | null): ManagedComputeProvider;
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

  const needingOwner = await store.lastOwnedOrganizations();
  if (needingOwner.length > 0) {
    return {
      status: 409,
      body: {
        error: `You are the last owner of ${needingOwner
          .map((org) => org.name)
          .join(
            ", ",
          )}. Make another member an owner, or delete the organization, before deleting your account.`,
        code: "last_owner",
        organizations: needingOwner,
      },
    };
  }

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

  for (const instance of await store.liveInstances()) {
    try {
      if (instance.provider_resource_id) {
        await deps
          .provider(instance.provider_kind as ProviderKind, instance.active_placement_id ?? null)
          .deleteInstance(instance.provider_resource_id, { placementId: instance.active_placement_id });
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
  const response = await fetchWithTimeout(
    "https://api.linear.app/oauth/revoke",
    { method: "POST", headers: { Authorization: `Bearer ${accessToken}` } },
    undefined,
    fetchImpl,
  );
  if (!response.ok && response.status !== 400 && response.status !== 401) {
    throw new Error(`Linear revoke returned HTTP ${response.status}`);
  }
}

type QueryResult = { data: unknown; error: { message: string } | null };

// supabase-js builders are too deeply generic to match a structural type, so
// `from` stays untyped, as in _shared/billing/store.ts.
export interface DeleteAccountClient {
  rpc(fn: string, args: Record<string, unknown>): PromiseLike<QueryResult>;
  // deno-lint-ignore no-explicit-any
  // biome-ignore lint/suspicious/noExplicitAny: supabase-js query builders are too generic to type structurally
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
    async lastOwnedOrganizations() {
      const owned = check(
        await client
          .from("organization_members")
          .select("org_id")
          .eq("user_id", userId)
          .eq("role", "owner"),
        "owned organization lookup",
      ) as { org_id: string }[];
      if (owned.length === 0) return [];

      const owners = check(
        await client
          .from("organization_members")
          .select("org_id, user_id")
          .eq("role", "owner")
          .in(
            "org_id",
            owned.map((row) => row.org_id),
          ),
        "organization owner lookup",
      ) as { org_id: string; user_id: string }[];
      const blocking = owned
        .map((row) => row.org_id)
        .filter(
          (orgId) =>
            !owners.some((o) => o.org_id === orgId && o.user_id !== userId),
        );
      if (blocking.length === 0) return [];

      const orgs = check(
        await client
          .from("organizations")
          .select("id, name")
          .in("id", blocking)
          .order("name"),
        "organization lookup",
      ) as OrganizationRef[];
      return orgs;
    },

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

      return [...ids];
    },

    async liveInstances() {
      return check(
        await client
          .from("remote_instances")
          .select("id, provider_kind, provider_resource_id, active_placement_id")
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
      // Tokens live in Vault (032_linear_tokens_vault.sql).
      const rows = check(
        await client.rpc("linear_tokens_load", { p_user_id: userId }),
        "Linear token lookup",
      ) as { access_token: string | null }[] | null;
      return rows?.[0]?.access_token ?? null;
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
