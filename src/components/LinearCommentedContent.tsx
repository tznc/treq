import { Loader2 } from "lucide-react";
import type { LinearComment } from "../lib/api-linear";
import {
  LinearAgentReviewCards,
  type LinearReviewTarget,
  LinearReviewButton,
  useLinearAgentReview,
} from "./LinearAgentReview";
import { MarkdownContent } from "./MarkdownContent";
import { cn } from "../lib/utils";

const HIGHLIGHT_CLASS =
  "bg-yellow-200/70 dark:bg-yellow-500/30 text-inherit rounded-sm px-0.5";
const AGENT_HIGHLIGHT_CLASS =
  "bg-violet-200/70 dark:bg-violet-500/30 text-inherit rounded-sm px-0.5";

interface QuotedAnchor {
  id: string;
  quote: string | null | undefined;
  className: string;
}

/**
 * Wraps each anchor's quoted excerpt in a <mark> so it lines up with its card
 * in the right-hand comments column, Notion/Linear-style. Linear's own inline
 * "highlight and comment" anchors are yellow; agent review findings on the
 * body are violet.
 */
function highlightQuotedText(content: string, anchors: QuotedAnchor[]): string {
  let result = content;
  for (const anchor of anchors) {
    const quote = anchor.quote?.trim();
    if (!quote) continue;
    const index = result.indexOf(quote);
    if (index === -1) continue;
    const before = result.slice(0, index);
    const after = result.slice(index + quote.length);
    const safeId = anchor.id.replace(/"/g, "&quot;");
    result = `${before}<mark class="${anchor.className}" data-comment-id="${safeId}">${quote}</mark>${after}`;
  }
  return result;
}

export function LinearCommentedContent({
  content,
  comments,
  isLoadingComments,
  commentsError,
  review,
}: {
  content: string;
  comments: LinearComment[];
  isLoadingComments: boolean;
  commentsError?: unknown;
  /** Enables agent review of this content and its comments. */
  review?: LinearReviewTarget;
}) {
  const agentReview = useLinearAgentReview(review, comments);
  // Agent quotes are multi-line markdown; only single-line ones can be
  // highlighted without breaking the rendered markdown structure.
  const agentAnchors = agentReview.bodyComments
    .filter((c) => c.quoted_text && !c.quoted_text.includes("\n"))
    .map((c) => ({
      id: c.id,
      quote: c.quoted_text,
      className: AGENT_HIGHLIGHT_CLASS,
    }));
  const highlighted = highlightQuotedText(content, [
    ...comments.map((c) => ({
      id: c.id,
      quote: c.quoted_text,
      className: HIGHLIGHT_CLASS,
    })),
    ...agentAnchors,
  ]);

  return (
    <div
      className="flex gap-6 items-start"
      data-testid="linear-commented-content"
    >
      <div className="flex-1 min-w-0">
        {agentReview.enabled && (
          <div className="flex items-center justify-end gap-2 mb-2">
            {agentReview.openComments.length > 0 && (
              <span className="text-xs text-violet-600 dark:text-violet-400">
                {agentReview.openComments.length} agent review comment
                {agentReview.openComments.length === 1 ? "" : "s"}
              </span>
            )}
            <LinearReviewButton review={agentReview} />
          </div>
        )}
        <MarkdownContent content={highlighted} className="text-sm" />
      </div>

      <div
        className={cn(
          "shrink-0 flex flex-col gap-4 border-l border-border pl-4",
          agentReview.enabled ? "w-80" : "w-64",
        )}
        data-testid="linear-comments-column"
      >
        {agentReview.bodyComments.length > 0 && (
          <div
            className="flex flex-col gap-3"
            data-testid="linear-agent-review-body"
          >
            <h3 className="text-xs font-medium text-muted-foreground uppercase">
              Agent review
            </h3>
            <LinearAgentReviewCards
              review={agentReview}
              comments={agentReview.bodyComments}
              locationLabel="Description"
            />
          </div>
        )}

        <h3 className="text-xs font-medium text-muted-foreground uppercase">
          Comments
        </h3>

        {isLoadingComments && (
          <Loader2 className="w-4 h-4 animate-spin text-muted-foreground" />
        )}

        {commentsError !== undefined && commentsError !== null && (
          <p className="text-sm text-destructive">
            {commentsError instanceof Error
              ? commentsError.message
              : "Failed to load comments"}
          </p>
        )}

        {!isLoadingComments && !commentsError && comments.length === 0 && (
          <p className="text-sm text-muted-foreground">No comments</p>
        )}

        {comments.map((comment) => (
          <div
            key={comment.id}
            data-testid={`linear-comment-${comment.id}`}
            className="flex flex-col gap-1.5"
          >
            {comment.quoted_text && (
              <blockquote className="text-xs text-muted-foreground italic border-l-2 border-yellow-400/70 dark:border-yellow-500/50 pl-2">
                {comment.quoted_text}
              </blockquote>
            )}
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span className="font-medium text-foreground">
                {comment.user?.name ?? "Unknown"}
              </span>
              <span>{formatCommentTimestamp(comment.created_at)}</span>
            </div>
            <MarkdownContent
              content={comment.body}
              className="text-sm prose-p:my-1"
            />
            <LinearAgentReviewCards
              review={agentReview}
              comments={agentReview.commentsOnLinearComment(comment.id)}
              locationLabel={`Comment by ${comment.user?.name ?? "Unknown"}`}
            />
          </div>
        ))}
      </div>
    </div>
  );
}

function formatCommentTimestamp(isoDate: string): string {
  const date = new Date(isoDate);
  if (Number.isNaN(date.getTime())) return isoDate;
  return date.toLocaleString();
}
