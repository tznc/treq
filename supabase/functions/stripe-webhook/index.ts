// Stripe webhook ingress (prds/billing-and-teams.md, "Edge functions").
//
// Stripe calls this endpoint directly, so it runs with verify_jwt = false and
// authenticates each request by its Stripe-Signature header. The logic lives
// in lib.ts. This file only binds it to Deno, the environment, and a
// service-role client.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { createStripeClient } from "../_shared/billing/stripe-api.ts";
import { handleStripeWebhook } from "./lib.ts";

const supabase = createClient(
  Deno.env.get("SUPABASE_URL") ?? "",
  Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  { auth: { autoRefreshToken: false, persistSession: false } },
);

const stripeKey = Deno.env.get("STRIPE_SECRET_KEY");
const stripe = stripeKey ? createStripeClient(stripeKey) : undefined;

Deno.serve(async (req) => {
  const result = await handleStripeWebhook(
    {
      method: req.method,
      // The signature covers the exact bytes Stripe sent, so read the raw
      // text and never re-serialize it before verifying.
      body: req.method === "POST" ? await req.text() : "",
      signature: req.headers.get("stripe-signature"),
    },
    {
      secret: Deno.env.get("STRIPE_WEBHOOK_SECRET"),
      stripe,
      rpc: async (fn, args) => {
        const { data, error } = await supabase.rpc(fn, args);
        return { data, error };
      },
    },
  );
  return new Response(result.body, {
    status: result.status,
    headers: { "Content-Type": "application/json" },
  });
});
