import {
  AlertTriangle,
  Bot,
  Check,
  Copy,
  MessageSquare,
  RefreshCw,
  Send,
  X,
} from "lucide-react";
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
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "../ui/tooltip";
import { cn, getFileName } from "../../lib/utils";
import { UseIdleAgentCheckbox } from "../UseIdleAgentCheckbox";
import type { LineComment } from "./types";

interface ReviewActionBarProps {
  showActionBar: boolean;
  hasConflicts: boolean;
  totalComments: number;
  comments: LineComment[];
  staleFiles: Set<string>;
  reviewPopoverOpen: boolean;
  setReviewPopoverOpen: (open: boolean) => void;
  finalReviewComment: string;
  setFinalReviewComment: (text: string) => void;
  showCancelDialog: boolean;
  setShowCancelDialog: (open: boolean) => void;
  copiedReview: boolean;
  sendingReview: boolean;
  handleCopyReview: () => void;
  handleCancelReview: () => void;
  handleRequestChanges: (
    mode: "plan" | "acceptEdits",
    useIdleAgent: boolean,
  ) => void;
  handleReloadWithPendingChanges: () => void;
  getAllOutdatedComments: () => LineComment[];
  handleCopyOutdatedComments: () => void;
  /** Launches a review agent over the current diff. Omitted when unavailable. */
  handleStartAgentReview?: () => void;
  startingAgentReview?: boolean;
  /** Count of unresolved local agent review comments on this diff. */
  agentReviewCommentCount?: number;
}

export function ReviewActionBar({
  showActionBar,
  hasConflicts,
  totalComments,
  comments,
  staleFiles,
  reviewPopoverOpen,
  setReviewPopoverOpen,
  finalReviewComment,
  setFinalReviewComment,
  showCancelDialog,
  setShowCancelDialog,
  copiedReview,
  sendingReview,
  handleCopyReview,
  handleCancelReview,
  handleRequestChanges,
  handleReloadWithPendingChanges,
  getAllOutdatedComments,
  handleCopyOutdatedComments,
  handleStartAgentReview,
  startingAgentReview = false,
  agentReviewCommentCount = 0,
}: ReviewActionBarProps) {
  const [useIdleAgent, setUseIdleAgent] = useState(false);
  const startReviewButton = handleStartAgentReview ? (
    <Button
      size="sm"
      variant="outline"
      className="gap-2"
      data-testid="start-agent-review"
      disabled={startingAgentReview}
      onClick={handleStartAgentReview}
    >
      <Bot className="w-3 h-3" />
      Start Review
    </Button>
  ) : null;

  const agentReviewCount =
    agentReviewCommentCount > 0 ? (
      <span
        data-testid="agent-review-comment-count"
        className="text-xs text-violet-600 dark:text-violet-400"
      >
        {agentReviewCommentCount} local review comment
        {agentReviewCommentCount !== 1 ? "s" : ""}
      </span>
    ) : null;

  return (
    <>
      {!showActionBar && startReviewButton && (
        <div className="sticky top-0 z-10 flex items-center justify-end gap-2 px-4 py-2 bg-muted/80 backdrop-blur-sm border-b border-border">
          {agentReviewCount}
          {startReviewButton}
        </div>
      )}
      {showActionBar && (
        <div className="sticky top-0 z-10 flex items-center justify-between px-4 py-2 bg-muted/80 backdrop-blur-sm border-b border-border">
          <div className="flex items-center gap-2 text-sm">
            <MessageSquare
              className={`w-4 h-4 ${hasConflicts ? "text-destructive" : "text-primary"}`}
            />
            <span className="text-muted-foreground">
              {totalComments} comment{totalComments !== 1 ? "s" : ""} pending
            </span>
            {agentReviewCount}
          </div>
          <div className="flex items-center gap-2">
            {startReviewButton}
            <Button
              size="sm"
              variant="outline"
              data-testid="discard-review"
              onClick={() => setShowCancelDialog(true)}
            >
              {hasConflicts ? "Reset" : "Discard"}
            </Button>
            <Popover
              open={reviewPopoverOpen}
              onOpenChange={setReviewPopoverOpen}
            >
              <PopoverTrigger asChild>
                <Button
                  size="sm"
                  variant="default"
                  className={cn(
                    "gap-2",
                    hasConflicts &&
                      "bg-destructive hover:bg-destructive/90 text-destructive-foreground",
                  )}
                >
                  <Send className="w-3 h-3" />
                  {hasConflicts ? "Resolve conflicts..." : "Finish review"}
                </Button>
              </PopoverTrigger>
              <PopoverContent
                align="end"
                side="bottom"
                className="w-80 relative"
              >
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        size="icon"
                        variant="ghost"
                        className="h-8 w-8 absolute top-2 right-2"
                        aria-label="Close"
                        onClick={() => setReviewPopoverOpen(false)}
                        disabled={sendingReview}
                      >
                        <X className="w-4 h-4" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>Close</TooltipContent>
                  </Tooltip>
                </TooltipProvider>
                <div className="space-y-3">
                  <div>
                    <h4 className="font-medium text-sm mb-1">
                      {hasConflicts
                        ? "Resolve conflicts"
                        : "Finish your review"}
                    </h4>
                    <p className="text-sm text-muted-foreground">
                      {totalComments} comment{totalComments !== 1 ? "s" : ""}{" "}
                      will be submitted.
                    </p>
                  </div>
                  <Textarea
                    value={finalReviewComment}
                    onChange={(event) =>
                      setFinalReviewComment(event.target.value)
                    }
                    placeholder="Add a summary comment (optional)..."
                    className="min-h-[80px] text-sm"
                  />
                  <UseIdleAgentCheckbox
                    checked={useIdleAgent}
                    onCheckedChange={setUseIdleAgent}
                    disabled={sendingReview}
                  />
                  <div className="flex justify-between gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={handleCopyReview}
                      className="gap-2"
                      disabled={sendingReview}
                      aria-label={
                        copiedReview ? "Copied review" : "Copy review"
                      }
                    >
                      {copiedReview ? (
                        <>
                          <Check className="w-3 h-3" />
                          Copied
                        </>
                      ) : (
                        <>
                          <Copy className="w-3 h-3" />
                          Copy
                        </>
                      )}
                    </Button>
                    <div className="flex gap-2">
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() =>
                          handleRequestChanges("plan", useIdleAgent)
                        }
                        disabled={sendingReview}
                      >
                        Plan
                      </Button>
                      <Button
                        size="sm"
                        onClick={() =>
                          handleRequestChanges("acceptEdits", useIdleAgent)
                        }
                        disabled={sendingReview}
                      >
                        Edit
                      </Button>
                    </div>
                  </div>
                </div>
              </PopoverContent>
            </Popover>
          </div>
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
            <AlertDialogAction
              data-testid="confirm-discard-review"
              onClick={handleCancelReview}
            >
              Discard
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {staleFiles.size > 0 && (
        <div className="flex items-center justify-between px-4 py-2 bg-amber-500/15 border-b border-amber-500/40">
          <div className="flex items-center gap-2 text-sm">
            <AlertTriangle className="w-4 h-4 text-amber-700 dark:text-amber-300" />
            <span className="text-amber-800 dark:text-amber-200">
              {staleFiles.size} file{staleFiles.size !== 1 ? "s" : ""} changed
              since you started reviewing
            </span>
            <span className="text-sm text-amber-700/80 dark:text-amber-300/80">
              ({Array.from(staleFiles).slice(0, 3).map(getFileName).join(", ")}
              {staleFiles.size > 3 ? ` +${staleFiles.size - 3} more` : ""})
            </span>
          </div>
          <div className="flex items-center gap-2">
            {getAllOutdatedComments().length > 0 && (
              <Button
                size="sm"
                variant="outline"
                className="gap-1.5 border-amber-500/60 text-amber-800 dark:text-amber-200 hover:bg-amber-500/25"
                onClick={handleCopyOutdatedComments}
              >
                <Copy className="w-3 h-3" />
                Copy Outdated ({getAllOutdatedComments().length})
              </Button>
            )}
            <Button
              size="sm"
              variant="outline"
              className="gap-1.5 border-amber-500/60 text-amber-800 dark:text-amber-200 hover:bg-amber-500/25"
              onClick={handleReloadWithPendingChanges}
            >
              <RefreshCw className="w-3 h-3" />
              Reload
            </Button>
          </div>
        </div>
      )}
    </>
  );
}
