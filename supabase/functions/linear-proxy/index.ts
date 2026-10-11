// Proxies authenticated Linear API requests to the Linear GraphQL endpoint.
// The client sends a GraphQL query/mutation, and this function forwards it
// using the stored access token from linear_oauth_tokens. An access token
// close to expiry is refreshed first and the new grant is stored. Requires
// Pro: a Free user gets 402 pro_required and uses a personal API key
// instead. The request logic lives in lib.ts so it can be unit tested.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { userHasPro } from "../_shared/billing/entitlement.ts";
import { linearTokenStore, proxyLinearRequest } from "./lib.ts";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers":
    "authorization, x-client-info, apikey, content-type",
};

function respond(body: string, status: number): Response {
  return new Response(body, {
    status,
    headers: { ...corsHeaders, "Content-Type": "application/json" },
  });
}

function json(body: unknown, status = 200): Response {
  return respond(JSON.stringify(body), status);
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") {
    return new Response(null, { headers: corsHeaders, status: 204 });
  }
  if (req.method !== "POST") {
    return json({ error: "Method not allowed" }, 405);
  }

  const authHeader = req.headers.get("authorization") ?? "";
  const userToken = authHeader.replace(/^Bearer\s+/i, "");
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

  let body: { query?: string; variables?: unknown };
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  if (!body.query) {
    return json({ error: "Missing query" }, 400);
  }

  const supabase = createClient(
    supabaseUrl,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );
  const clientId = Deno.env.get("LINEAR_CLIENT_ID");
  const clientSecret = Deno.env.get("LINEAR_CLIENT_SECRET");

  const result = await proxyLinearRequest(
    { query: body.query, variables: body.variables },
    {
      fetch,
      linearClient: clientId && clientSecret
        ? { id: clientId, secret: clientSecret }
        : null,
      hasPro: () => userHasPro(supabase, user.id),
      store: linearTokenStore(supabase, user.id),
    },
  );
  return respond(result.body, result.status);
});
