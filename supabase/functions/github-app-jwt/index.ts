// Signs a GitHub App JWT (RS256) for Postgres, which cannot sign RS256 and
// caches the result in Vault (072_integration_proxy.sql). Called only from
// Postgres with the x-github-app-jwt-key header.
//
// Env: GITHUB_APP_JWT_KEY (shared with Vault's github_app_jwt_key),
// GITHUB_APP_ID and GITHUB_APP_PRIVATE_KEY_BASE64.

import { appJWT } from "../_shared/github/app.ts";

// appJWT() sets exp to now + 540s; report a little less.
const LIFETIME_SEC = 500;

function constantTimeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

Deno.serve(async (req) => {
  const expected = Deno.env.get("GITHUB_APP_JWT_KEY") ?? "";
  const given = req.headers.get("x-github-app-jwt-key") ?? "";
  if (req.method !== "POST" || !expected || !constantTimeEqual(given, expected)) {
    return new Response(null, { status: 401 });
  }
  try {
    const jwt = await appJWT();
    const expiresAt = new Date(Date.now() + LIFETIME_SEC * 1000).toISOString();
    return Response.json({ jwt, expires_at: expiresAt });
  } catch (err) {
    console.error("[github-app-jwt]", err instanceof Error ? err.message : String(err));
    return new Response(null, { status: 500 });
  }
});
