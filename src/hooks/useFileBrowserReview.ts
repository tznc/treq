import { useState } from "react";
import useSWR from "swr";
import { v4 as uuidv4 } from "uuid";
import {
  clearFileBrowserReview,
  loadFileBrowserReview,
  saveFileBrowserReview,
} from "../lib/api-extra";
import { shouldWritePendingReview } from "../lib/should-write-pending-review";
import { setQueryData } from "../lib/swr-cache";
import {
  formatReviewMarkdown,
  type LineComment,
  type PendingComment,
  toApiLineComment,
  toLocalLineComment,
} from "../lib/review";
import type { useToast } from "../components/ui/toast";
import { useDebounce } from "./useDebounce";

interface UseFileBrowserReviewParams {
  repoPath: string | null;
  workspaceId: number | undefined;
  onCreateAgentWithReview:
    | ((
        review: string,
        mode: "plan" | "acceptEdits",
        useIdleAgent?: boolean,
      ) => Promise<void>)
    | undefined;
  addToast: ReturnType<typeof useToast>["addToast"];
}

/**
 * Owns the FileBrowser's own review session (comments accumulated while browsing
 * arbitrary files, batched and sent once). Kept independent from the Review/Changes
 * tab's session — see file_browser_reviews in local_db.rs.
 */
export function useFileBrowserReview({
  repoPath,
  workspaceId,
  onCreateAgentWithReview,
  addToast,
}: UseFileBrowserReviewParams) {
  const loadKey =
    repoPath && workspaceId !== undefined ? `${repoPath}:${workspaceId}` : null;
  const { data: saved } = useSWR(
    loadKey ? ["file-browser-review", repoPath, workspaceId] : null,
    () => loadFileBrowserReview(repoPath!, workspaceId!),
  );
  const loadedComments = saved?.comments.map(toLocalLineComment) ?? [];
  const loadedSummary = saved?.summary_text ?? "";

  const [draft, setDraft] = useState<{
    key: string;
    comments: LineComment[];
    summary: string;
  } | null>(null);

  const comments =
    draft && loadKey && draft.key === loadKey ? draft.comments : loadedComments;
  const finalReviewComment =
    draft && loadKey && draft.key === loadKey ? draft.summary : loadedSummary;

  const setComments = (
    next: LineComment[] | ((prev: LineComment[]) => LineComment[]),
  ) => {
    if (!loadKey) return;
    setDraft((prev) => {
      const current = prev?.key === loadKey ? prev.comments : loadedComments;
      const commentsNext = typeof next === "function" ? next(current) : next;
      const summary = prev?.key === loadKey ? prev.summary : loadedSummary;
      return { key: loadKey, comments: commentsNext, summary };
    });
  };

  const setFinalReviewComment = (summary: string) => {
    if (!loadKey) return;
    setDraft((prev) => ({
      key: loadKey,
      comments: prev?.key === loadKey ? prev.comments : loadedComments,
      summary,
    }));
  };

  const [editingCommentId, setEditingCommentId] = useState<string | null>(null);
  const [reviewPopoverOpen, setReviewPopoverOpen] = useState(false);
  const [showCancelDialog, setShowCancelDialog] = useState(false);
  const [copiedReview, setCopiedReview] = useState(false);
  const [sendingReview, setSendingReview] = useState(false);

  const debouncedComments = useDebounce(comments, 500);
  const debouncedSummary = useDebounce(finalReviewComment, 500);

  useSWR(
    loadKey &&
      shouldWritePendingReview({
        liveCommentCount: comments.length,
        liveSummary: finalReviewComment,
        debouncedCommentCount: debouncedComments.length,
        debouncedSummary,
      })
      ? [
          "file-browser-review-save",
          repoPath,
          workspaceId,
          JSON.stringify(debouncedComments.map(toApiLineComment)),
          debouncedSummary,
        ]
      : null,
    () =>
      saveFileBrowserReview(
        repoPath!,
        workspaceId!,
        debouncedComments.map(toApiLineComment),
        debouncedSummary.trim() || undefined,
      ),
  );

  const addComment = (pendingComment: PendingComment, text: string) => {
    if (!text.trim()) return;
    const newComment: LineComment = {
      id: uuidv4(),
      filePath: pendingComment.filePath,
      hunkId: pendingComment.hunkId,
      startLine: pendingComment.startLine,
      endLine: pendingComment.endLine,
      lineContent: pendingComment.lineContent,
      text: text.trim(),
      createdAt: new Date().toISOString(),
    };
    setComments((prev) => [...prev, newComment]);
  };

  const deleteComment = (commentId: string) => {
    setComments((prev) => prev.filter((comment) => comment.id !== commentId));
    setEditingCommentId((prev) => (prev === commentId ? null : prev));
  };

  const startEditComment = (commentId: string) => {
    setEditingCommentId(commentId);
  };

  const cancelEditComment = () => {
    setEditingCommentId(null);
  };

  const saveEditComment = (commentId: string, newText: string) => {
    if (!newText.trim()) return;
    setComments((prev) =>
      prev.map((comment) =>
        comment.id === commentId
          ? { ...comment, text: newText.trim() }
          : comment,
      ),
    );
    setEditingCommentId(null);
  };

  const formatMarkdown = () =>
    formatReviewMarkdown({ comments, finalReviewComment });

  const handleRequestChanges = async (
    mode: "plan" | "acceptEdits",
    useIdleAgent = false,
  ) => {
    setSendingReview(true);
    try {
      const markdown = formatMarkdown();
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
      setComments([]);
      setFinalReviewComment("");
      setReviewPopoverOpen(false);
      if (repoPath && workspaceId !== undefined) {
        await clearFileBrowserReview(repoPath, workspaceId);
        await setQueryData(
          ["file-browser-review", repoPath, workspaceId],
          null,
        );
      }
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
      setComments([]);
      setFinalReviewComment("");
      setShowCancelDialog(false);
      setReviewPopoverOpen(false);
      if (repoPath && workspaceId !== undefined) {
        await clearFileBrowserReview(repoPath, workspaceId);
        await setQueryData(
          ["file-browser-review", repoPath, workspaceId],
          null,
        );
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
      const markdown = formatMarkdown();
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

  return {
    comments,
    addComment,
    deleteComment,
    editingCommentId,
    startEditComment,
    cancelEditComment,
    saveEditComment,
    finalReviewComment,
    setFinalReviewComment,
    reviewPopoverOpen,
    setReviewPopoverOpen,
    showCancelDialog,
    setShowCancelDialog,
    copiedReview,
    sendingReview,
    handleRequestChanges,
    handleCancelReview,
    handleCopyReview,
  };
}
