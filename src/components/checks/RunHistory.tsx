import { useState } from "react";
import useSWR from "swr";
import { ChevronDown, ChevronRight, Loader2 } from "lucide-react";
import { getRunLogs, listWorkflowRuns } from "../../lib/api";
import type { RunJobSummary, RunSummary } from "../../lib/api-types";
import type { ChecksTarget } from "../../hooks/useWorkspaceChecks";
import { ChecksStatusIcon } from "./ChecksStatusIcon";

function formatRunTime(ts: string): string {
  const parsed = new Date(ts);
  if (Number.isNaN(parsed.getTime())) return ts;
  return parsed.toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function Chevron({ open }: { open: boolean }) {
  const Icon = open ? ChevronDown : ChevronRight;
  return <Icon className="h-3 w-3 shrink-0 text-muted-foreground" />;
}

/** Collapsible list of a workflow's past runs; each run expands to its job logs. */
export function RunHistory({
  target,
  filename,
}: {
  target: ChecksTarget;
  filename: string;
}) {
  const [open, setOpen] = useState(false);
  const { repoPath, workspaceId } = target;
  const { data: runs } = useSWR(
    open ? ["workflow-runs", repoPath, workspaceId, filename] : null,
    () => listWorkflowRuns(repoPath, workspaceId, filename),
  );
  return (
    <div className="border-t">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-1 px-4 py-2 text-left text-xs font-medium text-muted-foreground hover:bg-muted/50"
      >
        <Chevron open={open} />
        Run history
      </button>
      {open &&
        (runs ? (
          <div className="divide-y">
            {runs.map((run) => (
              <RunHistoryItem key={run.id} run={run} repoPath={repoPath} />
            ))}
          </div>
        ) : (
          <Loader2 className="mx-4 mb-2 h-3 w-3 animate-spin text-muted-foreground" />
        ))}
    </div>
  );
}

function RunHistoryItem({
  run,
  repoPath,
}: {
  run: RunSummary;
  repoPath: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div data-testid="run-history-item">
      <button
        type="button"
        aria-expanded={open}
        aria-label={`Run #${run.id}`}
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-2 px-6 py-1.5 text-left text-sm hover:bg-muted/50"
      >
        <Chevron open={open} />
        <ChecksStatusIcon status={run.status} className="h-3.5 w-3.5" />
        <span className="font-mono text-xs">#{run.id}</span>
        <span className="text-xs text-muted-foreground">
          {formatRunTime(run.started_at)}
        </span>
      </button>
      {open && (
        <div className="flex flex-col gap-2 px-6 pb-3">
          {run.jobs.map((job) => (
            <RunJobLogs
              key={job.job_id}
              runId={run.id}
              job={job}
              repoPath={repoPath}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function RunJobLogs({
  runId,
  job,
  repoPath,
}: {
  runId: number;
  job: RunJobSummary;
  repoPath: string;
}) {
  const { data: logs, error } = useSWR(
    job.has_logs ? ["run-logs", repoPath, runId, job.job_id] : null,
    () => getRunLogs(repoPath, runId, job.job_id),
  );
  return (
    <div data-testid="run-job-logs">
      <div className="mb-1 flex items-center gap-2 text-xs font-medium">
        <ChecksStatusIcon status={job.status} className="h-3.5 w-3.5" />
        {job.job_id}
      </div>
      {!job.has_logs ? (
        <div className="text-xs text-muted-foreground">No logs recorded.</div>
      ) : error ? (
        <div className="text-xs text-destructive">{String(error)}</div>
      ) : !logs ? (
        <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />
      ) : (
        <pre className="max-h-64 overflow-auto whitespace-pre-wrap rounded bg-muted p-2 font-mono text-xs">
          {logs.length === 0
            ? "(empty)"
            : logs
                .map((l) =>
                  l.stream === "stderr" ? `[stderr] ${l.body}` : l.body,
                )
                .join("\n")}
        </pre>
      )}
    </div>
  );
}
