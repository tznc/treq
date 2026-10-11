import { useIsMutating } from "../../../hooks/useMutation";
import { peekActiveRepository } from "../../../lib/active-repository";
import { remoteCapabilities } from "../../../lib/remote-capabilities";
import { invalidateQueries } from "../../../lib/swr-cache";
import { useState } from "react";
import {
  PushAfterCommitError,
  useCreateWorkspacePr,
} from "../../../hooks/useCreateWorkspacePr";
import {
  createPrMutationKey,
  useGitRemoteInfo,
  usePrInfoViaGh,
} from "../../../hooks/useMergeQueueStatus";
import {
  createCommit,
  getWorkspaceFileLines,
  jjRestoreAll,
  jjRestoreFile,
  jjRestoreSnapshot,
  jjSnapshotWorkingCopy,
  jjSplit,
  pushWorkspaceToRemote,
} from "../../../lib/api";
import type { Workspace } from "../../../lib/api-types";
import { deriveConventionalPrTitle } from "../../../lib/github-pr";
import { invalidateReviewChangeCount } from "../../../lib/review-change-count";
import type { useToast } from "../../ui/toast";
import type { CommitAction, DiffLineSelection, FileHunksData } from "../types";
import { computeHunkLineNumbers, parseHunkHeader } from "../../diff/hunk-lines";

interface UseFileActionsParams {
  workspacePath: string;
  repoPath: string | undefined;
  workspaceId: number | undefined;
  workspace: Workspace | null | undefined;
  baseBranch: string | undefined;
  readOnly: boolean;
  selectedUnstagedFiles: Set<string>;
  stagedFiles: Set<string>;
  setStagedFiles: React.Dispatch<React.SetStateAction<Set<string>>>;
  setSelectedUnstagedFiles: React.Dispatch<React.SetStateAction<Set<string>>>;
  allFileHunks: Map<string, FileHunksData>;
  diffLineSelection: DiffLineSelection | null;
  setContextMenuPosition: (pos: { x: number; y: number } | null) => void;
  invalidateCache: () => Promise<void>;
  loadChangedFiles: (forceApply?: boolean) => Promise<void>;
  refreshCommittedChanges: () => void;
  setCommittedSectionCollapsed: React.Dispatch<React.SetStateAction<boolean>>;
  addToast: ReturnType<typeof useToast>["addToast"];
}

export function useFileActions({
  workspacePath,
  repoPath,
  workspaceId,
  workspace,
  baseBranch,
  readOnly,
  selectedUnstagedFiles,
  stagedFiles,
  setStagedFiles,
  setSelectedUnstagedFiles,
  allFileHunks,
  diffLineSelection,
  setContextMenuPosition,
  invalidateCache,
  loadChangedFiles,
  refreshCommittedChanges,
  setCommittedSectionCollapsed,
  addToast,
}: UseFileActionsParams) {
  const [fileActionTarget, setFileActionTarget] = useState<string | null>(null);
  const [expandedContext, setExpandedContext] = useState<Map<string, string[]>>(
    new Map(),
  );
  const [localPendingAction, setPendingAction] = useState<
    "commit" | "push" | null
  >(null);
  const { data: remoteInfo } = useGitRemoteInfo(repoPath);
  const { data: prInfo } = usePrInfoViaGh(repoPath, workspace?.branch_name);
  const canCreatePr = !!remoteInfo && !prInfo && !!workspace && !!baseBranch;

  // Shared across surfaces (this dropdown item and the header's Create PR
  // button) so either one's in-flight PR creation shows as pending here too.
  const createPrActive = useIsMutating(
    createPrMutationKey(repoPath, workspaceId),
  );
  const pendingAction: CommitAction | null = createPrActive
    ? "pr"
    : localPendingAction;

  const handleUndoDiscard = async (snapshotId: string) => {
    try {
      await jjRestoreSnapshot(workspacePath, snapshotId);
      await invalidateCache();
      await loadChangedFiles(true);
      await invalidateReviewChangeCount(repoPath, workspaceId);
      addToast({
        description: "Discarded changes have been restored",
        title: "Restored",
        type: "success",
      });
    } catch (error) {
      addToast({
        description: error instanceof Error ? error.message : String(error),
        title: "Undo Failed",
        type: "error",
      });
    }
  };

  const handleDiscardAll = async () => {
    if (readOnly) return;
    try {
      const snapshotId = await jjSnapshotWorkingCopy(workspacePath);
      await jjRestoreAll(workspacePath);
      addToast({
        description: "All changes discarded",
        title: "Discarded",
        type: "success",
        action: {
          label: "Undo",
          onClick: () => handleUndoDiscard(snapshotId),
        },
      });
      await invalidateCache();
      await loadChangedFiles(true);
      await invalidateReviewChangeCount(repoPath, workspaceId);
    } catch (error) {
      addToast({
        description: error instanceof Error ? error.message : String(error),
        title: "Discard All Failed",
        type: "error",
      });
    }
  };

  const handleDiscardFiles = async (filePath: string) => {
    if (readOnly || !filePath) return;
    const filesToDiscard =
      selectedUnstagedFiles.has(filePath) && selectedUnstagedFiles.size > 0
        ? Array.from(selectedUnstagedFiles)
        : [filePath];
    setFileActionTarget(filePath);
    try {
      const snapshotId = await jjSnapshotWorkingCopy(workspacePath);
      await Promise.all(
        filesToDiscard.map((file) => jjRestoreFile(workspacePath, file)),
      );
      const count = filesToDiscard.length;
      addToast({
        description:
          count === 1
            ? `${filesToDiscard[0]} discarded`
            : `${count} files discarded`,
        title: "Discarded",
        type: "success",
        action: {
          label: "Undo",
          onClick: () => handleUndoDiscard(snapshotId),
        },
      });
      setSelectedUnstagedFiles(new Set());
      await invalidateCache();
      await loadChangedFiles(true);
      await invalidateReviewChangeCount(repoPath, workspaceId);
    } catch (error) {
      addToast({
        description: error instanceof Error ? error.message : String(error),
        title: "Discard Failed",
        type: "error",
      });
    } finally {
      setFileActionTarget(null);
    }
  };

  const handleCopyLineLocation = async () => {
    try {
      if (!diffLineSelection || diffLineSelection.lines.length === 0) return;
      const { filePath } = diffLineSelection;
      const fileData = allFileHunks.get(filePath);
      if (!fileData) return;
      let minLineNum = Infinity;
      let maxLineNum = -Infinity;
      for (const line of diffLineSelection.lines) {
        const hunk = fileData.hunks[line.hunkIndex];
        if (!hunk) continue;
        const lineNumbers = computeHunkLineNumbers(hunk);
        const lineNum =
          lineNumbers[line.lineIndex]?.new ??
          lineNumbers[line.lineIndex]?.old ??
          line.lineIndex + 1;
        minLineNum = Math.min(minLineNum, lineNum);
        maxLineNum = Math.max(maxLineNum, lineNum);
      }
      const locationStr =
        minLineNum === maxLineNum
          ? `${filePath}:${minLineNum}`
          : `${filePath}:${minLineNum}-${maxLineNum}`;
      await navigator.clipboard.writeText(locationStr);
      setContextMenuPosition(null);
      addToast({
        title: "Copied",
        description: "Line location copied to clipboard",
        type: "success",
      });
    } catch (error) {
      setContextMenuPosition(null);
      addToast({
        description: error instanceof Error ? error.message : String(error),
        title: "Failed to copy",
        type: "error",
      });
    }
  };

  const handleCopyLines = async () => {
    try {
      const lineContents =
        diffLineSelection?.lines?.map((l) => l.content).join("\n") || "";
      if (!lineContents) {
        setContextMenuPosition(null);
        return;
      }
      await navigator.clipboard.writeText(lineContents);
      setContextMenuPosition(null);
      addToast({
        title: "Copied",
        description: "Lines copied to clipboard",
        type: "success",
      });
    } catch (error) {
      setContextMenuPosition(null);
      addToast({
        description: error instanceof Error ? error.message : String(error),
        title: "Failed to copy",
        type: "error",
      });
    }
  };

  const performCommit = async (commitMsg: string): Promise<boolean> => {
    if (!commitMsg) {
      addToast({
        title: "Commit message",
        description: "Enter a commit message.",
        type: "error",
      });
      return false;
    }
    if (commitMsg.length > 500) {
      addToast({
        title: "Commit message",
        description: "Please keep the message under 500 characters.",
        type: "error",
      });
      return false;
    }
    try {
      const stagedPaths = Array.from(stagedFiles);
      let result: string;
      if (stagedPaths.length > 0) {
        const active = peekActiveRepository();
        if (
          active?.location.type === "ssh" &&
          !remoteCapabilities().splitCommit.supported
        ) {
          addToast({
            title: "Split unavailable",
            description: remoteCapabilities().splitCommit.reason,
            type: "error",
          });
          return false;
        }
        result = await jjSplit(workspacePath, commitMsg, stagedPaths);
        setStagedFiles(new Set());
      } else {
        result = await createCommit(repoPath!, workspaceId ?? null, commitMsg);
      }
      await invalidateCache();
      addToast({
        title: "Commit created",
        description: result.trim() || "Commit successful",
        type: "success",
      });
      setCommittedSectionCollapsed(true);
      // Await status refetches before reloading Review files so conflict
      // hints / sidebar indicators clear in the same turn as the commit
      // (resolve+commit must not leave a stale Conflicts section).
      await Promise.all([
        invalidateQueries(["workspace-status", repoPath, workspaceId ?? null]),
        invalidateQueries(["workspace-statuses", repoPath]),
        invalidateQueries(["workspace-commits", repoPath, workspaceId ?? null]),
        invalidateQueries(["commit-diff-viewer-commits", repoPath]),
        invalidateQueries(["linear-commits", repoPath]),
        invalidateReviewChangeCount(repoPath, workspaceId),
      ]);
      // Force-apply so an in-progress review refreshes instead of parking
      // the post-commit file list behind the stale-files banner.
      await Promise.all([loadChangedFiles(true), refreshCommittedChanges()]);
      return true;
    } catch (error) {
      addToast({
        title: "Commit failed",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
      return false;
    }
  };

  const handleCommit = async (commitMsg: string) => {
    setPendingAction("commit");
    try {
      await performCommit(commitMsg);
    } finally {
      setPendingAction(null);
    }
  };

  const handleCommitAndPush = async (commitMsg: string) => {
    setPendingAction("push");
    try {
      const committed = await performCommit(commitMsg);
      if (!committed) return;
      await pushWorkspaceToRemote(repoPath!, workspaceId ?? null);
      await invalidateQueries();
      addToast({
        title: "Pushed to remote",
        type: "success",
      });
    } catch (error) {
      addToast({
        title: "Failed to push",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    } finally {
      setPendingAction(null);
    }
  };

  const { createPr } = useCreateWorkspacePr(repoPath, workspaceId);

  const handleCommitAndCreatePR = async (commitMsg: string) => {
    if (!remoteInfo || !workspace || !baseBranch) {
      addToast({
        title: "Failed to create PR",
        description: "No GitHub remote or target branch detected",
        type: "error",
      });
      return;
    }
    await createPr({
      repoFullName: remoteInfo.full_name,
      branchName: workspace.branch_name,
      baseBranch,
      body: workspace.description ?? "",
      draft: false,
      prepare: async () => {
        // performCommit shows its own toast when it fails.
        const committed = await performCommit(commitMsg);
        if (!committed) return null;
        try {
          await pushWorkspaceToRemote(repoPath!, workspaceId ?? null);
        } catch (error) {
          throw new PushAfterCommitError(error);
        }
        return deriveConventionalPrTitle(commitMsg, workspace.branch_name);
      },
    });
  };

  const handleExpandContext = async (
    filePath: string,
    hunkIndex: number,
    direction: "before" | "after",
  ) => {
    const hunk = allFileHunks.get(filePath)?.hunks[hunkIndex];
    if (!hunk) return;
    const key = `${filePath}:${hunkIndex}:${direction}`;
    const existing = expandedContext.get(key) || [];
    const LINES_TO_FETCH = 20;
    const { newStart, newCount } = parseHunkHeader(hunk.header);
    let startLine: number;
    let endLine: number;
    if (direction === "before") {
      const hunkStartLine = newStart;
      endLine = hunkStartLine - 1 - existing.length;
      startLine = Math.max(1, endLine - LINES_TO_FETCH + 1);
      if (startLine > endLine) return;
    } else {
      const hunkEndLine = newStart + newCount - 1;
      startLine = hunkEndLine + 1 + existing.length;
      endLine = startLine + LINES_TO_FETCH - 1;
    }
    try {
      const result = await getWorkspaceFileLines(
        repoPath ?? "",
        workspaceId ?? null,
        filePath,
        false,
        startLine,
        endLine,
      );
      if (result.lines.length > 0) {
        setExpandedContext((prev) => {
          const next = new Map(prev);
          const prevLines = prev.get(key) || [];
          next.set(
            key,
            direction === "before"
              ? [...result.lines, ...prevLines]
              : [...prevLines, ...result.lines],
          );
          return next;
        });
      }
    } catch {
      /* context expansion non-critical */
    }
  };

  return {
    fileActionTarget,
    expandedContext,
    commitPending: pendingAction !== null,
    pendingAction,
    canCreatePr,
    hasPr: prInfo?.state === "OPEN",
    handleDiscardAll,
    handleDiscardFiles,
    handleCopyLineLocation,
    handleCopyLines,
    handleCommit,
    handleCommitAndPush,
    handleCommitAndCreatePR,
    handleExpandContext,
  };
}
