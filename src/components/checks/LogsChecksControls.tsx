import { Loader2, Play, RotateCw, ShieldCheck } from "lucide-react";
import { Button } from "../ui/button";
import type { WorkflowInfo } from "../../lib/api-types";
import type { WorkspaceChecks } from "../../hooks/useWorkspaceChecks";
import { ChecksStatusIcon } from "./ChecksStatusIcon";

/** Picks which workflow's check logs to show. An empty value means all. */
export function CheckWorkflowFilter({
  workflows,
  value,
  onChange,
}: {
  workflows: WorkflowInfo[];
  value: string;
  onChange: (filename: string) => void;
}) {
  return (
    <select
      aria-label="Filter by check"
      data-testid="check-workflow-filter"
      className="h-8 rounded-md border bg-background px-2 text-sm min-w-[130px]"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    >
      <option value="">All checks</option>
      {workflows.map((wf) => (
        <option key={wf.filename} value={wf.filename}>
          {wf.name}
        </option>
      ))}
    </select>
  );
}

/**
 * Run controls next to the check filter: runs the selected workflow, or all
 * of them when no workflow is selected, plus the trust prompt and setup
 * script re-run.
 */
export function LogsChecksActions({
  checks,
  workflow,
}: {
  checks: WorkspaceChecks;
  workflow: string;
}) {
  if (checks.workflows.length === 0) return null;
  const selected = checks.workflows.find((wf) => wf.filename === workflow);
  const latestRun = selected ? checks.latestRuns[selected.filename] : undefined;
  const running = selected
    ? checks.isRunning(selected.filename)
    : checks.summary.state === "running";
  const status = selected
    ? running
      ? "running"
      : latestRun?.status
    : checks.summary.state;
  const verb = selected && latestRun ? "Re-run" : "Run";
  const label = selected ? `${verb} ${selected.name}` : "Run all checks";
  const setup = checks.setupStatus;

  return (
    <div data-testid="logs-checks-actions" className="flex items-center gap-2">
      <span
        data-testid="selected-check-status"
        title={`Latest run: ${status ?? "not run"}`}
      >
        <ChecksStatusIcon status={status} />
      </span>
      <Button
        size="sm"
        variant="outline"
        aria-label={label}
        disabled={!checks.canRun || running}
        onClick={() =>
          void (
            selected ? checks.runWorkflow(selected) : checks.runAll()
          ).catch(() => undefined)
        }
      >
        {running ? (
          <Loader2 className="h-3 w-3 animate-spin mr-1" />
        ) : verb === "Re-run" ? (
          <RotateCw className="h-3 w-3 mr-1" />
        ) : (
          <Play className="h-3 w-3 mr-1" />
        )}
        {selected ? verb : "Run all"}
      </Button>
      {setup?.configured && (
        <Button
          size="sm"
          variant="ghost"
          aria-label="Re-run setup script"
          title={`Setup script: ${setup.status}`}
          disabled={checks.rerunningSetup || checks.setupRunning}
          onClick={() => void checks.rerunSetup()}
        >
          {checks.rerunningSetup || checks.setupRunning ? (
            <Loader2 className="h-3 w-3 animate-spin mr-1" />
          ) : (
            <ChecksStatusIcon status={setup.status} className="h-3 w-3 mr-1" />
          )}
          Setup
        </Button>
      )}
      {!checks.trusted && (
        <Button
          size="sm"
          variant="outline"
          className="border-amber-300 text-amber-800 dark:border-amber-800 dark:text-amber-200"
          title="Trust this repository to enable running workflow checks"
          onClick={() => void checks.trust()}
        >
          <ShieldCheck className="h-3 w-3 mr-1" />
          Trust Repository
        </Button>
      )}
    </div>
  );
}
