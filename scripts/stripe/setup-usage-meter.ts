#!/usr/bin/env -S deno run --allow-net --allow-env
// Sets up Stripe for cloud workspace usage billing
// (docs/design/sandbox-provider-adapter.md, section 5 and D7). TEST mode only.
//
// Usage:
//   STRIPE_SECRET_KEY=sk_test_... deno run --allow-net --allow-env \
//     scripts/stripe/setup-usage-meter.ts [--backfill]
//
// Idempotent. Each run:
//   1. Creates the Billing Meter `treq_sandbox_usage` (sum of payload.value,
//      customer from payload.stripe_customer_id) unless an active one exists.
//   2. Creates a monthly metered Price on the Pro product (found through the
//      `treq_pro_monthly` price) with lookup key `treq_sandbox_usage`, unless
//      an active price has that key. 1 unit = 1 micro-USD, so
//      unit_amount_decimal is 0.0001 (cents).
//   3. With --backfill: adds that price to every live Pro subscription that
//      lacks it. New checkouts get it from billing-checkout. Re-run with
//      --backfill after creating the price for the first time.
//
// Optional env: TREQ_STRIPE_USAGE_METER_EVENT, TREQ_STRIPE_USAGE_PRICE_LOOKUP_KEY
// (both default to treq_sandbox_usage).

import { createStripeClient } from "../../supabase/functions/_shared/billing/stripe-api.ts";

const key = Deno.env.get("STRIPE_SECRET_KEY") ?? "";
if (!key.startsWith("sk_test_") && !key.startsWith("rk_test_")) {
  console.error(
    key.startsWith("sk_live_") || key.startsWith("rk_live_")
      ? "Refusing to run with a live Stripe key. Use a test-mode key."
      : "STRIPE_SECRET_KEY must be a Stripe test-mode key (sk_test_...).",
  );
  Deno.exit(1);
}

const EVENT_NAME = Deno.env.get("TREQ_STRIPE_USAGE_METER_EVENT") || "treq_sandbox_usage";
const USAGE_LOOKUP_KEY = Deno.env.get("TREQ_STRIPE_USAGE_PRICE_LOOKUP_KEY") || "treq_sandbox_usage";
const PRO_LOOKUP_KEY = "treq_pro_monthly";
const backfill = Deno.args.includes("--backfill");
const stripe = createStripeClient(key);

type List<T> = { data: T[]; has_more: boolean };
type Meter = { id: string; event_name: string; status: string };
type Price = { id: string; product: string; lookup_key: string | null };
type Subscription = { id: string; status: string; items: { data: { price: { id: string } }[] } };

// 1. Meter
const meters = await stripe.request<List<Meter>>("GET", "/v1/billing/meters", {
  status: "active",
  limit: 100,
});
let meter = meters.data.find((m) => m.event_name === EVENT_NAME);
if (meter) {
  console.log(`meter: exists ${meter.id} (${EVENT_NAME})`);
} else {
  meter = await stripe.request<Meter>("POST", "/v1/billing/meters", {
    display_name: "Cloud workspace usage (micro-USD)",
    event_name: EVENT_NAME,
    "default_aggregation[formula]": "sum",
    "customer_mapping[type]": "by_id",
    "customer_mapping[event_payload_key]": "stripe_customer_id",
    "value_settings[event_payload_key]": "value",
  }, { idempotencyKey: `treq-setup-meter-${EVENT_NAME}` });
  console.log(`meter: created ${meter.id} (${EVENT_NAME})`);
}

// 2. Metered price on the Pro product
const prices = await stripe.request<List<Price>>("GET", "/v1/prices", {
  "lookup_keys[0]": PRO_LOOKUP_KEY,
  "lookup_keys[1]": USAGE_LOOKUP_KEY,
  active: true,
  limit: 2,
});
const proPrice = prices.data.find((p) => p.lookup_key === PRO_LOOKUP_KEY);
if (!proPrice) {
  console.error(`No active price with lookup key ${PRO_LOOKUP_KEY}; create Pro first.`);
  Deno.exit(1);
}
let usagePrice = prices.data.find((p) => p.lookup_key === USAGE_LOOKUP_KEY);
if (usagePrice) {
  console.log(`price: exists ${usagePrice.id} (${USAGE_LOOKUP_KEY})`);
} else {
  usagePrice = await stripe.request<Price>("POST", "/v1/prices", {
    currency: "usd",
    product: proPrice.product,
    nickname: "Cloud workspace usage",
    unit_amount_decimal: "0.0001", // cents per micro-USD
    "recurring[interval]": "month",
    "recurring[usage_type]": "metered",
    "recurring[meter]": meter.id,
    lookup_key: USAGE_LOOKUP_KEY,
  }, { idempotencyKey: `treq-setup-usage-price-${USAGE_LOOKUP_KEY}-${meter.id}` });
  console.log(`price: created ${usagePrice.id} on ${proPrice.product} (${USAGE_LOOKUP_KEY})`);
}

// 3. Backfill
if (!backfill) {
  console.log("subscriptions: skipped (pass --backfill to add the price to existing Pro subscriptions)");
  Deno.exit(0);
}
const LIVE = new Set(["trialing", "active", "past_due", "unpaid", "incomplete", "paused"]);
let added = 0;
let present = 0;
let startingAfter: string | undefined;
do {
  const params: Record<string, string | number> = { price: proPrice.id, status: "all", limit: 100 };
  if (startingAfter) params.starting_after = startingAfter;
  const page = await stripe.request<List<Subscription>>("GET", "/v1/subscriptions", params);
  for (const sub of page.data) {
    if (!LIVE.has(sub.status)) continue;
    if (sub.items.data.some((i) => i.price.id === usagePrice.id)) {
      present++;
      continue;
    }
    await stripe.request("POST", "/v1/subscription_items", {
      subscription: sub.id,
      price: usagePrice.id,
    }, { idempotencyKey: `treq-usage-item-${sub.id}-${usagePrice.id}` });
    added++;
    console.log(`subscription: added usage price to ${sub.id}`);
  }
  startingAfter = page.has_more ? page.data.at(-1)?.id : undefined;
} while (startingAfter);
console.log(`subscriptions: ${added} updated, ${present} already had the usage price`);
