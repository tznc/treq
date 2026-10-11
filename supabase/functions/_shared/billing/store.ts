// The billing rows billing-checkout and billing-portal need for one owner (a
// user, or an organization for Team), read and written with a service-role
// Supabase client. The client type is structural so the same code runs under
// Deno and in service-qa.

import type { BillingOwnerType } from "./checkout.ts";

export type BillingCustomer = {
  stripe_customer_id: string;
  trial_used_at: string | null;
};

export interface BillingStore {
  /** For a user, has_pro. For an organization, whether it has Team. */
  hasPro(): Promise<boolean>;
  getCustomer(): Promise<BillingCustomer | null>;
  /** Records the customer unless the owner has one, and returns the owner's customer. */
  attachCustomer(stripeCustomerId: string): Promise<string>;
}

/** Team checkout and the Team billing portal, for one signed-in user. */
export interface OrganizationBilling {
  /** Whether the user owns the organization. */
  isOwner(organizationId: string): Promise<boolean>;
  /** Team is billed per member beyond the included seats. */
  memberCount(organizationId: string): Promise<number>;
  store(organizationId: string): BillingStore;
}

type QueryResult = { data: unknown; error: { message: string } | null };

// supabase-js query builders are too deeply generic to match a structural
// type, so `from` stays untyped. The selects below are the only uses.
export interface ServiceClientLike {
  rpc(fn: string, args: Record<string, unknown>): PromiseLike<QueryResult>;
  // deno-lint-ignore no-explicit-any
  from(table: string): any;
}

function ownerStore(
  client: ServiceClientLike,
  ownerType: BillingOwnerType,
  ownerId: string,
): BillingStore {
  return {
    async hasPro() {
      const { data, error } =
        ownerType === "user"
          ? await client.rpc("has_pro", { p_user_id: ownerId })
          : await client.rpc("organization_has_team", { p_org_id: ownerId });
      if (error) throw new Error(`entitlement check failed: ${error.message}`);
      return data === true;
    },
    async getCustomer() {
      const { data, error }: QueryResult = await client
        .from("billing_customers")
        .select("stripe_customer_id, trial_used_at")
        .eq("owner_type", ownerType)
        .eq("owner_id", ownerId)
        .maybeSingle();
      if (error)
        throw new Error(`billing customer lookup failed: ${error.message}`);
      return (data as BillingCustomer | null) ?? null;
    },
    async attachCustomer(stripeCustomerId) {
      const { data, error } = await client.rpc("billing_attach_customer", {
        p_owner_type: ownerType,
        p_owner_id: ownerId,
        p_customer_id: stripeCustomerId,
      });
      if (error || typeof data !== "string") {
        throw new Error(
          `billing_attach_customer failed: ${error?.message ?? "no customer"}`,
        );
      }
      return data;
    },
  };
}

export function billingStoreFor(
  client: ServiceClientLike,
  userId: string,
): BillingStore {
  return ownerStore(client, "user", userId);
}

export function organizationBillingFor(
  client: ServiceClientLike,
  userId: string,
): OrganizationBilling {
  return {
    async isOwner(organizationId) {
      const { data, error }: QueryResult = await client
        .from("organization_members")
        .select("role")
        .eq("org_id", organizationId)
        .eq("user_id", userId)
        .maybeSingle();
      if (error) throw new Error(`membership lookup failed: ${error.message}`);
      return (data as { role?: string } | null)?.role === "owner";
    },
    async memberCount(organizationId) {
      const { count, error } = await client
        .from("organization_members")
        .select("user_id", { count: "exact", head: true })
        .eq("org_id", organizationId);
      if (error) throw new Error(`member count failed: ${error.message}`);
      return (count as number | null) ?? 0;
    },
    store: (organizationId) =>
      ownerStore(client, "organization", organizationId),
  };
}
