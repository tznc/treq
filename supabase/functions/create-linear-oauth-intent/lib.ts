// Request logic for create-linear-oauth-intent, free of Deno imports so it
// runs under service-qa. index.ts verifies the user's JWT and supplies the
// service-role store.
//
// Linear OAuth is a Pro feature (prds/billing-and-teams.md, "Enforcement").
// Linear through a personal API key stays free and never reaches this
// function.

import {
  PRO_REQUIRED_MESSAGES,
  proRequired,
} from "../_shared/billing/entitlement.ts";
import type { ServiceClientLike } from "../_shared/billing/store.ts";
import {
  type HandlerResult,
  intentExpiry,
  type IntentRow,
  newIntentState,
  sha256Hex,
} from "../_shared/intent-state.ts";

export interface LinearIntentStore {
  insertIntent(row: IntentRow): Promise<void>;
}

export type LinearIntentDeps = {
  userId: string;
  hasPro: () => Promise<boolean>;
  store: LinearIntentStore;
  /** LINEAR_CLIENT_ID; unset when Linear OAuth is not configured. */
  clientId: string | undefined;
  /** Base of the OAuth redirect URI (WEB_URL). */
  webUrl: string;
  now?: () => number;
};

export async function createLinearOAuthIntent(
  deps: LinearIntentDeps,
): Promise<HandlerResult> {
  if (!(await deps.hasPro())) {
    return proRequired(PRO_REQUIRED_MESSAGES.linearOAuth);
  }
  if (!deps.clientId) {
    return { status: 500, body: { error: "Linear OAuth is not configured" } };
  }

  const state = newIntentState();
  const expiresAt = intentExpiry((deps.now ?? Date.now)());
  try {
    await deps.store.insertIntent({
      user_id: deps.userId,
      state_hash: await sha256Hex(state),
      expires_at: expiresAt,
    });
  } catch (err) {
    console.error(
      "[create-linear-oauth-intent] insert failed:",
      err instanceof Error ? err.message : String(err),
    );
    return { status: 500, body: { error: "Failed to create OAuth intent" } };
  }

  const authorizationUrl = new URL("https://linear.app/oauth/authorize");
  authorizationUrl.searchParams.set("client_id", deps.clientId);
  authorizationUrl.searchParams.set(
    "redirect_uri",
    `${deps.webUrl}/integrations/linear/callback`,
  );
  authorizationUrl.searchParams.set("response_type", "code");
  authorizationUrl.searchParams.set("scope", "read,write");
  authorizationUrl.searchParams.set("state", state);

  return {
    status: 200,
    body: {
      authorization_url: authorizationUrl.toString(),
      expires_at: expiresAt,
    },
  };
}

export function linearIntentStore(client: ServiceClientLike): LinearIntentStore {
  return {
    async insertIntent(row) {
      const { error } = await client.from("linear_oauth_intents").insert(row);
      if (error) throw new Error(error.message);
    },
  };
}
