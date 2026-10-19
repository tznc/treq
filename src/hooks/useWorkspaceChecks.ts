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

/** Workflow files of workspace `workspaceId` running from this window. */
export function useRunningCheckFiles(workspaceId: number | null): string[] {
  const running = useRunningChecks((state) => state.running);
  if (workspaceId === null) return [];
  const prefix = `${workspaceId}:`;
  const files = Object.keys(running)
    .filter((key) => key.startsWith(prefix))
    .map((key) => key.slice(prefix.length).split(":")[0])
    .filter((file) => file !== "<setup>");
  return [...new Set(files)];
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
 * Every run started from this window, as `runKey`s, for the GPUI GitHub
 * view (src-tauri/src/gpui_shell/screen/github_data.rs `run_key`).
 */
export function useRunningCheckKeys(): string[] {
  const running = useRunningChecks((state) => state.running);
  return Object.keys(running).sort();
}

/** Runs workflow `filename` in `target`, tracked as running. */
export function runWorkspaceWorkflow(target: ChecksTarget, filename: string) {
  return trackRun(target, runKey(target.workspaceId, filename), () =>
    runWorkflowApi(
      target.repoPath,
      filename,
      target.workspaceId,
      target.workspacePath,
    ),
  );
}

/** Runs job `jobId` of workflow `filename` in `target`, tracked as running. */
export function runWorkspaceJob(
  target: ChecksTarget,
  filename: string,
  jobId: string,
) {
  return trackRun(target, runKey(target.workspaceId, filename, jobId), () =>
    runWorkflowJob(
      target.repoPath,
      filename,
      jobId,
      target.workspaceId,
      target.workspacePath,
    ),
  );
}

/** Reruns `target`'s setup script, tracked as running. */
export function rerunWorkspaceSetup(target: ChecksTarget) {
  return trackRun(target, runKey(target.workspaceId, "<setup>"), () =>
    rerunWorkspaceSetupScript(
      target.repoPath,
      target.workspaceId,
      target.workspacePath,
    ),
  );
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
    return runWorkspaceWorkflow(target, wf.filename);
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
      return runWorkspaceJob(target, wf.filename, jobId);
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
        await rerunWorkspaceSetup(target);
      } finally {
        void refetchSetupStatus();
      }
    },
  };
}

export type WorkspaceChecks = ReturnType<typeof useWorkspaceChecks>;
