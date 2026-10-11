// The GitHub side of completing an installation: the REST implementation of
// GitHubInstallationAccess (lib.ts). index.ts passes it the App's OAuth
// client credentials; tests pass a fake fetch, and service-qa replaces the
// whole adapter with a stub.
//
// With "Request user authorization (OAuth) during installation" on, GitHub
// redirects to the App's callback URL with a `code`. POST
// /login/oauth/access_token turns it into a user access token, and
// GET /user/installations lists the installations of this App that the user
// can access. The token is only passed to that call; nothing keeps it.

import { getInstallation } from "../_shared/merge-queue/github-adapter.ts";
import type { GitHubInstallationAccess } from "./lib.ts";

export type GitHubAppOAuthCredentials = {
  /** GITHUB_APP_CLIENT_ID: the Client ID on the App's settings page. */
  clientId: string;
  /** GITHUB_APP_CLIENT_SECRET: one of the App's client secrets. */
  clientSecret: string;
};

/** Pages of 100 read from /user/installations before answering false. */
export const MAX_INSTALLATION_PAGES = 10;

const PER_PAGE = 100;
const USER_AGENT = "treq-github-app/1.0";

export function githubInstallationAccess(
  credentials: GitHubAppOAuthCredentials,
  fetchImpl: typeof fetch = fetch,
): GitHubInstallationAccess {
  return {
    async exchangeCode(code) {
      if (!credentials.clientId || !credentials.clientSecret) {
        throw new Error(
          "GITHUB_APP_CLIENT_ID and GITHUB_APP_CLIENT_SECRET must be configured",
        );
      }
      const res = await fetchImpl(
        "https://github.com/login/oauth/access_token",
        {
          method: "POST",
          headers: {
            Accept: "application/json",
            "Content-Type": "application/x-www-form-urlencoded",
            "User-Agent": USER_AGENT,
          },
          body: new URLSearchParams({
            client_id: credentials.clientId,
            client_secret: credentials.clientSecret,
            code,
          }).toString(),
        },
      );
      if (!res.ok) {
        await res.body?.cancel();
        throw new Error(`GitHub token exchange → ${res.status}`);
      }
      // GitHub reports a bad or expired code with 200 and an `error` field.
      const answer = (await res.json()) as {
        access_token?: unknown;
        error?: unknown;
      };
      if (typeof answer.access_token !== "string" || !answer.access_token) {
        const reason =
          typeof answer.error === "string" ? answer.error : "no access_token";
        throw new Error(`GitHub token exchange refused: ${reason}`);
      }
      return answer.access_token;
    },

    async userCanAccessInstallation(userToken, installationId) {
      for (let page = 1; page <= MAX_INSTALLATION_PAGES; page++) {
        const res = await fetchImpl(
          `https://api.github.com/user/installations?per_page=${PER_PAGE}&page=${page}`,
          {
            headers: {
              Authorization: `Bearer ${userToken}`,
              Accept: "application/vnd.github+json",
              "X-GitHub-Api-Version": "2022-11-28",
              "User-Agent": USER_AGENT,
            },
          },
        );
        if (!res.ok) {
          await res.body?.cancel();
          throw new Error(`GitHub GET /user/installations → ${res.status}`);
        }
        const { installations = [], total_count: total = 0 } =
          (await res.json()) as {
            installations?: { id: number }[];
            total_count?: number;
          };
        if (installations.some((i) => i.id === installationId)) return true;
        if (installations.length < PER_PAGE || page * PER_PAGE >= total)
          return false;
      }
      return false;
    },

    getInstallation,
  };
}
