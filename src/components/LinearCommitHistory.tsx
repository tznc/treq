import { useEffect, useState } from "react";
import useSWR from "swr";
import { type JjLogCommit, type JjLogResult, listCommits } from "../lib/api";
import {
  cn,
  formatDayLabel,
  formatFullTimestamp,
  formatRelativeTime,
  getDayKey,
} from "../lib/utils";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "./ui/tooltip";
import { useRepositoryCacheKey } from "../lib/active-repository-context";
import { Loader2 } from "lucide-react";

interface LinearCommitHistoryProps {
  repoPath: string;
  workspaceId: number | null;
  onCommitClick?: (changeId: string) => void;
  /** Called when the commits result is loaded (allows parent to inspect counts) */
  onCommitsLoaded?: (result: JjLogResult) => void;
}

interface DayGroup {
  dayKey: string;
  label: string;
  commits: JjLogCommit[];
}

function normalizeCommits(commits: JjLogCommit[]): JjLogCommit[] {
  if (commits.length < 2) {
    return commits;
  }

  const [first, second, ...rest] = commits;
  const looksLikeWorkingCopyPlaceholder =
    first.description === "(no description)" &&
    first.bookmarks.length === 0 &&
    !first.is_immutable &&
    first.parent_ids.includes(second.commit_id);

  if (!looksLikeWorkingCopyPlaceholder) {
    return commits;
  }

  return [second, ...rest];
}

function filterWorkingCopyCommits(commits: JjLogCommit[]) {
  return commits.filter((commit) => !commit.is_working_copy);
}

function splitCommitsByTarget(commits: JjLogCommit[]) {
  return {
    workspaceCommits: commits.filter((commit) => !commit.on_target_only),
    targetBranchCommits: commits.filter((commit) => commit.on_target_only),
  };
}

function groupCommitsByDay(commits: JjLogCommit[]): DayGroup[] {
  const groups: DayGroup[] = [];
  for (const commit of commits) {
    const key = getDayKey(commit.timestamp);
    const last = groups[groups.length - 1];
    if (last && last.dayKey === key) {
      last.commits.push(commit);
    } else {
      groups.push({
        dayKey: key,
        label: formatDayLabel(commit.timestamp),
        commits: [commit],
      });
    }
  }
  return groups;
}

function getCommitHeadline(commit: JjLogCommit, isTentative: boolean): string {
  const headline = commit.description.split("\n")[0]?.trim();
  return headline && headline !== "(no description)"
    ? headline
    : isTentative
      ? "Working copy"
      : "Untitled commit";
}

export const LinearCommitHistory = ({
  repoPath,
  workspaceId,
  onCommitClick,
  onCommitsLoaded,
}: LinearCommitHistoryProps) => {
  const cacheKey = useRepositoryCacheKey(repoPath);
  const [limit, setLimit] = useState(14);
  const isHomeRepo = workspaceId == null;
  const includeTargetBranchHistory = workspaceId != null || isHomeRepo;

  useEffect(() => {
    setLimit(14);
  }, [repoPath, workspaceId]);

  const {
    data: commitsResult,
    isLoading: loading,
    isValidating,
  } = useSWR(
    cacheKey
      ? [
          "linear-commits",
          cacheKey,
          workspaceId,
          includeTargetBranchHistory,
          isHomeRepo ? limit : 14,
        ]
      : null,
    () =>
      listCommits(
        repoPath,
        workspaceId,
        includeTargetBranchHistory,
        undefined,
        isHomeRepo ? limit : 14,
      ),
  );
  const loadingMore = isValidating && limit > 14 && !loading;

  const {
    commits,
    tentativeWorkingCopy,
    targetBranchCommits,
    mergeBaseId,
    targetBranch,
    workspaceBranch,
  } = (() => {
    if (!commitsResult) {
      return {
        commits: [] as JjLogCommit[],
        tentativeWorkingCopy: null,
        targetBranchCommits: [] as JjLogCommit[],
        mergeBaseId: null as string | null,
        targetBranch: "",
        workspaceBranch: "",
      };
    }
    const split = splitCommitsByTarget(commitsResult.commits ?? []);
    return {
      commits: normalizeCommits(
        filterWorkingCopyCommits(split.workspaceCommits),
      ),
      tentativeWorkingCopy: commitsResult.tentative_working_copy ?? null,
      targetBranchCommits: split.targetBranchCommits,
      mergeBaseId: commitsResult.merge_base_id ?? null,
      targetBranch: commitsResult.target_branch ?? "",
      workspaceBranch: commitsResult.workspace_branch ?? "",
    };
  })();

  useEffect(() => {
    if (commitsResult) onCommitsLoaded?.(commitsResult);
  }, [commitsResult, onCommitsLoaded]);

  const dayGroups = groupCommitsByDay(commits);
  const targetDayGroups = groupCommitsByDay(targetBranchCommits);
  const isWorkspaceView = workspaceId != null;
  const isDefaultWorkspaceBranch =
    isWorkspaceView &&
    workspaceBranch.length > 0 &&
    workspaceBranch === targetBranch;
  const showWorkspaceTargetSection =
    isWorkspaceView &&
    !isDefaultWorkspaceBranch &&
    targetBranchCommits.length > 0;
  const hasDivergence = isHomeRepo && targetBranchCommits.length > 0;
  const hasTentativeWorkingCopy = tentativeWorkingCopy != null;

  if (loading) {
    return <LoadingState />;
  }

  if (
    commits.length === 0 &&
    !hasDivergence &&
    !showWorkspaceTargetSection &&
    !hasTentativeWorkingCopy
  ) {
    return (
      <div className="p-4">
        <h3 className="text-sm font-semibold text-muted-foreground mb-4">
          Commits
        </h3>
        <p className="text-sm text-muted-foreground text-center">
          No commits yet.
        </p>
        <p className="text-sm text-muted-foreground text-center">
          Changes you commit will appear here.
        </p>
      </div>
    );
  }

  let globalIndex = 0;

  return (
    <div className="h-full overflow-auto">
      <div className="p-4">
        <h3 className="text-sm font-semibold text-muted-foreground mb-4">
          Commits
        </h3>
        <div className="relative">
          <div
            className="absolute left-[7px] top-2 bottom-2 w-0.5 bg-border"
            aria-hidden="true"
          />

          {tentativeWorkingCopy && (
            <ul className="space-y-0 mb-3">
              <CommitItem
                commit={tentativeWorkingCopy.commit}
                isFirst={true}
                isTentative={true}
                tentativeWorkspaceLabel={tentativeWorkingCopy.workspace_label}
                onCommitClick={onCommitClick}
              />
            </ul>
          )}

          {dayGroups.map((group, groupIndex) => (
            <div
              key={`${group.dayKey}-${groupIndex}`}
              className="mt-5 first:mt-0"
            >
              <p className="text-xs text-muted-foreground mb-1 pl-7">
                {group.label}
              </p>
              <ul className="space-y-0">
                {group.commits.map((commit) => {
                  const isFirst = globalIndex === 0;
                  globalIndex++;
                  return (
                    <CommitItem
                      key={commit.commit_id}
                      commit={commit}
                      isFirst={isFirst}
                      onCommitClick={onCommitClick}
                    />
                  );
                })}
              </ul>
            </div>
          ))}

          {isHomeRepo && commits.length + 1 >= limit && (
            <div className="mt-3 pl-7">
              <button
                type="button"
                className="text-xs text-muted-foreground hover:text-foreground transition-colors"
                disabled={loadingMore}
                onClick={() => setLimit((prev) => prev + 14)}
              >
                {loadingMore ? (
                  <span className="flex items-center gap-1.5">
                    <Loader2 className="w-3 h-3 animate-spin" />
                    Loading...
                  </span>
                ) : (
                  "Load more commits"
                )}
              </button>
            </div>
          )}

          {hasDivergence && (
            <>
              <div className="my-4 pl-7">
                <div className="flex items-center gap-2">
                  <div className="flex-1 border-t border-dashed border-border" />
                  <span className="text-xs text-muted-foreground whitespace-nowrap px-2">
                    Diverged from{" "}
                    <span className="font-mono">{targetBranch}</span>
                    {mergeBaseId && (
                      <span className="ml-1 font-mono opacity-60">
                        @ {mergeBaseId}
                      </span>
                    )}
                  </span>
                  <div className="flex-1 border-t border-dashed border-border" />
                </div>
              </div>

              {targetDayGroups.map((group, groupIndex) => (
                <div
                  key={`target-${group.dayKey}-${groupIndex}`}
                  className="mt-5 first:mt-0"
                >
                  <p className="text-xs text-muted-foreground mb-1 pl-7">
                    {group.label}
                  </p>
                  <ul className="space-y-0">
                    {group.commits.map((commit) => (
                      <CommitItem
                        key={commit.commit_id}
                        commit={commit}
                        isFirst={false}
                        isTargetOnly={true}
                        onCommitClick={onCommitClick}
                      />
                    ))}
                  </ul>
                </div>
              ))}
            </>
          )}

          {showWorkspaceTargetSection && (
            <>
              {commits.length === 0 && (
                <p className="text-sm text-muted-foreground pl-7 mb-3">
                  There are no commits within this workspace branch yet.
                </p>
              )}
              <div className="my-4 pl-7">
                <div className="flex items-center gap-2">
                  <div className="flex-1 border-t border-dashed border-border" />
                  <span className="text-xs text-muted-foreground whitespace-nowrap px-2">
                    Recent on {targetBranch}
                  </span>
                  <div className="flex-1 border-t border-dashed border-border" />
                </div>
              </div>

              {targetDayGroups.map((group, groupIndex) => (
                <div
                  key={`target-${group.dayKey}-${groupIndex}`}
                  className="mt-5 first:mt-0"
                >
                  <p className="text-xs text-muted-foreground mb-1 pl-7">
                    {group.label}
                  </p>
                  <ul className="space-y-0">
                    {group.commits.map((commit) => (
                      <CommitItem
                        key={commit.commit_id}
                        commit={commit}
                        isFirst={false}
                        isTargetOnly={true}
                        onCommitClick={onCommitClick}
                      />
                    ))}
                  </ul>
                </div>
              ))}
            </>
          )}
        </div>
      </div>
    </div>
  );
};

interface CommitItemProps {
  commit: JjLogCommit;
  isFirst: boolean;
  isTargetOnly?: boolean;
  isTentative?: boolean;
  tentativeWorkspaceLabel?: string;
  onCommitClick?: (changeId: string) => void;
}

function CommitItem({
  commit,
  isFirst,
  isTargetOnly,
  isTentative,
  tentativeWorkspaceLabel,
  onCommitClick,
}: CommitItemProps) {
  const firstLine = getCommitHeadline(commit, isTentative ?? false);
  const hasStats = commit.insertions > 0 || commit.deletions > 0;

  return (
    <li
      className={cn(
        "relative flex items-start gap-3 py-2 px-2 -mx-2 rounded-md group transition-all duration-200 hover:bg-muted",
        isTentative &&
          "bg-yellow-50/80 hover:bg-yellow-100/80 dark:bg-yellow-950/20 dark:hover:bg-yellow-950/30 border border-yellow-200/70 dark:border-yellow-900/60",
        isTargetOnly && "opacity-60",
      )}
    >
      <div className="relative z-10 flex-shrink-0">
        <div
          className={cn(
            "w-[14px] h-[14px] rounded-full border-2 border-background",
            isFirst ? "bg-primary" : "bg-muted-foreground",
            isTentative && "bg-yellow-500",
            isTargetOnly && "bg-muted-foreground/50",
          )}
        />
      </div>

      <div
        className={cn(
          "flex-1 min-w-0 pt-0.5 rounded-md",
          isFirst &&
            !isTargetOnly &&
            "bg-accent/50 p-2 -m-2 shadow-sm border border-accent",
          isTentative &&
            "bg-yellow-50/80 p-2 -m-2 shadow-sm dark:bg-yellow-950/20",
          isTargetOnly && "bg-muted/30 p-2 -m-2 rounded-md",
          onCommitClick && "cursor-pointer hover:bg-muted/40 transition-colors",
        )}
        onClick={
          onCommitClick ? () => onCommitClick(commit.change_id) : undefined
        }
      >
        {isTentative ? (
          <div className="space-y-0.5">
            <p className="text-sm truncate" title={firstLine}>
              {firstLine}
            </p>
            {tentativeWorkspaceLabel && (
              <p className="text-sm text-muted-foreground font-mono truncate">
                {tentativeWorkspaceLabel}
              </p>
            )}
          </div>
        ) : (
          <p className="text-sm truncate" title={firstLine}>
            {firstLine}
          </p>
        )}
        <div className="flex items-center gap-2 mt-0.5 flex-wrap">
          {commit.is_immutable && (
            <span className="text-xs px-1.5 py-0.5 rounded-full bg-muted text-muted-foreground border border-border font-medium">
              Immutable
            </span>
          )}
          <TooltipProvider delay={300}>
            <Tooltip>
              <TooltipTrigger asChild>
                <span className="text-xs text-muted-foreground">
                  {formatRelativeTime(commit.timestamp)}
                </span>
              </TooltipTrigger>
              <TooltipContent>
                <p>{formatFullTimestamp(commit.timestamp)}</p>
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
          {hasStats && (
            <span className="text-xs text-muted-foreground ml-auto">
              <span className="text-green-600">+{commit.insertions}</span>{" "}
              <span className="text-red-600">-{commit.deletions}</span>
            </span>
          )}
        </div>
      </div>
    </li>
  );
}

function LoadingState() {
  return (
    <div className="h-full flex items-center justify-center p-4">
      <p className="text-sm text-muted-foreground">Loading commits...</p>
    </div>
  );
}
