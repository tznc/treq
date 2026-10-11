import { openUrl } from "@tauri-apps/plugin-opener";
import { GitMerge } from "lucide-react";
import {
  useEnqueueWorkspace,
  useMergeQueueEnabled,
  useMergeQueueStatus,
} from "../../hooks/useMergeQueueStatus";
import { FEATURES } from "../../lib/features";
import { isProRequiredError } from "../../lib/pro-required";
import { WEB_URL } from "../../lib/supabase";
import { Button } from "../ui/button";
import { useToast } from "../ui/toast";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "../ui/tooltip";

const INACTIVE_STATUSES = ["merged", "failed", "dequeued"];

/**
 * Add/remove the branch from the merge queue. Renders nothing until the
 * mergeQueue build flag is on and the repo has opted in via the GitHub panel.
 */
export const MergeQueueButton = ({
  repoPath,
  branchName,
}: {
  repoPath: string;
  branchName: string;
}) => {
  if (!FEATURES.mergeQueue) return null;
  return <MergeQueueButtonInner repoPath={repoPath} branchName={branchName} />;
};

const MergeQueueButtonInner = ({
  repoPath,
  branchName,
}: {
  repoPath: string;
  branchName: string;
}) => {
  const { addToast } = useToast();
  const { data: queueEnabled } = useMergeQueueEnabled(repoPath);
  const { data: queueStatus } = useMergeQueueStatus(repoPath, branchName);
  const { enqueue, dequeue } = useEnqueueWorkspace(repoPath, branchName);
  if (queueEnabled !== true) return null;

  const isInQueue =
    !!queueStatus && !INACTIVE_STATUSES.includes(queueStatus.status);

  const handleClick = async () => {
    try {
      if (isInQueue) {
        await dequeue.mutateAsync();
        addToast({ title: "Removed from merge queue", type: "success" });
      } else {
        await enqueue.mutateAsync();
        addToast({ title: "Added to merge queue", type: "success" });
      }
    } catch (err) {
      if (isProRequiredError(err)) {
        addToast({
          title: "The merge queue needs Pro",
          type: "info",
          action: {
            label: "Upgrade to Pro",
            onClick: () => void openUrl(`${WEB_URL}/dashboard`),
          },
        });
        return;
      }
      addToast({
        title: "Queue error",
        description: (err as Error).message,
        type: "error",
      });
    }
  };

  return (
    <TooltipProvider delay={200}>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant={
              queueStatus && queueStatus.status !== "dequeued"
                ? "secondary"
                : "outline"
            }
            size="sm"
            className="gap-1"
            disabled={enqueue.isPending || dequeue.isPending}
            onClick={handleClick}
          >
            <GitMerge className="w-4 h-4" />
            {isInQueue ? "Queued" : "Add to Queue"}
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {queueStatus
            ? queueStatus.status === "merged"
              ? "Merged via queue"
              : queueStatus.status === "failed"
                ? `Failed: ${queueStatus.failure_reason ?? "unknown"}`
                : `In merge queue at position ${queueStatus.position}`
            : "Add this branch to the merge queue"}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
};
