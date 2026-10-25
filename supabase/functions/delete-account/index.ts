// Deletes the signed-in user's Treq account: cancels their subscriptions,
// destroys their Treq Cloud machines, then deletes the auth user. Called by
// the desktop app, the mobile app and the web dashboard with the user's
// Supabase JWT. Answers 409 with a sentence to show when the user must hand
// over an organization first. The logic and its order live in lib.ts.
// Env: STRIPE_SECRET_KEY (needed only for users with a Stripe customer), and
// the Sprites secrets remote-instance uses.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { createStripeClient } from "../_shared/billing/stripe-api.ts";
import {
  SpritesProvider,
  spritesConfigFromEnv,
} from "../_shared/remote/sprites-adapter.ts";
import {
  isSpritesStubEnabled,
  StubSpritesProvider,
} from "../_shared/remote/stub-sprites-adapter.ts";
import { accountDeletionStore, deleteAccount } from "./lib.ts";

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

  const service = createClient(
    supabaseUrl,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );
  const stripeKey = Deno.env.get("STRIPE_SECRET_KEY");

  const result = await deleteAccount({
    userId: user.id,
    store: accountDeletionStore(service, user.id),
    stripe: stripeKey ? createStripeClient(stripeKey) : null,
    // Built on first use, so a user without a cloud machine needs no
    // Sprites secrets.
    provider: {
      deleteInstance: (id) =>
        (isSpritesStubEnabled()
          ? new StubSpritesProvider()
          : new SpritesProvider(spritesConfigFromEnv())
        ).deleteInstance(id),
    },
  });
  return json(result.body, result.status);
});
