import { openUrl } from "@tauri-apps/plugin-opener";
import { Zap } from "lucide-react";
import { usePreviewFeature } from "../../stores/featurePreviewStore";

/** Issue-tracker fields a workspace may carry in its `metadata` JSON. */
export interface WorkspaceTrackerMetadata {
  linear_issue_key?: string;
  linear_issue_url?: string;
  linear_issue_title?: string;
}

const BADGE_CLASS =
  "flex items-center gap-1 px-2 py-0.5 rounded-full bg-muted text-xs font-medium text-muted-foreground hover:bg-muted/80 transition-colors shrink-0";

/**
 * Header badge linking to the workspace's Linear issue. Renders only when the
 * Linear integration preview is on.
 */
export const WorkspaceTrackerBadges = ({
  metadata,
}: {
  metadata: WorkspaceTrackerMetadata | null;
}) => {
  const linearEnabled = usePreviewFeature("linearIntegration");
  if (!metadata) return null;

  return (
    <>
      {linearEnabled &&
        metadata.linear_issue_key &&
        metadata.linear_issue_url && (
          <button
            type="button"
            onClick={() => void openUrl(metadata.linear_issue_url!)}
            data-testid="linear-issue-badge"
            title={metadata.linear_issue_title}
            className={BADGE_CLASS}
          >
            <Zap className="w-3 h-3" />
            {metadata.linear_issue_key}
          </button>
        )}
    </>
  );
};
