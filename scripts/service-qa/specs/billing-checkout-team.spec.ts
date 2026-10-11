/**
 * Team checkout and the Team billing portal against the local database.
 *
 * Runs billing-checkout and billing-portal's handlers (lib.ts) in this
 * process with the real service-role stores, so organization ownership,
 * organization_has_team, billing_customers and billing_attach_customer are
 * the real ones. Only the outbound Stripe client is a stub: local runs have
 * no Stripe secret key. The subscription arrives as signed webhook events
 * posted with ../stripe.ts.
 */
import { randomBytes } from "node:crypto";
import { expect, it } from "vitest";
import { userHasPro } from "../../../supabase/functions/_shared/billing/entitlement";
import {
  billingStoreFor,
  organizationBillingFor,
} from "../../../supabase/functions/_shared/billing/store";
import type {
  StripeClient,
  StripeParams,
} from "../../../supabase/functions/_shared/billing/stripe-api";
import { createCheckout } from "../../../supabase/functions/billing-checkout/lib";
import { createPortal } from "../../../supabase/functions/billing-portal/lib";
import { handleOrganizationsRequest } from "../../../supabase/functions/organizations/lib";
import { clearBilling } from "../billing";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser, type TestUser } from "../seed";
import { postStripeEvent } from "../stripe";

const WEB_URL = "http://localhost:3001";

type StripeCall = { method: string; path: string; params?: StripeParams };

function stubStripe(customerId: string) {
  const calls: StripeCall[] = [];
  const stripe = {
    request: async (method: string, path: string, params?: StripeParams) => {
      calls.push({ method, path, params });
      if (path === "/v1/customers") return { id: customerId };
      if (path === "/v1/prices") return { data: [{ id: "price_sqa_team" }] };
      if (path === "/v1/checkout/sessions") {
        return { id: "cs_sqa_team", client_secret: "cs_sqa_team_secret" };
      }
      if (path === "/v1/billing_portal/sessions") {
        return { url: "https://billing.stripe.com/p/session/sqa-team" };
      }
      throw new Error(`unexpected Stripe call ${method} ${path}`);
    },
  } as StripeClient;
  return { stripe, calls };
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

it("sells Team to an organization's owner and gives every member Pro", async () => {
  const admin = getServiceClient();
  const owner = await createTestUser();
  const member = await createTestUser();
  const customerId = `cus_sqa${randomBytes(8).toString("hex")}`;
  const subscriptionId = `sub_sqa${randomBytes(8).toString("hex")}`;
  let orgId: string | null = null;

  const rpc = async (fn: string, args: Record<string, unknown>) => {
    const { data, error } = await admin.rpc(fn, args);
    return { data, error };
  };
  const depsFor = (user: TestUser, customer: string) => {
    const { stripe, calls } = stubStripe(customer);
    return {
      calls,
      deps: {
        user: { id: user.user.id, email: user.email },
        store: billingStoreFor(admin, user.user.id),
        organizations: organizationBillingFor(admin, user.user.id),
        stripe,
        webUrl: WEB_URL,
      },
    };
  };

  try {
    const created = await handleOrganizationsRequest(
      { action: "create", name: "Team Checkout Co" },
      { userId: owner.user.id, rpc, webUrl: WEB_URL },
    );
    orgId = (created.body.organization as { id: string }).id;
    const invited = await handleOrganizationsRequest(
      {
        action: "invite",
        organization_id: orgId,
        email: "buyer-member@example.com",
      },
      { userId: owner.user.id, rpc, webUrl: WEB_URL },
    );
    // Invite links carry the token in the fragment.
    const token = new URLSearchParams(
      new URL(String(invited.body.accept_url)).hash.slice(1),
    ).get("invite");
    await handleOrganizationsRequest(
      { action: "accept", token },
      { userId: member.user.id, rpc, webUrl: WEB_URL },
    );

    // ── A member cannot buy Team for the organization ────────────────────
    const asMember = depsFor(member, "cus_unused");
    const refused = await createCheckout(
      { plan: "team", organization_id: orgId },
      asMember.deps,
    );
    expect(refused.status).toBe(403);
    expect(asMember.calls).toEqual([]);

    // ── The owner opens checkout for the organization ────────────────────
    const asOwner = depsFor(owner, customerId);
    const opened = await createCheckout(
      { plan: "team", organization_id: orgId },
      asOwner.deps,
    );
    expect(opened).toEqual({
      status: 200,
      body: { client_secret: "cs_sqa_team_secret" },
    });
    expect(asOwner.calls.map((c) => c.path)).toEqual([
      "/v1/customers",
      "/v1/prices",
      "/v1/checkout/sessions",
    ]);
    expect(asOwner.calls[0].params).toMatchObject({
      "metadata[owner_type]": "organization",
      "metadata[owner_id]": orgId,
    });
    expect(asOwner.calls[1].params).toMatchObject({
      "lookup_keys[0]": "treq_team_monthly",
    });
    const sessionParams = asOwner.calls[2].params ?? {};
    expect(sessionParams).toMatchObject({
      customer: customerId,
      client_reference_id: orgId,
      "metadata[owner_type]": "organization",
      "metadata[owner_id]": orgId,
      "subscription_data[metadata][owner_type]": "organization",
      return_url: `${WEB_URL}/dashboard?tab=team&session_id={CHECKOUT_SESSION_ID}`,
    });
    expect(sessionParams).not.toHaveProperty(
      "subscription_data[trial_period_days]",
    );
    const customerRow = await admin
      .from("billing_customers")
      .select("owner_type, owner_id, stripe_customer_id")
      .eq("stripe_customer_id", customerId)
      .single();
    expect(customerRow.data).toEqual({
      owner_type: "organization",
      owner_id: orgId,
      stripe_customer_id: customerId,
    });
    const ownerCustomer = await billingStoreFor(
      admin,
      owner.user.id,
    ).getCustomer();
    expect(ownerCustomer).toBeNull();

    await recordOutcome("billing-checkout-team-01-session", {
      expectations: [
        "A member asking for Team checkout gets 403 and Stripe is never called.",
        "The owner's checkout creates a Stripe customer owned by the organization (billing_customers owner_type organization), resolves treq_team_monthly, and opens a session whose metadata and client_reference_id name the organization, returning to the Team tab.",
        "The Team session has no trial_period_days, and the owner gets no personal customer.",
      ],
      details: { refused, sessionParams, customerRow: customerRow.data },
    });

    // ── The webhook records Team; every member has Pro ───────────────────
    const completed = await postStripeEvent(
      event("checkout.session.completed", {
        id: "cs_sqa_team",
        object: "checkout.session",
        mode: "subscription",
        customer: customerId,
        subscription: subscriptionId,
        client_reference_id: orgId,
        metadata: { owner_type: "organization", owner_id: orgId },
      }),
    );
    expect(completed.body).toEqual({ received: true, result: "applied" });
    const subscribed = await postStripeEvent(
      event("customer.subscription.created", {
        id: subscriptionId,
        object: "subscription",
        customer: customerId,
        status: "active",
        cancel_at_period_end: false,
        trial_end: null,
        items: {
          data: [
            {
              current_period_end: Math.floor(Date.now() / 1000) + 30 * 86_400,
              price: { id: "price_sqa_team", lookup_key: "treq_team_monthly" },
            },
          ],
        },
      }),
    );
    expect(subscribed.body).toEqual({ received: true, result: "applied" });
    expect(await userHasPro(admin, member.user.id)).toBe(true);
    expect(await userHasPro(admin, owner.user.id)).toBe(true);

    const again = depsFor(owner, "cus_unused");
    const second = await createCheckout(
      { plan: "team", organization_id: orgId },
      again.deps,
    );
    expect(second.status).toBe(409);
    expect(again.calls).toEqual([]);

    const portal = depsFor(owner, "cus_unused");
    const portalResult = await createPortal({
      ...portal.deps,
      organizationId: orgId,
    });
    expect(portalResult.status).toBe(200);
    expect(portal.calls[0].params).toEqual({
      customer: customerId,
      return_url: `${WEB_URL}/dashboard?tab=team`,
    });
    const memberPortal = await createPortal({
      ...depsFor(member, "cus_unused").deps,
      organizationId: orgId,
    });
    expect(memberPortal.status).toBe(403);

    await recordOutcome("billing-checkout-team-02-webhook-and-portal", {
      expectations: [
        "Signed checkout.session.completed and customer.subscription.created events with the organization as owner make has_pro true for the owner and the member.",
        "A second Team checkout for the same organization answers 409 without calling Stripe.",
        "billing-portal opens the organization's customer for the owner, returning to the Team tab, and answers a member 403.",
      ],
      details: {
        completed: completed.body,
        subscribed: subscribed.body,
        second,
        portalResult,
        memberPortal,
      },
    });
  } finally {
    if (orgId) {
      await admin
        .from("billing_subscriptions")
        .delete()
        .eq("stripe_customer_id", customerId);
      await clearBilling(admin, [orgId]);
      await admin.from("organizations").delete().eq("id", orgId);
    }
    await deleteTestUser(admin, owner.user.id);
    await deleteTestUser(admin, member.user.id);
  }
}, 120_000);
