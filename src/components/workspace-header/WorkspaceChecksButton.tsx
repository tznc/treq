import { usePrCiStatus } from "../../hooks/useMergeQueueStatus";
import {
  type ChecksTarget,
  useWorkspaceChecks,
} from "../../hooks/useWorkspaceChecks";
import { ChecksPopover } from "../checks/ChecksPopover";

/**
 * Header Checks button for the open workspace: Treq workflow checks plus the
 * GitHub checks of the branch's PR, when `prBranch` names a branch with one.
 */
export function WorkspaceChecksButton({
  target,
  prBranch,
  onOpenLogs,
}: {
  target: ChecksTarget;
  prBranch?: string;
  onOpenLogs?: () => void;
}) {
  const checks = useWorkspaceChecks(target);
  const { data: ciStatus } = usePrCiStatus(
    prBranch ? target.repoPath : undefined,
    prBranch,
  );
  return (
    <ChecksPopover
      checks={checks}
      ciStatus={ciStatus}
      onOpenLogs={onOpenLogs}
    />
  );
}
