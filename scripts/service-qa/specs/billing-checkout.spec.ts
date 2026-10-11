/**
 * billing-checkout and billing-portal against the local database.
 *
 * Runs the functions' handlers (lib.ts) in this process with the real
 * service-role store (`_shared/billing/store.ts`), so has_pro,
 * billing_customers and billing_attach_customer are the real ones. Only the
 * outbound Stripe client is a stub: local runs have no Stripe secret key.
 * Entitlement comes from signed webhook events posted with ../stripe.ts.
 */
import { randomBytes } from "node:crypto";
import { expect, it } from "vitest";
import { billingStoreFor } from "../../../supabase/functions/_shared/billing/store";
import type {
  StripeClient,
  StripeParams,
} from "../../../supabase/functions/_shared/billing/stripe-api";
import { createCheckout } from "../../../supabase/functions/billing-checkout/lib";
import { createPortal } from "../../../supabase/functions/billing-portal/lib";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
} from "../seed";
import { postStripeEvent } from "../stripe";

const WEB_URL = "http://localhost:3001";

type StripeCall = { method: string; path: string; params?: StripeParams };

function stubStripe(customerId: string): {
  stripe: StripeClient;
  calls: StripeCall[];
} {
  const calls: StripeCall[] = [];
  const stripe = {
    request: async (method: string, path: string, params?: StripeParams) => {
      calls.push({ method, path, params });
      if (path === "/v1/customers") return { id: customerId };
      if (path === "/v1/prices") return { data: [{ id: "price_sqa_pro" }] };
      if (path === "/v1/checkout/sessions") {
        return { id: "cs_sqa", client_secret: "cs_sqa_secret" };
      }
      if (path === "/v1/billing_portal/sessions") {
        return { url: "https://billing.stripe.com/p/session/sqa" };
      }
      throw new Error(`unexpected Stripe call ${method} ${path}`);
    },
  } as StripeClient;
  return { stripe, calls };
}

function sessionParams(calls: StripeCall[]): StripeParams | undefined {
  return calls.find((c) => c.path === "/v1/checkout/sessions")?.params;
}

function event(type: string, object: Record<string, unknown>) {
  return {
    id: `evt_sqa${randomBytes(8).toString("hex")}`,
    object: "event",
    type,
    created: Math.floor(Date.now() / 1000),
    data: { object },
  };
}

it("opens checkout with one trial per user and a portal for the customer", async () => {
  const admin = getServiceClient();
  const buyer = await createTestUser();
  const stranger = await createTestUser();
  const customerId = `cus_sqa${randomBytes(8).toString("hex")}`;
  const subscriptionId = `sub_sqa${randomBytes(8).toString("hex")}`;

  try {
    const store = billingStoreFor(admin, buyer.user.id);
    const user = { id: buyer.user.id, email: buyer.email };

    // ── First checkout: customer created, recorded, trial offered ────────
    const first = stubStripe(customerId);
    const opened = await createCheckout(
      { plan: "pro" },
      { user, store, stripe: first.stripe, webUrl: WEB_URL },
    );
    expect(opened).toEqual({
      status: 200,
      body: { client_secret: "cs_sqa_secret" },
    });
    expect(first.calls.map((c) => c.path)).toEqual([
      "/v1/customers",
      "/v1/prices",
      "/v1/checkout/sessions",
    ]);
    expect(sessionParams(first.calls)).toMatchObject({
      customer: customerId,
      client_reference_id: buyer.user.id,
      "metadata[owner_id]": buyer.user.id,
      "subscription_data[trial_period_days]": 14,
    });

    const { client: buyerClient } = await signInWithEmailPassword(
      buyer.email,
      buyer.password,
    );
    const ownRow = await buyerClient
      .from("billing_customers")
      .select("stripe_customer_id, trial_used_at");
    expect(ownRow.data).toEqual([
      { stripe_customer_id: customerId, trial_used_at: null },
    ]);

    const second = stubStripe("cus_sqa_should_not_be_created");
    await createCheckout(
      { plan: "pro" },
      { user, store, stripe: second.stripe, webUrl: WEB_URL },
    );
    expect(second.calls.map((c) => c.path)).toEqual([
      "/v1/prices",
      "/v1/checkout/sessions",
    ]);
    expect(sessionParams(second.calls)?.customer).toBe(customerId);

    await recordOutcome("billing-checkout-01-customer-and-trial", {
      expectations: [
        "The first checkout creates a Stripe customer, records it in billing_customers through billing_attach_customer, and opens an embedded session with a 14-day trial.",
        "A second checkout reuses the recorded customer instead of creating another.",
        "The buyer reads their own billing_customers row through RLS.",
      ],
      details: {
        firstCalls: first.calls,
        secondCalls: second.calls.map((c) => c.path),
        ownRow: ownRow.data,
      },
    });

    // ── The webhook makes the buyer Pro, so checkout refuses ─────────────
    const trialEnd = Math.floor(Date.now() / 1000) + 14 * 86_400;
    const subscription = {
      id: subscriptionId,
      object: "subscription",
      customer: customerId,
      status: "trialing",
      cancel_at_period_end: false,
      trial_end: trialEnd,
      items: {
        data: [
          {
            current_period_end: trialEnd,
            price: { id: "price_sqa_pro", lookup_key: "treq_pro_monthly" },
          },
        ],
      },
    };
    const completed = await postStripeEvent(
      event("checkout.session.completed", {
        id: "cs_sqa",
        object: "checkout.session",
        mode: "subscription",
        customer: customerId,
        subscription: subscriptionId,
        client_reference_id: buyer.user.id,
        metadata: { owner_type: "user", owner_id: buyer.user.id },
      }),
    );
    expect(completed.body).toEqual({ received: true, result: "applied" });
    const created = await postStripeEvent(
      event("customer.subscription.created", subscription),
    );
    expect(created.body).toEqual({ received: true, result: "applied" });

    const third = stubStripe("cus_unused");
    const refused = await createCheckout(
      { plan: "pro" },
      { user, store, stripe: third.stripe, webUrl: WEB_URL },
    );
    expect(refused.status).toBe(409);
    expect(third.calls).toEqual([]);

    const portal = stubStripe("cus_unused");
    const portalResult = await createPortal({
      store,
      stripe: portal.stripe,
      webUrl: WEB_URL,
    });
    expect(portalResult).toEqual({
      status: 200,
      body: { url: "https://billing.stripe.com/p/session/sqa" },
    });
    expect(portal.calls[0]?.params).toEqual({
      customer: customerId,
      return_url: `${WEB_URL}/dashboard?tab=subscription`,
    });

    const strangerPortal = await createPortal({
      store: billingStoreFor(admin, stranger.user.id),
      stripe: stubStripe("cus_unused").stripe,
      webUrl: WEB_URL,
    });
    expect(strangerPortal.status).toBe(404);

    await recordOutcome("billing-checkout-02-pro-refuses-and-portal", {
      expectations: [
        "Once signed webhook events make the buyer Pro, billing-checkout answers 409 without calling Stripe.",
        "billing-portal opens a session for the buyer's own customer with the dashboard as the return URL.",
        "A user with no billing customer gets 404 from billing-portal.",
      ],
      details: { refused, portalResult, strangerPortal },
    });

    // ── After the trial subscription ends, no second trial ──────────────
    const ended = await postStripeEvent(
      event("customer.subscription.deleted", {
        ...subscription,
        status: "canceled",
      }),
    );
    expect(ended.body).toEqual({ received: true, result: "applied" });

    const again = stubStripe("cus_unused");
    const reopened = await createCheckout(
      { plan: "pro" },
      { user, store, stripe: again.stripe, webUrl: WEB_URL },
    );
    expect(reopened.status).toBe(200);
    expect(sessionParams(again.calls)).not.toHaveProperty(
      "subscription_data[trial_period_days]",
    );
    expect(sessionParams(again.calls)?.customer).toBe(customerId);

    await recordOutcome("billing-checkout-03-one-trial", {
      expectations: [
        "After the trialing subscription is deleted, checkout opens again on the same customer without trial_period_days.",
      ],
      details: { reopenedParams: sessionParams(again.calls) },
    });
  } finally {
    await admin
      .from("billing_subscriptions")
      .delete()
      .eq("stripe_customer_id", customerId);
    await admin
      .from("billing_customers")
      .delete()
      .in("owner_id", [buyer.user.id, stranger.user.id]);
    await deleteTestUser(admin, buyer.user.id);
    await deleteTestUser(admin, stranger.user.id);
  }
}, 120_000);
