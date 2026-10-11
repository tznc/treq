// Request logic for billing-checkout, free of Deno and Supabase imports so it
// runs under the unit tests and service-qa. index.ts verifies the user's JWT
// and supplies the stores and Stripe client.
//
// Pro is bought by and for the signed-in user. Team is bought by an owner of
// an organization, for the organization: the organization is the Stripe
// customer's owner and the subscription's owner, so every member gets Pro.

import {
  type BillingOwnerType,
  buildCheckoutSessionParams,
  parseCheckoutBody,
} from "../_shared/billing/checkout.ts";
import {
  type BillingPlan,
  LOOKUP_KEY_BY_PLAN,
} from "../_shared/billing/plans.ts";
import type {
  BillingStore,
  OrganizationBilling,
} from "../_shared/billing/store.ts";
import {
  StripeApiError,
  type StripeClient,
  type StripeParams,
} from "../_shared/billing/stripe-api.ts";
import { teamQuantity } from "../_shared/billing/team-seats.ts";

export type CheckoutDeps = {
  user: { id: string; email?: string | null };
  /** The user's billing rows. */
  store: BillingStore;
  /** Needed for Team checkout. */
  organizations?: OrganizationBilling;
  stripe: StripeClient;
  webUrl: string;
};

export type CheckoutResult = { status: number; body: Record<string, unknown> };

const PLAN_NAMES: Readonly<Record<BillingPlan, string>> = {
  pro: "Pro",
  team: "Team",
};

type Owner = {
  type: BillingOwnerType;
  id: string;
  store: BillingStore;
  /** Units to buy: 1 for Pro, the member count for Team. */
  quantity: number;
};

// Returns the owner's Stripe customer, creating and recording one on first
// checkout. The idempotency key makes concurrent first checkouts share one
// Stripe customer. If they still diverge, billing_attach_customer keeps the
// first one recorded and the other stays unused in Stripe.
async function customerFor(owner: Owner, deps: CheckoutDeps) {
  const existing = await owner.store.getCustomer();
  if (existing) return existing;

  const params: StripeParams = {};
  // The email is for Stripe receipts only. Ownership never depends on it.
  if (deps.user.email) params.email = deps.user.email;
  params["metadata[owner_type]"] = owner.type;
  params["metadata[owner_id]"] = owner.id;
  const created = await deps.stripe.request<{ id: string }>(
    "POST",
    "/v1/customers",
    params,
    { idempotencyKey: `treq-customer-${owner.type}-${owner.id}` },
  );
  await owner.store.attachCustomer(created.id);
  const recorded = await owner.store.getCustomer();
  if (!recorded) throw new Error("billing customer was not recorded");
  return recorded;
}

async function openSession(
  plan: BillingPlan,
  owner: Owner,
  deps: CheckoutDeps,
): Promise<CheckoutResult> {
  try {
    const customer = await customerFor(owner, deps);

    const prices = await deps.stripe.request<{ data: { id: string }[] }>(
      "GET",
      "/v1/prices",
      {
        "lookup_keys[0]": LOOKUP_KEY_BY_PLAN[plan],
        active: true,
        limit: 1,
      },
    );
    const priceId = prices.data[0]?.id;
    if (!priceId) {
      return {
        status: 500,
        body: { error: `The ${PLAN_NAMES[plan]} price is not configured` },
      };
    }

    const session = await deps.stripe.request<{ client_secret: string }>(
      "POST",
      "/v1/checkout/sessions",
      buildCheckoutSessionParams({
        customerId: customer.stripe_customer_id,
        priceId,
        ownerId: owner.id,
        ownerType: owner.type,
        trialEligible: plan === "pro" && customer.trial_used_at === null,
        webUrl: deps.webUrl,
        quantity: owner.quantity,
      }),
    );
    return { status: 200, body: { client_secret: session.client_secret } };
  } catch (err) {
    if (err instanceof StripeApiError) {
      console.error(`[billing-checkout] Stripe ${err.status}: ${err.message}`);
      return { status: 502, body: { error: "Stripe request failed" } };
    }
    throw err;
  }
}

export async function createCheckout(
  body: unknown,
  deps: CheckoutDeps,
): Promise<CheckoutResult> {
  const parsed = parseCheckoutBody(body);
  if (!parsed.ok)
    return { status: parsed.status, body: { error: parsed.error } };

  if (parsed.plan === "team") {
    if (!deps.organizations)
      throw new Error("organization billing is not configured");
    if (!(await deps.organizations.isOwner(parsed.organizationId))) {
      return {
        status: 403,
        body: { error: "Only an owner of this organization can buy Team." },
      };
    }
    const store = deps.organizations.store(parsed.organizationId);
    // One Team per organization. A second would bill twice.
    if (await store.hasPro()) {
      return {
        status: 409,
        body: {
          error:
            "This organization already has Team. Use Manage billing to change it.",
        },
      };
    }
    // Team's price is graduated, so buying one unit per member bills the
    // seats beyond the included five. Joins and leaves update it later.
    const members = await deps.organizations.memberCount(parsed.organizationId);
    return openSession(
      "team",
      {
        type: "organization",
        id: parsed.organizationId,
        store,
        quantity: teamQuantity(members),
      },
      deps,
    );
  }

  // One Pro subscription per user. A second one would bill twice, and a
  // second trial would stack on the first.
  if (await deps.store.hasPro()) {
    return {
      status: 409,
      body: { error: "You already have Pro. Use Manage billing to change it." },
    };
  }
  return openSession(
    "pro",
    { type: "user", id: deps.user.id, store: deps.store, quantity: 1 },
    deps,
  );
}
