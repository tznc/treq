import { useState } from "react";
import { Bot, Check, Terminal, Trash2, Wand2 } from "lucide-react";
import { Button } from "../ui/button";
import { cn } from "../../lib/utils";
import type { AgentReviewComment } from "../../lib/api-types-review";
import { buildSuggestionDiff } from "./suggestionDiff";

interface AgentReviewCommentCardProps {
  comment: AgentReviewComment;
  /**
   * The current text of lines `start_line..end_line`, used to render the
   * suggestion as a diff. Undefined when the range is not on screen, in which
   * case the suggestion falls back to plain text.
   */
  originalText?: string;
  onResolve: (commentId: string) => Promise<void>;
  onDelete: (commentId: string) => Promise<void>;
  onApplySuggestion: (commentId: string) => Promise<void>;
  onSendToAgent?: (comment: AgentReviewComment) => Promise<void>;
  onError: (message: string) => void;
  /** Replaces the `file:line` label, e.g. for content that is not a file. */
  locationLabel?: string;
  /** Label of the apply button; "Apply" by default. */
  applyLabel?: string;
}

/**
 * One local-only review comment from the review agent. Styled apart from both
 * the user's own draft `LineComment` cards and the read-only GitHub review
 * threads, and badged so it is never read as something that exists on GitHub.
 */
export function AgentReviewCommentCard({
  comment,
  originalText,
  onResolve,
  onDelete,
  onApplySuggestion,
  onSendToAgent,
  onError,
  locationLabel,
  applyLabel = "Apply",
}: AgentReviewCommentCardProps) {
  const [pending, setPending] = useState(false);
  const suggestionDiff =
    comment.suggested_replacement != null
      ? buildSuggestionDiff(originalText, comment.suggested_replacement)
      : null;

  const run = async (action: () => Promise<void>) => {
    setPending(true);
    try {
      await action();
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setPending(false);
    }
  };

  return (
    <div
      data-testid="agent-review-comment-card"
      className="rounded-md border border-violet-500/40 bg-violet-500/5 p-[12px] space-y-2"
    >
      <div className="flex items-center gap-2">
        <Bot className="w-3.5 h-3.5 text-violet-500 flex-shrink-0" />
        <span className="text-xs font-medium text-violet-600 dark:text-violet-400">
          Local
        </span>
        <span className="text-xs text-muted-foreground">
          {locationLabel ?? (
            <>
              {comment.file_path}:{comment.start_line}
              {comment.end_line !== comment.start_line &&
                `-${comment.end_line}`}
            </>
          )}
        </span>
      </div>

      <p className="text-sm whitespace-pre-wrap">{comment.comment_text}</p>

      {comment.suggested_replacement != null && (
        <div className="rounded border border-border/60 overflow-hidden">
          <div className="px-[8px] py-[4px] text-xs text-muted-foreground bg-muted/60 border-b border-border/60">
            Suggested change
          </div>
          {suggestionDiff ? (
            <div
              data-testid="agent-review-suggestion"
              className="text-xs font-mono"
            >
              {suggestionDiff.map((row, rowIndex) => (
                <div
                  // Rows are positional and have no stable identity of their own.
                  key={rowIndex}
                  data-suggestion-row={row.type}
                  className={cn(
                    "flex items-start px-[8px] py-[1px] whitespace-pre-wrap break-all",
                    row.type === "removed" && "bg-red-500/10",
                    row.type === "added" && "bg-green-500/10",
                  )}
                >
                  <span className="w-3 flex-shrink-0 select-none text-muted-foreground">
                    {row.type === "removed"
                      ? "-"
                      : row.type === "added"
                        ? "+"
                        : " "}
                  </span>
                  <span className="flex-1">
                    {row.segments.map((segment, segmentIndex) => (
                      <span
                        key={segmentIndex}
                        className={cn(
                          segment.changed &&
                            row.type === "removed" &&
                            "bg-red-500/30 line-through",
                          segment.changed &&
                            row.type === "added" &&
                            "bg-green-500/30",
                        )}
                      >
                        {segment.text}
                      </span>
                    ))}
                  </span>
                </div>
              ))}
            </div>
          ) : (
            <pre
              data-testid="agent-review-suggestion"
              className="px-[8px] py-[6px] text-xs font-mono whitespace-pre-wrap break-all bg-green-500/10"
            >
              {comment.suggested_replacement}
            </pre>
          )}
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2">
        {comment.suggested_replacement != null && (
          <Button
            size="sm"
            variant="default"
            className="gap-1.5"
            disabled={pending}
            onClick={() => run(() => onApplySuggestion(comment.id))}
          >
            <Wand2 className="w-3 h-3" />
            {applyLabel}
          </Button>
        )}
        {onSendToAgent && (
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            disabled={pending}
            onClick={() => run(() => onSendToAgent(comment))}
          >
            <Terminal className="w-3 h-3" />
            Send to agent
          </Button>
        )}
        <Button
          size="sm"
          variant="outline"
          className="gap-1.5"
          disabled={pending}
          onClick={() => run(() => onResolve(comment.id))}
        >
          <Check className="w-3 h-3" />
          Resolve
        </Button>
        <Button
          size="sm"
          variant="ghost"
          className="gap-1.5 text-muted-foreground"
          disabled={pending}
          onClick={() => run(() => onDelete(comment.id))}
        >
          <Trash2 className="w-3 h-3" />
          Delete
        </Button>
      </div>
    </div>
  );
}
