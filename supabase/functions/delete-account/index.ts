// Deletes the signed-in user's Treq account (TREQ-274). Called by the
// desktop app with the user's Supabase JWT; the account deleted is always
// the one that JWT names. The logic and its ordering live in lib.ts.
// Env: STRIPE_SECRET_KEY (required only when the user has a subscription to
// cancel), TREQ_SPRITES_API_URL / TREQ_SPRITES_API_TOKEN or
// TREQ_REMOTE_SPRITES_STUB for cloud workspace teardown.

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
import {
  deleteAccount,
  deleteAccountStoreFor,
  revokeLinearToken,
} from "./lib.ts";

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

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const anonKey = Deno.env.get("SUPABASE_ANON_KEY") ?? "";
  const service = createClient(
    supabaseUrl,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
    { auth: { autoRefreshToken: false, persistSession: false } },
  );
  const stripeKey = Deno.env.get("STRIPE_SECRET_KEY");

  try {
    const result = await deleteAccount({
      method: req.method,
      authorization: req.headers.get("authorization"),
      verifyUser: async (token) => {
        const userClient = createClient(supabaseUrl, anonKey, {
          global: { headers: { Authorization: `Bearer ${token}` } },
        });
        const { data, error } = await userClient.auth.getUser();
        return error || !data.user ? null : { id: data.user.id };
      },
      storeFor: (userId) => deleteAccountStoreFor(service, userId),
      stripe: stripeKey ? createStripeClient(stripeKey) : null,
      provider: () =>
        isSpritesStubEnabled()
          ? new StubSpritesProvider()
          : new SpritesProvider(spritesConfigFromEnv()),
      revokeLinearToken: (token) => revokeLinearToken(token),
    });
    return json(result.body, result.status);
  } catch (err) {
    console.error(
      "[delete-account] failed:",
      err instanceof Error ? err.message : String(err),
    );
    return json(
      {
        error:
          "Account deletion failed. Your account may not be deleted. Try again.",
      },
      500,
    );
  }
});
