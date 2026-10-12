/* eslint-disable max-lines, max-params, max-nested-callbacks, no-await-in-loop */

import useSWR from "swr";
import { invalidateQueries } from "../lib/swr-cache";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  Archive,
  ArrowRightLeft,
  ChevronRight,
  FileText,
  GitMerge,
  Loader2,
  Pencil,
  Plus,
  Clock,
  RotateCcw,
  Trash2,
  Undo2,
} from "lucide-react";
import { Fragment, useEffect, useRef, useState } from "react";
import {
  abandonCommit,
  getCommitDiff,
  getCommitFileDiff,
  type JjDiffHunk,
  type JjFileDiff,
  type JjLogCommit,
  type JjRevisionDiff,
  listCommits,
  revertCommit,
  shiftMutableCommitsToNow,
  stashCommit,
  undoCommit,
  undoRepoOperation,
} from "../lib/api";
import { resolvableConflictedCommits } from "../lib/resolvable-conflicted-commits";
import { getLanguageFromPath } from "../lib/syntax-highlight";
import { HighlightedLine } from "./diff/HighlightedLine";
import {
  computeHunkLineNumbers,
  getLinePrefix,
  getLineTypeClass,
} from "./diff/hunk-lines";
import {
  cn,
  formatDayLabel,
  formatFullTimestamp,
  formatRelativeTime,
  getDayKey,
} from "../lib/utils";
import { CommentInput } from "./CommentInput";
import { EditCommitDescriptionDialog } from "./EditCommitDescriptionDialog";
import { EditCommitTimestampDialog } from "./EditCommitTimestampDialog";
import { ResolveConflictsDialog } from "./ResolveConflictsDialog";
import { Button } from "./ui/button";
import type { SessionCreationInfo } from "../types/sessions";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { useToast } from "./ui/toast";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "./ui/tooltip";

interface CommitDiffViewerProps {
  repoPath: string;
  workspaceId: number | null;
  scrollToCommitId?: string | null;
  onScrollComplete?: () => void;
  onCommitMoved?: () => void;
  onCommitAbandoned?: () => void;
  /** Called after a commit is stashed (e.g. open the stash browser). */
  onCommitStashed?: () => void;
  onCreateAgentWithComment?: (
    filePath: string,
    startLine: number,
    endLine: number,
    lineContent: string[],
    commentText: string,
    commitShortId: string,
    mode: "plan" | "acceptEdits",
  ) => void;
  /** Called when user wants to move a commit to a new workspace */
  onMoveCommitToNewWorkspace?: (commit: JjLogCommit) => void;
  /** Called when user wants to move a commit to an existing workspace */
  onMoveCommitToExistingWorkspace?: (commit: JjLogCommit) => void;
  /** Called when the tentative working copy should be shown in Review tab */
  onViewTentativeChanges?: () => void;
  /** Called when the tentative working copy should be discarded */
  onDeleteTentativeChanges?: () => void;
  /** Start an agent session (used by Resolve conflicts…). */
  onSessionCreated?: (session: SessionCreationInfo) => void;
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

function getCommitHeadline(commit: JjLogCommit): string {
  const headline = commit.description.split("\n")[0]?.trim();
  return headline && headline !== "(no description)"
    ? headline
    : "Working copy";
}

export const CommitDiffViewer = ({
  repoPath,
  workspaceId,
  scrollToCommitId,
  onScrollComplete,
  onCommitAbandoned,
  onCommitStashed,
  onCreateAgentWithComment,
  onMoveCommitToNewWorkspace,
  onMoveCommitToExistingWorkspace,
  onViewTentativeChanges,
  onDeleteTentativeChanges,
  onSessionCreated,
}: CommitDiffViewerProps) => {
  const isHomeRepo = workspaceId == null;
  const [removedCommitIds, setRemovedCommitIds] = useState<Set<string>>(
    new Set(),
  );
  const [hideTentativeWorkingCopy, setHideTentativeWorkingCopy] =
    useState(false);
  const [targetBranchLimit, setTargetBranchLimit] = useState(10);
  const [homeRepoLimit, setHomeRepoLimit] = useState(15);
  const [expandedCommits, setExpandedCommits] = useState<Set<string>>(
    new Set(),
  );
  const [commitDiffs, setCommitDiffs] = useState<
    Map<string, { diff: JjRevisionDiff; loading: boolean; error?: string }>
  >(new Map());
  const containerRef = useRef<HTMLDivElement>(null);
  const hideCommitTimeoutsRef = useRef<Map<string, number>>(new Map());

  // Move/abandon commit state
  const [removingCommitIds, setRemovingCommitIds] = useState<Set<string>>(
    new Set(),
  );
  const { addToast } = useToast();
  const [shiftingToNow, setShiftingToNow] = useState(false);

  // Edit description dialog state
  const [editingCommit, setEditingCommit] = useState<JjLogCommit | null>(null);
  const [timestampCommit, setTimestampCommit] = useState<JjLogCommit | null>(
    null,
  );

  const handleEditDescription = (commit: JjLogCommit) => {
    setEditingCommit(commit);
  };

  const handleEditTimestamp = (commit: JjLogCommit) => {
    setTimestampCommit(commit);
  };

  const handleDescriptionEdited = () => {
    void invalidateQueries([
      "commit-diff-viewer-commits",
      repoPath,
      workspaceId,
    ]);
  };

  const handleShiftToNow = async () => {
    if (workspaceId == null || shiftingToNow) return;
    setShiftingToNow(true);
    try {
      await shiftMutableCommitsToNow(repoPath, workspaceId);
      addToast({
        title: "Timestamps updated",
        description:
          "Mutable commits on this branch now end at the current time",
        type: "success",
      });
      handleDescriptionEdited();
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      addToast({
        title: "Failed to shift timestamps",
        description: errorMsg,
        type: "error",
      });
    } finally {
      setShiftingToNow(false);
    }
  };

  useEffect(() => {
    setTargetBranchLimit(10);
    setHomeRepoLimit(15);
    setRemovedCommitIds(new Set());
  }, [repoPath, workspaceId, isHomeRepo]);

  const {
    data: commitsResult,
    isLoading: loading,
    isValidating: isFetching,
  } = useSWR(
    repoPath
      ? [
          "commit-diff-viewer-commits",
          repoPath,
          workspaceId,
          isHomeRepo,
          targetBranchLimit,
          homeRepoLimit,
        ]
      : null,
    () => {
      if (isHomeRepo) {
        return listCommits(
          repoPath,
          null,
          true,
          targetBranchLimit > 10 ? targetBranchLimit : undefined,
          homeRepoLimit > 15 ? homeRepoLimit : undefined,
        );
      }
      return listCommits(
        repoPath,
        workspaceId,
        true,
        targetBranchLimit > 10 ? targetBranchLimit : undefined,
      );
    },
    { keepPreviousData: true },
  );
  const loadingMore = isFetching && !loading;

  const {
    commits: baseCommits,
    targetBranchCommits,
    tentativeWorkingCopy,
    tentativeWorkingCopyLabel,
    workspaceBranch,
    targetBranchCommitsBranch,
  } = (() => {
    const empty = {
      commits: [] as JjLogCommit[],
      targetBranchCommits: [] as JjLogCommit[],
      tentativeWorkingCopy: null as JjLogCommit | null,
      tentativeWorkingCopyLabel: null as string | null,
      workspaceBranch: null as string | null,
      targetBranchCommitsBranch: null as string | null,
    };
    if (!commitsResult) {
      return empty;
    }

    const { workspaceCommits, targetBranchCommits: targetOnly } =
      splitCommitsByTarget(commitsResult.commits ?? []);

    if (isHomeRepo) {
      return {
        ...empty,
        commits: normalizeCommits(workspaceCommits),
        targetBranchCommits: targetOnly,
        targetBranchCommitsBranch: commitsResult.target_branch ?? null,
      };
    }

    return {
      commits: normalizeCommits(workspaceCommits),
      targetBranchCommits: targetOnly,
      tentativeWorkingCopy:
        commitsResult.tentative_working_copy?.commit ?? null,
      tentativeWorkingCopyLabel:
        commitsResult.tentative_working_copy?.workspace_label ?? null,
      workspaceBranch: commitsResult.workspace_branch ?? null,
      targetBranchCommitsBranch: commitsResult.target_branch ?? null,
    };
  })();

  const commits = baseCommits.filter(
    (commit) => !removedCommitIds.has(commit.commit_id),
  );

  const isDefaultWorkspaceBranch =
    !isHomeRepo &&
    workspaceBranch != null &&
    targetBranchCommitsBranch != null &&
    workspaceBranch === targetBranchCommitsBranch;

  const conflictedCommits = resolvableConflictedCommits(
    [...commits, ...targetBranchCommits],
    !isHomeRepo && !isDefaultWorkspaceBranch,
  );

  const [resolveDialogOpen, setResolveDialogOpen] = useState(false);
  const [resolveChangeIds, setResolveChangeIds] = useState<string[] | null>(
    null,
  );

  const openResolveAll = () => {
    setResolveChangeIds(conflictedCommits.map((commit) => commit.change_id));
    setResolveDialogOpen(true);
  };

  const openResolveOne = (commit: JjLogCommit) => {
    setResolveChangeIds([commit.change_id]);
    setResolveDialogOpen(true);
  };

  const handleMoveToNew = (commit: JjLogCommit) => {
    onMoveCommitToNewWorkspace?.(commit);
  };

  const handleMoveToExisting = (commit: JjLogCommit) => {
    onMoveCommitToExistingWorkspace?.(commit);
  };

  const REMOVE_ANIMATION_MS = 220;

  const resolveCommitById = (commitId: string) =>
    commits.find((c) => c.commit_id === commitId) ??
    (tentativeWorkingCopy?.commit_id === commitId
      ? tentativeWorkingCopy
      : undefined);

  const loadCommitDiff = async (commitId: string): Promise<JjRevisionDiff> => {
    const commit = resolveCommitById(commitId);
    // Commit IDs resolve more consistently for per-commit diffs; keep
    // change_id as a fallback for rewritten/alternate revisions.
    const revisions = [commitId, commit?.change_id].filter(
      (value, index, values): value is string =>
        typeof value === "string" &&
        value.length > 0 &&
        values.indexOf(value) === index,
    );

    let lastError: unknown;
    for (const revision of revisions) {
      try {
        const diff = await getCommitDiff(repoPath, workspaceId, revision);
        if (
          diff.too_large_to_render ||
          diff.committed_files.length > 0 ||
          diff.hunks_by_file.length > 0
        ) {
          return diff;
        }
      } catch (error) {
        lastError = error;
      }
    }

    if (lastError) throw lastError;
    return {
      committed_files: [],
      hunks_by_file: [],
      too_large_to_render: false,
      render_block_reason: null,
    };
  };

  const loadCommitFileDiff = async (
    commitId: string,
    filePath: string,
  ): Promise<JjFileDiff> => {
    const commit = resolveCommitById(commitId);
    const revisions = [commitId, commit?.change_id].filter(
      (value, index, values): value is string =>
        typeof value === "string" &&
        value.length > 0 &&
        values.indexOf(value) === index,
    );

    let lastError: unknown;
    for (const revision of revisions) {
      try {
        return await getCommitFileDiff(
          repoPath,
          workspaceId,
          revision,
          filePath,
        );
      } catch (error) {
        lastError = error;
      }
    }

    throw lastError ?? new Error("Failed to load commit file diff");
  };

  const handleAbandon = async (commit: JjLogCommit) => {
    if (!repoPath || !workspaceId) return;

    const firstLine = commit.description.split("\n")[0] || "(no message)";
    const confirmed = await ask(
      `Abandon commit ${commit.short_id} — ${firstLine}?`,
      { title: "Delete Commit", kind: "warning" },
    );
    if (!confirmed) return;

    try {
      const operationId = await abandonCommit(
        repoPath,
        workspaceId,
        commit.change_id,
      );
      setRemovingCommitIds((prev) => new Set(prev).add(commit.commit_id));

      const previousHide = hideCommitTimeoutsRef.current.get(commit.commit_id);
      if (previousHide !== undefined) {
        window.clearTimeout(previousHide);
      }
      const hideTimeout = window.setTimeout(() => {
        hideCommitTimeoutsRef.current.delete(commit.commit_id);
        setRemovedCommitIds((prev) => new Set(prev).add(commit.commit_id));
        setRemovingCommitIds((prev) => {
          const next = new Set(prev);
          next.delete(commit.commit_id);
          return next;
        });
        onCommitAbandoned?.();
      }, REMOVE_ANIMATION_MS);
      hideCommitTimeoutsRef.current.set(commit.commit_id, hideTimeout);

      addToast({
        title: "Commit deleted",
        description: `Abandoned commit ${commit.short_id}`,
        type: "success",
        action: {
          label: "Undo",
          onClick: () => {
            void (async () => {
              try {
                const pendingHide = hideCommitTimeoutsRef.current.get(
                  commit.commit_id,
                );
                if (pendingHide !== undefined) {
                  window.clearTimeout(pendingHide);
                  hideCommitTimeoutsRef.current.delete(commit.commit_id);
                }
                await undoRepoOperation(repoPath, workspaceId, operationId);
                setRemovedCommitIds((prev) => {
                  const next = new Set(prev);
                  next.delete(commit.commit_id);
                  return next;
                });
                setRemovingCommitIds((prev) => {
                  const next = new Set(prev);
                  next.delete(commit.commit_id);
                  return next;
                });
                await invalidateQueries([
                  "commit-diff-viewer-commits",
                  repoPath,
                  workspaceId,
                ]);
                addToast({
                  title: "Restored",
                  description: `Restored commit ${commit.short_id}`,
                  type: "success",
                });
              } catch (undoErr) {
                addToast({
                  title: "Undo Failed",
                  description:
                    undoErr instanceof Error
                      ? undoErr.message
                      : String(undoErr),
                  type: "error",
                });
              }
            })();
          },
        },
      });
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      addToast({
        title: "Failed to delete commit",
        description: errorMsg,
        type: "error",
      });
    }
  };

  const handleStashCommit = async (commit: JjLogCommit) => {
    if (!repoPath) return;

    try {
      const entry = await stashCommit(repoPath, workspaceId, commit.change_id);
      setRemovingCommitIds((prev) => new Set(prev).add(commit.commit_id));
      onCommitStashed?.();

      window.setTimeout(() => {
        setRemovedCommitIds((prev) => new Set(prev).add(commit.commit_id));
        setRemovingCommitIds((prev) => {
          const next = new Set(prev);
          next.delete(commit.commit_id);
          return next;
        });
        onCommitAbandoned?.();
      }, REMOVE_ANIMATION_MS);

      void invalidateQueries(["stashes", repoPath]);
      void invalidateQueries([
        "commit-diff-viewer-commits",
        repoPath,
        workspaceId,
      ]);

      addToast({
        title: "Commit stashed",
        description: `Parked ${entry.short_commit_id} — apply it onto another workspace from Stashed Changes`,
        type: "success",
      });
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      addToast({
        title: "Failed to stash commit",
        description: errorMsg,
        type: "error",
      });
    }
  };

  const handleUndo = async (commit: JjLogCommit) => {
    if (!repoPath || !workspaceId) return;

    const firstLine = commit.description.split("\n")[0] || "(no message)";
    const confirmed = await ask(
      `Undo commit ${commit.short_id} — ${firstLine}? Its changes will be returned to your working copy.`,
      { title: "Undo Commit", kind: "warning" },
    );
    if (!confirmed) return;

    try {
      await undoCommit(repoPath, workspaceId, commit.change_id);
      setRemovingCommitIds((prev) => new Set(prev).add(commit.commit_id));

      window.setTimeout(() => {
        setRemovedCommitIds((prev) => new Set(prev).add(commit.commit_id));
        setRemovingCommitIds((prev) => {
          const next = new Set(prev);
          next.delete(commit.commit_id);
          return next;
        });
        onCommitAbandoned?.();
      }, REMOVE_ANIMATION_MS);

      addToast({
        title: "Commit undone",
        description: `Undid commit ${commit.short_id}`,
        type: "success",
      });
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      addToast({
        title: "Failed to undo commit",
        description: errorMsg,
        type: "error",
      });
    }
  };

  const handleRevert = async (commit: JjLogCommit) => {
    if (!repoPath || !workspaceId) return;

    const firstLine = commit.description.split("\n")[0] || "(no message)";
    const confirmed = await ask(
      `Revert commit ${commit.short_id} — ${firstLine}? This creates a new commit that reverses its changes.`,
      { title: "Revert Commit", kind: "warning" },
    );
    if (!confirmed) return;

    try {
      await revertCommit(repoPath, workspaceId, commit.change_id);
      void invalidateQueries([
        "commit-diff-viewer-commits",
        repoPath,
        workspaceId,
      ]);
      onCommitAbandoned?.();

      addToast({
        title: "Commit reverted",
        description: `Reverted commit ${commit.short_id}`,
        type: "success",
      });
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      addToast({
        title: "Failed to revert commit",
        description: errorMsg,
        type: "error",
      });
    }
  };

  // Scroll to commit when scrollToCommitId changes
  useEffect(() => {
    if (!scrollToCommitId || loading) return;
    // Find the commit by change_id to get its commit_id for the key
    const commit =
      commits.find((c) => c.change_id === scrollToCommitId) ??
      (tentativeWorkingCopy?.change_id === scrollToCommitId
        ? tentativeWorkingCopy
        : undefined);
    if (!commit) return;
    const el = containerRef.current?.querySelector(
      `[data-commit-id="${commit.commit_id}"]`,
    );
    if (el) {
      el.scrollIntoView({ behavior: "smooth", block: "start" });
      // Auto-expand the commit
      fetchAndExpand(commit.commit_id);
      onScrollComplete?.();
    }
  }, [scrollToCommitId, loading, commits, tentativeWorkingCopy]);

  const fetchAndExpand = (commitId: string) => {
    setExpandedCommits((prev) => {
      if (prev.has(commitId)) return prev;
      const next = new Set(prev);
      next.add(commitId);
      return next;
    });
    // Fetch diff if not already loaded
    if (!commitDiffs.has(commitId)) {
      setCommitDiffs((prev) => {
        const next = new Map(prev);
        next.set(commitId, {
          diff: {
            committed_files: [],
            hunks_by_file: [],
            too_large_to_render: false,
            render_block_reason: null,
          },
          loading: true,
        });
        return next;
      });
      loadCommitDiff(commitId)
        .then((diff) => {
          setCommitDiffs((prev) => {
            const next = new Map(prev);
            next.set(commitId, { diff, loading: false });
            return next;
          });
        })
        .catch((err) => {
          setCommitDiffs((prev) => {
            const next = new Map(prev);
            next.set(commitId, {
              diff: {
                committed_files: [],
                hunks_by_file: [],
                too_large_to_render: false,
                render_block_reason: null,
              },
              loading: false,
              error: String(err),
            });
            return next;
          });
        });
    }
  };

  const toggleCommit = (commitId: string) => {
    setExpandedCommits((prev) => {
      const next = new Set(prev);
      if (next.has(commitId)) {
        next.delete(commitId);
      } else {
        next.add(commitId);
        // Fetch diff if not already loaded
        if (!commitDiffs.has(commitId)) {
          setCommitDiffs((prev) => {
            const next = new Map(prev);
            next.set(commitId, {
              diff: {
                committed_files: [],
                hunks_by_file: [],
                too_large_to_render: false,
                render_block_reason: null,
              },
              loading: true,
            });
            return next;
          });
          loadCommitDiff(commitId)
            .then((diff) => {
              setCommitDiffs((prev) => {
                const next = new Map(prev);
                next.set(commitId, { diff, loading: false });
                return next;
              });
            })
            .catch((err) => {
              setCommitDiffs((prev) => {
                const next = new Map(prev);
                next.set(commitId, {
                  diff: {
                    committed_files: [],
                    hunks_by_file: [],
                    too_large_to_render: false,
                    render_block_reason: null,
                  },
                  loading: false,
                  error: String(err),
                });
                return next;
              });
            });
        }
      }
      return next;
    });
  };

  const dayGroups = groupCommitsByDay(commits);

  const targetBranchDayGroups = groupCommitsByDay(targetBranchCommits);
  const showTargetBranchSection =
    !isDefaultWorkspaceBranch && targetBranchCommits.length > 0;
  const showTentativeWorkingCopy = !isHomeRepo && tentativeWorkingCopy != null;
  const hasMutableWorkspaceCommits = commits.some(
    (commit) =>
      !commit.is_immutable && !commit.on_target_only && !commit.is_working_copy,
  );

  useEffect(() => {
    setHideTentativeWorkingCopy(false);
  }, [tentativeWorkingCopy?.commit_id]);

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center p-4">
        <Loader2 className="w-4 h-4 animate-spin mr-2" />
        <p className="text-muted-foreground">Loading commits...</p>
      </div>
    );
  }

  if (
    commits.length === 0 &&
    !showTargetBranchSection &&
    !showTentativeWorkingCopy
  ) {
    return (
      <div className="p-4 text-center">
        <p className="text-muted-foreground">No commits yet.</p>
        <p className="text-muted-foreground">
          Changes you commit will appear here.
        </p>
      </div>
    );
  }

  let globalIndex = 0;

  return (
    <>
      <div ref={containerRef} className="h-full overflow-auto">
        <div className="p-4">
          {workspaceId != null && hasMutableWorkspaceCommits && (
            <div className="mb-3 flex justify-end">
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="gap-1.5"
                onClick={handleShiftToNow}
                disabled={shiftingToNow}
                data-testid="shift-commits-to-now"
              >
                {shiftingToNow ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Clock className="h-4 w-4" />
                )}
                Shift to now
              </Button>
            </div>
          )}
          {conflictedCommits.length > 0 && (
            <div
              className="mb-4 flex items-center justify-between gap-3 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2"
              data-testid="resolve-conflicts-banner"
              role="alert"
            >
              <div className="min-w-0">
                <p className="text-sm font-medium text-destructive">
                  {conflictedCommits.length} conflicted commit
                  {conflictedCommits.length === 1 ? "" : "s"}
                </p>
                <p className="text-xs text-muted-foreground">
                  Resolve in place via a short-lived resolve workspace — no
                  extra commit.
                </p>
              </div>
              <Button
                type="button"
                size="sm"
                variant="destructive"
                className="shrink-0 gap-1.5"
                onClick={openResolveAll}
                data-testid="resolve-conflicts-button"
              >
                <GitMerge className="h-4 w-4" />
                Resolve conflicts…
              </Button>
            </div>
          )}
          <div className="relative">
            <div
              className="absolute left-[7px] top-2 bottom-2 w-0.5 bg-border"
              aria-hidden="true"
            />

            {showTentativeWorkingCopy &&
              tentativeWorkingCopy &&
              !hideTentativeWorkingCopy && (
                <ul className="space-y-0 mb-3">
                  <CommitWithDiff
                    commit={tentativeWorkingCopy}
                    isFirst={true}
                    isExpanded={expandedCommits.has(
                      tentativeWorkingCopy.commit_id,
                    )}
                    isTentative={true}
                    tentativeWorkspaceLabel={tentativeWorkingCopyLabel}
                    diffData={commitDiffs.get(tentativeWorkingCopy.commit_id)}
                    canAction={true}
                    isRemoving={false}
                    onToggle={() =>
                      toggleCommit(tentativeWorkingCopy.commit_id)
                    }
                    onMoveToNew={handleMoveToNew}
                    onMoveToExisting={handleMoveToExisting}
                    onAbandon={handleAbandon}
                    onStash={handleStashCommit}
                    onEditDescription={() => {}}
                    onEditTimestamp={() => {}}
                    onCreateAgentWithComment={onCreateAgentWithComment}
                    onLoadDeferredFileDiff={(filePath) =>
                      loadCommitFileDiff(
                        tentativeWorkingCopy.commit_id,
                        filePath,
                      ).then((fileDiff) => {
                        setCommitDiffs((prev) => {
                          const next = new Map(prev);
                          const current = next.get(
                            tentativeWorkingCopy.commit_id,
                          );
                          if (!current) return prev;
                          next.set(tentativeWorkingCopy.commit_id, {
                            ...current,
                            diff: {
                              ...current.diff,
                              hunks_by_file: [
                                ...current.diff.hunks_by_file.filter(
                                  (candidate) =>
                                    candidate.path !== fileDiff.path,
                                ),
                                fileDiff,
                              ],
                            },
                          });
                          return next;
                        });
                      })
                    }
                    onViewTentativeChanges={onViewTentativeChanges}
                    onDeleteTentativeChanges={async () => {
                      await onDeleteTentativeChanges?.();
                      setHideTentativeWorkingCopy(true);
                    }}
                  />
                </ul>
              )}

            {dayGroups.map((group, groupIndex) => (
              <div
                key={`${group.dayKey}-${groupIndex}`}
                className="mt-5 first:mt-0"
              >
                <p className="text-base text-muted-foreground mb-1 pl-7">
                  {group.label}
                </p>
                <div className="space-y-0">
                  {group.commits.map((commit) => {
                    const isFirst = globalIndex === 0;
                    globalIndex++;
                    const isExpanded = expandedCommits.has(commit.commit_id);
                    const diffData = commitDiffs.get(commit.commit_id);

                    return (
                      <CommitWithDiff
                        key={commit.commit_id}
                        commit={commit}
                        isFirst={isFirst}
                        isExpanded={isExpanded}
                        diffData={diffData}
                        onToggle={() => toggleCommit(commit.commit_id)}
                        canAction={!commit.is_immutable}
                        isRemoving={removingCommitIds.has(commit.commit_id)}
                        onMoveToNew={handleMoveToNew}
                        onMoveToExisting={handleMoveToExisting}
                        onAbandon={handleAbandon}
                        onStash={handleStashCommit}
                        onUndo={handleUndo}
                        onRevert={handleRevert}
                        onEditDescription={handleEditDescription}
                        onEditTimestamp={handleEditTimestamp}
                        onResolveConflict={
                          onSessionCreated &&
                          conflictedCommits.some(
                            (candidate) =>
                              candidate.change_id === commit.change_id,
                          )
                            ? openResolveOne
                            : undefined
                        }
                        onCreateAgentWithComment={onCreateAgentWithComment}
                        onLoadDeferredFileDiff={(filePath) =>
                          loadCommitFileDiff(commit.commit_id, filePath).then(
                            (fileDiff) => {
                              setCommitDiffs((prev) => {
                                const next = new Map(prev);
                                const current = next.get(commit.commit_id);
                                if (!current) return prev;
                                next.set(commit.commit_id, {
                                  ...current,
                                  diff: {
                                    ...current.diff,
                                    hunks_by_file: [
                                      ...current.diff.hunks_by_file.filter(
                                        (candidate) =>
                                          candidate.path !== fileDiff.path,
                                      ),
                                      fileDiff,
                                    ],
                                  },
                                });
                                return next;
                              });
                            },
                          )
                        }
                      />
                    );
                  })}
                </div>
              </div>
            ))}

            {isHomeRepo && commits.length + 1 >= homeRepoLimit && (
              <div className="mt-3 pl-7">
                <button
                  type="button"
                  className="text-base text-muted-foreground hover:text-foreground transition-colors"
                  disabled={loadingMore}
                  onClick={() => setHomeRepoLimit((prev) => prev + 15)}
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

            {showTargetBranchSection && (
              <>
                {commits.length === 0 && (
                  <p className="text-muted-foreground mb-3 pl-7">
                    {isHomeRepo
                      ? "There are no commits on this branch yet."
                      : "There are no commits within this workspace branch yet."}
                  </p>
                )}
                <div className="border-t border-border my-4 mx-2" />
                <p className="text-base font-semibold text-muted-foreground mb-2 pl-7">
                  Recent on {targetBranchCommitsBranch}
                </p>
                {targetBranchDayGroups.map((group, groupIndex) => (
                  <div
                    key={`tb-${group.dayKey}-${groupIndex}`}
                    className="mt-5 first:mt-0"
                  >
                    <p className="text-base text-muted-foreground mb-1 pl-7">
                      {group.label}
                    </p>
                    <div className="space-y-0">
                      {group.commits.map((commit) => {
                        const isExpanded = expandedCommits.has(
                          commit.commit_id,
                        );
                        const diffData = commitDiffs.get(commit.commit_id);

                        return (
                          <CommitWithDiff
                            key={commit.commit_id}
                            commit={commit}
                            isFirst={false}
                            isExpanded={isExpanded}
                            diffData={diffData}
                            onToggle={() => toggleCommit(commit.commit_id)}
                            canAction={false}
                            isRemoving={false}
                            onMoveToNew={() => {}}
                            onMoveToExisting={() => {}}
                            onAbandon={() => {}}
                            onStash={() => {}}
                            onRevert={handleRevert}
                            onEditDescription={() => {}}
                            onEditTimestamp={() => {}}
                            onCreateAgentWithComment={onCreateAgentWithComment}
                            onLoadDeferredFileDiff={(filePath) =>
                              loadCommitFileDiff(
                                commit.commit_id,
                                filePath,
                              ).then((fileDiff) => {
                                setCommitDiffs((prev) => {
                                  const next = new Map(prev);
                                  const current = next.get(commit.commit_id);
                                  if (!current) return prev;
                                  next.set(commit.commit_id, {
                                    ...current,
                                    diff: {
                                      ...current.diff,
                                      hunks_by_file: [
                                        ...current.diff.hunks_by_file.filter(
                                          (candidate) =>
                                            candidate.path !== fileDiff.path,
                                        ),
                                        fileDiff,
                                      ],
                                    },
                                  });
                                  return next;
                                });
                              })
                            }
                          />
                        );
                      })}
                    </div>
                  </div>
                ))}
                {targetBranchCommits.length >= targetBranchLimit && (
                  <div className="mt-3 pl-7">
                    <button
                      type="button"
                      className="text-base text-muted-foreground hover:text-foreground transition-colors"
                      disabled={loadingMore}
                      onClick={() => setTargetBranchLimit((prev) => prev + 10)}
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
              </>
            )}
          </div>
        </div>
      </div>
      {workspaceId != null && (
        <EditCommitDescriptionDialog
          open={editingCommit != null}
          onOpenChange={(open) => {
            if (!open) setEditingCommit(null);
          }}
          repoPath={repoPath}
          workspaceId={workspaceId}
          commit={editingCommit}
          onSuccess={handleDescriptionEdited}
        />
      )}
      {workspaceId != null && (
        <EditCommitTimestampDialog
          open={timestampCommit != null}
          onOpenChange={(open) => {
            if (!open) setTimestampCommit(null);
          }}
          repoPath={repoPath}
          workspaceId={workspaceId}
          commit={timestampCommit}
          onSuccess={handleDescriptionEdited}
        />
      )}
      {onSessionCreated && (
        <ResolveConflictsDialog
          open={resolveDialogOpen}
          onOpenChange={setResolveDialogOpen}
          repoPath={repoPath}
          workspaceId={workspaceId}
          changeIds={resolveChangeIds}
          onSessionCreated={onSessionCreated}
        />
      )}
    </>
  );
};

// --- Sub-components ---

interface CommitWithDiffProps {
  commit: JjLogCommit;
  isFirst: boolean;
  isExpanded: boolean;
  isTentative?: boolean;
  tentativeWorkspaceLabel?: string | null;
  diffData?: {
    diff: JjRevisionDiff;
    loading: boolean;
    error?: string;
  };
  onToggle: () => void;
  canAction: boolean;
  isRemoving: boolean;
  onMoveToNew: (commit: JjLogCommit) => void;
  onMoveToExisting: (commit: JjLogCommit) => void;
  onAbandon: (commit: JjLogCommit) => void;
  onStash: (commit: JjLogCommit) => void;
  /** Undo the latest commit in the workspace's own lineage. Only valid when `isFirst`. */
  onUndo?: (commit: JjLogCommit) => void;
  /** Revert any commit (except the working copy) by creating a new commit that reverses it. */
  onRevert?: (commit: JjLogCommit) => void;
  onEditDescription: (commit: JjLogCommit) => void;
  onEditTimestamp: (commit: JjLogCommit) => void;
  onResolveConflict?: (commit: JjLogCommit) => void;
  onViewTentativeChanges?: () => void;
  onDeleteTentativeChanges?: () => void;
  onCreateAgentWithComment?: (
    filePath: string,
    startLine: number,
    endLine: number,
    lineContent: string[],
    commentText: string,
    commitShortId: string,
    mode: "plan" | "acceptEdits",
  ) => void;
  onLoadDeferredFileDiff: (filePath: string) => Promise<void>;
}

function CommitWithDiff({
  commit,
  isFirst,
  isExpanded,
  isTentative,
  diffData,
  onToggle,
  isRemoving,
  onMoveToNew,
  onMoveToExisting,
  onAbandon,
  onStash,
  onUndo,
  onRevert,
  onEditDescription,
  onEditTimestamp,
  onResolveConflict,
  onViewTentativeChanges,
  onDeleteTentativeChanges,
  onCreateAgentWithComment,
  onLoadDeferredFileDiff,
  tentativeWorkspaceLabel,
  canAction,
}: CommitWithDiffProps) {
  const firstLine = getCommitHeadline(commit);
  const hasStats = commit.insertions > 0 || commit.deletions > 0;
  const isConflicted = Boolean(commit.has_conflicts);

  const handleAgentComment = (
    filePath: string,
    startLine: number,
    endLine: number,
    lineContent: string[],
    commentText: string,
    mode: "plan" | "acceptEdits",
  ) => {
    onCreateAgentWithComment?.(
      filePath,
      startLine,
      endLine,
      lineContent,
      commentText,
      commit.short_id,
      mode,
    );
  };

  return (
    <div data-commit-id={commit.commit_id}>
      {/* Commit header row */}
      <button
        type="button"
        onClick={onToggle}
        className={cn(
          "relative flex items-start gap-3 py-2 px-2 -mx-2 w-full text-left rounded-md group transition-all duration-200 hover:bg-muted",
          isTentative &&
            "bg-yellow-50/80 hover:bg-yellow-100/80 dark:bg-yellow-950/20 dark:hover:bg-yellow-950/30",
        )}
      >
        <div className="relative z-10 flex-shrink-0">
          <div
            className={cn(
              "w-[14px] h-[14px] rounded-full border-2 border-background",
              isFirst ? "bg-primary" : "bg-muted-foreground",
              isTentative && "bg-yellow-500",
            )}
          />
        </div>

        <div className="flex-1 min-w-0 pt-0.5">
          <div className="flex items-center gap-1.5">
            <ChevronRight
              className={cn(
                "w-4 h-4 text-muted-foreground transition-transform flex-shrink-0",
                isExpanded && "rotate-90",
              )}
            />
            <div className="min-w-0">
              <p className="truncate" title={firstLine}>
                {firstLine}
                {isTentative && tentativeWorkspaceLabel ? (
                  <span className="ml-2 font-mono text-muted-foreground">
                    - {tentativeWorkspaceLabel}
                  </span>
                ) : null}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-2 mt-0.5 pl-5">
            {isConflicted && (
              <span
                className="px-1.5 py-0.5 rounded-full bg-destructive/10 text-destructive border border-destructive/30 font-medium"
                data-testid={`commit-conflict-badge-${commit.change_id}`}
              >
                Conflict
              </span>
            )}
            {commit.is_immutable && (
              <span className="px-1.5 py-0.5 rounded-full bg-muted text-muted-foreground border border-border font-medium">
                Immutable
              </span>
            )}
            <TooltipProvider delay={300}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <span className="text-base text-muted-foreground">
                    {formatRelativeTime(commit.timestamp)}
                  </span>
                </TooltipTrigger>
                <TooltipContent>
                  <p>{formatFullTimestamp(commit.timestamp)}</p>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
            {hasStats && (
              <span className="text-base text-muted-foreground ml-auto">
                <span className="text-green-600">+{commit.insertions}</span>{" "}
                <span className="text-red-600">-{commit.deletions}</span>
              </span>
            )}
          </div>
        </div>
      </button>

      {/* Expanded diff content */}
      {isExpanded && (
        <div className="ml-7 mb-3">
          <div className="flex items-center gap-2 mb-2 mt-2">
            {isTentative ? (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-1.5"
                  onClick={onViewTentativeChanges}
                >
                  <FileText className="w-4 h-4" />
                  View changes
                </Button>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button variant="outline" size="sm" className="gap-1.5">
                      <ArrowRightLeft className="w-4 h-4" />
                      Move changes
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start">
                    <DropdownMenuItem onClick={() => onMoveToNew(commit)}>
                      Move to New Workspace
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => onMoveToExisting(commit)}>
                      Move to Existing Workspace
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-1.5 border-destructive/30 text-destructive hover:bg-destructive/10 hover:text-destructive"
                  onClick={onDeleteTentativeChanges}
                  disabled={isRemoving}
                >
                  <Trash2 className="w-4 h-4" />
                  Delete changes
                </Button>
              </>
            ) : (
              <>
                {isConflicted && onResolveConflict && (
                  <Button
                    variant="destructive"
                    size="sm"
                    className="gap-1.5"
                    onClick={() => onResolveConflict(commit)}
                    data-testid={`resolve-commit-${commit.change_id}`}
                  >
                    <GitMerge className="w-4 h-4" />
                    Resolve…
                  </Button>
                )}
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-1.5"
                  onClick={() => onEditDescription(commit)}
                >
                  <Pencil className="w-4 h-4" />
                  Edit description
                </Button>
                {isFirst && onUndo && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="gap-1.5"
                    onClick={() => onUndo(commit)}
                    disabled={isRemoving}
                  >
                    <Undo2 className="w-4 h-4" />
                    Undo commit
                  </Button>
                )}
                {onRevert && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="gap-1.5"
                    onClick={() => onRevert(commit)}
                  >
                    <RotateCcw className="w-4 h-4" />
                    Revert commit
                  </Button>
                )}
                {canAction && !commit.is_immutable && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="gap-1.5"
                    onClick={() => onEditTimestamp(commit)}
                    data-testid={`edit-timestamp-${commit.change_id}`}
                  >
                    <Clock className="w-4 h-4" />
                    Edit timestamp
                  </Button>
                )}
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button variant="outline" size="sm" className="gap-1.5">
                      <ArrowRightLeft className="w-4 h-4" />
                      Move commit
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start">
                    <DropdownMenuItem onClick={() => onMoveToNew(commit)}>
                      Move to New Workspace
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => onMoveToExisting(commit)}>
                      Move to Existing Workspace
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-1.5"
                  onClick={() => onStash(commit)}
                  disabled={isRemoving}
                  data-testid="stash-commit-button"
                >
                  <Archive className="w-4 h-4" />
                  Stash commit
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-1.5 border-destructive/30 text-destructive hover:bg-destructive/10 hover:text-destructive"
                  onClick={() => onAbandon(commit)}
                  disabled={isRemoving}
                >
                  <Trash2 className="w-4 h-4" />
                  Delete commit
                </Button>
              </>
            )}
          </div>

          <div className="border border-border rounded-md overflow-hidden">
            {diffData?.loading ? (
              <div className="flex items-center gap-2 p-3 text-muted-foreground">
                <Loader2 className="w-4 h-4 animate-spin" />
                Loading diff...
              </div>
            ) : diffData?.error ? (
              <div className="p-3 text-destructive">
                Failed to load diff: {diffData.error}
              </div>
            ) : diffData?.diff ? (
              <CommitDiffContent
                diff={diffData.diff}
                onCreateAgentWithComment={
                  onCreateAgentWithComment ? handleAgentComment : undefined
                }
                onLoadDeferredFileDiff={onLoadDeferredFileDiff}
              />
            ) : null}
          </div>
        </div>
      )}
    </div>
  );
}

interface CommitDiffContentProps {
  diff: JjRevisionDiff;
  onCreateAgentWithComment?: (
    filePath: string,
    startLine: number,
    endLine: number,
    lineContent: string[],
    commentText: string,
    mode: "plan" | "acceptEdits",
  ) => void;
  onLoadDeferredFileDiff: (filePath: string) => Promise<void>;
}

interface DiffLineSelection {
  filePath: string;
  lines: { hunkIndex: number; lineIndex: number; content: string }[];
}

interface PendingComment {
  filePath: string;
  hunkIndex: number;
  displayAtLineIndex: number;
  startLine: number;
  endLine: number;
  lineContent: string[];
}

function CommitDiffContent({
  diff,
  onCreateAgentWithComment,
  onLoadDeferredFileDiff,
}: CommitDiffContentProps) {
  // Always show all files expanded
  const [expandedFiles, setExpandedFiles] = useState<Set<string>>(
    () => new Set(diff.committed_files.map((file) => file.path)),
  );
  const [loadingDeferredFiles, setLoadingDeferredFiles] = useState<Set<string>>(
    new Set(),
  );
  const [deferredFileErrors, setDeferredFileErrors] = useState<
    Map<string, string>
  >(new Map());

  // Comment/selection state
  const [diffLineSelection, setDiffLineSelection] =
    useState<DiffLineSelection | null>(null);
  const [isSelecting, setIsSelecting] = useState(false);
  const [selectionAnchor, setSelectionAnchor] = useState<{
    filePath: string;
    hunkIndex: number;
    lineIndex: number;
  } | null>(null);
  const [pendingComment, setPendingComment] = useState<PendingComment | null>(
    null,
  );
  const [showCommentInput, setShowCommentInput] = useState(false);

  // Global mouseup listener to end drag selection
  useEffect(() => {
    const handleMouseUp = () => {
      if (isSelecting) {
        setIsSelecting(false);
      }
    };
    window.addEventListener("mouseup", handleMouseUp);
    return () => window.removeEventListener("mouseup", handleMouseUp);
  }, [isSelecting]);

  if (diff.too_large_to_render) {
    return (
      <div className="p-3 text-muted-foreground">
        {diff.render_block_reason ?? "This commit diff is too large to render."}
      </div>
    );
  }

  if (diff.committed_files.length === 0) {
    return <div className="p-3 text-muted-foreground">No changes</div>;
  }

  const toggleFile = (path: string) => {
    setExpandedFiles((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  const buildSelectionRange = (
    filePath: string,
    fromHunk: number,
    fromLine: number,
    toHunk: number,
    toLine: number,
  ): { hunkIndex: number; lineIndex: number; content: string }[] | null => {
    const fileDiff = diff.hunks_by_file.find((f) => f.path === filePath);
    if (!fileDiff) return null;

    const lines: { hunkIndex: number; lineIndex: number; content: string }[] =
      [];

    const fromGlobal = getGlobalLineIndex(fileDiff.hunks, fromHunk, fromLine);
    const toGlobal = getGlobalLineIndex(fileDiff.hunks, toHunk, toLine);

    const startGlobal = Math.min(fromGlobal, toGlobal);
    const endGlobal = Math.max(fromGlobal, toGlobal);

    let globalIdx = 0;
    for (let hi = 0; hi < fileDiff.hunks.length; hi++) {
      for (let li = 0; li < fileDiff.hunks[hi].lines.length; li++) {
        if (globalIdx >= startGlobal && globalIdx <= endGlobal) {
          lines.push({
            hunkIndex: hi,
            lineIndex: li,
            content: fileDiff.hunks[hi].lines[li],
          });
        }
        globalIdx++;
      }
    }

    return lines;
  };

  const handleLineMouseDown = (
    e: React.MouseEvent,
    filePath: string,
    hunkIndex: number,
    lineIndex: number,
    lineContent: string,
  ) => {
    if (!onCreateAgentWithComment) return;
    e.preventDefault();

    // Shift-click: extend selection from existing anchor
    if (
      e.shiftKey &&
      selectionAnchor &&
      selectionAnchor.filePath === filePath
    ) {
      const lines = buildSelectionRange(
        filePath,
        selectionAnchor.hunkIndex,
        selectionAnchor.lineIndex,
        hunkIndex,
        lineIndex,
      );
      if (lines) {
        setDiffLineSelection({ filePath, lines });
      }
      // Don't reset anchor or start drag
      setShowCommentInput(false);
      setPendingComment(null);
      return;
    }

    setIsSelecting(true);
    setSelectionAnchor({ filePath, hunkIndex, lineIndex });
    setDiffLineSelection({
      filePath,
      lines: [{ hunkIndex, lineIndex, content: lineContent }],
    });
    // Reset any pending comment
    setShowCommentInput(false);
    setPendingComment(null);
  };

  const handleLineMouseEnter = (
    filePath: string,
    hunkIndex: number,
    lineIndex: number,
  ) => {
    if (
      !isSelecting ||
      !selectionAnchor ||
      selectionAnchor.filePath !== filePath
    )
      return;

    const lines = buildSelectionRange(
      filePath,
      selectionAnchor.hunkIndex,
      selectionAnchor.lineIndex,
      hunkIndex,
      lineIndex,
    );
    if (lines) {
      setDiffLineSelection({ filePath, lines });
    }
  };

  const handleLineMouseUp = () => {
    setIsSelecting(false);
  };

  const handleAddComment = (
    filePath: string,
    hunkIndex: number,
    lineIndex: number,
    lineContent: string,
    lineNum: number,
  ) => {
    if (!onCreateAgentWithComment) return;

    // If there's a multi-line selection, use that
    if (
      diffLineSelection &&
      diffLineSelection.filePath === filePath &&
      diffLineSelection.lines.length > 1
    ) {
      handleAddCommentFromSelection();
      return;
    }

    setPendingComment({
      filePath,
      hunkIndex,
      displayAtLineIndex: lineIndex,
      startLine: lineNum,
      endLine: lineNum,
      lineContent: [lineContent],
    });
    setShowCommentInput(true);
  };

  const handleAddCommentFromSelection = () => {
    if (!diffLineSelection || diffLineSelection.lines.length === 0) return;

    const { filePath } = diffLineSelection;
    const fileDiff = diff.hunks_by_file.find((f) => f.path === filePath);
    if (!fileDiff) return;

    const lineContents: string[] = [];
    let minLineNum = Infinity;
    let maxLineNum = -Infinity;
    let lastHunkIndex = 0;
    let lastLineIndex = 0;

    for (const line of diffLineSelection.lines) {
      const hunk = fileDiff.hunks[line.hunkIndex];
      if (!hunk) continue;

      const lineNumbers = computeHunkLineNumbers(hunk);
      const lineNum =
        lineNumbers[line.lineIndex]?.new ??
        lineNumbers[line.lineIndex]?.old ??
        line.lineIndex + 1;

      minLineNum = Math.min(minLineNum, lineNum);
      maxLineNum = Math.max(maxLineNum, lineNum);
      lineContents.push(line.content);
      lastHunkIndex = line.hunkIndex;
      lastLineIndex = line.lineIndex;
    }

    setPendingComment({
      filePath,
      hunkIndex: lastHunkIndex,
      displayAtLineIndex: lastLineIndex,
      startLine: minLineNum,
      endLine: maxLineNum,
      lineContent: lineContents,
    });
    setShowCommentInput(true);
  };

  const handleCommentSubmit = (text: string, mode: "plan" | "acceptEdits") => {
    if (!pendingComment || !onCreateAgentWithComment) return;

    onCreateAgentWithComment(
      pendingComment.filePath,
      pendingComment.startLine,
      pendingComment.endLine,
      pendingComment.lineContent,
      text,
      mode,
    );

    setShowCommentInput(false);
    setPendingComment(null);
    setDiffLineSelection(null);
  };

  const handleCommentCancel = () => {
    setShowCommentInput(false);
    setPendingComment(null);
  };

  const isLineSelected = (
    filePath: string,
    hunkIndex: number,
    lineIndex: number,
  ) => {
    if (!diffLineSelection || diffLineSelection.filePath !== filePath)
      return false;
    return diffLineSelection.lines.some(
      (l) => l.hunkIndex === hunkIndex && l.lineIndex === lineIndex,
    );
  };

  return (
    <div className="divide-y divide-border">
      {diff.committed_files.map((file) => {
        const fileDiff = diff.hunks_by_file.find((f) => f.path === file.path);
        const isFileExpanded = expandedFiles.has(file.path);
        const isDeferred = file.diff_deferred && !fileDiff;
        const isLoadingDeferred = loadingDeferredFiles.has(file.path);
        const deferredFileError = deferredFileErrors.get(file.path);

        return (
          <div key={file.path}>
            <button
              type="button"
              onClick={() => toggleFile(file.path)}
              className="flex items-center gap-2 px-3 py-1.5 w-full text-left hover:bg-muted/40 transition-colors"
            >
              <ChevronRight
                className={cn(
                  "w-4 h-4 text-muted-foreground transition-transform flex-shrink-0",
                  isFileExpanded && "rotate-90",
                )}
              />
              <FileText className="w-4 h-4 text-muted-foreground flex-shrink-0" />
              <span className="font-mono truncate flex-1">{file.path}</span>
              <span
                className={cn(
                  "text-base font-medium px-1.5 py-0.5 rounded",
                  file.status === "A" && "text-green-600 bg-green-500/10",
                  file.status === "M" && "text-blue-600 bg-blue-500/10",
                  file.status === "D" && "text-red-600 bg-red-500/10",
                )}
              >
                {file.status}
              </span>
            </button>

            {isFileExpanded && (
              <div className="bg-muted/20">
                {fileDiff ? (
                  fileDiff.hunks.map((hunk, hunkIndex) => (
                    <HunkView
                      key={hunk.id}
                      hunk={hunk}
                      filePath={file.path}
                      hunkIndex={hunkIndex}
                      hasCommentSupport={!!onCreateAgentWithComment}
                      isLineSelected={isLineSelected}
                      showCommentInput={showCommentInput}
                      pendingComment={pendingComment}
                      onLineMouseDown={handleLineMouseDown}
                      onLineMouseEnter={handleLineMouseEnter}
                      onLineMouseUp={handleLineMouseUp}
                      onAddComment={handleAddComment}
                      onCommentSubmit={handleCommentSubmit}
                      onCommentCancel={handleCommentCancel}
                    />
                  ))
                ) : isDeferred ? (
                  <div className="flex items-center justify-between gap-3 border-t border-border px-3 py-3">
                    <p className="text-muted-foreground">
                      This diff has {file.changed_line_count} changed lines.
                    </p>
                    <div className="flex items-center gap-3">
                      {deferredFileError && (
                        <span className="text-base text-destructive">
                          {deferredFileError}
                        </span>
                      )}
                      <button
                        type="button"
                        className="rounded-md border border-border px-2 py-1 text-base text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-60"
                        disabled={isLoadingDeferred}
                        onClick={() => {
                          setLoadingDeferredFiles((prev) => {
                            const next = new Set(prev);
                            next.add(file.path);
                            return next;
                          });
                          setDeferredFileErrors((prev) => {
                            const next = new Map(prev);
                            next.delete(file.path);
                            return next;
                          });
                          void onLoadDeferredFileDiff(file.path)
                            .catch((error) => {
                              setDeferredFileErrors((prev) => {
                                const next = new Map(prev);
                                next.set(
                                  file.path,
                                  error instanceof Error
                                    ? error.message
                                    : String(error),
                                );
                                return next;
                              });
                            })
                            .finally(() => {
                              setLoadingDeferredFiles((prev) => {
                                const next = new Set(prev);
                                next.delete(file.path);
                                return next;
                              });
                            });
                        }}
                      >
                        {isLoadingDeferred ? "Loading..." : "Load diff"}
                      </button>
                    </div>
                  </div>
                ) : null}
                {!fileDiff && !isDeferred && (
                  <div className="border-t border-border px-3 py-3 text-muted-foreground">
                    No diff content available.
                  </div>
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

// Helper to get a global line index across all hunks
function getGlobalLineIndex(
  hunks: JjDiffHunk[],
  hunkIndex: number,
  lineIndex: number,
): number {
  let idx = 0;
  for (let h = 0; h < hunkIndex; h++) {
    idx += hunks[h].lines.length;
  }
  return idx + lineIndex;
}

interface HunkViewProps {
  hunk: JjDiffHunk;
  filePath: string;
  hunkIndex: number;
  hasCommentSupport: boolean;
  isLineSelected: (
    filePath: string,
    hunkIndex: number,
    lineIndex: number,
  ) => boolean;
  showCommentInput: boolean;
  pendingComment: PendingComment | null;
  onLineMouseDown: (
    e: React.MouseEvent,
    filePath: string,
    hunkIndex: number,
    lineIndex: number,
    lineContent: string,
  ) => void;
  onLineMouseEnter: (
    filePath: string,
    hunkIndex: number,
    lineIndex: number,
  ) => void;
  onLineMouseUp: () => void;
  onAddComment: (
    filePath: string,
    hunkIndex: number,
    lineIndex: number,
    lineContent: string,
    lineNum: number,
  ) => void;
  onCommentSubmit: (text: string, mode: "plan" | "acceptEdits") => void;
  onCommentCancel: () => void;
}

function HunkView({
  hunk,
  filePath,
  hunkIndex,
  hasCommentSupport,
  isLineSelected,
  showCommentInput,
  pendingComment,
  onLineMouseDown,
  onLineMouseEnter,
  onLineMouseUp,
  onAddComment,
  onCommentSubmit,
  onCommentCancel,
}: HunkViewProps) {
  const lineNumbers = computeHunkLineNumbers(hunk);
  const language = getLanguageFromPath(filePath);

  return (
    <div className="font-mono">
      {/* Hunk header */}
      <div className="px-3 py-0.5 bg-muted/60 text-muted-foreground border-t border-border text-base">
        {hunk.header}
      </div>
      {/* Hunk lines */}
      {hunk.lines.map((line, i) => {
        const { old: oldNum, new: newNum } = lineNumbers[i] ?? {};
        const bgClass = getLineTypeClass(line);
        const prefix = getLinePrefix(line);

        const actualLineNum = newNum ?? oldNum ?? i + 1;
        const selected = isLineSelected(filePath, hunkIndex, i);
        const showCommentInputHere =
          showCommentInput &&
          pendingComment &&
          pendingComment.filePath === filePath &&
          pendingComment.hunkIndex === hunkIndex &&
          i === pendingComment.displayAtLineIndex;

        return (
          <Fragment key={i}>
            <div
              className={cn(
                "group flex",
                bgClass,
                selected && "!bg-blue-500/10",
              )}
              onMouseEnter={() => onLineMouseEnter(filePath, hunkIndex, i)}
              onMouseUp={onLineMouseUp}
            >
              {/* Line number gutter — click to start selection */}
              <div
                className={cn(
                  "w-10 text-right pr-1 text-muted-foreground/60 select-none flex-shrink-0",
                  hasCommentSupport && "cursor-pointer hover:bg-muted/50",
                )}
                onMouseDown={(e) =>
                  onLineMouseDown(e, filePath, hunkIndex, i, line)
                }
              >
                {oldNum ?? ""}
              </div>
              <div
                className={cn(
                  "w-10 text-right pr-1 text-muted-foreground/60 select-none flex-shrink-0",
                  hasCommentSupport && "cursor-pointer hover:bg-muted/50",
                )}
                onMouseDown={(e) =>
                  onLineMouseDown(e, filePath, hunkIndex, i, line)
                }
              >
                {newNum ?? ""}
              </div>
              {/* Plus button column */}
              {hasCommentSupport ? (
                <div className="w-5 flex-shrink-0 flex items-center justify-center select-none">
                  <button
                    className="p-[1px] rounded bg-primary text-primary-foreground hover:bg-primary/90 invisible group-hover:visible"
                    onMouseDown={(e) => e.stopPropagation()}
                    onClick={(e) => {
                      e.stopPropagation();
                      onAddComment(filePath, hunkIndex, i, line, actualLineNum);
                    }}
                    title="Add comment"
                  >
                    <Plus className="w-3 h-3" />
                  </button>
                </div>
              ) : (
                <span className="w-4 text-center text-muted-foreground/60 select-none flex-shrink-0">
                  {prefix}
                </span>
              )}
              {/* Line prefix when comment support is enabled */}
              {hasCommentSupport && (
                <span className="w-4 text-center text-muted-foreground/60 select-none flex-shrink-0">
                  {prefix}
                </span>
              )}
              <span className="flex-1 whitespace-pre overflow-x-auto">
                <HighlightedLine content={line.slice(1)} language={language} />
              </span>
            </div>

            {/* Comment input */}
            {showCommentInputHere && pendingComment && (
              <CommentInput
                key={`comment-${pendingComment.filePath}-${hunkIndex}-${pendingComment.displayAtLineIndex}`}
                onSubmit={() => {}}
                onSubmitWithMode={onCommentSubmit}
                onCancel={onCommentCancel}
                filePath={pendingComment.filePath}
                startLine={pendingComment.startLine}
                endLine={pendingComment.endLine}
              />
            )}
          </Fragment>
        );
      })}
    </div>
  );
}
