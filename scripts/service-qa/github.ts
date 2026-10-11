/**
 * A stand-in for GitHub in complete-github-installation's in-process runs.
 * Local runs have no GitHub App, so specs pass this as the handler's
 * `github` adapter instead of the REST one in github.ts. Everything else
 * (intents, has_pro, the link function) is the real local database.
 *
 * Each OAuth code stands for one GitHub user and lists the installation IDs
 * that user can access. An unknown code fails the exchange the way GitHub
 * answers bad_verification_code.
 */
import type {
  GitHubInstallation,
  GitHubInstallationAccess,
} from "../../supabase/functions/complete-github-installation/lib";

export type StubGitHubUsers = Record<string, number[]>;

export function stubGitHubInstallationAccess(
  users: StubGitHubUsers,
  account: (id: number) => GitHubInstallation["account"],
): GitHubInstallationAccess {
  const tokenPrefix = "stub-user-token:";
  return {
    async exchangeCode(code) {
      if (!(code in users)) {
        throw new Error("GitHub token exchange refused: bad_verification_code");
      }
      return `${tokenPrefix}${code}`;
    },
    async userCanAccessInstallation(userToken, installationId) {
      const code = userToken.slice(tokenPrefix.length);
      return (users[code] ?? []).includes(installationId);
    },
    async getInstallation(id) {
      return { id, account: account(id), app_id: 1 };
    },
  };
}
