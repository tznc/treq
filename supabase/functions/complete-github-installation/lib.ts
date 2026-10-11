// Request logic for complete-github-installation, free of Deno imports so it
// runs under service-qa. index.ts verifies the user's JWT and supplies the
// service-role store and the GitHub calls (github.ts).
//
// GitHub warns that the installation_id on its setup redirect can be forged.
// So the App asks for user authorization during installation, and GitHub
// adds a `code` to the redirect. The handler exchanges it for a user access
// token and links the installation only when it is in that GitHub user's
// GET /user/installations. Without a code it refuses, so an App configured
// without that setting fails closed. The user token is never stored.
//
// Linking requires Pro (prds/billing-and-teams.md, "Enforcement"). The check
// runs before the intent is consumed, so a user who upgrades can retry with
// the same unexpired intent. An installation already linked stays linked
// when Pro ends; only new links are refused.
//
// An installation that belongs to a Treq organization can only be relinked
// by an owner of that organization (github_link_installation in
// 028_organizations_team.sql). Any other installation keeps the old rule:
// the last user to link it owns it, and an owner can then attach it to an
// organization from the dashboard's Team tab.

import {
  PRO_REQUIRED_MESSAGES,
  proRequired,
} from "../_shared/billing/entitlement.ts";
import type { ServiceClientLike } from "../_shared/billing/store.ts";
import { type HandlerResult, sha256Hex } from "../_shared/intent-state.ts";

export type GitHubInstallation = {
  id: number;
  account: { login: string; type: string; avatar_url: string | null };
  app_id: number;
};

export type LinkResult = {
  result: "linked" | "organization_owner_required";
  organization_id: string | null;
};

export const ORGANIZATION_OWNER_REQUIRED =
  "This GitHub App installation belongs to a Treq organization. Only an owner of that organization can relink it.";

export interface InstallationLinkStore {
  /**
   * Consumes the caller's unexpired, unconsumed intent with this state hash.
   * Returns null when there is none.
   */
  consumeIntent(
    userId: string,
    stateHash: string,
    nowIso: string,
  ): Promise<{ id: string } | null>;
  linkInstallation(
    installation: GitHubInstallation,
    userId: string,
  ): Promise<LinkResult>;
}

/** Every GitHub call the handler makes. service-qa stubs this. */
export interface GitHubInstallationAccess {
  /**
   * Exchanges the OAuth code from the install redirect for a user access
   * token. Throws when GitHub refuses the code or cannot be reached.
   */
  exchangeCode(code: string): Promise<string>;
  /**
   * Whether this installation of the App is in the token user's
   * GET /user/installations. Throws when GitHub answers an error.
   */
  userCanAccessInstallation(
    userToken: string,
    installationId: number,
  ): Promise<boolean>;
  /** App-authenticated lookup on GitHub; null when it does not exist. */
  getInstallation(installationId: number): Promise<GitHubInstallation | null>;
}

export type CompleteInstallationDeps = {
  userId: string;
  hasPro: () => Promise<boolean>;
  store: InstallationLinkStore;
  github: GitHubInstallationAccess;
};

export const GITHUB_AUTHORIZATION_REQUIRED =
  "GitHub did not send an authorization code with this installation, so Treq cannot confirm your GitHub account can access it. Start the installation again from the dashboard.";

export const INSTALLATION_NOT_ACCESSIBLE =
  "Your GitHub account cannot access this GitHub App installation. Install the app from the dashboard with the GitHub account that manages it.";

export const GITHUB_VERIFICATION_FAILED =
  "Treq could not confirm this installation with GitHub. Start the installation again from the dashboard.";

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export async function completeInstallation(
  body: unknown,
  deps: CompleteInstallationDeps,
): Promise<HandlerResult> {
  const { installation_id, state, code } = (body ?? {}) as {
    installation_id?: unknown;
    state?: unknown;
    code?: unknown;
  };
  const installationId = Number(installation_id);
  if (
    !Number.isInteger(installationId) ||
    installationId <= 0 ||
    typeof state !== "string" ||
    !state
  ) {
    return { status: 400, body: { error: "Missing installation_id or state" } };
  }
  if (typeof code !== "string" || !code) {
    return {
      status: 400,
      body: {
        error: GITHUB_AUTHORIZATION_REQUIRED,
        code: "github_authorization_required",
      },
    };
  }

  if (!(await deps.hasPro())) {
    return proRequired(PRO_REQUIRED_MESSAGES.githubApp);
  }

  // Atomically consume the intent: single-use, unexpired, owned by the
  // caller. Consuming before the GitHub lookup prevents concurrent reuse.
  let intent: { id: string } | null;
  try {
    intent = await deps.store.consumeIntent(
      deps.userId,
      await sha256Hex(state),
      new Date().toISOString(),
    );
  } catch (err) {
    console.error(
      "[complete-github-installation] intent lookup failed:",
      errorMessage(err),
    );
    return { status: 500, body: { error: "Failed to verify install intent" } };
  }
  if (!intent) {
    return {
      status: 403,
      body: { error: "Install intent is invalid, expired or already used" },
    };
  }

  // Prove the signed-in user's GitHub account can access the installation.
  // The token lives only in this block.
  let accessible: boolean;
  try {
    const userToken = await deps.github.exchangeCode(code);
    accessible = await deps.github.userCanAccessInstallation(
      userToken,
      installationId,
    );
  } catch (err) {
    console.error(
      "[complete-github-installation] GitHub user verification failed:",
      errorMessage(err),
    );
    return {
      status: 502,
      body: {
        error: GITHUB_VERIFICATION_FAILED,
        code: "github_verification_failed",
      },
    };
  }
  if (!accessible) {
    console.log(
      JSON.stringify({
        operation: "installation_link_refused",
        reason: "installation_not_accessible",
        installation_id: installationId,
        user_id: deps.userId,
      }),
    );
    return {
      status: 403,
      body: {
        error: INSTALLATION_NOT_ACCESSIBLE,
        code: "installation_not_accessible",
      },
    };
  }

  // Verify the installation actually exists for our app on GitHub's side.
  let installation: GitHubInstallation | null;
  try {
    installation = await deps.github.getInstallation(installationId);
  } catch (err) {
    console.error(
      "[complete-github-installation] GitHub lookup failed:",
      errorMessage(err),
    );
    return {
      status: 502,
      body: { error: "Failed to verify installation with GitHub" },
    };
  }
  if (!installation) {
    return { status: 404, body: { error: "Installation not found on GitHub" } };
  }

  let link: LinkResult;
  try {
    link = await deps.store.linkInstallation(installation, deps.userId);
  } catch (err) {
    console.error(
      "[complete-github-installation] link failed:",
      errorMessage(err),
    );
    return { status: 500, body: { error: "Failed to link installation" } };
  }
  if (link.result === "organization_owner_required") {
    console.log(
      JSON.stringify({
        operation: "installation_relink_refused",
        installation_id: installation.id,
        user_id: deps.userId,
      }),
    );
    return {
      status: 403,
      body: {
        error: ORGANIZATION_OWNER_REQUIRED,
        code: "organization_owner_required",
      },
    };
  }

  console.log(
    JSON.stringify({
      operation: "installation_linked",
      installation_id: installation.id,
      account_login: installation.account.login,
      user_id: deps.userId,
    }),
  );
  // The callback page sends a GitHub organization's installation that no
  // Treq organization owns yet to the Team tab, where an owner attaches it.
  return {
    status: 200,
    body: {
      ok: true,
      account_login: installation.account.login,
      account_type: installation.account.type,
      organization_id: link.organization_id,
    },
  };
}

export function installationLinkStore(
  client: ServiceClientLike,
): InstallationLinkStore {
  return {
    async consumeIntent(userId, stateHash, nowIso) {
      const { data, error } = await client
        .from("github_install_intents")
        .update({ consumed_at: nowIso })
        .eq("state_hash", stateHash)
        .eq("user_id", userId)
        .is("consumed_at", null)
        .gte("expires_at", nowIso)
        .select("id")
        .maybeSingle();
      if (error) throw new Error(error.message);
      return (data as { id: string } | null) ?? null;
    },
    async linkInstallation(installation, userId) {
      const { data, error } = await client.rpc("github_link_installation", {
        p_actor: userId,
        p_installation_id: installation.id,
        p_account_login: installation.account.login,
        p_account_type: installation.account.type,
        p_account_avatar_url: installation.account.avatar_url ?? null,
        p_app_id: installation.app_id,
      });
      if (error) throw new Error(error.message);
      return data as LinkResult;
    },
  };
}
