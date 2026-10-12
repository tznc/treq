import type {
  GhIssue,
  GhListPage,
  GhPullRequest,
  GhReviewThread,
} from "./api-types";
import { invoke } from "@tauri-apps/api/core";
import { assertLocalOperation } from "./repository-adapter";

export const GH_LIST_PAGE_SIZE = 30;

/** One page of issues, newest first. Pass the previous page's `endCursor`
 * as `after` to get the next page. */
export const ghListIssues = (
  repoFullName: string,
  state: string,
  limit = GH_LIST_PAGE_SIZE,
  after?: string | null,
): Promise<GhListPage<GhIssue>> =>
  invoke("gh_list_issues", { repoFullName, state, limit, after });

export const ghViewIssue = (
  repoFullName: string,
  issueNumber: number,
): Promise<GhIssue> => invoke("gh_view_issue", { repoFullName, issueNumber });

/** Opens (or creates) the `github-<number>-<slug>` workspace for an issue. */
export const githubOpenOrCreateWorkspaceFromIssue = async (
  repoPath: string,
  number: number,
  title: string,
  url: string,
): Promise<{ workspace_id: number; created: boolean }> => {
  assertLocalOperation(repoPath, "Opening a GitHub issue");
  return invoke("github_open_or_create_workspace_from_issue", {
    repoPath,
    number,
    title,
    url,
  });
};

export const ghCreateIssue = (
  repoFullName: string,
  title: string,
  body: string,
): Promise<number> => invoke("gh_create_issue", { repoFullName, title, body });

export const ghCreateIssueComment = (
  repoFullName: string,
  issueNumber: number,
  body: string,
): Promise<void> =>
  invoke("gh_create_issue_comment", { repoFullName, issueNumber, body });

/** Reasons `gh issue close --reason` accepts. */
export type IssueCloseReason = "completed" | "not planned";

export const ghCloseIssue = (
  repoFullName: string,
  issueNumber: number,
  reason?: IssueCloseReason,
): Promise<void> =>
  invoke("gh_close_issue", { repoFullName, issueNumber, reason });

export const ghReopenIssue = (
  repoFullName: string,
  issueNumber: number,
): Promise<void> => invoke("gh_reopen_issue", { repoFullName, issueNumber });

export const ghEditIssue = (
  repoFullName: string,
  issueNumber: number,
  title: string,
  body: string,
): Promise<void> =>
  invoke("gh_edit_issue", { repoFullName, issueNumber, title, body });

/** Permanently deletes the issue. Callers must confirm with the user first. */
export const ghDeleteIssue = (
  repoFullName: string,
  issueNumber: number,
): Promise<void> => invoke("gh_delete_issue", { repoFullName, issueNumber });

export const ghListPrs = (
  repoFullName: string,
  state: string,
  limit = GH_LIST_PAGE_SIZE,
  page = 1,
): Promise<GhListPage<GhPullRequest>> =>
  invoke("gh_list_prs", { repoFullName, state, limit, page });

export const ghViewPr = (
  repoFullName: string,
  prNumber: number,
): Promise<GhPullRequest> => invoke("gh_view_pr", { repoFullName, prNumber });

/** Read-only: lists PR review comment threads (with resolved/outdated state). */
export const ghListPrReviewThreads = (
  owner: string,
  repo: string,
  prNumber: number,
): Promise<GhReviewThread[]> =>
  invoke("gh_list_pr_review_threads", { owner, repo, prNumber });

export const ghCreatePrComment = (
  repoFullName: string,
  prNumber: number,
  body: string,
): Promise<void> =>
  invoke("gh_create_pr_comment", { repoFullName, prNumber, body });

export const ghClosePr = (
  repoFullName: string,
  prNumber: number,
): Promise<void> => invoke("gh_close_pr", { repoFullName, prNumber });

export const ghReopenPr = (
  repoFullName: string,
  prNumber: number,
): Promise<void> => invoke("gh_reopen_pr", { repoFullName, prNumber });

export const ghSetPrDraft = (
  repoFullName: string,
  prNumber: number,
  draft: boolean,
): Promise<void> =>
  invoke("gh_set_pr_draft", { repoFullName, prNumber, draft });

export const ghCreatePr = (
  repoFullName: string,
  title: string,
  body: string,
  baseBranch: string,
  headBranch: string,
  draft = false,
): Promise<number> =>
  invoke("gh_create_pr", {
    repoFullName,
    title,
    body,
    baseBranch,
    headBranch,
    draft,
  });
