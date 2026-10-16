import useSWR from "swr";
import { create } from "zustand";
import {
  getWorkspaceSetupStatus,
  isRepoTrusted,
  listWorkflowRuns,
  listWorkflows,
  rerunWorkspaceSetupScript,
  runWorkflow as runWorkflowApi,
  runWorkflowJob,
  trustRepo,
} from "../lib/api";
import type { RunSummary, WorkflowInfo, Workspace } from "../lib/api-types";
import { getFullWorkspacePath } from "../lib/utils";
import { summarizeChecks } from "../lib/checks-summary";
import { invalidateQueries } from "../lib/swr-cache";

/** The workspace a set of workflow checks runs against. */
export interface ChecksTarget {
  repoPath: string;
  workspaceId: number;
  workspacePath: string;
}

export function checksTargetOf(
  workspace: Workspace | null | undefined,
): ChecksTarget | null {
  if (!workspace) return null;
  return {
    repoPath: workspace.repo_path,
    workspaceId: workspace.id,
    workspacePath: getFullWorkspacePath(workspace),
  };
}

// Shared across every surface so a run started from the command palette
// shows as running in the header, the PR panel and the Logs tab.
const useRunningChecks = create<{ running: Record<string, true> }>(() => ({
  running: {},
}));

function setRunning(key: string, on: boolean) {
  useRunningChecks.setState((state) => {
    const running = { ...state.running };
    if (on) running[key] = true;
    else delete running[key];
    return { running };
  });
}

function runKey(workspaceId: number, filename: string, jobId?: string) {
  return jobId
    ? `${workspaceId}:${filename}:${jobId}`
    : `${workspaceId}:${filename}`;
}

async function trackRun<T>(
  target: ChecksTarget,
  key: string,
  run: () => Promise<T>,
): Promise<T> {
  setRunning(key, true);
  try {
    return await run();
  } finally {
    setRunning(key, false);
    const { repoPath, workspaceId } = target;
    void invalidateQueries(["workflow-runs", repoPath, workspaceId]);
    void invalidateQueries(["workflow-latest-runs", repoPath, workspaceId]);
    void invalidateQueries(["workspace-commits", repoPath, workspaceId]);
    void invalidateQueries(["repo-logs", repoPath]);
  }
}

/**
 * Workflow checks for one workspace: the repo's workflows, trust state,
 * setup script status, the latest run of each workflow, and run actions.
 * Pass null to disable every fetch.
 */
export function useWorkspaceChecks(target: ChecksTarget | null) {
  const repoPath = target?.repoPath;
  const workspaceId = target?.workspaceId;

  const { data: trusted, isLoading: trustLoading } = useSWR(
    repoPath ? ["repo-trusted", repoPath] : null,
    () => isRepoTrusted(repoPath as string),
  );
  const { data: workflowsData, isLoading: workflowsLoading } = useSWR(
    repoPath ? ["workflows", repoPath] : null,
    () => listWorkflows(repoPath as string),
  );
  const workflows = workflowsData ?? [];
  const filenames = workflows.map((wf) => wf.filename);
  const { data: latestRuns = {} } = useSWR(
    target && filenames.length > 0
      ? ["workflow-latest-runs", repoPath, workspaceId, filenames.join("|")]
      : null,
    async () => {
      const entries = await Promise.all(
        filenames.map(
          async (filename) =>
            [
              filename,
              (
                await listWorkflowRuns(
                  repoPath as string,
                  workspaceId as number,
                  filename,
                  1,
                )
              )[0],
            ] as const,
        ),
      );
      return Object.fromEntries(entries) as Record<
        string,
        RunSummary | undefined
      >;
    },
  );
  const { data: setupStatus, mutate: refetchSetupStatus } = useSWR(
    target ? ["workspace-setup-status", repoPath, workspaceId] : null,
    () => getWorkspaceSetupStatus(repoPath as string, workspaceId as number),
    {
      refreshInterval: (data) =>
        data?.status === "pending" || data?.status === "running" ? 1500 : 0,
    },
  );
  const running = useRunningChecks((state) => state.running);
  const setupKey = runKey(workspaceId ?? -1, "<setup>");

  const setupRunning =
    !!setupStatus?.configured &&
    (setupStatus.status === "pending" || setupStatus.status === "running");
  const isRunning = (filename: string, jobId?: string) =>
    workspaceId !== undefined &&
    !!running[runKey(workspaceId, filename, jobId)];
  const runningFilenames = filenames.filter(
    (filename) =>
      isRunning(filename) ||
      Object.keys(running).some((key) =>
        key.startsWith(`${runKey(workspaceId ?? -1, filename)}:`),
      ),
  );

  const runWorkflow = (wf: WorkflowInfo) => {
    if (!target) return Promise.resolve([]);
    return trackRun(target, runKey(target.workspaceId, wf.filename), () =>
      runWorkflowApi(
        target.repoPath,
        wf.filename,
        target.workspaceId,
        target.workspacePath,
      ),
    );
  };

  return {
    target,
    workflows,
    isLoading: trustLoading || workflowsLoading,
    trusted: !!trusted,
    canRun: !!trusted && !setupRunning,
    latestRuns,
    setupStatus,
    setupRunning,
    rerunningSetup: !!running[setupKey],
    summary: summarizeChecks(workflows, latestRuns, runningFilenames),
    isRunning,
    runWorkflow,
    runJob: (wf: WorkflowInfo, jobId: string) => {
      if (!target) return Promise.resolve(undefined);
      return trackRun(
        target,
        runKey(target.workspaceId, wf.filename, jobId),
        () =>
          runWorkflowJob(
            target.repoPath,
            wf.filename,
            jobId,
            target.workspaceId,
            target.workspacePath,
          ),
      );
    },
    runAll: () => Promise.all(workflows.map(runWorkflow)),
    trust: async () => {
      if (!repoPath) return;
      await trustRepo(repoPath);
      void invalidateQueries(["repo-trusted", repoPath]);
    },
    rerunSetup: async () => {
      if (!target) return;
      try {
        await trackRun(target, setupKey, () =>
          rerunWorkspaceSetupScript(
            target.repoPath,
            target.workspaceId,
            target.workspacePath,
          ),
        );
      } finally {
        void refetchSetupStatus();
      }
    },
  };
}

export type WorkspaceChecks = ReturnType<typeof useWorkspaceChecks>;
