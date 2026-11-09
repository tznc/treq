// POST /github/token, for sandboxes: GitHub App installation tokens (they
// act as treq[bot], so an agent cannot approve its owner's pull requests)
// for the repositories a sandbox works on, one token per installation. A
// repository is included only when the owner's own GitHub account can push
// to it, checked with the owner's user token. Kept free of Deno and
// Supabase imports for the unit tests.

import type { Result } from "./lib.ts";

export const MAX_REPOSITORIES = 50;

// What an agent may do, narrower than the App's own permissions. Workflow
// files are read through `contents`; GitHub's `workflows` permission only
// grants write, so it is left out.
export const TOKEN_PERMISSIONS = {
  contents: "write",
  pull_requests: "write",
  issues: "write",
  metadata: "read",
  actions: "read",
  checks: "read",
  statuses: "read",
} as const;

const REPO_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;

const GITHUB_HEADERS = {
  Accept: "application/vnd.github+json",
  "X-GitHub-Api-Version": "2022-11-28",
  "User-Agent": "treq-github-app/1.0",
};

export interface GitHubTokenDeps {
  /** The owner's linked installation that can see `owner/name`, if any. */
  installationFor(fullName: string): Promise<number | null>;
  /** The owner's GitHub user token, refreshed; null when they must reconnect. */
  userToken(): Promise<string | null>;
  appJwt(): Promise<string>;
  fetch: typeof fetch;
}

type RepoError = { repository: string; code: string; error: string };

async function ownerCanPush(repository: string, userToken: string, deps: GitHubTokenDeps) {
  const res = await deps.fetch(`https://api.github.com/repos/${repository}`, {
    headers: { ...GITHUB_HEADERS, Authorization: `Bearer ${userToken}` },
  });
  if (!res.ok) {
    await res.body?.cancel();
    if (res.status === 404 || res.status === 403) return false;
    throw new Error(`GitHub GET /repos/${repository} → ${res.status}`);
  }
  const repo = (await res.json()) as { permissions?: { push?: boolean } };
  return repo.permissions?.push === true;
}

export async function sandboxGithubTokens(
  repositories: unknown,
  deps: GitHubTokenDeps,
): Promise<Result> {
  if (
    !Array.isArray(repositories) || repositories.length === 0 ||
    repositories.length > MAX_REPOSITORIES ||
    !repositories.every((r) => typeof r === "string" && REPO_PATTERN.test(r))
  ) {
    return {
      status: 400,
      body: { error: `repositories must be 1-${MAX_REPOSITORIES} owner/name strings` },
    };
  }

  const userToken = await deps.userToken();
  if (!userToken) {
    return {
      status: 401,
      body: {
        error: "The sandbox owner must reconnect the treq GitHub App.",
        code: "github_reconnect",
      },
    };
  }

  const errors: RepoError[] = [];
  const byInstallation = new Map<number, string[]>();
  for (const repository of new Set(repositories as string[])) {
    const installation = await deps.installationFor(repository);
    if (installation === null) {
      errors.push({
        repository,
        code: "not_installed",
        error: "The treq GitHub App is not installed for this repository",
      });
      continue;
    }
    if (!(await ownerCanPush(repository, userToken, deps))) {
      errors.push({
        repository,
        code: "github_permission_denied",
        error: "The sandbox owner cannot push to this repository",
      });
      continue;
    }
    byInstallation.set(installation, [...(byInstallation.get(installation) ?? []), repository]);
  }

  const tokens = [];
  for (const [installation, repos] of byInstallation) {
    const res = await deps.fetch(
      `https://api.github.com/app/installations/${installation}/access_tokens`,
      {
        method: "POST",
        headers: {
          ...GITHUB_HEADERS,
          Authorization: `Bearer ${await deps.appJwt()}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          repositories: repos.map((r) => r.split("/")[1]),
          permissions: TOKEN_PERMISSIONS,
        }),
      },
    );
    if (!res.ok) {
      console.error("[integration-proxy] GitHub token mint failed:", res.status);
      await res.body?.cancel();
      return { status: 502, body: { error: "Failed to mint GitHub token" } };
    }
    const grant = await res.json();
    tokens.push({ repositories: repos, token: grant.token, expires_at: grant.expires_at });
  }

  return { status: tokens.length ? 200 : 403, body: { tokens, errors } };
}
