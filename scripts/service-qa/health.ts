/**
 * Fail fast when the local Supabase CLI stack is not reachable.
 * Also warms the Edge runtime so the first node_modules isolate boot does
 * not eat a whole spec timeout.
 */
import {
  getAnonKey,
  getFunctionsBaseUrl,
  LOCAL_ANON_KEY,
  LOCAL_SUPABASE_URL,
  resolveLocalKeys,
} from "./clients";

const HEALTH_TIMEOUT_MS = 5_000;
const EDGE_WARM_TIMEOUT_MS = 120_000;

export async function assertLocalSupabaseUp(): Promise<void> {
  const { url, anonKey } = (() => {
    try {
      return resolveLocalKeys();
    } catch {
      return { url: LOCAL_SUPABASE_URL, anonKey: LOCAL_ANON_KEY };
    }
  })();

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), HEALTH_TIMEOUT_MS);

  let response: Response;
  try {
    response = await fetch(`${url}/rest/v1/`, {
      method: "GET",
      headers: {
        apikey: anonKey,
        Authorization: `Bearer ${anonKey}`,
      },
      signal: controller.signal,
    });
  } catch (err) {
    clearTimeout(timer);
    throw new Error(
      `Local Supabase is not reachable at ${url}. ` +
        `Start it with \`npm run service-qa:up\` (or \`make start && make db.reset\`), ` +
        `then re-run service-qa. See .claude/skills/service-qa/SKILL.md.`,
      { cause: err },
    );
  } finally {
    clearTimeout(timer);
  }

  // PostgREST returns 200 on /rest/v1/ with an OpenAPI doc when healthy.
  if (!response.ok && response.status !== 404) {
    throw new Error(
      `Local Supabase at ${url} responded HTTP ${response.status}. ` +
        `Run \`npm run service-qa:up\` and check \`supabase status\`.`,
    );
  }

  // SERVICE_QA_NO_EDGE=1 is for a stack whose Edge runtime cannot boot
  // functions (it cannot fetch `npm:` packages). Specs that need an Edge
  // Function then fail on their own, and the Stripe webhook spec runs its
  // handler in-process (see stripe.ts).
  if (process.env.SERVICE_QA_NO_EDGE === "1") return;
  await warmEdgeFunctions();
}

/** Hit exchange-desktop-token once so Deno loads node_modules before specs. */
async function warmEdgeFunctions(): Promise<void> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), EDGE_WARM_TIMEOUT_MS);
  try {
    const response = await fetch(
      `${getFunctionsBaseUrl()}/exchange-desktop-token`,
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${getAnonKey()}`,
          apikey: getAnonKey(),
        },
        body: JSON.stringify({}),
        signal: controller.signal,
      },
    );
    // 400 "Token is required" means the worker booted and ran.
    if (response.status !== 400 && response.status !== 200) {
      const body = await response.text().catch(() => "");
      throw new Error(
        `Edge warm-up for exchange-desktop-token returned HTTP ${response.status}: ${body}. ` +
          `Ensure \`npm install --prefix supabase/functions\` has run (see service-qa:up).`,
      );
    }
  } catch (err) {
    if (err instanceof Error && err.message.startsWith("Edge warm-up")) {
      throw err;
    }
    throw new Error(
      "Edge Functions are not reachable. " +
        "Run `npm run service-qa:up` and confirm edge_runtime is up.",
      { cause: err },
    );
  } finally {
    clearTimeout(timer);
  }
}
