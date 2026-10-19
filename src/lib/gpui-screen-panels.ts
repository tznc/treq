// The repository panels GPUI draws: Linear, GitHub, PR review, merge queue.

/**
 * LinearPanel, which GPUI reads from the backend's Linear client
 * (src-tauri/src/gpui_shell/screen/linear.rs). It sets "linear.kickoff" to
 * an issue to start a session for, and "linear.startReview" to the review
 * prompt and agent to run.
 */
export interface GpuiLinearScreen {
  kind: "linear";
  repoPath: string;
}

/**
 * GitHubPanel's Issues and Pull Requests tabs, which GPUI reads and changes
 * through the backend itself (src-tauri/src/gpui_shell/screen/github.rs).
 * React says what the URL route shows, and GPUI hands back navigation as
 * `github.*` presses (`useGpuiGithubScreen`).
 */
export interface GpuiGithubScreen {
  kind: "github";
  repoPath: string;
  tab: "issues" | "prs";
  filter: "draft" | "open" | "closed" | "all";
  /** The issue or pull request whose detail shows. */
  selected: number | null;
  /** The new issue form or new pull request dialog is open. */
  creating: boolean;
  /** The Merge Queue tab, when the build offers it (drawn by `GpuiMergeQueueScreen`). */
  mergeQueue: { pro: boolean } | null;
  /** Workflow runs started from this window (`useRunningCheckKeys`). */
  runningChecks: string[];
}

/**
 * PrReviewScreen, which GPUI reads, comments on and submits itself
 * (src-tauri/src/gpui_shell/screen/pr_review.rs). It presses
 * "prReview.close" to leave and sends "prReview.startAgentReview"
 * ({ prompt, agent }) to open the agent terminal.
 */
export interface GpuiPrReviewScreen {
  kind: "prReview";
  repoPath: string;
  prNumber: number;
}

/**
 * GitHubPanel's Merge Queue tab (src-tauri/src/gpui_shell/screen/merge_queue.rs).
 * The queue lives in Supabase, so React sends its entries; GPUI reads the
 * workspaces' stats and the target branch's tip from the backend.
 */
export interface GpuiMergeQueueScreen {
  kind: "mergeQueue";
  repoPath: string;
  fullName: string | null;
  remoteLoading: boolean;
  isPro: boolean;
  queueEnabled: boolean | null;
  queueLoading: boolean;
  entries: {
    branch_name: string;
    pr_number: number | null;
    pr_title?: string | null;
    status: string;
    position: number;
    target_branch: string;
    insertions?: number | null;
    deletions?: number | null;
  }[];
  dequeuing: boolean;
  showMergedHistory: boolean;
  historyLimit: number;
}
