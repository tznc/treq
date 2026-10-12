import { ScrollText } from "lucide-react";
import { Button } from "../ui/button";
import type { Workspace } from "../../lib/api-types";
import {
  checksTargetOf,
  useWorkspaceChecks,
} from "../../hooks/useWorkspaceChecks";
import { requestOpenWorkspaceLogs } from "../../lib/open-logs-request";
import { usePreviewFeature } from "../../stores/featurePreviewStore";
import { WorkspaceChecksPanel } from "../checks/WorkspaceChecksPanel";

/** Treq workflow checks for the PR's workspace, with a link to its logs. */
export function PrWorkspaceChecks({
  workspace,
  onOpenWorkspace,
}: {
  workspace: Workspace;
  onOpenWorkspace?: (workspaceId: number) => void;
}) {
  const logsEnabled = usePreviewFeature("logs");
  const target = checksTargetOf(workspace);
  const checks = useWorkspaceChecks(target);
  if (!target) return null;
  return (
    <div className="space-y-2" data-testid="pr-treq-checks">
      <div className="flex items-center justify-between">
        <h3 className="text-base font-semibold uppercase tracking-widest text-muted-foreground">
          Treq checks
        </h3>
        {logsEnabled && onOpenWorkspace && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              requestOpenWorkspaceLogs(workspace.id);
              onOpenWorkspace(workspace.id);
            }}
          >
            <ScrollText className="h-3 w-3 mr-1" />
            Open in Logs
          </Button>
        )}
      </div>
      <div className="border border-border rounded-md overflow-hidden">
        <WorkspaceChecksPanel target={target} checks={checks} compact />
      </div>
    </div>
  );
}
