/**
 * URL routes for the GitHub integration pages, built on top of wouter.
 *
 * These are hash routes (see the `useHashLocation` Router in App.tsx), so
 * everything lives after the `#` and never touches `location.search` — that's
 * where the app's own `?repo=...` window-identity param lives. For the same
 * reason, filters are encoded as a path segment rather than `?filter=...`:
 * wouter's hash-location `navigate()` treats a `?` in the target as the
 * *page's* real query string, not part of the hash.
 */

export type GitHubTab = "issues" | "prs" | "merge-queue";
export type GitHubStateFilter = "draft" | "open" | "closed" | "all";

export const GITHUB_BASE_PATH = "/github";

export function githubTabPath(tab: "merge-queue"): string {
  return `${GITHUB_BASE_PATH}/${tab}`;
}

export function githubListPath(
  tab: "issues" | "prs",
  filter: GitHubStateFilter = "open",
): string {
  return `${GITHUB_BASE_PATH}/${tab}/${filter}`;
}

export function githubNewItemPath(
  tab: "issues" | "prs",
  filter: GitHubStateFilter = "open",
): string {
  return `${githubListPath(tab, filter)}/new`;
}

export function githubDetailPath(
  tab: "issues" | "prs",
  number: number,
  filter: GitHubStateFilter = "open",
): string {
  return `${githubListPath(tab, filter)}/${number}`;
}

/** Maps a PR's gh state to the filter tab that will surface it in the list. */
export function stateFilterForPrState(
  prState: string | null | undefined,
): GitHubStateFilter {
  switch (prState?.toUpperCase()) {
    case "OPEN":
      return "open";
    case "CLOSED":
      return "closed";
    default:
      // MERGED PRs aren't returned by gh's "closed" filter, so fall back to "all".
      return "all";
  }
}

/** Full-screen review of a PR's diff. */
export function githubPrReviewPath(
  number: number,
  filter: GitHubStateFilter = "open",
): string {
  return `${githubDetailPath("prs", number, filter)}/review`;
}

export function parsePrReviewPath(
  path: string,
): { number: number; filter: GitHubStateFilter } | null {
  const match = path.match(/^\/github\/prs\/([^/]+)\/(\d+)\/review$/);
  if (!match) return null;
  const filter = ["draft", "open", "closed", "all"].includes(match[1])
    ? (match[1] as GitHubStateFilter)
    : "open";
  return { number: Number(match[2]), filter };
}
