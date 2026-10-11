// Checkout Session parameters for Pro and Team (prds/billing-and-teams.md,
// "Edge functions"). Kept pure so the trial and ownership rules are unit
// tested without Stripe.

import type { BillingPlan } from "./plans.ts";
import type { StripeParams } from "./stripe-api.ts";

export const PRO_TRIAL_DAYS = 14;

/** Pro belongs to a user. Team belongs to an organization. */
export type BillingOwnerType = "user" | "organization";

export const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export type CheckoutBody =
  | { ok: true; plan: Extract<BillingPlan, "pro"> }
  | { ok: true; plan: Extract<BillingPlan, "team">; organizationId: string }
  | { ok: false; status: 400; error: string };

export function parseCheckoutBody(body: unknown): CheckoutBody {
  const fields =
    typeof body === "object" && body !== null
      ? (body as { plan?: unknown; organization_id?: unknown })
      : {};
  if (fields.plan === "pro") return { ok: true, plan: "pro" };
  if (fields.plan === "team") {
    const organizationId = fields.organization_id;
    if (
      typeof organizationId === "string" &&
      UUID_PATTERN.test(organizationId)
    ) {
      return {
        ok: true,
        plan: "team",
        organizationId: organizationId.toLowerCase(),
      };
    }
    return {
      ok: false,
      status: 400,
      error:
        'Team checkout needs { "plan": "team", "organization_id": "<uuid>" }',
    };
  }
  return {
    ok: false,
    status: 400,
    error:
      'Body must be { "plan": "pro" } or { "plan": "team", "organization_id": "<uuid>" }',
  };
}

/** Where each owner manages billing on the dashboard. */
export function dashboardBillingUrl(
  webUrl: string,
  ownerType: BillingOwnerType,
): string {
  const tab = ownerType === "organization" ? "team" : "subscription";
  return `${webUrl.replace(/\/+$/, "")}/dashboard?tab=${tab}`;
}

export function checkoutReturnUrl(
  webUrl: string,
  ownerType: BillingOwnerType = "user",
): string {
  // Stripe replaces {CHECKOUT_SESSION_ID} with the session id on return.
  return `${dashboardBillingUrl(webUrl, ownerType)}&session_id={CHECKOUT_SESSION_ID}`;
}

/**
 * The owner travels in metadata and client_reference_id. The webhook trusts
 * metadata because only this server, holding the secret key, can set it.
 * The card is always collected, so a trial converts without a second step.
 * Only a user's Pro has a trial. Team never does (open decision B01).
 */
export function buildCheckoutSessionParams(input: {
  customerId: string;
  priceId: string;
  ownerId: string;
  ownerType?: BillingOwnerType;
  trialEligible: boolean;
  webUrl: string;
  /** Team's member count. Pro is always 1. */
  quantity?: number;
}): StripeParams {
  const ownerType = input.ownerType ?? "user";
  const params: StripeParams = {
    ui_mode: "embedded",
    mode: "subscription",
    customer: input.customerId,
    "line_items[0][price]": input.priceId,
    "line_items[0][quantity]": input.quantity ?? 1,
    payment_method_collection: "always",
    client_reference_id: input.ownerId,
    "metadata[owner_type]": ownerType,
    "metadata[owner_id]": input.ownerId,
    "subscription_data[metadata][owner_type]": ownerType,
    "subscription_data[metadata][owner_id]": input.ownerId,
    return_url: checkoutReturnUrl(input.webUrl, ownerType),
  };
  if (input.trialEligible && ownerType === "user") {
    params["subscription_data[trial_period_days]"] = PRO_TRIAL_DAYS;
  }
  return params;
}
