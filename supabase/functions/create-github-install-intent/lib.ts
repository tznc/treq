// Request logic for create-github-install-intent, free of Deno imports so it
// runs under service-qa. index.ts verifies the user's JWT and supplies the
// service-role store.
//
// Installing the GitHub App is a Pro feature (prds/billing-and-teams.md,
// "Enforcement"), so a Free user gets 402 before any intent is written.

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

export interface InstallIntentStore {
  insertIntent(row: IntentRow): Promise<void>;
}

export type InstallIntentDeps = {
  userId: string;
  hasPro: () => Promise<boolean>;
  store: InstallIntentStore;
  now?: () => number;
};

export async function createInstallIntent(
  deps: InstallIntentDeps,
): Promise<HandlerResult> {
  if (!(await deps.hasPro())) {
    return proRequired(PRO_REQUIRED_MESSAGES.githubApp);
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
      "[create-github-install-intent] insert failed:",
      err instanceof Error ? err.message : String(err),
    );
    return { status: 500, body: { error: "Failed to create install intent" } };
  }
  return { status: 200, body: { state, expires_at: expiresAt } };
}

export function installIntentStore(
  client: ServiceClientLike,
): InstallIntentStore {
  return {
    async insertIntent(row) {
      const { error } = await client.from("github_install_intents").insert(row);
      if (error) throw new Error(error.message);
    },
  };
}
