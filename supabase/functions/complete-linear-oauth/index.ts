// Completes the Linear OAuth flow: verifies the single-use intent state belongs
// to the authenticated user, exchanges the authorization code for an access token,
// queries Linear for the workspace name, stores the token, and consumes the intent.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";

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

async function sha256Hex(input: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(input),
  );
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

async function getWorkspaceInfo(
  accessToken: string,
): Promise<{ name: string; id: string }> {
  const response = await fetch("https://api.linear.app/graphql", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Authorization": accessToken,
    },
    body: JSON.stringify({
      query: `query { viewer { organization { id name } } }`,
    }),
  });

  if (!response.ok) {
    throw new Error(`Linear API error: ${response.status}`);
  }

  const result = await response.json();
  if (result.errors?.[0]) {
    throw new Error(`Linear GraphQL error: ${result.errors[0].message}`);
  }

  const org = result.data?.viewer?.organization;
  return {
    name: org?.name ?? "Unknown",
    id: org?.id ?? "",
  };
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

  let body: { code?: string; state?: string };
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  const code = body.code;
  const state = body.state;
  if (!code || !state) {
    return json({ error: "Missing code or state" }, 400);
  }

  const supabase = createClient(
    supabaseUrl,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );

  const stateHash = await sha256Hex(state);
  const { data: intent, error: intentError } = await supabase
    .from("linear_oauth_intents")
    .update({ consumed_at: new Date().toISOString() })
    .eq("state_hash", stateHash)
    .eq("user_id", user.id)
    .is("consumed_at", null)
    .gte("expires_at", new Date().toISOString())
    .select("id")
    .maybeSingle();

  if (intentError) {
    console.error("[complete-linear-oauth] intent lookup failed:", intentError.message);
    return json({ error: "Failed to verify OAuth intent" }, 500);
  }
  if (!intent) {
    return json({ error: "OAuth intent is invalid, expired or already used" }, 403);
  }

  const linearClientId = Deno.env.get("LINEAR_CLIENT_ID");
  const linearClientSecret = Deno.env.get("LINEAR_CLIENT_SECRET");
  if (!linearClientId || !linearClientSecret) {
    console.error("[complete-linear-oauth] Linear credentials not configured");
    return json({ error: "Linear OAuth is not configured" }, 500);
  }

  const redirectUri = `${Deno.env.get("WEB_URL") ?? "https://treq.dev"}/integrations/linear/callback`;
  let tokenResponse: {
    access_token: string;
    refresh_token?: string;
    expires_in?: number;
  };
  try {
    const tokenRes = await fetch("https://api.linear.app/oauth/token", {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        client_id: linearClientId,
        client_secret: linearClientSecret,
        code,
        redirect_uri: redirectUri,
        grant_type: "authorization_code",
      }).toString(),
    });

    if (!tokenRes.ok) {
      console.error(
        "[complete-linear-oauth] token exchange failed:",
        tokenRes.status,
      );
      return json({ error: "Failed to exchange authorization code" }, 502);
    }

    tokenResponse = await tokenRes.json();
  } catch (err) {
    console.error(
      "[complete-linear-oauth] token fetch error:",
      err instanceof Error ? err.message : String(err),
    );
    return json({ error: "Failed to communicate with Linear" }, 502);
  }

  const accessToken = tokenResponse.access_token;
  const refreshToken = tokenResponse.refresh_token ?? null;
  const expiresIn = tokenResponse.expires_in;

  if (!accessToken) {
    return json({ error: "No access token in Linear response" }, 502);
  }

  let workspaceInfo = { name: "Unknown", id: "" };
  try {
    workspaceInfo = await getWorkspaceInfo(accessToken);
  } catch (err) {
    console.error(
      "[complete-linear-oauth] workspace query failed:",
      err instanceof Error ? err.message : String(err),
    );
    return json({ error: "Failed to fetch workspace information" }, 502);
  }

  const expiresAt = expiresIn
    ? new Date(Date.now() + expiresIn * 1000).toISOString()
    : null;

  const { error: storeError } = await supabase.from("linear_oauth_tokens")
    .upsert(
      {
        user_id: user.id,
        access_token: accessToken,
        refresh_token: refreshToken,
        expires_at: expiresAt,
        linear_workspace_id: workspaceInfo.id,
        linear_workspace_name: workspaceInfo.name,
        updated_at: new Date().toISOString(),
      },
      { onConflict: "user_id" },
    );

  if (storeError) {
    console.error("[complete-linear-oauth] store failed:", storeError.message);
    return json({ error: "Failed to store Linear token" }, 500);
  }

  console.log(
    JSON.stringify({
      operation: "linear_oauth_linked",
      workspace_name: workspaceInfo.name,
      user_id: user.id,
    }),
  );

  return json({
    ok: true,
    workspace_name: workspaceInfo.name,
  });
});
