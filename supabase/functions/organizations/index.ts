// Organizations and Team seats (prds/billing-and-teams.md, "Organizations
// and seats"). Called by the web dashboard with the user's Supabase JWT.
// Body: { action, ... } where action is create, invite, accept,
// revoke_invite, remove_member, leave, promote_member, demote_owner,
// delete_organization or attach_installation. The logic lives in lib.ts and
// the rules in 028_organizations_team.sql and 029_organization_owners.sql.
// Env: WEB_URL for invite links (default https://treq.dev), and
// STRIPE_SECRET_KEY to keep Team's billed seats equal to its member count.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { createStripeClient } from "../_shared/billing/stripe-api.ts";
import {
  syncTeamSeats,
  teamSeatsStoreFor,
} from "../_shared/billing/team-seats.ts";
import { handleOrganizationsRequest } from "./lib.ts";

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

  let body: unknown;
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  const stripeKey = Deno.env.get("STRIPE_SECRET_KEY");
  const service = createClient(
    supabaseUrl,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );

  try {
    const result = await handleOrganizationsRequest(body, {
      userId: user.id,
      rpc: async (fn, args) => {
        const { data, error } = await service.rpc(fn, args);
        return { data, error };
      },
      webUrl: Deno.env.get("WEB_URL") ?? "https://treq.dev",
      syncSeats: stripeKey
        ? (organizationId) =>
            syncTeamSeats(organizationId, {
              store: teamSeatsStoreFor(service),
              stripe: createStripeClient(stripeKey),
            })
        : undefined,
    });
    return json(result.body, result.status);
  } catch (err) {
    console.error(
      "[organizations] failed:",
      err instanceof Error ? err.message : String(err),
    );
    return json({ error: "Organization request failed" }, 500);
  }
});
