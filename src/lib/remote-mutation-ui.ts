import { create } from "zustand";
import type { MutationDispatchResult } from "./remote-dispatch";
import { invalidateQueries } from "./swr-cache";
import { peekActiveRepository, repositoryCacheKey } from "./active-repository";
import { scheduleRefreshWorkspaceChanges } from "./change-file-drag";

interface RemoteMutationFeedbackState {
  ambiguousReason: string | null;
  lastStatus: MutationDispatchResult["status"] | null;
  report: (result: MutationDispatchResult) => void;
  clearAmbiguous: () => void;
}

export function invalidateRemoteRepositoryData() {
  const repo = peekActiveRepository();
  if (!repo) {
    void invalidateQueries(["workspaces"]);
    void invalidateQueries(["workspace-statuses"]);
    void invalidateQueries(["workspace-changed-files"]);
    void invalidateQueries(["workspace-diff"]);
    void invalidateQueries(["linear-commits"]);
    void invalidateQueries(["workspace-commits"]);
    void invalidateQueries(["workspace-status"]);
    void invalidateQueries(["workspace-overview"]);
    void invalidateQueries(["commit-diff-viewer-commits"]);
    return;
  }
  const key = repositoryCacheKey(repo);
  void invalidateQueries(["workspaces", key]);
  void invalidateQueries(["workspace-statuses", key]);
  void invalidateQueries(["workspace-changed-files"]);
  void invalidateQueries(["workspace-diff"]);
  void invalidateQueries(["linear-commits", key]);
  void invalidateQueries(["workspace-commits", key]);
  // The selected workspace's status (which carries its conflict state) and
  // overview are keyed by the repository cache key. The commits list is keyed
  // by the remote repo path, so it is refreshed unscoped like the diff keys.
  void invalidateQueries(["workspace-status", key]);
  void invalidateQueries(["workspace-overview", key]);
  void invalidateQueries(["commit-diff-viewer-commits"]);
  void invalidateQueries(["repo-branch", key]);
  void invalidateQueries(["repo-status", key]);
  void invalidateQueries(["remote-review"]);
  void invalidateQueries(["workspace-review-change-count"]);
}

/**
 * Reloads everything shown for the active remote repository, including the
 * changes viewer and file browser, which load outside SWR.
 */
export function refreshRemoteRepository() {
  invalidateRemoteRepositoryData();
  scheduleRefreshWorkspaceChanges();
}

export const useRemoteMutationFeedback = create<RemoteMutationFeedbackState>(
  (set) => ({
    ambiguousReason: null,
    lastStatus: null,
    report: (result) => {
      if (result.status === "ambiguous") {
        set({
          lastStatus: "ambiguous",
          ambiguousReason: result.reason,
        });
        // The cached view may miss a change that did land, and the user
        // decides from it whether to retry.
        refreshRemoteRepository();
        return;
      }
      set({ lastStatus: result.status, ambiguousReason: null });
      invalidateRemoteRepositoryData();
    },
    clearAmbiguous: () => set({ ambiguousReason: null }),
  }),
);

export function applyMutationDispatchResult<T>(
  result: MutationDispatchResult<T>,
): T | undefined {
  useRemoteMutationFeedback.getState().report(result);
  if (result.status === "applied") return result.value;
  return undefined;
}
