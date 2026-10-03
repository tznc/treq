import { Bot, Loader2 } from "lucide-react";
import { createContext, useContext, useState } from "react";
import {
  prepareLinearReview,
  type ReviewLaunch,
} from "../lib/agent-review-launch";
import {
  type LinearComment,
  linearApplyReviewSuggestion,
} from "../lib/api-linear";
import {
  type AgentReviewComment,
  LINEAR_REVIEW_BODY_FILE,
  type LinearReviewTargetType,
  linearReviewCommentFile,
} from "../lib/api-types-review";
import { AgentReviewCommentCard } from "./changes-diff-viewer/AgentReviewCommentCard";
import { useAgentReviewComments } from "./changes-diff-viewer/hooks/useAgentReviewComments";
import { Button } from "./ui/button";
import { useToast } from "./ui/toast";

/** The Linear entity a review covers, as the panel already has it loaded. */
export interface LinearReviewTarget {
  targetType: LinearReviewTargetType;
  targetId: string;
  title: string;
  url: string;
  /** The issue description, project description or document content. */
  body: string;
  /** Called after a suggestion lands in Linear, to reload what is shown. */
  onContentChanged?: () => void;
}

interface LinearReviewContextValue {
  repoPath: string;
  /** Opens an agent session with the review prompt; absent when unsupported. */
  onStartAgentReview?: (launch: ReviewLaunch) => Promise<void>;
}

export const LinearReviewContext = createContext<LinearReviewContextValue>({
  repoPath: "",
});

export function useLinearAgentReview(
  target: LinearReviewTarget | undefined,
  comments: LinearComment[],
) {
  const { repoPath, onStartAgentReview } = useContext(LinearReviewContext);
  const { addToast } = useToast();
  const [starting, setStarting] = useState(false);
  const review = useAgentReviewComments({
    repoPath: repoPath || undefined,
    targetType: target?.targetType ?? "",
    targetId: target?.targetId,
  });

  const reportError = (title: string) => (error: unknown) =>
    addToast({
      title,
      description: error instanceof Error ? error.message : String(error),
      type: "error",
    });

  const startReview =
    target && onStartAgentReview
      ? async () => {
          setStarting(true);
          try {
            const launch = await prepareLinearReview({
              repoPath,
              targetType: target.targetType,
              targetId: target.targetId,
              snapshot: {
                title: target.title,
                url: target.url,
                body: target.body,
                comments: comments.map((comment) => ({
                  id: comment.id,
                  author: comment.user?.name ?? null,
                  created_at: comment.created_at,
                  quoted_text: comment.quoted_text ?? null,
                  body: comment.body,
                })),
              },
            });
            await onStartAgentReview(launch);
          } catch (error) {
            reportError("Failed to start review")(error);
          } finally {
            setStarting(false);
          }
        }
      : undefined;

  const applySuggestion = async (commentId: string) => {
    await linearApplyReviewSuggestion(repoPath, commentId);
    await review.refreshAgentReviewComments();
    target?.onContentChanged?.();
  };

  const forFile = (file: string) =>
    review.openAgentReviewComments.filter((c) => c.file_path === file);

  return {
    enabled: target !== undefined,
    starting,
    startReview,
    openComments: review.openAgentReviewComments,
    bodyComments: forFile(LINEAR_REVIEW_BODY_FILE),
    commentsOnLinearComment: (id: string) =>
      forFile(linearReviewCommentFile(id)),
    resolve: review.resolveAgentComment,
    remove: review.deleteAgentComment,
    applySuggestion,
    onError: reportError("Review comment action failed"),
  };
}

export type LinearAgentReviewState = ReturnType<typeof useLinearAgentReview>;

export function LinearReviewButton({
  review,
}: {
  review: LinearAgentReviewState;
}) {
  if (!review.startReview) return null;
  return (
    <Button
      size="sm"
      variant="outline"
      className="h-7 gap-1.5 text-xs"
      disabled={review.starting}
      onClick={() => void review.startReview?.()}
      data-testid="linear-start-review"
    >
      {review.starting ? (
        <Loader2 className="w-3 h-3 animate-spin" />
      ) : (
        <Bot className="w-3 h-3" />
      )}
      Review with agent
    </Button>
  );
}

/** Agent review cards for one reviewed Linear text. */
export function LinearAgentReviewCards({
  review,
  comments,
  locationLabel,
}: {
  review: LinearAgentReviewState;
  comments: AgentReviewComment[];
  locationLabel: string;
}) {
  return (
    <>
      {comments.map((comment) => (
        <AgentReviewCommentCard
          key={comment.id}
          comment={comment}
          originalText={comment.quoted_text ?? undefined}
          locationLabel={locationLabel}
          applyLabel="Apply to Linear"
          onResolve={review.resolve}
          onDelete={review.remove}
          onApplySuggestion={review.applySuggestion}
          onError={review.onError}
        />
      ))}
    </>
  );
}
