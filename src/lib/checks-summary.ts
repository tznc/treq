import type { RunSummary, WorkflowInfo } from "./api-types";

type ChecksState = "none" | "not-run" | "running" | "passed" | "failed";

export interface ChecksSummary {
  state: ChecksState;
  total: number;
  passed: number;
  failed: number;
  running: number;
}

/**
 * Rolls the latest run of each workflow into one status. Any running workflow
 * makes the whole set "running"; otherwise any failure makes it "failed".
 */
export function summarizeChecks(
  workflows: WorkflowInfo[],
  latestRuns: Record<string, RunSummary | undefined>,
  runningFilenames: string[],
): ChecksSummary {
  let passed = 0;
  let failed = 0;
  let running = 0;
  for (const wf of workflows) {
    const status = runningFilenames.includes(wf.filename)
      ? "running"
      : latestRuns[wf.filename]?.status;
    if (status === "passed") passed++;
    else if (status === "failed") failed++;
    else if (status === "running") running++;
  }
  const total = workflows.length;
  const state: ChecksState =
    total === 0
      ? "none"
      : running > 0
        ? "running"
        : failed > 0
          ? "failed"
          : passed === total
            ? "passed"
            : "not-run";
  return { state, total, passed, failed, running };
}
