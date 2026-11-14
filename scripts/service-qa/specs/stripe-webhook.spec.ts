/**
 * Stripe webhook → billing tables → entitlement, against the local stack.
 *
 * Posts signed Stripe events (see ../stripe.ts for the transport), then reads
 * the result the way clients do: a signed-in user's public.subscriptions row,
 * has_pro, and RLS-filtered billing rows.
 */
import { randomBytes } from "node:crypto";
import { expect, it } from "@jest/globals";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
} from "../seed";
import { postStripeEvent, STRIPE_WEBHOOK_TRANSPORT } from "../stripe";

const DAY = 86_400;

function stripeId(prefix: string): string {
  return `${prefix}_sqa${randomBytes(8).toString("hex")}`;
}

function subscriptionEvent(opts: {
  type: string;
  subscriptionId: string;
  customerId: string;
  status: string;
  created: number;
  periodEnd: number;
  trialEnd?: number | null;
  cancelAtPeriodEnd?: boolean;
}): Record<string, unknown> {
  return {
    id: stripeId("evt"),
    object: "event",
    type: opts.type,
    created: opts.created,
    livemode: false,
    data: {
      object: {
        id: opts.subscriptionId,
        object: "subscription",
        customer: opts.customerId,
        status: opts.status,
        cancel_at_period_end: opts.cancelAtPeriodEnd ?? false,
        cancel_at: null,
        trial_end: opts.trialEnd ?? null,
        items: {
          object: "list",
          data: [
            {
              id: stripeId("si"),
              object: "subscription_item",
              current_period_end: opts.periodEnd,
              price: { id: stripeId("price"), lookup_key: "treq_pro_monthly" },
            },
          ],
        },
      },
    },
  };
}

function checkoutCompletedEvent(opts: {
  customerId: string;
  subscriptionId: string;
  ownerId: string | null;
  clientReferenceId: string;
}): Record<string, unknown> {
  return {
    id: stripeId("evt"),
    object: "event",
    type: "checkout.session.completed",
    created: Math.floor(Date.now() / 1000),
    livemode: false,
    data: {
      object: {
        id: stripeId("cs_test"),
        object: "checkout.session",
        mode: "subscription",
        status: "complete",
        customer: opts.customerId,
        subscription: opts.subscriptionId,
        client_reference_id: opts.clientReferenceId,
        metadata: opts.ownerId
          ? { owner_type: "user", owner_id: opts.ownerId }
          : {},
      },
    },
  };
}

it("records signed Stripe events and derives Pro from them by user id", async () => {
  const admin = getServiceClient();
  const owner = await createTestUser();
  const other = await createTestUser();

  try {
    const { client: ownerClient } = await signInWithEmailPassword(
      owner.email,
      owner.password,
    );
    const { client: otherClient } = await signInWithEmailPassword(
      other.email,
      other.password,
    );

    const customerId = stripeId("cus");
    const subscriptionId = stripeId("sub");
    const now = Math.floor(Date.now() / 1000);
    const trialEnd = now + 14 * DAY;

    // ── Subscription first, mapping second (Stripe does not order events) ──
    const created = await postStripeEvent(
      subscriptionEvent({
        type: "customer.subscription.created",
        subscriptionId,
        customerId,
        status: "trialing",
        created: now,
        periodEnd: trialEnd,
        trialEnd,
      }),
    );
    expect(created).toEqual({
      status: 200,
      body: { received: true, result: "applied" },
    });

    const beforeMapping = await ownerClient.from("subscriptions").select("*");
    expect(beforeMapping.error).toBeNull();
    expect(beforeMapping.data).toEqual([
      { plan: "free", status: "inactive", current_period_end: null },
    ]);

    const checkout = checkoutCompletedEvent({
      customerId,
      subscriptionId,
      ownerId: owner.user.id,
      clientReferenceId: owner.user.id,
    });
    const mapped = await postStripeEvent(checkout);
    expect(mapped).toEqual({
      status: 200,
      body: { received: true, result: "applied" },
    });

    const trialing = await ownerClient.from("subscriptions").select("*");
    expect(trialing.error).toBeNull();
    expect(trialing.data).toEqual([
      {
        plan: "pro",
        status: "trialing",
        current_period_end: new Date(trialEnd * 1000)
          .toISOString()
          .replace("Z", "")
          .replace(".000", ""),
      },
    ]);
    const ownerHasPro = await ownerClient.rpc("has_pro", {
      p_user_id: owner.user.id,
    });
    expect(ownerHasPro).toMatchObject({ data: true, error: null });
    const customerRow = await ownerClient
      .from("billing_customers")
      .select("stripe_customer_id, trial_used_at")
      .single();
    expect(customerRow.error).toBeNull();
    expect(customerRow.data?.stripe_customer_id).toBe(customerId);
    expect(customerRow.data?.trial_used_at).not.toBeNull();

    await recordOutcome("stripe-webhook-01-trial-entitles-owner", {
      expectations: [
        "A signed customer.subscription.created that arrives before checkout.session.completed is stored, but entitles nobody until the customer is mapped.",
        "After checkout.session.completed maps the customer by metadata owner id, the owner's public.subscriptions row reads pro / trialing with the trial end, and has_pro is true.",
        "The owner can read their billing_customers row, and trial_used_at is set.",
      ],
      details: {
        transport: STRIPE_WEBHOOK_TRANSPORT,
        created,
        mapped,
        subscriptions: trialing.data,
        customer: customerRow.data,
      },
    });

    // ── Replays and forgeries change nothing ─────────────────────────────
    const replay = await postStripeEvent(checkout);
    expect(replay).toEqual({
      status: 200,
      body: { received: true, result: "duplicate" },
    });

    const forgedDelete = subscriptionEvent({
      type: "customer.subscription.deleted",
      subscriptionId,
      customerId,
      status: "canceled",
      created: now + 60,
      periodEnd: trialEnd,
    });
    const badSignature = await postStripeEvent(forgedDelete, {
      secret: "whsec_attacker",
    });
    expect(badSignature.status).toBe(400);
    const unsigned = await postStripeEvent(forgedDelete, { signature: null });
    expect(unsigned.status).toBe(400);
    expect(
      (await ownerClient.rpc("has_pro", { p_user_id: owner.user.id })).data,
    ).toBe(true);

    // A Payment Link lets anyone set client_reference_id. Without metadata
    // the session must not move the owner's customer to someone else.
    const spoof = await postStripeEvent(
      checkoutCompletedEvent({
        customerId,
        subscriptionId,
        ownerId: null,
        clientReferenceId: other.user.id,
      }),
    );
    expect(spoof.status).toBe(200);
    expect(spoof.body).toMatchObject({
      received: true,
      ignored: expect.any(String),
    });
    const steal = await postStripeEvent(
      checkoutCompletedEvent({
        customerId,
        subscriptionId,
        ownerId: other.user.id,
        clientReferenceId: other.user.id,
      }),
    );
    expect(steal.body).toEqual({ received: true, result: "owner_conflict" });

    const otherSees = await otherClient
      .from("billing_subscriptions")
      .select("*");
    expect(otherSees.error).toBeNull();
    expect(otherSees.data).toEqual([]);
    const otherProbe = await otherClient.rpc("has_pro", {
      p_user_id: owner.user.id,
    });
    expect(otherProbe).toMatchObject({ data: false, error: null });
    const otherOwn = await otherClient.from("subscriptions").select("plan");
    expect(otherOwn.data).toEqual([{ plan: "free" }]);

    await recordOutcome("stripe-webhook-02-replay-and-spoofing", {
      expectations: [
        "Replaying the same checkout.session.completed event returns result duplicate.",
        "A deletion signed with the wrong secret, or unsigned, is rejected with 400 and the owner keeps Pro.",
        "A session naming another user only in client_reference_id is ignored, one naming them in metadata is an owner_conflict, and the other user cannot see the owner's rows or probe has_pro for them.",
      ],
      details: {
        replay,
        badSignature,
        unsigned,
        spoof,
        steal,
        otherProbe: otherProbe.data,
      },
    });

    // ── Cancel at period end, then deletion ──────────────────────────────
    const periodEnd = now + 44 * DAY;
    const canceling = await postStripeEvent(
      subscriptionEvent({
        type: "customer.subscription.updated",
        subscriptionId,
        customerId,
        status: "active",
        created: now + 120,
        periodEnd,
        trialEnd,
        cancelAtPeriodEnd: true,
      }),
    );
    expect(canceling.body).toEqual({ received: true, result: "applied" });
    const cancelingRow = await ownerClient
      .from("subscriptions")
      .select("plan, status");
    expect(cancelingRow.data).toEqual([{ plan: "pro", status: "canceled" }]);

    const deleted = await postStripeEvent(
      subscriptionEvent({
        type: "customer.subscription.deleted",
        subscriptionId,
        customerId,
        status: "canceled",
        created: now + 180,
        periodEnd,
        trialEnd,
      }),
    );
    expect(deleted.body).toEqual({ received: true, result: "applied" });
    const ended = await ownerClient.from("subscriptions").select("*");
    expect(ended.data).toEqual([
      { plan: "free", status: "inactive", current_period_end: null },
    ]);
    expect(
      (await ownerClient.rpc("has_pro", { p_user_id: owner.user.id })).data,
    ).toBe(false);

    await recordOutcome("stripe-webhook-03-cancel-then-end", {
      expectations: [
        "A subscription set to cancel at the period end still reads plan pro with status canceled, the contract the desktop app already understands.",
        "customer.subscription.deleted ends the entitlement: public.subscriptions reads free / inactive and has_pro is false.",
      ],
      details: {
        canceling,
        cancelingRow: cancelingRow.data,
        deleted,
        ended: ended.data,
      },
    });
  } finally {
    // Billing rows have no foreign key to auth.users, so remove them here.
    await admin
      .from("billing_subscriptions")
      .delete()
      .in("owner_id", [owner.user.id, other.user.id]);
    await admin
      .from("billing_customers")
      .delete()
      .in("owner_id", [owner.user.id, other.user.id]);
    await deleteTestUser(admin, owner.user.id);
    await deleteTestUser(admin, other.user.id);
  }
}, 120_000);
