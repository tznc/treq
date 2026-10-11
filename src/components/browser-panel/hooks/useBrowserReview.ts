import { useCallback, useEffect, useState } from "react";
import {
  clearPendingPageReview,
  loadPendingPageReview,
  savePendingPageReview,
} from "../../../lib/api";
import { useDebounce } from "../../../hooks/useDebounce";
import type { useToast } from "../../ui/toast";
import type { ElementComment } from "../types";
import {
  formatPageReviewMarkdown,
  toApiElementComment,
  toLocalElementComment,
} from "../utils";

interface UseBrowserReviewParams {
  url: string;
  repoPath: string | undefined;
  workspaceId: number | undefined;
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

export function useBrowserReview({
  url,
  repoPath,
  workspaceId,
  onCreateAgentWithReview,
  onReviewSubmitted,
  addToast,
}: UseBrowserReviewParams) {
  const [comments, setComments] = useState<ElementComment[]>([]);
  const [summary, setSummary] = useState("");
  const [sendingReview, setSendingReview] = useState(false);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    const load = async () => {
      if (!repoPath || workspaceId === undefined) return;
      try {
        const pending = await loadPendingPageReview(repoPath, workspaceId);
        if (pending) {
          setComments(pending.comments.map(toLocalElementComment));
          if (pending.summary_text) setSummary(pending.summary_text);
        }
      } catch (error) {
        console.error("Failed to load pending page review:", error);
      } finally {
        setLoaded(true);
      }
    };
    load();
  }, [repoPath, workspaceId]);

  const debouncedComments = useDebounce(comments, 500);
  const debouncedSummary = useDebounce(summary, 500);

  useEffect(() => {
    const save = async () => {
      if (!loaded || !repoPath || workspaceId === undefined) return;
      if (debouncedComments.length === 0 && !debouncedSummary.trim()) return;
      try {
        await savePendingPageReview(
          repoPath,
          workspaceId,
          url,
          debouncedComments.map(toApiElementComment),
          debouncedSummary.trim() || undefined,
        );
      } catch (error) {
        console.error("Failed to auto-save page review:", error);
      }
    };
    save();
  }, [debouncedComments, debouncedSummary, repoPath, workspaceId, url, loaded]);

  const addComment = useCallback((comment: ElementComment) => {
    setComments((prev) => [...prev, comment]);
  }, []);

  const deleteComment = useCallback((commentId: string) => {
    setComments((prev) => prev.filter((c) => c.id !== commentId));
  }, []);

  const formatReviewMarkdown = useCallback(
    () => formatPageReviewMarkdown(url, comments, summary),
    [url, comments, summary],
  );

  const clearReviewState = useCallback(() => {
    setComments([]);
    setSummary("");
  }, []);

  const handleRequestChanges = useCallback(
    async (mode: "plan" | "acceptEdits", useIdleAgent = false) => {
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
        clearReviewState();
        if (repoPath && workspaceId !== undefined) {
          await clearPendingPageReview(repoPath, workspaceId);
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
    },
    [
      formatReviewMarkdown,
      onCreateAgentWithReview,
      addToast,
      clearReviewState,
      repoPath,
      workspaceId,
      onReviewSubmitted,
    ],
  );

  const handleCancelReview = useCallback(async () => {
    try {
      clearReviewState();
      if (repoPath && workspaceId !== undefined) {
        await clearPendingPageReview(repoPath, workspaceId);
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
  }, [repoPath, workspaceId, addToast, clearReviewState]);

  return {
    comments,
    summary,
    setSummary,
    sendingReview,
    addComment,
    deleteComment,
    formatReviewMarkdown,
    handleRequestChanges,
    handleCancelReview,
  };
}
