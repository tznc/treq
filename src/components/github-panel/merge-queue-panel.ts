import useSWR from "swr";
import {
  useGitRemoteInfo,
  useMergeQueueEnabled,
} from "../../hooks/useMergeQueueStatus";
import type { MutationResult } from "../../hooks/useMutation";
import { useGpuiScreen } from "../../lib/gpui-screen";
import {
  MERGE_QUEUE_HISTORY_PAGE_SIZE,
  type QueueEntry,
} from "../../lib/merge-queue-stacks";
import { supabase } from "../../lib/supabase";
import { pollMs } from "../../lib/swr-cache";

/** The repo's merge queue, in position order, read while `enabled`. */
export function useQueueEntries(enabled: boolean, repoFullName: string) {
  return useSWR(
    enabled ? ["repo-branch-queue-statuses-panel", repoFullName] : null,
    async () => {
      const { data, error } = await supabase.rpc(
        "get_repo_branch_queue_statuses",
        { p_repo_full_name: repoFullName },
      );
      if (error) throw error;
      return ((data ?? []) as QueueEntry[]).slice().sort((a, b) => {
        if (a.position !== b.position) return a.position - b.position;
        return a.branch_name.localeCompare(b.branch_name);
      });
    },
    { refreshInterval: pollMs(30_000) },
  );
}

interface MergeQueuePanel {
  repoPath: string;
  isPro: boolean;
  queueLoading: boolean;
  queueEntries: QueueEntry[];
  dequeueBranches: MutationResult<string[], string[]>;
  showMergedHistory: boolean;
  setShowMergedHistory: (show: boolean) => void;
  historyLimit: number;
  setHistoryLimit: (limit: number) => void;
  onTabChange: (tab: string) => void;
  onRefresh: () => void;
  onOpenSettings?: (tab?: string) => void;
}

/**
 * Has GPUI draw GitHubPanel's Merge Queue tab (screen/merge_queue.rs) when
 * `show`, and applies what it sends back: the queue itself is in Supabase,
 * so the entries and removals stay with React.
 */
export function useGpuiMergeQueue(show: boolean, panel: MergeQueuePanel) {
  const { dequeueBranches } = panel;
  // Both are SWR reads the panel makes too, so these share its cache.
  const { data: remoteInfo, isLoading: remoteLoading } = useGitRemoteInfo(
    panel.repoPath,
  );
  const { data: queueEnabled } = useMergeQueueEnabled(panel.repoPath);
  useGpuiScreen(
    show
      ? {
          kind: "mergeQueue",
          repoPath: panel.repoPath,
          fullName: remoteInfo?.full_name ?? null,
          remoteLoading,
          isPro: panel.isPro,
          queueEnabled: queueEnabled ?? null,
          queueLoading: panel.queueLoading,
          entries: panel.queueEntries,
          dequeuing: dequeueBranches.isPending,
          showMergedHistory: panel.showMergedHistory,
          historyLimit: panel.historyLimit,
        }
      : null,
    (action) => {
      if (action.kind === "press") {
        if (action.id.startsWith("github.tab:"))
          panel.onTabChange(action.id.slice("github.tab:".length));
        if (action.id === "mergeQueue.refresh") panel.onRefresh();
        if (action.id === "mergeQueue.openSettings")
          panel.onOpenSettings?.("integrations");
        return;
      }
      if (action.kind !== "field") return;
      const { field, value } = action;
      if (field === "mergeQueue.showMerged") {
        panel.setShowMergedHistory(value === true);
        if (value === true)
          panel.setHistoryLimit(MERGE_QUEUE_HISTORY_PAGE_SIZE);
      }
      if (field === "mergeQueue.historyLimit" && typeof value === "number")
        panel.setHistoryLimit(value);
      if (
        field === "mergeQueue.dequeue" &&
        Array.isArray(value) &&
        !dequeueBranches.isPending
      )
        dequeueBranches.mutate(value.map(String));
    },
  );
}
