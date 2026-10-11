import { MessageSquare, Send, Trash2 } from "lucide-react";
import { useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "../ui/alert-dialog";
import { Button } from "../ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { Textarea } from "../ui/textarea";
import type { ElementComment } from "./types";
import { UseIdleAgentCheckbox } from "../UseIdleAgentCheckbox";

export interface BrowserReviewActionBarProps {
  comments: ElementComment[];
  summary: string;
  setSummary: (value: string) => void;
  sendingReview: boolean;
  popoverOpen: boolean;
  setPopoverOpen: (open: boolean) => void;
  showCancelDialog: boolean;
  setShowCancelDialog: (open: boolean) => void;
  onDeleteComment: (commentId: string) => void;
  onCancelReview: () => void;
  onRequestChanges: (
    mode: "plan" | "acceptEdits",
    useIdleAgent: boolean,
  ) => void;
}

export function BrowserReviewActionBar({
  comments,
  summary,
  setSummary,
  sendingReview,
  popoverOpen,
  setPopoverOpen,
  showCancelDialog,
  setShowCancelDialog,
  onDeleteComment,
  onCancelReview,
  onRequestChanges,
}: BrowserReviewActionBarProps) {
  const [useIdleAgent, setUseIdleAgent] = useState(false);
  const hasContent = comments.length > 0 || summary.trim().length > 0;

  return (
    <div className="border-t bg-card">
      <div className="flex items-center justify-between px-3 py-2">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <MessageSquare className="w-4 h-4" />
          {comments.length} comment{comments.length !== 1 ? "s" : ""}
        </div>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => setShowCancelDialog(true)}
            disabled={!hasContent}
          >
            Discard
          </Button>
          <Popover open={popoverOpen} onOpenChange={setPopoverOpen}>
            <PopoverTrigger asChild>
              <Button size="sm" disabled={!hasContent} className="gap-2">
                <Send className="w-3 h-3" />
                Finish review
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" side="top" className="w-80">
              <div className="space-y-3">
                <div>
                  <h4 className="font-medium text-sm mb-1">
                    Finish your page review
                  </h4>
                  <p className="text-sm text-muted-foreground">
                    {comments.length} comment{comments.length !== 1 ? "s" : ""}{" "}
                    will be submitted.
                  </p>
                </div>
                <Textarea
                  value={summary}
                  onChange={(event) => setSummary(event.target.value)}
                  placeholder="Add a summary comment (optional)..."
                  className="min-h-[80px] text-sm"
                />
                <UseIdleAgentCheckbox
                  checked={useIdleAgent}
                  onCheckedChange={setUseIdleAgent}
                  disabled={sendingReview}
                />
                <div className="flex justify-end gap-2">
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => onRequestChanges("plan", useIdleAgent)}
                    disabled={sendingReview}
                  >
                    Plan
                  </Button>
                  <Button
                    size="sm"
                    onClick={() =>
                      onRequestChanges("acceptEdits", useIdleAgent)
                    }
                    disabled={sendingReview}
                  >
                    Edit
                  </Button>
                </div>
              </div>
            </PopoverContent>
          </Popover>
        </div>
      </div>

      {comments.length > 0 && (
        <div className="max-h-40 overflow-auto border-t divide-y">
          {comments.map((comment) => (
            <div
              key={comment.id}
              className="flex items-start justify-between gap-2 px-3 py-2 text-sm"
            >
              <div className="min-w-0">
                <div className="font-mono text-xs text-muted-foreground truncate">
                  {comment.selector} ({comment.tag})
                </div>
                <p className="truncate">{comment.text}</p>
              </div>
              <button
                type="button"
                onClick={() => onDeleteComment(comment.id)}
                className="text-destructive shrink-0"
                title="Delete comment"
                aria-label={`Delete comment on ${comment.selector}`}
              >
                <Trash2 className="w-3.5 h-3.5" />
              </button>
            </div>
          ))}
        </div>
      )}

      <AlertDialog open={showCancelDialog} onOpenChange={setShowCancelDialog}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Discard review?</AlertDialogTitle>
            <AlertDialogDescription>
              This will discard all {comments.length} pending comment
              {comments.length !== 1 ? "s" : ""}. This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep reviewing</AlertDialogCancel>
            <AlertDialogAction onClick={onCancelReview}>
              Discard
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
