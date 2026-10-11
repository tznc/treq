import React, { useEffect, useRef, useState } from "react";
import useSWR from "swr";
import { useDebounce } from "../../../hooks/useDebounce";
import {
  type ConflictRegion,
  clearPendingReview,
  loadPendingReview,
  markFileViewed,
  savePendingReview,
  unmarkFileViewed,
} from "../../../lib/api";
import type { ParsedFileChange } from "../../../lib/git-utils";
import { shouldWritePendingReview } from "../../../lib/should-write-pending-review";
import { setQueryData } from "../../../lib/swr-cache";
import {
  FILE_COMMENT_HUNK_ID,
  formatReviewMarkdown as formatReviewMarkdownShared,
} from "../../../lib/review";
import type { useToast } from "../../ui/toast";
import type { ConflictComment, FileHunksData, LineComment } from "../types";
import {
  computeHunksHash,
  toApiLineComment,
  toLocalLineComment,
} from "../utils";

interface UseReviewParams {
  comments: LineComment[];
  setComments: (comments: LineComment[]) => void;
  conflictComments: Map<string, ConflictComment>;
  setConflictComments: (comments: Map<string, ConflictComment>) => void;
  setHasUserAddedComments: (v: boolean) => void;
  allFileHunks: Map<string, FileHunksData>;
  setAllFileHunks: React.Dispatch<
    React.SetStateAction<Map<string, FileHunksData>>
  >;
  files: ParsedFileChange[];
  workspacePath: string;
  repoPath: string | undefined;
  workspaceId: number | undefined;
  conflictRegionsByFile: Map<string, ConflictRegion[]>;
  actualConflictedFiles: string[];
  setCollapsedFiles: React.Dispatch<React.SetStateAction<Set<string>>>;
  // Filled with applyChangedFiles once isInReviewMode is known
  applyChangedFilesRef: React.MutableRefObject<
    (parsed: ParsedFileChange[], forceApply?: boolean) => void
  >;
  isReloadingRef: React.MutableRefObject<boolean>;
  onCreateAgentWithReview:
    | ((
        review: string,
        mode: "plan" | "acceptEdits",
        useIdleAgent?: boolean,
      ) => Promise<void>)
    | undefined;
  onReviewSubmitted: (() => void) | undefined;
  addToast: ReturnType<typeof useToast>["addToast"];
}

export function useReview({
  comments,
  setComments,
  conflictComments,
  setConflictComments,
  setHasUserAddedComments,
  allFileHunks,
  setAllFileHunks,
  files,
  workspacePath,
  repoPath,
  workspaceId,
  conflictRegionsByFile,
  actualConflictedFiles,
  setCollapsedFiles,
  applyChangedFilesRef,
  isReloadingRef,
  onCreateAgentWithReview,
  onReviewSubmitted,
  addToast,
}: UseReviewParams) {
  const [reviewPopoverOpen, setReviewPopoverOpen] = useState(false);
  const [finalReviewComment, setFinalReviewComment] = useState("");
  const [showCancelDialog, setShowCancelDialog] = useState(false);
  const [copiedReview, setCopiedReview] = useState(false);
  const [sendingReview, setSendingReview] = useState(false);
  const [staleFiles, setStaleFiles] = useState<Set<string>>(new Set());
  const [pendingFilesData, setPendingFilesData] = useState<
    ParsedFileChange[] | null
  >(null);
  const [pendingHunksData, setPendingHunksData] = useState<Map<
    string,
    FileHunksData
  > | null>(null);
  const [viewedFiles, setViewedFiles] = useState<
    Map<string, { viewedAt: string; contentHash: string }>
  >(new Map());

  const pendingReviewKey =
    repoPath && workspaceId !== undefined ? `${repoPath}:${workspaceId}` : null;
  const hydratedReviewKeyRef = useRef<string | null>(null);
  if (
    pendingReviewKey &&
    hydratedReviewKeyRef.current !== null &&
    hydratedReviewKeyRef.current !== pendingReviewKey
  ) {
    // New workspace only — do not reset when workspaceId briefly goes undefined.
    hydratedReviewKeyRef.current = null;
  }

  const { data: pendingReview, isLoading: pendingReviewLoading } = useSWR(
    repoPath && workspaceId !== undefined
      ? ["pending-review", repoPath, workspaceId]
      : null,
    () => loadPendingReview(repoPath!, workspaceId!),
  );

  useEffect(() => {
    if (!pendingReviewKey || pendingReviewLoading) return;
    if (hydratedReviewKeyRef.current === pendingReviewKey) return;
    hydratedReviewKeyRef.current = pendingReviewKey;
    const loaded = pendingReview?.comments ?? [];
    const loadedConflictComments = pendingReview?.conflict_comments ?? [];
    if (loaded.length === 0 && loadedConflictComments.length === 0) return;
    if (loaded.length > 0) setComments(loaded.map(toLocalLineComment));
    if (loadedConflictComments.length > 0) {
      setConflictComments(
        new Map(loadedConflictComments.map((c) => [c.conflictId, c])),
      );
    }
    if (pendingReview?.summary_text)
      setFinalReviewComment(pendingReview.summary_text);
    setHasUserAddedComments(true);
  }, [
    pendingReview,
    pendingReviewLoading,
    pendingReviewKey,
    setComments,
    setConflictComments,
    setFinalReviewComment,
    setHasUserAddedComments,
  ]);

  const debouncedComments = useDebounce(comments, 500);
  const debouncedSummary = useDebounce(finalReviewComment, 500);
  const debouncedConflictComments = useDebounce(conflictComments, 500);

  const persistSuppressedRef = useRef(false);
  const persistGenRef = useRef(0);

  useEffect(() => {
    if (!repoPath || workspaceId === undefined) return;
    if (
      comments.length > 0 ||
      finalReviewComment.trim() ||
      conflictComments.size > 0
    ) {
      persistSuppressedRef.current = false;
    }
    if (persistSuppressedRef.current) return;
    if (
      !shouldWritePendingReview({
        liveCommentCount: comments.length,
        liveSummary: finalReviewComment,
        debouncedCommentCount: debouncedComments.length,
        debouncedSummary,
        liveConflictCommentCount: conflictComments.size,
        debouncedConflictCommentCount: debouncedConflictComments.size,
      })
    ) {
      return;
    }
    const gen = persistGenRef.current;
    void savePendingReview(
      repoPath,
      workspaceId,
      debouncedComments.map(toApiLineComment),
      undefined,
      debouncedSummary.trim() || undefined,
      Array.from(debouncedConflictComments.values()),
    ).then(async () => {
      if (persistSuppressedRef.current || gen !== persistGenRef.current) {
        await clearPendingReview(repoPath, workspaceId);
        await setQueryData(["pending-review", repoPath, workspaceId], null);
      }
    });
  }, [
    repoPath,
    workspaceId,
    comments.length,
    finalReviewComment,
    conflictComments.size,
    debouncedComments,
    debouncedSummary,
    debouncedConflictComments,
  ]);

  useEffect(() => {
    if (files.length === 0) return;
    const currentFilePaths = new Set(files.map((f) => f.path));
    const staleViewedFiles: string[] = [];
    setViewedFiles((prev) => {
      let hasChanges = false;
      const next = new Map(prev);
      for (const [filePath, viewData] of prev.entries()) {
        if (!currentFilePaths.has(filePath)) {
          next.delete(filePath);
          hasChanges = true;
          continue;
        }
        const fileData = allFileHunks.get(filePath);
        if (fileData && !fileData.isLoading && fileData.hunks.length > 0) {
          const currentHash = computeHunksHash(fileData.hunks);
          if (viewData.contentHash && currentHash !== viewData.contentHash) {
            next.delete(filePath);
            staleViewedFiles.push(filePath);
            hasChanges = true;
          }
        }
      }
      return hasChanges ? next : prev;
    });
    for (const filePath of staleViewedFiles) {
      void unmarkFileViewed(workspacePath, filePath);
    }
  }, [files, allFileHunks, workspacePath]);

  const handleMarkFileViewed = async (filePath: string) => {
    const fileData = allFileHunks.get(filePath);
    // A diff still loading holds placeholder hunks. Hashing those would
    // record a hash the real diff never matches, so the stale-content check
    // above would un-mark the file as soon as it loads. An empty hash is
    // never treated as stale.
    const contentHash =
      fileData && !fileData.isLoading && fileData.hunks.length > 0
        ? computeHunksHash(fileData.hunks)
        : "";
    const now = new Date().toISOString();
    setViewedFiles((prev) =>
      new Map(prev).set(filePath, { contentHash, viewedAt: now }),
    );
    setCollapsedFiles((prev) => new Set(prev).add(filePath));
    try {
      await markFileViewed(workspacePath, filePath, contentHash);
    } catch {
      void 0; // mark-viewed best-effort
    }
  };

  const handleUnmarkFileViewed = async (filePath: string) => {
    setViewedFiles((prev) => {
      const next = new Map(prev);
      next.delete(filePath);
      return next;
    });
    setCollapsedFiles((prev) => {
      const next = new Set(prev);
      next.delete(filePath);
      return next;
    });
    try {
      await unmarkFileViewed(workspacePath, filePath);
    } catch {
      void 0; // unmark-viewed best-effort
    }
  };

  const formatReviewMarkdown = () =>
    formatReviewMarkdownShared({
      comments,
      finalReviewComment,
      conflictComments,
      conflictRegionsByFile,
      actualConflictedFiles,
    });

  const handleRequestChanges = async (
    mode: "plan" | "acceptEdits",
    useIdleAgent = false,
  ) => {
    setSendingReview(true);
    try {
      const markdown = formatReviewMarkdown();
      if (onCreateAgentWithReview) {
        await onCreateAgentWithReview(markdown, mode, useIdleAgent);
      } else {
        addToast({
          title: "No handler provided",
          description: "onCreateAgentWithReview callback not available",
          type: "error",
        });
        return;
      }
      if (pendingFilesData)
        applyChangedFilesRef.current(pendingFilesData, true);
      if (pendingHunksData) setAllFileHunks(pendingHunksData);
      setPendingFilesData(null);
      setPendingHunksData(null);
      setStaleFiles(new Set());
      persistSuppressedRef.current = true;
      persistGenRef.current += 1;
      setComments([]);
      setConflictComments(new Map());
      setHasUserAddedComments(false);
      setFinalReviewComment("");
      setReviewPopoverOpen(false);
      if (repoPath && workspaceId !== undefined) {
        await clearPendingReview(repoPath, workspaceId);
        await setQueryData(["pending-review", repoPath, workspaceId], null);
      }
      onReviewSubmitted?.();
    } catch (error) {
      addToast({
        description: error instanceof Error ? error.message : String(error),
        title: "Failed to send review",
        type: "error",
      });
    } finally {
      setSendingReview(false);
    }
  };

  const handleCancelReview = async () => {
    try {
      persistSuppressedRef.current = true;
      persistGenRef.current += 1;
      setComments([]);
      setConflictComments(new Map());
      setHasUserAddedComments(false);
      setFinalReviewComment("");
      setShowCancelDialog(false);
      setReviewPopoverOpen(false);
      if (repoPath && workspaceId !== undefined) {
        await clearPendingReview(repoPath, workspaceId);
        await setQueryData(["pending-review", repoPath, workspaceId], null);
      }
      addToast({
        title: "Review canceled",
        description: "All comments have been discarded",
        type: "success",
      });
    } catch (error) {
      addToast({
        description: error instanceof Error ? error.message : String(error),
        title: "Failed to cancel review",
        type: "error",
      });
    }
  };

  const handleCopyReview = async () => {
    try {
      const markdown = formatReviewMarkdown();
      await navigator.clipboard.writeText(markdown);
      setCopiedReview(true);
      setTimeout(() => setCopiedReview(false), 2000);
    } catch (error) {
      addToast({
        description: error instanceof Error ? error.message : String(error),
        title: "Failed to copy",
        type: "error",
      });
    }
  };

  const handleReloadWithPendingChanges = () => {
    isReloadingRef.current = true;
    if (pendingFilesData) {
      const orphanedComments: LineComment[] = [];
      const validComments: LineComment[] = [];
      for (const comment of comments) {
        const newFileData = pendingHunksData?.get(comment.filePath);
        const fileStillExists = pendingFilesData.some(
          (f) => f.path === comment.filePath,
        );
        if (!fileStillExists || !newFileData) {
          orphanedComments.push(comment);
          continue;
        }
        if (comment.hunkId === FILE_COMMENT_HUNK_ID) {
          validComments.push(comment);
          continue;
        }
        const hunkStillExists = newFileData.hunks.some(
          (h) => h.id === comment.hunkId,
        );
        if (!hunkStillExists) {
          orphanedComments.push(comment);
          continue;
        }
        validComments.push(comment);
      }
      if (orphanedComments.length > 0) {
        addToast({
          description: `${orphanedComments.length} outdated comment${orphanedComments.length > 1 ? "s" : ""} discarded (lines changed)`,
          title: "Comments discarded",
          type: "info",
        });
      }
      setComments(validComments);
      applyChangedFilesRef.current(pendingFilesData, true);
    }
    if (pendingHunksData) {
      setAllFileHunks(pendingHunksData);
      setPendingHunksData(null);
    }
    setStaleFiles(new Set());
    setPendingFilesData(null);
    setTimeout(() => {
      isReloadingRef.current = false;
    }, 100);
  };

  return {
    reviewPopoverOpen,
    setReviewPopoverOpen,
    finalReviewComment,
    setFinalReviewComment,
    showCancelDialog,
    setShowCancelDialog,
    copiedReview,
    sendingReview,
    staleFiles,
    setStaleFiles,
    pendingFilesData,
    setPendingFilesData,
    pendingHunksData,
    setPendingHunksData,
    viewedFiles,
    formatReviewMarkdown,
    handleRequestChanges,
    handleCancelReview,
    handleCopyReview,
    handleReloadWithPendingChanges,
    handleMarkFileViewed,
    handleUnmarkFileViewed,
  };
}
