import { useRoute } from "wouter";
import { FEATURES } from "../../lib/features";
import type { GitHubStateFilter, GitHubTab } from "../../lib/githubRoutes";

/**
 * The GitHub view's tab, filter, selection, and create-form state, which
 * all live in the URL (as /github/<tab>/<filter>/<selector?>) so browser
 * back/forward moves between them.
 */
export function useGithubRoute() {
  const [isMergeQueueRoute] = useRoute("/github/merge-queue");
  const [isIssuesRoute, issuesParams] = useRoute<{
    filter: string;
    selector?: string;
  }>("/github/issues/:filter/:selector?");
  const [isPrsRoute, prsParams] = useRoute<{
    filter: string;
    selector?: string;
  }>("/github/prs/:filter/:selector?");

  // With the mergeQueue build flag off, a direct #/github/merge-queue link
  // falls back to the Issues tab instead of rendering the hidden tab.
  const activeTab: GitHubTab =
    FEATURES.mergeQueue && isMergeQueueRoute
      ? "merge-queue"
      : isPrsRoute
        ? "prs"
        : "issues";
  const routeParams = activeTab === "prs" ? prsParams : issuesParams;
  const rawFilter = (isIssuesRoute || isPrsRoute) && routeParams?.filter;
  const currentFilter: GitHubStateFilter =
    rawFilter === "closed" || rawFilter === "all"
      ? rawFilter
      : rawFilter === "draft" && activeTab === "prs"
        ? "draft"
        : "open";
  const selector = routeParams?.selector;

  const parsedSelectedNumber =
    selector !== undefined && selector !== "new" ? Number(selector) : NaN;
  const selectedNumber = Number.isFinite(parsedSelectedNumber)
    ? parsedSelectedNumber
    : null;

  return {
    activeTab,
    currentFilter,
    showCreateForm: selector === "new",
    selectedNumber,
  };
}
