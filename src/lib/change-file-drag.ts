import type { WorkspaceChangesRefreshDetail } from "./workspace-refresh";

/** Reserved source/destination branch for the home repo (matches Rust HOME_MOVE_ENDPOINT). */
export const HOME_MOVE_ENDPOINT = ".";

/** Dispatched after a successful move so the Review tab reloads its file list. */
export const REFRESH_WORKSPACE_CHANGES_EVENT = "treq:refresh-workspace-changes";

/** Pending move opened from the "Move to workspace…" menu. */
export interface ChangeFilesMoveRequest {
  files: string[];
  sourceBranch: string;
  destinationBranch: string;
  destinationLabel: string;
}

export function dispatchRefreshWorkspaceChanges(
  detail?: WorkspaceChangesRefreshDetail,
): void {
  window.dispatchEvent(
    new CustomEvent<WorkspaceChangesRefreshDetail>(
      REFRESH_WORKSPACE_CHANGES_EVENT,
      { detail: detail ?? {} },
    ),
  );
}

/** Trailing debounce for watcher/agent disk bursts. Immediate dispatch stays for user mutations. */
export const WORKSPACE_CHANGES_REFRESH_DEBOUNCE_MS = 500;

let workspaceChangesRefreshTimer: ReturnType<typeof setTimeout> | null = null;
let pendingWorkspaceChangesRefresh: WorkspaceChangesRefreshDetail = {};

export function scheduleRefreshWorkspaceChanges(
  detail?: WorkspaceChangesRefreshDetail,
): void {
  if (detail?.workspaceId !== undefined) {
    pendingWorkspaceChangesRefresh.workspaceId = detail.workspaceId;
  }
  if (detail?.changedPaths && detail.changedPaths.length > 0) {
    pendingWorkspaceChangesRefresh.changedPaths = [
      ...new Set([
        ...(pendingWorkspaceChangesRefresh.changedPaths ?? []),
        ...detail.changedPaths,
      ]),
    ];
  }
  if (workspaceChangesRefreshTimer !== null) {
    clearTimeout(workspaceChangesRefreshTimer);
  }
  workspaceChangesRefreshTimer = setTimeout(() => {
    workspaceChangesRefreshTimer = null;
    const pending = pendingWorkspaceChangesRefresh;
    pendingWorkspaceChangesRefresh = {};
    dispatchRefreshWorkspaceChanges(pending);
  }, WORKSPACE_CHANGES_REFRESH_DEBOUNCE_MS);
}

export function cancelScheduledRefreshWorkspaceChanges(): void {
  pendingWorkspaceChangesRefresh = {};
  if (workspaceChangesRefreshTimer === null) return;
  clearTimeout(workspaceChangesRefreshTimer);
  workspaceChangesRefreshTimer = null;
}
