import { createContext, useContext } from "react";
import type { JjDiffHunk } from "../../lib/api";
import type { AgentReviewComment } from "../../lib/api-types-review";
import { AgentReviewCommentCard } from "./AgentReviewCommentCard";
import { buildHunkLineTexts } from "./utils";
import type { CommentLineQuery } from "./types";

export interface AgentReviewContextValue {
  getAgentCommentsForLine: (query: CommentLineQuery) => AgentReviewComment[];
  resolveAgentComment: (commentId: string) => Promise<void>;
  deleteAgentComment: (commentId: string) => Promise<void>;
  applyAgentSuggestion: (commentId: string) => Promise<void>;
  /**
   * Opens a fresh agent terminal seeded with this one comment. Absent when the
   * host screen has no way to create a terminal session.
   */
  sendAgentCommentToAgent?: (comment: AgentReviewComment) => Promise<void>;
  onAgentCommentError: (message: string) => void;
}

/**
 * Local-only agent review comments reach the diff rows through context rather
 * than the `DiffContentArea` prop bundle: they are read at one leaf (the diff
 * line) and nothing between the viewer and that leaf needs them.
 */
export const AgentReviewContext = createContext<AgentReviewContextValue | null>(
  null,
);

function useAgentReview(): AgentReviewContextValue | null {
  return useContext(AgentReviewContext);
}

interface AgentReviewInlineListProps {
  filePath: string;
  lineNumber: number;
  lineSide: "old" | "new";
  /**
   * The hunk this line belongs to. A comment's suggestion is diffed against
   * the lines it replaces, read from here; ranges that reach outside the hunk
   * simply render as plain text.
   */
  hunk?: JjDiffHunk;
}

/**
 * Text of `start_line..end_line` on `side`, or undefined when any line of the
 * range is missing from the loaded hunk.
 */
function originalTextForComment(
  comment: AgentReviewComment,
  lineTexts: Map<string, string> | undefined,
): string | undefined {
  if (!lineTexts) return undefined;
  const side = comment.side ?? "new";
  const lines: string[] = [];
  for (let line = comment.start_line; line <= comment.end_line; line++) {
    const text = lineTexts.get(`${side}:${line}`);
    if (text === undefined) return undefined;
    lines.push(text);
  }
  return lines.join("\n");
}

/** Inline block of agent review comments anchored to one diff line. */
export function AgentReviewInlineList({
  filePath,
  lineNumber,
  lineSide,
  hunk,
}: AgentReviewInlineListProps) {
  const agentReview = useAgentReview();
  if (!agentReview) return null;

  const comments = agentReview.getAgentCommentsForLine({
    filePath,
    hunkId: "",
    lineNumber,
    side: lineSide,
  });
  if (comments.length === 0) return null;

  const lineTexts = hunk ? buildHunkLineTexts(hunk) : undefined;

  return (
    <div
      data-testid="agent-review-inline-list"
      className="border-y border-violet-500/30 bg-violet-500/[0.03] px-[16px] py-[8px] space-y-2"
    >
      {comments.map((comment) => (
        <AgentReviewCommentCard
          key={comment.id}
          comment={comment}
          originalText={originalTextForComment(comment, lineTexts)}
          onResolve={agentReview.resolveAgentComment}
          onDelete={agentReview.deleteAgentComment}
          onApplySuggestion={agentReview.applyAgentSuggestion}
          onSendToAgent={agentReview.sendAgentCommentToAgent}
          onError={agentReview.onAgentCommentError}
        />
      ))}
    </div>
  );
}
