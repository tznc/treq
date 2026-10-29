// Deletes the signed-in user's Treq account (TREQ-274). Called by the
// desktop app with the user's Supabase JWT; the account deleted is always
// the one that JWT names. The logic and its ordering live in lib.ts.
// Env: STRIPE_SECRET_KEY (required only when the user has a subscription to
// cancel), and the provider secrets (provider-registry.ts) or
// TREQ_REMOTE_PROVIDER_STUB for cloud workspace teardown.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { createStripeClient } from "../_shared/billing/stripe-api.ts";
import { placementProviders } from "../_shared/remote/byos.ts";
import {
  deleteAccount,
  deleteAccountStoreFor,
  revokeLinearToken,
} from "./lib.ts";
import { corsHeaders, json } from "../_shared/http/json.ts";

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
      provider: placementProviders(service),
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
