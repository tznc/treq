// Request logic for the stripe-webhook Edge Function, kept free of Deno and
// Supabase imports so it runs under the repo's unit tests and service-qa.
// The caller supplies the webhook secret and an RPC function bound to the
// service role.
//
// The webhook only translates Stripe events into calls to security definer
// SQL functions (025_billing_entitlement.sql). Those functions own
// idempotency, event ordering, and the customer-to-owner mapping.

import {
  type BillingPlan,
  PLAN_BY_LOOKUP_KEY,
  SUBSCRIPTION_STATUSES,
  type SubscriptionStatus,
} from "../_shared/billing/plans.ts";
import { verifyStripeSignature } from "../_shared/billing/stripe-signature.ts";

type SubscriptionEventType =
  | "customer.subscription.created"
  | "customer.subscription.updated"
  | "customer.subscription.deleted";

const SUBSCRIPTION_EVENT_TYPES: ReadonlySet<string> = new Set([
  "customer.subscription.created",
  "customer.subscription.updated",
  "customer.subscription.deleted",
]);

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export type SubscriptionEventArgs = {
  p_event_id: string;
  p_event_type: SubscriptionEventType;
  p_event_created: string;
  p_subscription_id: string;
  p_customer_id: string;
  p_plan: BillingPlan;
  p_status: SubscriptionStatus;
  p_current_period_end: string | null;
  p_cancel_at_period_end: boolean;
  p_trial_end: string | null;
};

export type BillingOwnerType = "user" | "organization";

export type CheckoutCompletedArgs = {
  p_event_id: string;
  p_customer_id: string;
  p_owner_type: BillingOwnerType;
  p_owner_id: string;
};

export type EventAction =
  | {
      action: "rpc";
      fn: "billing_record_subscription_event";
      args: SubscriptionEventArgs;
    }
  | {
      action: "rpc";
      fn: "billing_record_checkout_completed";
      args: CheckoutCompletedArgs;
    }
  | { action: "ignore"; reason: string };

type StripeEvent = {
  id: string;
  type: string;
  created: number;
  data: { object: Record<string, unknown> };
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isStripeEvent(value: unknown): value is StripeEvent {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    value.id.length > 0 &&
    typeof value.type === "string" &&
    typeof value.created === "number" &&
    isRecord(value.data) &&
    isRecord(value.data.object)
  );
}

function unixToIso(value: unknown): string | null {
  return typeof value === "number"
    ? new Date(value * 1000).toISOString()
    : null;
}

// Stripe sends either the id or, when expanded, the object.
function idOf(value: unknown): string | null {
  if (typeof value === "string" && value.length > 0) return value;
  if (isRecord(value) && typeof value.id === "string") return value.id;
  return null;
}

function subscriptionItems(
  sub: Record<string, unknown>,
): Record<string, unknown>[] {
  const items = isRecord(sub.items) ? sub.items.data : null;
  return Array.isArray(items) ? items.filter(isRecord) : [];
}

function planOf(items: Record<string, unknown>[]): BillingPlan | null {
  for (const item of items) {
    const price = isRecord(item.price) ? item.price : null;
    const key = price?.lookup_key;
    if (
      typeof key === "string" &&
      Object.prototype.hasOwnProperty.call(PLAN_BY_LOOKUP_KEY, key)
    ) {
      return PLAN_BY_LOOKUP_KEY[key];
    }
  }
  return null;
}

// API versions from 2025-03-31 moved the billing period from the
// subscription to its items. Read whichever the endpoint's version sends.
function periodEndOf(
  sub: Record<string, unknown>,
  items: Record<string, unknown>[],
): number | null {
  if (typeof sub.current_period_end === "number") return sub.current_period_end;
  const ends = items
    .map((item) => item.current_period_end)
    .filter((end): end is number => typeof end === "number");
  return ends.length > 0 ? Math.max(...ends) : null;
}

function subscriptionAction(event: StripeEvent): EventAction {
  const sub = event.data.object;
  const subscriptionId = idOf(sub.id);
  const customerId = idOf(sub.customer);
  if (!subscriptionId || !customerId) {
    return { action: "ignore", reason: "subscription without id or customer" };
  }

  const items = subscriptionItems(sub);
  const plan = planOf(items);
  if (!plan) {
    return { action: "ignore", reason: "no Treq price on subscription" };
  }

  // A deleted subscription has ended, whatever status the object carries.
  const status =
    event.type === "customer.subscription.deleted" ? "canceled" : sub.status;
  if (!SUBSCRIPTION_STATUSES.includes(status as SubscriptionStatus)) {
    return {
      action: "ignore",
      reason: `unknown subscription status ${String(status)}`,
    };
  }

  const periodEnd = periodEndOf(sub, items);
  // The customer portal can end a subscription with cancel_at instead of
  // cancel_at_period_end. Both mean "entitled until the period ends".
  const cancelAt = typeof sub.cancel_at === "number" ? sub.cancel_at : null;
  const canceling =
    sub.cancel_at_period_end === true ||
    (cancelAt !== null && periodEnd !== null && cancelAt <= periodEnd);

  return {
    action: "rpc",
    fn: "billing_record_subscription_event",
    args: {
      p_event_id: event.id,
      p_event_type: event.type as SubscriptionEventType,
      p_event_created: new Date(event.created * 1000).toISOString(),
      p_subscription_id: subscriptionId,
      p_customer_id: customerId,
      p_plan: plan,
      p_status: status as SubscriptionStatus,
      p_current_period_end: unixToIso(periodEnd),
      p_cancel_at_period_end: canceling,
      p_trial_end: unixToIso(sub.trial_end),
    },
  };
}

// The owner comes from metadata, which only billing-checkout can write
// because it holds the secret key. client_reference_id must agree with it:
// a Payment Link accepts client_reference_id from its URL, so on its own it
// would let anyone attach their Stripe customer to another user. The owner
// is a user for Pro and an organization for Team.
function checkoutAction(event: StripeEvent): EventAction {
  const session = event.data.object;
  if (session.mode !== "subscription") {
    return {
      action: "ignore",
      reason: "checkout session is not a subscription",
    };
  }
  const customerId = idOf(session.customer);
  if (!customerId) {
    return { action: "ignore", reason: "checkout session without customer" };
  }

  const metadata = isRecord(session.metadata) ? session.metadata : {};
  const ownerType = metadata.owner_type;
  if (ownerType !== "user" && ownerType !== "organization") {
    return {
      action: "ignore",
      reason: "checkout session without a user or organization owner",
    };
  }
  const ownerId = metadata.owner_id;
  if (typeof ownerId !== "string" || !UUID_PATTERN.test(ownerId)) {
    return {
      action: "ignore",
      reason: "checkout session owner id is not a UUID",
    };
  }
  if (session.client_reference_id !== ownerId) {
    return {
      action: "ignore",
      reason: "checkout session client_reference_id does not match its owner",
    };
  }

  return {
    action: "rpc",
    fn: "billing_record_checkout_completed",
    args: {
      p_event_id: event.id,
      p_customer_id: customerId,
      p_owner_type: ownerType,
      p_owner_id: ownerId.toLowerCase(),
    },
  };
}

/** Decides what a verified Stripe event writes, if anything. */
export function actionForEvent(event: unknown): EventAction {
  if (!isStripeEvent(event)) {
    return { action: "ignore", reason: "not a Stripe event" };
  }
  if (SUBSCRIPTION_EVENT_TYPES.has(event.type)) {
    return subscriptionAction(event);
  }
  if (event.type === "checkout.session.completed") return checkoutAction(event);
  return { action: "ignore", reason: `unhandled event type ${event.type}` };
}

export type RpcResult = { data: unknown; error: { message: string } | null };

export interface WebhookDeps {
  /** STRIPE_WEBHOOK_SECRET. Missing means the endpoint refuses everything. */
  secret: string | undefined;
  rpc: (fn: string, args: Record<string, unknown>) => Promise<RpcResult>;
  nowSeconds?: () => number;
  log?: (entry: Record<string, unknown>) => void;
}

export type WebhookRequest = {
  method: string;
  body: string;
  signature: string | null;
};

export type WebhookResult = { status: number; body: string };

function json(body: unknown, status: number): WebhookResult {
  return { status, body: JSON.stringify(body) };
}

function defaultLog(entry: Record<string, unknown>): void {
  const line = JSON.stringify({ function: "stripe-webhook", ...entry });
  if (entry.level === "error") console.error(line);
  else console.log(line);
}

// SQL results that need a person to look at them. Retrying cannot fix them,
// so the webhook still answers 200.
const ALERT_RESULTS: ReadonlySet<unknown> = new Set([
  "owner_conflict",
  "unknown_owner",
]);

export async function handleStripeWebhook(
  request: WebhookRequest,
  deps: WebhookDeps,
): Promise<WebhookResult> {
  const log = deps.log ?? defaultLog;

  if (request.method !== "POST") {
    return json({ error: "Method not allowed" }, 405);
  }

  // An unsigned webhook is never accepted. A missing secret is a
  // configuration failure, not a reason to skip verification.
  if (!deps.secret) {
    log({ level: "error", message: "STRIPE_WEBHOOK_SECRET is not configured" });
    return json({ error: "Webhook secret not configured" }, 500);
  }

  const check = await verifyStripeSignature(
    request.body,
    request.signature,
    deps.secret,
    { nowSeconds: deps.nowSeconds?.() },
  );
  if (!check.ok) {
    log({
      level: "warn",
      message: "rejected webhook signature",
      reason: check.reason,
    });
    return json({ error: "Invalid signature" }, 400);
  }

  let event: unknown;
  try {
    event = JSON.parse(request.body);
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }
  if (!isStripeEvent(event)) return json({ error: "Not a Stripe event" }, 400);

  const action = actionForEvent(event);
  if (action.action === "ignore") {
    log({
      level: "info",
      message: "ignored event",
      stripe_event_id: event.id,
      type: event.type,
      reason: action.reason,
    });
    return json({ received: true, ignored: action.reason }, 200);
  }

  const { data, error } = await deps.rpc(action.fn, action.args);
  if (error) {
    log({
      level: "error",
      message: "billing write failed",
      stripe_event_id: event.id,
      type: event.type,
      error: error.message,
    });
    // 500 makes Stripe retry. The event id was not recorded, because the
    // write and the id commit together.
    return json({ error: "Write failed" }, 500);
  }

  log({
    level: ALERT_RESULTS.has(data) ? "error" : "info",
    message: "processed event",
    stripe_event_id: event.id,
    type: event.type,
    result: data,
  });
  return json({ received: true, result: data }, 200);
}
