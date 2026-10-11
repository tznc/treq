import { useState, type MouseEvent as ReactMouseEvent } from "react";
import { type AssetLineComment } from "../../lib/sendAssetReview";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import { Textarea } from "../ui/textarea";

interface TextAssetLineReviewProps {
  content: string;
  comments: AssetLineComment[];
  onCommentsChange: (comments: AssetLineComment[]) => void;
  onUnsavedComposerChange?: (hasUnsaved: boolean) => void;
}

export function TextAssetLineReview({
  content,
  comments,
  onCommentsChange,
  onUnsavedComposerChange,
}: TextAssetLineReviewProps) {
  const lines = content.length === 0 ? [""] : content.split("\n");
  const [pending, setPendingState] = useState<{
    startLine: number;
    endLine: number;
    text: string;
  } | null>(null);

  const setPending = (
    next: {
      startLine: number;
      endLine: number;
      text: string;
    } | null,
  ) => {
    setPendingState(next);
    onUnsavedComposerChange?.(next != null);
  };

  const selectLine = (lineNumber: number, event: ReactMouseEvent) => {
    if (event.shiftKey && pending) {
      setPending({
        ...pending,
        startLine: Math.min(pending.startLine, lineNumber),
        endLine: Math.max(pending.endLine, lineNumber),
      });
      return;
    }
    setPending({ startLine: lineNumber, endLine: lineNumber, text: "" });
  };

  const commitPending = () => {
    if (!pending || !pending.text.trim()) return;
    const start = pending.startLine;
    const end = pending.endLine;
    const lineContent = lines.slice(start - 1, end);
    const id =
      typeof crypto !== "undefined" && crypto.randomUUID
        ? crypto.randomUUID()
        : `lc-${Date.now()}`;
    onCommentsChange([
      ...comments,
      {
        id,
        startLine: start,
        endLine: end,
        lineContent,
        text: pending.text.trim(),
      },
    ]);
    setPending(null);
  };

  return (
    <div
      data-testid="treq-send-text-preview"
      className="max-h-[80vh] w-full max-w-6xl overflow-auto rounded-lg bg-zinc-900 opacity-100 shadow-2xl font-mono text-sm leading-relaxed text-zinc-100"
    >
      {lines.map((line, index) => {
        const lineNumber = index + 1;
        const isPending =
          pending != null &&
          lineNumber >= pending.startLine &&
          lineNumber <= pending.endLine;
        const lineComments = comments.filter(
          (comment) =>
            lineNumber >= comment.startLine && lineNumber <= comment.endLine,
        );
        return (
          <div key={lineNumber}>
            <button
              type="button"
              data-testid={`treq-send-text-line-${lineNumber}`}
              className={cn(
                "flex w-full items-start gap-3 px-4 py-0.5 text-left hover:bg-white/5",
                isPending && "bg-red-500/15",
                lineComments.length > 0 && "bg-amber-500/10",
              )}
              onClick={(event) => selectLine(lineNumber, event)}
            >
              <span className="w-8 shrink-0 select-none text-right text-xs text-zinc-500">
                {lineNumber}
              </span>
              <span className="min-w-0 flex-1 whitespace-pre-wrap break-words">
                {line || " "}
              </span>
            </button>
            {pending && pending.endLine === lineNumber && (
              <div
                data-testid="treq-send-line-comment-composer"
                className="border-t border-white/10 bg-zinc-900 px-4 py-3"
              >
                <Textarea
                  value={pending.text}
                  onChange={(event) =>
                    setPending({ ...pending, text: event.target.value })
                  }
                  placeholder="Comment on these lines"
                  aria-label="Line comment"
                  className="min-h-[72px] text-sm"
                  autoFocus
                />
                <div className="mt-2 flex justify-end gap-2">
                  <Button
                    type="button"
                    variant="ghost"
                    size="xs"
                    onClick={() => setPending(null)}
                  >
                    Cancel
                  </Button>
                  <Button
                    type="button"
                    size="xs"
                    data-testid="treq-send-line-comment-save"
                    disabled={!pending.text.trim()}
                    onClick={commitPending}
                  >
                    Add comment
                  </Button>
                </div>
              </div>
            )}
            {comments
              .filter((comment) => comment.endLine === lineNumber)
              .map((comment) => (
                <div
                  key={comment.id}
                  data-testid={`treq-send-line-comment-${comment.id}`}
                  className="border-t border-white/10 bg-zinc-900/80 px-4 py-2 text-sm text-zinc-100"
                >
                  <p className="text-[10px] uppercase tracking-wide text-zinc-500">
                    Lines {comment.startLine}
                    {comment.endLine !== comment.startLine
                      ? `–${comment.endLine}`
                      : ""}
                  </p>
                  <p className="whitespace-pre-wrap">{comment.text}</p>
                </div>
              ))}
          </div>
        );
      })}
    </div>
  );
}
