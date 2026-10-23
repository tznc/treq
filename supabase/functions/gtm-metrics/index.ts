// Aggregate service metrics for the GTM digest in biz-tools
// (biz-tools docs/gtm/treq.md, "Measurement").
//
//   GET /gtm-metrics?from=YYYY-MM-DD&to=YYYY-MM-DD
//   x-gtm-metrics-secret: <GTM_METRICS_SECRET>
//
// Returns counts only, computed by public.gtm_metrics()
// (024_gtm_metrics.sql), so no personal data leaves Supabase. There is no
// Supabase JWT on these requests (verify_jwt = false in config.toml); the
// shared secret is the credential.
//
// One PostgREST call as the service role, so this uses fetch rather than
// supabase-js.

import { fetchWithTimeout } from "../_shared/http/fetch-with-timeout.ts";

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", "Cache-Control": "no-store" },
  });
}

// Compares SHA-256 digests with no early exit, so the time taken does not
// depend on how much of the secret matched or on its length.
async function secretMatches(presented: string, expected: string): Promise<boolean> {
  const encoder = new TextEncoder();
  const [a, b] = await Promise.all(
    [presented, expected].map(async (s) =>
      new Uint8Array(await crypto.subtle.digest("SHA-256", encoder.encode(s)))
    ),
  );
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
  return diff === 0;
}

// A real calendar date: 2026-02-30 matches the pattern but is rejected.
function isDate(value: string | null): value is string {
  if (value === null || !DATE_RE.test(value)) return false;
  const parsed = new Date(`${value}T00:00:00Z`);
  return !Number.isNaN(parsed.getTime()) && parsed.toISOString().startsWith(value);
}

Deno.serve(async (req) => {
  if (req.method !== "GET") return json({ error: "Method not allowed" }, 405);

  const secret = Deno.env.get("GTM_METRICS_SECRET") ?? "";
  if (!secret) return json({ error: "Metrics are not configured" }, 503);
  const presented = req.headers.get("x-gtm-metrics-secret") ?? "";
  if (!(await secretMatches(presented, secret))) {
    return json({ error: "Unauthorized" }, 401);
  }

  const params = new URL(req.url).searchParams;
  const from = params.get("from");
  const to = params.get("to");
  if (!isDate(from) || !isDate(to) || from > to) {
    return json({ error: "from and to must be YYYY-MM-DD dates with from <= to" }, 400);
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const serviceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "";
  try {
    const res = await fetchWithTimeout(`${supabaseUrl}/rest/v1/rpc/gtm_metrics`, {
      method: "POST",
      headers: {
        apikey: serviceKey,
        Authorization: `Bearer ${serviceKey}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ p_from: from, p_to: to }),
    });
    if (!res.ok) {
      console.error(`gtm-metrics: gtm_metrics RPC failed with HTTP ${res.status}: ${await res.text()}`);
      return json({ error: "Failed to compute metrics" }, 500);
    }
    return json(await res.json());
  } catch (err) {
    console.error(`gtm-metrics: gtm_metrics RPC failed: ${(err as Error).message}`);
    return json({ error: "Failed to compute metrics" }, 500);
  }
});
