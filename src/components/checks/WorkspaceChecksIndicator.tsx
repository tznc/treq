import { Play } from "lucide-react";
import { Button } from "../ui/button";
import type { ChecksSummary } from "../../lib/checks-summary";
import type { WorkspaceChecks } from "../../hooks/useWorkspaceChecks";
import { ChecksStatusIcon } from "./ChecksStatusIcon";

function summaryLabel(summary: ChecksSummary): string {
  if (summary.state === "running") return "Checks running";
  if (summary.state === "not-run") return "Checks not run";
  return `Checks ${summary.passed}/${summary.total}`;
}

/**
 * Compact pass/fail summary of the latest workflow runs with a run-all
 * button. Renders nothing when the repo defines no workflows.
 */
export function WorkspaceChecksIndicator({
  checks,
  onOpenDetails,
}: {
  checks: WorkspaceChecks;
  onOpenDetails?: () => void;
}) {
  const { summary } = checks;
  if (summary.state === "none") return null;
  const label = summaryLabel(summary);
  return (
    <div
      data-testid="workspace-checks-indicator"
      data-state={summary.state}
      className="inline-flex items-center rounded-md border h-8"
    >
      <button
        type="button"
        className="inline-flex items-center gap-1.5 px-2 h-full text-xs disabled:cursor-default"
        aria-label={onOpenDetails ? `${label}, open in Logs` : label}
        title={onOpenDetails ? "Open check logs" : undefined}
        disabled={!onOpenDetails}
        onClick={onOpenDetails}
      >
        <ChecksStatusIcon status={summary.state} className="h-3.5 w-3.5" />
        {label}
      </button>
      <Button
        size="sm"
        variant="ghost"
        className="h-full px-2 rounded-l-none border-l"
        aria-label="Run all checks"
        title={
          checks.trusted ? "Run all checks" : "Trust the repository to run"
        }
        disabled={!checks.canRun || summary.state === "running"}
        onClick={() => void checks.runAll().catch(() => undefined)}
      >
        <Play className="h-3 w-3" />
      </Button>
    </div>
  );
}
