import useSWR from "swr";
import {
  AlertTriangle,
  CheckCircle2,
  CircleDot,
  Loader2,
  Play,
  RotateCw,
  ShieldCheck,
  XCircle,
} from "lucide-react";
import { Button } from "../ui/button";
import { listWorkflowRuns } from "../../lib/api";
import type { RunSummary, WorkflowInfo } from "../../lib/api-types";
import type {
  ChecksTarget,
  WorkspaceChecks,
} from "../../hooks/useWorkspaceChecks";
import { ChecksStatusIcon } from "./ChecksStatusIcon";

interface Props {
  target: ChecksTarget;
  checks: WorkspaceChecks;
  /** Hides step results and run history, for the PR detail panel. */
  compact?: boolean;
}

function formatRunTime(ts: string): string {
  const parsed = new Date(ts);
  if (Number.isNaN(parsed.getTime())) return ts;
  return parsed.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** Workflow list with trust prompt, setup script state and run controls. */
export function WorkspaceChecksPanel({ target, checks, compact }: Props) {
  if (checks.isLoading) {
    return (
      <div className="flex items-center gap-2 p-4 text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" />
        Loading checks…
      </div>
    );
  }

  const setup = checks.setupStatus;
  return (
    <div data-testid="workspace-checks" className="flex flex-col gap-3 p-4">
      {checks.workflows.length > 0 && !checks.trusted && (
        <div className="flex items-center justify-between rounded-md border border-amber-200 bg-amber-50 px-4 py-3 text-sm dark:border-amber-900 dark:bg-amber-950">
          <div className="flex items-center gap-2 text-amber-800 dark:text-amber-200">
            <ShieldCheck className="h-4 w-4 shrink-0" />
            <span>
              Trust this repository to enable running workflow checks.
            </span>
          </div>
          <Button
            size="sm"
            variant="outline"
            className="ml-4 shrink-0"
            onClick={() => void checks.trust()}
          >
            Trust Repository
          </Button>
        </div>
      )}

      {setup?.configured && (
        <div
          data-testid="setup-script-status"
          className="flex items-center justify-between gap-2 rounded-md border px-4 py-2 text-sm"
        >
          <div className="flex items-center gap-2 min-w-0">
            {setup.status === "failed" ? (
              <AlertTriangle className="h-4 w-4 shrink-0 text-destructive" />
            ) : (
              <ChecksStatusIcon status={setup.status} />
            )}
            <span>
              {checks.setupRunning
                ? "Running workspace setup script before checks can run…"
                : setup.status === "failed"
                  ? "Workspace setup script failed. Checks can still run."
                  : `Setup script: ${setup.status}`}
            </span>
          </div>
          <Button
            size="sm"
            variant="ghost"
            className="shrink-0"
            disabled={checks.rerunningSetup || checks.setupRunning}
            onClick={() => void checks.rerunSetup()}
          >
            {checks.rerunningSetup ? (
              <Loader2 className="h-3 w-3 animate-spin mr-1" />
            ) : (
              <RotateCw className="h-3 w-3 mr-1" />
            )}
            Re-run setup script
          </Button>
        </div>
      )}

      {checks.workflows.length === 0 ? (
        <div className="text-sm text-muted-foreground">
          No workflows found. Add YAML files to{" "}
          <code className="font-mono">.treq/workflows/</code> to define checks.
        </div>
      ) : (
        checks.workflows.map((wf) => (
          <WorkflowCard
            key={wf.filename}
            workflow={wf}
            target={target}
            checks={checks}
            latestRun={checks.latestRuns[wf.filename]}
            compact={compact}
          />
        ))
      )}
    </div>
  );
}

function RunButton({
  label,
  rerun,
  running,
  disabled,
  onClick,
  variant,
}: {
  label: string;
  rerun: boolean;
  running: boolean;
  disabled: boolean;
  onClick: () => void;
  variant: "outline" | "ghost";
}) {
  const verb = rerun ? "Re-run" : "Run";
  return (
    <Button
      size="sm"
      variant={variant}
      aria-label={`${verb} ${label}`}
      disabled={disabled || running}
      onClick={onClick}
    >
      {running ? (
        <Loader2 className="h-3 w-3 animate-spin mr-1" />
      ) : rerun ? (
        <RotateCw className="h-3 w-3 mr-1" />
      ) : (
        <Play className="h-3 w-3 mr-1" />
      )}
      {verb}
    </Button>
  );
}

function WorkflowCard({
  workflow: wf,
  target,
  checks,
  latestRun,
  compact,
}: {
  workflow: WorkflowInfo;
  target: ChecksTarget;
  checks: WorkspaceChecks;
  latestRun: RunSummary | undefined;
  compact?: boolean;
}) {
  const workflowRunning = checks.isRunning(wf.filename);
  const status = workflowRunning ? "running" : latestRun?.status;
  return (
    <div
      data-testid="workflow-card"
      className="rounded-md border bg-card text-card-foreground"
    >
      <div className="flex items-center justify-between gap-2 px-4 py-2 border-b">
        <div className="flex items-center gap-2 min-w-0">
          <ChecksStatusIcon status={status} />
          <div className="min-w-0">
            <div className="font-medium truncate">{wf.name}</div>
            <div className="text-xs text-muted-foreground font-mono truncate">
              {wf.filename}
              {latestRun && ` · run #${latestRun.id} ${latestRun.status}`}
            </div>
          </div>
        </div>
        <RunButton
          label={wf.name}
          rerun={!!latestRun}
          running={workflowRunning}
          disabled={!checks.canRun}
          variant="outline"
          onClick={() => void checks.runWorkflow(wf).catch(() => undefined)}
        />
      </div>

      <div className="divide-y">
        {wf.jobs.map((job) => {
          const runJob = latestRun?.jobs.find((j) => j.job_id === job.id);
          return (
            <div key={job.id} className="px-4 py-2">
              <div className="flex items-center justify-between">
                <span className="flex items-center gap-2 text-sm">
                  <ChecksStatusIcon
                    status={
                      checks.isRunning(wf.filename, job.id)
                        ? "running"
                        : runJob?.status
                    }
                    className="h-3.5 w-3.5"
                  />
                  {job.name}
                </span>
                <RunButton
                  label={`job ${job.name}`}
                  rerun={!!runJob}
                  running={checks.isRunning(wf.filename, job.id)}
                  disabled={!checks.canRun || workflowRunning}
                  variant="ghost"
                  onClick={() =>
                    void checks.runJob(wf, job.id).catch(() => undefined)
                  }
                />
              </div>
              {!compact && (
                <div className="flex flex-col gap-1 ml-6 mt-1">
                  {job.steps.map((step, idx) => {
                    const stepResult = runJob?.steps[idx];
                    return (
                      <div
                        key={`${idx}-${step.name}`}
                        className="flex items-center gap-2 text-sm"
                      >
                        {stepResult === undefined ? (
                          <CircleDot className="h-4 w-4 text-muted-foreground shrink-0" />
                        ) : stepResult.success ? (
                          <CheckCircle2
                            data-testid="step-result-pass"
                            className="h-4 w-4 text-green-500 shrink-0"
                          />
                        ) : (
                          <XCircle
                            data-testid="step-result-fail"
                            className="h-4 w-4 text-red-500 shrink-0"
                          />
                        )}
                        <span>{step.name}</span>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {!compact && <RunHistory target={target} filename={wf.filename} />}
    </div>
  );
}

function RunHistory({
  target,
  filename,
}: {
  target: ChecksTarget;
  filename: string;
}) {
  const { repoPath, workspaceId } = target;
  const { data: runs = [] } = useSWR(
    ["workflow-runs", repoPath, workspaceId, filename],
    () => listWorkflowRuns(repoPath, workspaceId, filename),
  );
  if (runs.length === 0) return null;
  return (
    <details className="border-t">
      <summary className="cursor-pointer px-4 py-2 text-xs font-medium text-muted-foreground">
        Run history ({runs.length})
      </summary>
      <div className="divide-y">
        {runs.map((run) => (
          <div
            key={run.id}
            data-testid="run-history-item"
            className="flex items-center gap-2 px-4 py-1.5 text-sm"
          >
            <ChecksStatusIcon status={run.status} />
            <span className="font-mono text-xs">#{run.id}</span>
            <span className="text-muted-foreground text-xs">
              {formatRunTime(run.started_at)}
            </span>
          </div>
        ))}
      </div>
    </details>
  );
}
