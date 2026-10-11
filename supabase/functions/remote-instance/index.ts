// Edge function: managed compute instance lifecycle for Remote Development
// (prds/remote-development.md, Phase 2: Sprites provisioning).
//
// POST body: { action, idempotency_key?, region?, size_preset? }
// action:
//   "ensure"       - provision lazily, idempotent (Goal 1 / "Provisioning trigger")
//   "status"       - read current instance + endpoint status
//   "wake"         - request a suspended instance resume
//   "reprovision"  - repair the existing Sprite in place
//   "delete"       - tear down the instance
//   "list_regions" - closed set of region codes
//   "list_sizes"   - closed set of size presets
//
// `ensure`, `wake` and `reprovision` require Pro and answer 402
// `pro_required` otherwise (prds/billing-and-teams.md, "Enforcement").
//
// Auth: user JWT in Authorization header. Every mutating action verifies the
// Supabase principal and that the instance (if referenced) belongs to them —
// per the PRD's "Edge Functions verify both the Supabase principal and
// resource ownership instead of relying only on client-supplied IDs."
//
// Provider credentials (Fly Sprites token) are read from Edge Function
// secrets and never returned to the client. The request logic lives in
// lib.ts.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { userHasPro } from "../_shared/billing/entitlement.ts";
import { correlationIdFromRequest } from "../_shared/remote/correlation.ts";
import {
  SpritesProvider,
  spritesConfigFromEnv,
} from "../_shared/remote/sprites-adapter.ts";
import {
  isSpritesStubEnabled,
  StubSpritesProvider,
} from "../_shared/remote/stub-sprites-adapter.ts";
import { corsHeaders, handleRemoteInstanceAction, json } from "./lib.ts";

Deno.serve(async (req) => {
  if (req.method === "OPTIONS")
    return new Response(null, { headers: corsHeaders, status: 204 });
  if (req.method !== "POST") return json({ error: "Method not allowed" }, 405);

  const authHeader = req.headers.get("authorization") ?? "";
  const userToken = authHeader.replace(/^Bearer\s+/i, "");
  if (!userToken) return json({ error: "Unauthorized" }, 401);

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const supabaseAnonKey = Deno.env.get("SUPABASE_ANON_KEY") ?? "";
  const supabaseServiceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "";

  const supabaseUser = createClient(supabaseUrl, supabaseAnonKey, {
    global: { headers: { Authorization: `Bearer ${userToken}` } },
  });
  const {
    data: { user },
    error: authError,
  } = await supabaseUser.auth.getUser();
  if (authError || !user) return json({ error: "Unauthorized" }, 401);

  // deno-lint-ignore no-explicit-any
  let body: Record<string, any>;
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  const supabase = createClient(supabaseUrl, supabaseServiceKey);
  const stub = isSpritesStubEnabled();
  return await handleRemoteInstanceAction(body, {
    supabase,
    ownerUserId: user.id,
    correlationId: correlationIdFromRequest(req),
    hasPro: () => userHasPro(supabase, user.id),
    stub,
    provider: () =>
      stub
        ? new StubSpritesProvider()
        : new SpritesProvider(spritesConfigFromEnv()),
  });
});
