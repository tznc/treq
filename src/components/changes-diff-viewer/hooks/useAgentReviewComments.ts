import useSWR from "swr";
import {
  applyAgentReviewSuggestion,
  deleteAgentReviewComment,
  listAgentReviewComments,
  resolveAgentReviewComment,
} from "../../../lib/api";
import type { AgentReviewComment } from "../../../lib/api-types-review";
import { pollMs } from "../../../lib/swr-cache";
import type { CommentLineQuery } from "../types";

interface UseAgentReviewCommentsParams {
  repoPath: string | undefined;
  /** What was reviewed, e.g. "workspace_diff" or "file_browser_file". */
  targetType: string;
  /** Identifier within the target type; the hook idles while this is unset. */
  targetId: string | undefined;
  /** Poll interval; how soon a comment the agent just wrote shows up. */
  refreshMs?: number;
}

/**
 * Local-only review comments left by a review agent on one reviewed target.
 * These never reach GitHub, so they are loaded and mutated entirely through
 * the local DB rather than the GitHub review-thread path.
 */
export function useAgentReviewComments({
  repoPath,
  targetType,
  targetId,
  refreshMs = 10_000,
}: UseAgentReviewCommentsParams) {
  const { data, mutate } = useSWR<AgentReviewComment[]>(
    repoPath && targetId
      ? ["agent-review-comments", repoPath, targetType, targetId]
      : null,
    () => listAgentReviewComments(repoPath ?? "", targetType, targetId ?? ""),
    { refreshInterval: pollMs(refreshMs) },
  );

  // The command can be stubbed out in tests and on transports that do not
  // implement it, so never assume the response is already a list.
  const comments = Array.isArray(data) ? data : [];

  const openComments = comments.filter(
    (comment) => comment.status !== "resolved",
  );

  const commentsByLineKey = new Map<string, AgentReviewComment[]>();
  for (const comment of openComments) {
    const side = comment.side ?? "new";
    const key = `${comment.file_path}:${comment.end_line}:${side}`;
    const existing = commentsByLineKey.get(key);
    if (existing) existing.push(comment);
    else commentsByLineKey.set(key, [comment]);
  }

  // Anchored on the range's last line, matching how local line comments render
  // their card under the final line of the range.
  const getAgentCommentsForLine = ({
    filePath,
    lineNumber,
    side,
  }: CommentLineQuery): AgentReviewComment[] =>
    commentsByLineKey.get(`${filePath}:${lineNumber}:${side}`) ?? [];

  const resolveComment = async (commentId: string) => {
    if (!repoPath) return;
    await resolveAgentReviewComment(repoPath, commentId);
    await mutate();
  };

  const deleteComment = async (commentId: string) => {
    if (!repoPath) return;
    await deleteAgentReviewComment(repoPath, commentId);
    await mutate();
  };

  const applySuggestion = async (commentId: string) => {
    if (!repoPath) return;
    await applyAgentReviewSuggestion(repoPath, commentId);
    await mutate();
  };

  return {
    agentReviewComments: comments,
    openAgentReviewComments: openComments,
    getAgentCommentsForLine,
    resolveAgentComment: resolveComment,
    deleteAgentComment: deleteComment,
    applyAgentSuggestion: applySuggestion,
    refreshAgentReviewComments: mutate,
  };
}
