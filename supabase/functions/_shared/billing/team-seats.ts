// Team seats (prds/billing-and-teams.md, "Organizations and seats"). The
// treq_team_monthly price is graduated: the first 5 units carry the flat
// US$199 and each unit after that costs US$8. So the subscription's quantity
// is the organization's member count, and Stripe bills the extra seats.
// Pending invites are not billed.
//
// The organizations Edge Function calls syncTeamSeats after every join,
// leave and removal. Team checkout starts at the member count too.

import { LOOKUP_KEY_BY_PLAN } from "./plans.ts";
import type { ServiceClientLike } from "./store.ts";
import type { StripeClient } from "./stripe-api.ts";

/** Members covered by Team's flat price. */
export const TEAM_INCLUDED_SEATS = 5;

/** A subscription needs a quantity of at least 1. */
export function teamQuantity(members: number): number {
  return Math.max(1, members);
}

export interface TeamSeatsStore {
  memberCount(organizationId: string): Promise<number>;
  /** The organization's Team subscription that is still billing, if any. */
  billingSubscriptionId(organizationId: string): Promise<string | null>;
}

export type SeatSyncResult = "no_subscription" | "unchanged" | "updated";

type SubscriptionItems = {
  items?: {
    data?: {
      id: string;
      quantity?: number;
      price?: { lookup_key?: string | null };
    }[];
  };
};

/**
 * Sets the Team item's quantity to the member count, prorated. Reading the
 * count just before writing keeps concurrent calls converging on it.
 */
export async function syncTeamSeats(
  organizationId: string,
  deps: { store: TeamSeatsStore; stripe: StripeClient },
): Promise<SeatSyncResult> {
  const subscriptionId = await deps.store.billingSubscriptionId(organizationId);
  if (!subscriptionId) return "no_subscription";

  const subscription = await deps.stripe.request<SubscriptionItems>(
    "GET",
    `/v1/subscriptions/${encodeURIComponent(subscriptionId)}`,
  );
  const item = subscription.items?.data?.find(
    (candidate) => candidate.price?.lookup_key === LOOKUP_KEY_BY_PLAN.team,
  );
  if (!item) {
    throw new Error(`subscription ${subscriptionId} has no Team price`);
  }

  const quantity = teamQuantity(await deps.store.memberCount(organizationId));
  if (item.quantity === quantity) return "unchanged";

  await deps.stripe.request(
    "POST",
    `/v1/subscription_items/${encodeURIComponent(item.id)}`,
    { quantity, proration_behavior: "create_prorations" },
  );
  return "updated";
}

type QueryResult = { data: unknown; error: { message: string } | null };

// Statuses Stripe still bills. A subscription set to cancel at period end
// stays active until then, so its seats still change.
const BILLING_STATUSES = ["trialing", "active", "past_due"];

export function teamSeatsStoreFor(client: ServiceClientLike): TeamSeatsStore {
  return {
    async memberCount(organizationId) {
      const { count, error } = (await client
        .from("organization_members")
        .select("user_id", { count: "exact", head: true })
        .eq("org_id", organizationId)) as QueryResult & {
        count: number | null;
      };
      if (error) throw new Error(`member count failed: ${error.message}`);
      return count ?? 0;
    },
    async billingSubscriptionId(organizationId) {
      const { data, error }: QueryResult = await client
        .from("billing_subscriptions")
        .select("stripe_subscription_id")
        .eq("owner_type", "organization")
        .eq("owner_id", organizationId)
        .eq("plan", "team")
        .in("status", BILLING_STATUSES)
        .order("current_period_end", { ascending: false, nullsFirst: false })
        .limit(1)
        .maybeSingle();
      if (error) {
        throw new Error(`Team subscription lookup failed: ${error.message}`);
      }
      return (
        (data as { stripe_subscription_id?: string } | null)
          ?.stripe_subscription_id ?? null
      );
    },
  };
}
