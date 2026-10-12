import { invoke } from "@tauri-apps/api/core";
import type { PrCiStatus, PrInfo } from "./api-types";
import { localReadOr } from "./local-only";
import { assertLocalOperation } from "./repository-adapter";

/**
 * Force-fetch PR info via `gh pr view` and warm the Rust cache.
 * Prefer cache reads (`getCachedPrInfo` / `listCachedPrStatuses`) for UI
 * polling; use this only after mutations that need an immediate refresh.
 */
export const getPrInfoViaGh = (
  repoPath: string,
  branchName: string,
): Promise<PrInfo | null> =>
  localReadOr(repoPath, null, () =>
    invoke("get_pr_info_via_gh", { repoPath, branchName }),
  );

/** Start Rust-side background PR-status polling for a repo's workspaces. */
export const startPrStatusPolling = (repoPath: string): Promise<void> =>
  localReadOr(repoPath, undefined, () =>
    invoke("start_pr_status_polling", { repoPath }),
  );

/** Stop Rust-side background PR-status polling for a repo. */
export const stopPrStatusPolling = (repoPath: string): Promise<void> =>
  localReadOr(repoPath, undefined, () =>
    invoke("stop_pr_status_polling", { repoPath }),
  );

/**
 * Cached PR statuses (`branch -> PrInfo | null`) maintained by the Rust
 * background poller. Never shells out to `gh` from the UI thread.
 */
export const listCachedPrStatuses = (
  repoPath: string,
): Promise<Record<string, PrInfo | null>> =>
  localReadOr(repoPath, {}, () =>
    invoke("list_cached_pr_statuses", { repoPath }),
  );

/** Single-branch read from the Rust PR-status cache (no `gh` call). */
export const getCachedPrInfo = (
  repoPath: string,
  branchName: string,
): Promise<PrInfo | null> =>
  localReadOr(repoPath, null, () =>
    invoke("get_cached_pr_info", { repoPath, branchName }),
  );

/**
 * Cached CI rollups (`branch -> PrCiStatus | null`) from the Rust background
 * poller. Never shells out to `gh` from the UI thread.
 */
export const listCachedPrCiStatuses = (
  repoPath: string,
): Promise<Record<string, PrCiStatus | null>> =>
  localReadOr(repoPath, {}, () =>
    invoke("list_cached_pr_ci_statuses", { repoPath }),
  );

/** Single-branch CI read from the Rust cache (no `gh` call). */
export const getCachedPrCiStatus = (
  repoPath: string,
  branchName: string,
): Promise<PrCiStatus | null> =>
  localReadOr(repoPath, null, () =>
    invoke("get_cached_pr_ci_status", { repoPath, branchName }),
  );

/** Force a full-repo cache refresh (e.g. after bulk workspace changes). */
export const refreshPrStatuses = (repoPath: string): Promise<void> =>
  localReadOr(repoPath, undefined, () =>
    invoke("refresh_pr_statuses", { repoPath }),
  );

/**
 * Queue an out-of-band PR+CI refresh for one branch. Returns immediately;
 * the Rust background worker updates the cache and emits
 * `pr-statuses-updated` when done. Call when opening a workspace so the
 * UI does not wait for the next poll tick.
 */
export const refreshPrBranchStatus = (
  repoPath: string,
  branchName: string,
): Promise<void> =>
  localReadOr(repoPath, undefined, () =>
    invoke("refresh_pr_branch_status", { repoPath, branchName }),
  );

export type OpenOrCreateWorkspaceFromPrResult = {
  workspaceId: number;
  created: boolean;
};

/** Open or create a workspace for a GitHub PR head branch (base becomes target). */
export const openOrCreateWorkspaceFromPr = async (
  repoPath: string,
  headBranch: string,
  baseBranch: string,
  title?: string,
  description?: string,
): Promise<OpenOrCreateWorkspaceFromPrResult> => {
  assertLocalOperation(repoPath, "Opening a pull request");
  return invoke("open_or_create_workspace_from_pr", {
    repoPath,
    headBranch,
    baseBranch,
    title: title ?? null,
    description: description ?? null,
  });
};
