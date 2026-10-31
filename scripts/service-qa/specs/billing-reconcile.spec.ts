/**
 * Reconcile job against the local stack: a Pro row left 'active' by a lost
 * customer.subscription.deleted webhook is corrected from a Stripe read.
 * Stripe itself is the only stub (the outbound adapter); every write goes
 * through billing_stale_subscriptions and billing_record_subscription_snapshot.
 */
import { expect, it } from "vitest";
import {
  reconcileStaleSubscriptions,
  reconcileStoreFor,
} from "../../../supabase/functions/_shared/billing/reconcile";
import type { StripeClient } from "../../../supabase/functions/_shared/billing/stripe-api";
import { grantPro, clearBilling } from "../billing";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

it("ends Pro that a lost deletion webhook left active", async () => {
  const admin = getServiceClient();
  const { user } = await createTestUser();
  try {
    const grant = await grantPro(admin, user.id);
    // The period ended three days ago and no webhook said so.
    await admin
      .from("billing_subscriptions")
      .update({
        current_period_end: new Date(Date.now() - 3 * 86_400_000).toISOString(),
        stripe_event_created: new Date(Date.now() - 60_000).toISOString(),
      })
      .eq("stripe_subscription_id", grant.subscriptionId);

    const stripe = {
      request: async (_method: string, path: string) => {
        expect(path).toBe(`/v1/subscriptions/${grant.subscriptionId}`);
        return {
          id: grant.subscriptionId,
          customer: grant.customerId,
          status: "canceled",
          cancel_at_period_end: false,
          cancel_at: null,
          trial_end: null,
          items: {
            data: [
              {
                current_period_end: Math.floor(Date.now() / 1000) - 3 * 86_400,
                price: { lookup_key: "treq_pro_monthly" },
              },
            ],
          },
        };
      },
    } as StripeClient;

    const before = (await admin.rpc("has_pro", { p_user_id: user.id })).data;
    const result = await reconcileStaleSubscriptions({
      store: reconcileStoreFor(admin),
      stripe,
    });
    const after = (await admin.rpc("has_pro", { p_user_id: user.id })).data;
    expect(before).toBe(true);
    expect(result.applied).toBeGreaterThanOrEqual(1);
    expect(after).toBe(false);

    await recordOutcome("billing-reconcile-01-lost-deletion", {
      expectations: [
        "billing_stale_subscriptions lists an active Pro row whose period ended 3 days ago.",
        "Writing Stripe's 'canceled' copy through billing_record_subscription_snapshot ends Pro.",
      ],
      details: { before, after, result },
    });
  } finally {
    await clearBilling(admin, [user.id]);
    await deleteTestUser(admin, user.id);
  }
}, 60000);
