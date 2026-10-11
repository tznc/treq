/**
 * Pro enforcement on Linear OAuth (`create-linear-oauth-intent`,
 * `linear-proxy`) against the local database.
 *
 * Runs the handlers (lib.ts) in this process with their real service-role
 * stores, so has_pro, linear_oauth_intents and linear_oauth_tokens are the
 * real ones. Only Linear's GraphQL endpoint is a stub `fetch`. Pro comes from
 * the billing write functions (../billing.ts).
 */
import { expect, it, vi } from "vitest";
import {
  PRO_REQUIRED_MESSAGES,
  userHasPro,
} from "../../../supabase/functions/_shared/billing/entitlement";
import { sha256Hex } from "../../../supabase/functions/_shared/intent-state";
import {
  createLinearOAuthIntent,
  linearIntentStore,
} from "../../../supabase/functions/create-linear-oauth-intent/lib";
import {
  linearTokenStore,
  proxyLinearRequest,
} from "../../../supabase/functions/linear-proxy/lib";
import { clearBilling, endPro, grantPro } from "../billing";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

const VIEWER = { data: { viewer: { id: "linear-user" } } };

function stubLinear() {
  return vi.fn(
    async (_url: string | URL | Request, _init?: RequestInit) =>
      new Response(JSON.stringify(VIEWER), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
  );
}

it("requires Pro for Linear OAuth and the Linear proxy", async () => {
  const admin = getServiceClient();
  const free = await createTestUser();
  const pro = await createTestUser();

  try {
    const grant = await grantPro(admin, pro.user.id, { status: "trialing" });
    const intentDeps = (userId: string) => ({
      userId,
      hasPro: () => userHasPro(admin, userId),
      store: linearIntentStore(admin),
      clientId: "sqa-linear-client",
      webUrl: "http://localhost:3001",
    });

    // ── create-linear-oauth-intent ────────────────────────────────────────
    const freeIntent = await createLinearOAuthIntent(intentDeps(free.user.id));
    expect(freeIntent).toEqual({
      status: 402,
      body: { error: PRO_REQUIRED_MESSAGES.linearOAuth, code: "pro_required" },
    });
    const freeRows = await admin
      .from("linear_oauth_intents")
      .select("id")
      .eq("user_id", free.user.id);
    expect(freeRows.data).toEqual([]);

    const proIntent = await createLinearOAuthIntent(intentDeps(pro.user.id));
    expect(proIntent.status).toBe(200);
    const authorizeUrl = new URL(proIntent.body.authorization_url as string);
    const state = authorizeUrl.searchParams.get("state")!;
    const proRows = await admin
      .from("linear_oauth_intents")
      .select("state_hash")
      .eq("user_id", pro.user.id);
    expect(proRows.data).toEqual([{ state_hash: await sha256Hex(state) }]);

    await recordOutcome("billing-enforcement-linear-01-intent", {
      expectations: [
        "create-linear-oauth-intent answers a Free user 402 pro_required and writes no intent row.",
        "A trialing Pro user gets an authorization URL whose state hash is stored in linear_oauth_intents.",
      ],
      details: { freeIntent, proStatus: proIntent.status },
    });

    // ── linear-proxy ──────────────────────────────────────────────────────
    // Both users hold an OAuth token, as if they connected while Pro.
    await admin.from("linear_oauth_tokens").insert([
      {
        user_id: free.user.id,
        access_token: "lin_oauth_free",
        linear_workspace_id: "ws",
        linear_workspace_name: "Workspace",
      },
      {
        user_id: pro.user.id,
        access_token: "lin_oauth_pro",
        linear_workspace_id: "ws",
        linear_workspace_name: "Workspace",
      },
    ]);
    const proxy = (userId: string, fetch: ReturnType<typeof stubLinear>) =>
      proxyLinearRequest(
        { query: "query { viewer { id } }" },
        {
          fetch: fetch as unknown as typeof globalThis.fetch,
          linearClient: null,
          hasPro: () => userHasPro(admin, userId),
          store: linearTokenStore(admin, userId),
        },
      );

    const freeFetch = stubLinear();
    const freeProxy = await proxy(free.user.id, freeFetch);
    expect(freeProxy.status).toBe(402);
    expect(JSON.parse(freeProxy.body)).toEqual({
      error: PRO_REQUIRED_MESSAGES.linearOAuth,
      code: "pro_required",
    });
    expect(freeFetch).not.toHaveBeenCalled();

    const proFetch = stubLinear();
    const proProxy = await proxy(pro.user.id, proFetch);
    expect(proProxy.status).toBe(200);
    expect(JSON.parse(proProxy.body)).toEqual(VIEWER);
    const sentAuth = new Headers(proFetch.mock.calls[0]?.[1]?.headers).get(
      "authorization",
    );
    expect(sentAuth).toBe("Bearer lin_oauth_pro");

    await endPro(admin, grant);
    const lapsedFetch = stubLinear();
    const lapsedProxy = await proxy(pro.user.id, lapsedFetch);
    expect(lapsedProxy.status).toBe(402);
    expect(lapsedFetch).not.toHaveBeenCalled();

    await recordOutcome("billing-enforcement-linear-02-proxy", {
      expectations: [
        "linear-proxy answers a Free user 402 pro_required, with a message pointing at the personal API key, and never calls Linear.",
        "For the Pro user it forwards the query with their stored OAuth token and passes Linear's 200 through.",
        "Once the subscription is deleted the same user gets 402 and Linear is not called.",
      ],
      details: {
        freeProxy: JSON.parse(freeProxy.body),
        proStatus: proProxy.status,
        lapsedStatus: lapsedProxy.status,
      },
    });
  } finally {
    await clearBilling(admin, [free.user.id, pro.user.id]);
    await deleteTestUser(admin, free.user.id);
    await deleteTestUser(admin, pro.user.id);
  }
}, 120_000);
