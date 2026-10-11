/**
 * Pro entitlement for service-qa users, written through the billing
 * functions the stripe-webhook handler calls
 * (`billing_record_checkout_completed` and
 * `billing_record_subscription_event` in 025_billing_entitlement.sql). has_pro
 * and every server-side gate then see a real subscription row, without a
 * Stripe account or a running Edge runtime.
 */
import type { SupabaseClient } from "@supabase/supabase-js";
import { randomBytes } from "node:crypto";

export type ProGrant = {
  userId: string;
  customerId: string;
  subscriptionId: string;
};

async function rpcResult(
  admin: SupabaseClient,
  fn: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  const { data, error } = await admin.rpc(fn, args);
  if (error) throw new Error(`${fn} failed: ${error.message}`);
  return data;
}

// Event ids this process recorded, so clearBilling can remove them and
// pgTAP's row counts on billing_events stay exact after a service-qa run.
const recordedEvents = new Set<string>();

function eventId(): string {
  const id = `evt_sqa${randomBytes(8).toString("hex")}`;
  recordedEvents.add(id);
  return id;
}

/**
 * Gives the user an active (or trialing) Pro subscription. Pass the
 * customer of an earlier grant to subscribe again: an owner keeps its first
 * Stripe customer.
 */
export async function grantPro(
  admin: SupabaseClient,
  userId: string,
  opts?: { status?: "active" | "trialing"; customerId?: string },
): Promise<ProGrant> {
  const suffix = randomBytes(8).toString("hex");
  const grant: ProGrant = {
    userId,
    customerId: opts?.customerId ?? `cus_sqa${suffix}`,
    subscriptionId: `sub_sqa${suffix}`,
  };
  const mapped = await rpcResult(admin, "billing_record_checkout_completed", {
    p_event_id: eventId(),
    p_customer_id: grant.customerId,
    p_owner_type: "user",
    p_owner_id: userId,
  });
  if (mapped !== "applied") throw new Error(`checkout mapping: ${mapped}`);
  const created = await rpcResult(admin, "billing_record_subscription_event", {
    p_event_id: eventId(),
    p_event_type: "customer.subscription.created",
    p_event_created: new Date().toISOString(),
    p_subscription_id: grant.subscriptionId,
    p_customer_id: grant.customerId,
    p_plan: "pro",
    p_status: opts?.status ?? "active",
    p_current_period_end: new Date(Date.now() + 30 * 86_400_000).toISOString(),
    p_cancel_at_period_end: false,
    p_trial_end: null,
  });
  if (created !== "applied") throw new Error(`subscription created: ${created}`);
  return grant;
}

/**
 * Gives an organization an active Team subscription, so every member has
 * Pro. The grant's `userId` holds the organization id.
 */
export async function grantTeam(
  admin: SupabaseClient,
  organizationId: string,
): Promise<ProGrant> {
  const suffix = randomBytes(8).toString("hex");
  const grant: ProGrant = {
    userId: organizationId,
    customerId: `cus_sqa${suffix}`,
    subscriptionId: `sub_sqa${suffix}`,
  };
  const mapped = await rpcResult(admin, "billing_record_checkout_completed", {
    p_event_id: eventId(),
    p_customer_id: grant.customerId,
    p_owner_type: "organization",
    p_owner_id: organizationId,
  });
  if (mapped !== "applied") throw new Error(`checkout mapping: ${mapped}`);
  const created = await rpcResult(admin, "billing_record_subscription_event", {
    p_event_id: eventId(),
    p_event_type: "customer.subscription.created",
    p_event_created: new Date().toISOString(),
    p_subscription_id: grant.subscriptionId,
    p_customer_id: grant.customerId,
    p_plan: "team",
    p_status: "active",
    p_current_period_end: new Date(Date.now() + 30 * 86_400_000).toISOString(),
    p_cancel_at_period_end: false,
    p_trial_end: null,
  });
  if (created !== "applied") throw new Error(`subscription created: ${created}`);
  return grant;
}

/** Ends the subscription, as `customer.subscription.deleted` does. */
export async function endPro(
  admin: SupabaseClient,
  grant: ProGrant,
): Promise<void> {
  const ended = await rpcResult(admin, "billing_record_subscription_event", {
    p_event_id: eventId(),
    p_event_type: "customer.subscription.deleted",
    // Later than the event that created it, so it is not stale.
    p_event_created: new Date(Date.now() + 1_000).toISOString(),
    p_subscription_id: grant.subscriptionId,
    p_customer_id: grant.customerId,
    p_plan: "pro",
    p_status: "canceled",
    p_current_period_end: new Date().toISOString(),
    p_cancel_at_period_end: false,
    p_trial_end: null,
  });
  if (ended !== "applied") throw new Error(`subscription deleted: ${ended}`);
}

/**
 * Removes the users' billing rows and the events this process recorded.
 * Deleting a user does not cascade to them.
 */
export async function clearBilling(
  admin: SupabaseClient,
  userIds: string[],
): Promise<void> {
  await admin.from("billing_subscriptions").delete().in("owner_id", userIds);
  await admin.from("billing_customers").delete().in("owner_id", userIds);
  if (recordedEvents.size > 0) {
    await admin
      .from("billing_events")
      .delete()
      .in("stripe_event_id", [...recordedEvents]);
    recordedEvents.clear();
  }
}
