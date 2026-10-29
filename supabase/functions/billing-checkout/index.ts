// Opens an embedded Stripe Checkout session for Pro, or for Team on behalf of
// an organization the caller owns, and returns its client secret
// (prds/billing-and-teams.md, "Edge functions"). Called by the web dashboard
// with the user's Supabase JWT. The logic lives in lib.ts.
// Env: STRIPE_SECRET_KEY, and WEB_URL for the return URL (default
// https://treq.dev), and TREQ_STRIPE_USAGE_PRICE_LOOKUP_KEY for Pro's
// metered usage price (default treq_sandbox_usage).

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import {
  billingStoreFor,
  organizationBillingFor,
} from "../_shared/billing/store.ts";
import { createStripeClient } from "../_shared/billing/stripe-api.ts";
import { createCheckout } from "./lib.ts";
import { corsHeaders, json } from "../_shared/http/json.ts";

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") {
    return new Response(null, { headers: corsHeaders, status: 204 });
  }
  if (req.method !== "POST") return json({ error: "Method not allowed" }, 405);

  const userToken = (req.headers.get("authorization") ?? "").replace(
    /^Bearer\s+/i,
    "",
  );
  if (!userToken) return json({ error: "Unauthorized" }, 401);

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const supabaseUser = createClient(
    supabaseUrl,
    Deno.env.get("SUPABASE_ANON_KEY") ?? "",
    { global: { headers: { Authorization: `Bearer ${userToken}` } } },
  );
  const {
    data: { user },
    error: authError,
  } = await supabaseUser.auth.getUser();
  if (authError || !user) return json({ error: "Unauthorized" }, 401);

  const stripeKey = Deno.env.get("STRIPE_SECRET_KEY");
  if (!stripeKey) {
    console.error("[billing-checkout] STRIPE_SECRET_KEY is not configured");
    return json({ error: "Billing is not configured" }, 500);
  }

  let body: unknown;
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  const service = createClient(
    supabaseUrl,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );

  try {
    const result = await createCheckout(body, {
      user: { id: user.id, email: user.email },
      store: billingStoreFor(service, user.id),
      organizations: organizationBillingFor(service, user.id),
      stripe: createStripeClient(stripeKey),
      webUrl: Deno.env.get("WEB_URL") ?? "https://treq.dev",
      usagePriceLookupKey:
        Deno.env.get("TREQ_STRIPE_USAGE_PRICE_LOOKUP_KEY") || undefined,
    });
    return json(result.body, result.status);
  } catch (err) {
    console.error(
      "[billing-checkout] failed:",
      err instanceof Error ? err.message : String(err),
    );
    return json({ error: "Checkout failed" }, 500);
  }
});
