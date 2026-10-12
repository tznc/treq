import { GitMerge, Loader2 } from "lucide-react";

import useSWR from "swr";
import { useRepositoryCacheKey } from "../../lib/active-repository-context";
import type { MutationResult } from "../../hooks/useMutation";
import {
  getRepoDefaultBranch,
  listCommits,
  listWorkspaceStatuses,
} from "../../lib/api";
import {
  MERGE_QUEUE_HISTORY_PAGE_SIZE,
  partitionQueueStacks,
  takeHistoryPage,
  type QueueEntry,
} from "../../lib/merge-queue-stacks";
import {
  sumWorkspaceDiffStats,
  type WorkspaceDiffStats,
} from "../../lib/workspace-stack";
import { Button } from "../ui/button";
import { EmptyState } from "./shared";
import {
  MergeQueueDisabled,
  MergeQueueUpsell,
  QueueStackBlock,
  TargetBranchMarker,
  mergeLocalDiffStats,
} from "./merge-queue-parts";

interface MergeQueueListProps {
  repoPath: string;
  queueLoading: boolean;
  queueEntries: QueueEntry[];
  dequeueBranches: MutationResult<string[], string[]>;
  showMergedHistory: boolean;
  historyLimit: number;
  onHistoryLimitChange: (limit: number) => void;
}

function MergeQueueList({
  repoPath,
  queueLoading,
  queueEntries,
  dequeueBranches,
  showMergedHistory,
  historyLimit,
  onHistoryLimitChange,
}: MergeQueueListProps) {
  const repoCacheKey = useRepositoryCacheKey(repoPath);
  const { data: workspaceStatuses } = useSWR(
    repoCacheKey && queueEntries.length > 0
      ? ["workspace-statuses", repoCacheKey]
      : null,
    () => listWorkspaceStatuses(repoPath),
  );

  const { data: defaultBranchName } = useSWR(
    repoPath ? ["repo-default-branch", repoPath] : null,
    () => getRepoDefaultBranch(repoPath),
  );

  const { data: homeLog } = useSWR(
    repoPath ? ["workspace-commits", repoPath, null, "merge-queue-tip"] : null,
    () => listCommits(repoPath, null, false, undefined, 1),
  );

  const branchToWorkspaceId = (() => {
    const map = new Map<string, number>();
    for (const status of workspaceStatuses ?? []) {
      map.set(status.current.branch_name, status.current.id);
    }
    return map;
  })();

  const branchesNeedingStats = queueEntries.filter(
    (entry) =>
      entry.insertions == null &&
      entry.deletions == null &&
      branchToWorkspaceId.has(entry.branch_name),
  );

  const statsWorkspaceIds = branchesNeedingStats.map(
    (entry) => branchToWorkspaceId.get(entry.branch_name)!,
  );
  const { data: commitLists } = useSWR(
    repoPath && statsWorkspaceIds.length > 0
      ? ["workspace-commits-many", repoPath, statsWorkspaceIds.join(",")]
      : null,
    () =>
      Promise.all(
        statsWorkspaceIds.map((workspaceId) =>
          listCommits(repoPath, workspaceId),
        ),
      ),
  );

  const localStatsByBranch = (() => {
    const map = new Map<string, WorkspaceDiffStats>();
    branchesNeedingStats.forEach((entry, index) => {
      const result = commitLists?.[index];
      if (!result) return;
      map.set(entry.branch_name, sumWorkspaceDiffStats(result.commits));
    });
    return map;
  })();

  const enrichedEntries = mergeLocalDiffStats(queueEntries, localStatsByBranch);

  if (queueLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const { active, history } = partitionQueueStacks(enrichedEntries);
  const { visible: visibleHistory, hasMore } = takeHistoryPage(
    history,
    showMergedHistory ? historyLimit : 0,
  );
  // Next-to-merge stack sits just above the target-branch terminator.
  const displayActive = [...active].reverse();
  const targetBranch =
    active[0]?.targetBranch ??
    history[0]?.targetBranch ??
    defaultBranchName ??
    "main";
  // Newest fully-merged stack's root is what last landed on the target tip.
  const tipEntry = history[0]?.entries[0] ?? null;
  const tipPrNumber = tipEntry?.pr_number ?? null;
  const tipPrTitle = tipEntry?.pr_title ?? null;
  const tipCommit = homeLog?.commits[0] ?? null;
  const tipShortId = tipCommit?.short_id ?? null;
  const tipCommitId = tipCommit?.commit_id ?? null;

  return (
    <div className="flex flex-col h-full" data-testid="merge-queue-list">
      {/*
        Continuous rail: p-3 (12) + card p-4 (16) + half node (7) = 35px.
        Drawn above card backgrounds so it stays visible through cards and
        the gaps between them, linking every PR down to the target branch.
      */}
      <div className="relative flex-1 overflow-y-auto p-3">
        <div
          className="pointer-events-none absolute left-[35px] top-3 bottom-3 z-[5] w-0.5 bg-border"
          data-testid="merge-queue-rail"
          aria-hidden="true"
        />

        <div className="relative space-y-3">
          {displayActive.length === 0 ? (
            <div className="relative z-10 py-8">
              <EmptyState icon={GitMerge} message="Merge queue is empty." />
            </div>
          ) : (
            displayActive.map((stack) => (
              <QueueStackBlock
                key={stack.entries[0].branch_name}
                stack={stack}
                dequeueBranches={dequeueBranches}
              />
            ))
          )}

          <TargetBranchMarker
            targetBranch={targetBranch}
            tipShortId={tipShortId}
            tipCommitId={tipCommitId}
            tipPrNumber={tipPrNumber}
            tipPrTitle={tipPrTitle}
          />

          {showMergedHistory && visibleHistory.length > 0 && (
            <div data-testid="merge-queue-history" className="space-y-3 pt-1">
              {visibleHistory.map((stack) => (
                <QueueStackBlock
                  key={`history-${stack.entries[0].branch_name}`}
                  stack={stack}
                  dequeueBranches={dequeueBranches}
                  showRemove={false}
                />
              ))}
              {hasMore && (
                <div className="relative z-10 flex justify-center pt-1 pb-1">
                  <Button
                    variant="outline"
                    size="sm"
                    className="text-base"
                    onClick={() =>
                      onHistoryLimitChange(
                        historyLimit + MERGE_QUEUE_HISTORY_PAGE_SIZE,
                      )
                    }
                  >
                    Load more
                  </Button>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export interface MergeQueueTabProps {
  isPro: boolean;
  hasRemote: boolean;
  repoPath: string;
  queueEnabled: boolean | undefined;
  queueLoading: boolean;
  queueEntries: QueueEntry[];
  dequeueBranches: MutationResult<string[], string[]>;
  showMergedHistory: boolean;
  historyLimit: number;
  onHistoryLimitChange: (limit: number) => void;
  onOpenSettings?: (tab?: string) => void;
}

export function MergeQueueTab({
  isPro,
  hasRemote,
  repoPath,
  queueEnabled,
  queueLoading,
  queueEntries,
  dequeueBranches,
  showMergedHistory,
  historyLimit,
  onHistoryLimitChange,
  onOpenSettings,
}: MergeQueueTabProps) {
  if (!isPro) return <MergeQueueUpsell />;
  if (!hasRemote) return null;
  if (!queueEnabled)
    return <MergeQueueDisabled onOpenSettings={onOpenSettings} />;

  return (
    <MergeQueueList
      repoPath={repoPath}
      queueLoading={queueLoading}
      queueEntries={queueEntries}
      dequeueBranches={dequeueBranches}
      showMergedHistory={showMergedHistory}
      historyLimit={historyLimit}
      onHistoryLimitChange={onHistoryLimitChange}
    />
  );
}
