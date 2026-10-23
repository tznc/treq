import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { consumeDesktopToken, exchangeDesktopToken, type TokenTable } from "./lib.ts";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
};

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") {
    return new Response("ok", { headers: corsHeaders });
  }

  const supabaseAdmin = createClient(
    Deno.env.get("SUPABASE_URL") ?? "",
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? ""
  );
  const supabaseAnon = createClient(
    Deno.env.get("SUPABASE_URL") ?? "",
    Deno.env.get("SUPABASE_ANON_KEY") ?? ""
  );

  const body = await req.json().catch(() => null);
  const { status, body: payload } = await exchangeDesktopToken(
    {
      consumeToken: (token, now) =>
        consumeDesktopToken(
          supabaseAdmin.from("desktop_auth_tokens") as unknown as TokenTable,
          token,
          now,
        ),
      createSession: async (userId) => {
        const { data: { user } } = await supabaseAdmin.auth.admin.getUserById(userId);
        if (!user?.email) return null;

        // Mint a magic link and verify it server-side to get a session.
        const { data: link, error: linkError } = await supabaseAdmin.auth.admin.generateLink({
          type: "magiclink",
          email: user.email,
        });
        if (linkError || !link) return null;

        const { data: verified, error: verifyError } = await supabaseAnon.auth.verifyOtp({
          token_hash: link.properties.hashed_token,
          type: "magiclink",
        });
        if (verifyError || !verified.session) return null;

        return {
          access_token: verified.session.access_token,
          refresh_token: verified.session.refresh_token,
          expires_in: verified.session.expires_in,
          user: verified.session.user,
        };
      },
      now: () => new Date(),
    },
    body,
  );

  return new Response(JSON.stringify(payload), {
    status,
    headers: { ...corsHeaders, "Content-Type": "application/json" },
  });
});
