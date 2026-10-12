import { act, renderHook, waitFor } from "@testing-library/react";
import React from "react";
import { SWRConfig } from "swr";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../lib/api";
import type { JobResult, RunSummary, WorkflowInfo } from "../lib/api-types";
import { createTestSWRConfig, SWRMutateScope } from "../lib/swr-cache";
import { checksTargetOf, useWorkspaceChecks } from "./useWorkspaceChecks";

vi.mock("../lib/api", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    isRepoTrusted: vi.fn(),
    trustRepo: vi.fn(),
    listWorkflows: vi.fn(),
    listWorkflowRuns: vi.fn(),
    getWorkspaceSetupStatus: vi.fn(),
    runWorkflow: vi.fn(),
    runWorkflowJob: vi.fn(),
    rerunWorkspaceSetupScript: vi.fn(),
  };
});

const CI: WorkflowInfo = {
  filename: "ci.yaml",
  name: "CI",
  jobs: [{ id: "test", name: "Test", steps: [{ name: "Unit" }] }],
};
const LINT: WorkflowInfo = { filename: "lint.yaml", name: "Lint", jobs: [] };
const PASSED: RunSummary = {
  id: 7,
  filename: "ci.yaml",
  status: "passed",
  started_at: "2026-01-01T00:00:00Z",
  completed_at: null,
  jobs: [],
};
const TARGET = { repoPath: "/repo", workspaceId: 3, workspacePath: "/ws" };

function wrapper({ children }: { children: React.ReactNode }) {
  return React.createElement(
    SWRConfig,
    { value: createTestSWRConfig() },
    React.createElement(SWRMutateScope, null, children),
  );
}

beforeEach(() => {
  vi.mocked(api.isRepoTrusted).mockResolvedValue(true);
  vi.mocked(api.listWorkflows).mockResolvedValue([CI, LINT]);
  vi.mocked(api.listWorkflowRuns).mockImplementation(async (_r, _w, file) =>
    file === "ci.yaml" ? [PASSED] : [],
  );
  vi.mocked(api.getWorkspaceSetupStatus).mockResolvedValue({
    configured: false,
    status: "not_configured",
    run_id: null,
  });
  vi.mocked(api.runWorkflow).mockReset();
});

describe("useWorkspaceChecks", () => {
  it("does nothing without a target", () => {
    const { result } = renderHook(() => useWorkspaceChecks(null), { wrapper });
    expect(result.current.summary.state).toBe("none");
    expect(api.listWorkflows).not.toHaveBeenCalledWith(undefined);
  });

  it("loads workflows and the latest run of each", async () => {
    const { result } = renderHook(() => useWorkspaceChecks(TARGET), {
      wrapper,
    });
    await waitFor(() =>
      expect(result.current.latestRuns["ci.yaml"]).toEqual(PASSED),
    );
    expect(result.current.canRun).toBe(true);
    expect(result.current.summary).toMatchObject({
      state: "not-run",
      passed: 1,
      total: 2,
    });
    expect(api.listWorkflowRuns).toHaveBeenCalledWith("/repo", 3, "ci.yaml", 1);
  });

  it("marks a workflow running until its run resolves", async () => {
    let finish: (value: JobResult[]) => void = () => undefined;
    vi.mocked(api.runWorkflow).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const { result } = renderHook(() => useWorkspaceChecks(TARGET), {
      wrapper,
    });
    await waitFor(() => expect(result.current.workflows).toHaveLength(2));

    let run: Promise<JobResult[]> = Promise.resolve([]);
    act(() => {
      run = result.current.runWorkflow(CI);
    });
    expect(result.current.isRunning("ci.yaml")).toBe(true);
    expect(result.current.summary.state).toBe("running");
    expect(api.runWorkflow).toHaveBeenCalledWith("/repo", "ci.yaml", 3, "/ws");

    await act(async () => {
      finish([{ job_id: "test", steps: [], success: true }]);
      await run;
    });
    expect(result.current.isRunning("ci.yaml")).toBe(false);
  });

  it("runAll runs every workflow", async () => {
    vi.mocked(api.runWorkflow).mockResolvedValue([]);
    const { result } = renderHook(() => useWorkspaceChecks(TARGET), {
      wrapper,
    });
    await waitFor(() => expect(result.current.workflows).toHaveLength(2));
    await act(async () => {
      await result.current.runAll();
    });
    expect(api.runWorkflow).toHaveBeenCalledTimes(2);
  });

  it("blocks runs while the setup script is running", async () => {
    vi.mocked(api.getWorkspaceSetupStatus).mockResolvedValue({
      configured: true,
      status: "running",
      run_id: 1,
    });
    const { result } = renderHook(() => useWorkspaceChecks(TARGET), {
      wrapper,
    });
    await waitFor(() => expect(result.current.setupRunning).toBe(true));
    expect(result.current.canRun).toBe(false);
  });

  it("builds a target from a workspace", () => {
    expect(
      checksTargetOf({
        id: 5,
        repo_path: "/repo",
        workspace_path: "feat",
      } as Parameters<typeof checksTargetOf>[0]),
    ).toEqual({
      repoPath: "/repo",
      workspaceId: 5,
      workspacePath: "/repo/.treq/workspaces/feat",
    });
  });
});
