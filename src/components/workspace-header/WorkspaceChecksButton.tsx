import {
  type ChecksTarget,
  useWorkspaceChecks,
} from "../../hooks/useWorkspaceChecks";
import { WorkspaceChecksIndicator } from "../checks/WorkspaceChecksIndicator";

/** Header checks summary for the open workspace. */
export function WorkspaceChecksButton({
  target,
  onOpenLogs,
}: {
  target: ChecksTarget;
  onOpenLogs?: () => void;
}) {
  const checks = useWorkspaceChecks(target);
  return (
    <WorkspaceChecksIndicator checks={checks} onOpenDetails={onOpenLogs} />
  );
}
