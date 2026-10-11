// Returns a Stripe customer portal URL for the signed-in user's customer, or
// with `organization_id` for an organization they own
// (prds/billing-and-teams.md, "Edge functions"). Called by the web dashboard
// with the user's Supabase JWT. The logic lives in lib.ts.
// Env: STRIPE_SECRET_KEY, and WEB_URL for the return URL (default
// https://treq.dev).

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import {
  billingStoreFor,
  organizationBillingFor,
} from "../_shared/billing/store.ts";
import { createStripeClient } from "../_shared/billing/stripe-api.ts";
import { createPortal } from "./lib.ts";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers":
    "authorization, x-client-info, apikey, content-type",
};

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...corsHeaders, "Content-Type": "application/json" },
  });
}

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
    console.error("[billing-portal] STRIPE_SECRET_KEY is not configured");
    return json({ error: "Billing is not configured" }, 500);
  }

  // An empty body opens the user's own portal.
  const body = (await req.json().catch(() => null)) as {
    organization_id?: unknown;
  } | null;

  const service = createClient(
    supabaseUrl,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );

  try {
    const result = await createPortal({
      store: billingStoreFor(service, user.id),
      organizations: organizationBillingFor(service, user.id),
      organizationId: body?.organization_id,
      stripe: createStripeClient(stripeKey),
      webUrl: Deno.env.get("WEB_URL") ?? "https://treq.dev",
    });
    return json(result.body, result.status);
  } catch (err) {
    console.error(
      "[billing-portal] failed:",
      err instanceof Error ? err.message : String(err),
    );
    return json({ error: "Billing portal failed" }, 500);
  }
});
