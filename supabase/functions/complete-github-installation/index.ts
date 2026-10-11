// Completes the GitHub App installation flow: verifies the single-use intent
// state belongs to the authenticated user, exchanges GitHub's OAuth `code`
// and checks the installation is in that GitHub user's installations,
// confirms the installation exists on GitHub via app-level auth, links it,
// and consumes the intent. Requires Pro. The logic lives in lib.ts.
//
// A browser-supplied installation_id alone never determines ownership.
//
// Env: GITHUB_APP_CLIENT_ID and GITHUB_APP_CLIENT_SECRET (the App's OAuth
// client), GITHUB_APP_ID and GITHUB_APP_PRIVATE_KEY_BASE64 (app-level auth).
// The App must have "Request user authorization (OAuth) during
// installation" turned on, or every link fails with
// github_authorization_required.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { userHasPro } from "../_shared/billing/entitlement.ts";
import { githubInstallationAccess } from "./github.ts";
import { completeInstallation, installationLinkStore } from "./lib.ts";

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

  let body: unknown;
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  const supabase = createClient(
    supabaseUrl,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );

  try {
    const result = await completeInstallation(body, {
      userId: user.id,
      hasPro: () => userHasPro(supabase, user.id),
      store: installationLinkStore(supabase),
      github: githubInstallationAccess({
        clientId: Deno.env.get("GITHUB_APP_CLIENT_ID") ?? "",
        clientSecret: Deno.env.get("GITHUB_APP_CLIENT_SECRET") ?? "",
      }),
    });
    return json(result.body, result.status);
  } catch (err) {
    console.error(
      "[complete-github-installation] failed:",
      err instanceof Error ? err.message : String(err),
    );
    return json({ error: "Failed to complete installation" }, 500);
  }
});
