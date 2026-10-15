import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import * as api from "../../lib/api";
import type { RunSummary, Workspace, WorkflowInfo } from "../../lib/api-types";
import { useWorkspaceChecks } from "../../hooks/useWorkspaceChecks";
import { useOpenLogsRequest } from "../../lib/open-logs-request";
import { useFeaturePreviewStore } from "../../stores/featurePreviewStore";
import { CommandPalette } from "../CommandPalette";
import { PrWorkspaceChecks } from "../github-panel/PrWorkspaceChecks";
import { ChecksPopover } from "./ChecksPopover";
import { WorkspaceChecksPanel } from "./WorkspaceChecksPanel";

vi.mock("../../lib/api", async () => {
  const actual =
    await vi.importActual<typeof import("../../lib/api")>("../../lib/api");
  return {
    ...actual,
    isRepoTrusted: vi.fn(),
    trustRepo: vi.fn(),
    listWorkflows: vi.fn(),
    listWorkflowRuns: vi.fn(),
    getRunLogs: vi.fn(),
    getWorkspaceSetupStatus: vi.fn(),
    runWorkflow: vi.fn(),
    runWorkflowJob: vi.fn(),
    rerunWorkspaceSetupScript: vi.fn(),
    getCachedPrInfo: vi.fn(async () => null),
  };
});

const CI: WorkflowInfo = {
  filename: "ci.yaml",
  name: "CI",
  jobs: [{ id: "test", name: "Test", steps: [{ name: "Unit" }] }],
};
const LINT: WorkflowInfo = {
  filename: "lint.yaml",
  name: "Lint",
  jobs: [{ id: "lint", name: "Lint job", steps: [{ name: "Biome" }] }],
};
const CI_FAILED: RunSummary = {
  id: 9,
  filename: "ci.yaml",
  status: "failed",
  started_at: "2026-01-01T00:00:00Z",
  completed_at: "2026-01-01T00:00:05Z",
  jobs: [
    {
      job_id: "test",
      status: "failed",
      steps: [{ name: "Unit", success: false }],
      has_logs: true,
    },
  ],
};
const TARGET = { repoPath: "/repo", workspaceId: 3, workspacePath: "/ws" };
const WORKSPACE = {
  id: 3,
  repo_path: "/repo",
  workspace_path: "/ws",
  branch_name: "feat/x",
} as Workspace;

function Harness({ indicator }: { indicator?: () => void }) {
  const checks = useWorkspaceChecks(TARGET);
  return (
    <>
      <ChecksPopover checks={checks} onOpenLogs={indicator} />
      <WorkspaceChecksPanel checks={checks} />
    </>
  );
}

beforeEach(() => {
  vi.mocked(api.isRepoTrusted).mockResolvedValue(true);
  vi.mocked(api.trustRepo).mockResolvedValue(undefined);
  vi.mocked(api.listWorkflows).mockResolvedValue([CI, LINT]);
  vi.mocked(api.listWorkflowRuns).mockImplementation(async (_r, _w, file) =>
    file === "ci.yaml" ? [CI_FAILED] : [],
  );
  vi.mocked(api.getWorkspaceSetupStatus).mockResolvedValue({
    configured: true,
    status: "passed",
    run_id: 1,
  });
  vi.mocked(api.runWorkflow).mockReset().mockResolvedValue([]);
  vi.mocked(api.runWorkflowJob).mockReset();
  vi.mocked(api.rerunWorkspaceSetupScript).mockReset();
});

describe("WorkspaceChecksPanel", () => {
  it("shows the logs of a past run from run history", async () => {
    const user = userEvent.setup();
    vi.mocked(api.getRunLogs).mockResolvedValue([
      { body: "1 test failed", stream: "stderr" },
    ] as Awaited<ReturnType<typeof api.getRunLogs>>);
    render(<Harness />);

    await user.click(
      await screen.findByRole("button", { name: "Run history" }),
    );
    await user.click(await screen.findByRole("button", { name: "Run #9" }));
    expect(await screen.findByText("[stderr] 1 test failed")).toBeTruthy();
    expect(api.getRunLogs).toHaveBeenCalledWith("/repo", 9, "test");
  });

  it("shows latest status and re-runs workflows and jobs", async () => {
    const user = userEvent.setup();
    vi.mocked(api.runWorkflowJob).mockResolvedValue({
      job_id: "test",
      steps: [],
      success: true,
    });
    render(<Harness />);

    await user.click(await screen.findByRole("button", { name: "Re-run CI" }));
    expect(api.runWorkflow).toHaveBeenCalledWith("/repo", "ci.yaml", 3, "/ws");
    expect(screen.getByRole("button", { name: "Run Lint" })).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Re-run job Test" }));
    expect(api.runWorkflowJob).toHaveBeenCalledWith(
      "/repo",
      "ci.yaml",
      "test",
      3,
      "/ws",
    );
  });

  it("asks to trust the repo before runs are allowed", async () => {
    const user = userEvent.setup();
    vi.mocked(api.isRepoTrusted).mockResolvedValue(false);
    render(<Harness />);

    const trust = await screen.findByRole("button", {
      name: "Trust Repository",
    });
    expect(
      (screen.getByRole("button", { name: "Re-run CI" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    await user.click(trust);
    expect(api.trustRepo).toHaveBeenCalledWith("/repo");
  });

  it("re-runs the setup script", async () => {
    const user = userEvent.setup();
    vi.mocked(api.rerunWorkspaceSetupScript).mockResolvedValue({
      job_id: "setup",
      steps: [],
      success: true,
    });
    render(<Harness />);
    await user.click(
      await screen.findByRole("button", { name: /Re-run setup script/ }),
    );
    expect(api.rerunWorkspaceSetupScript).toHaveBeenCalledWith(
      "/repo",
      3,
      "/ws",
    );
  });
});

describe("ChecksPopover", () => {
  it("summarizes the latest runs, runs all and opens logs", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    render(<Harness indicator={onOpen} />);

    const trigger = await screen.findByTestId("workspace-checks-button");
    await waitFor(() => expect(trigger.dataset.checksState).toBe("failed"));
    expect(trigger.textContent).toContain("0/2");
    await user.click(trigger);

    const popover = await screen.findByTestId("checks-popover");
    const rows = within(popover).getAllByTestId("checks-popover-workflow");
    expect(rows.map((row) => row.textContent)).toEqual([
      "CIrun #9 failed",
      "Lintnot run",
    ]);
    await user.click(
      within(popover).getByRole("button", { name: "Run all checks" }),
    );
    expect(api.runWorkflow).toHaveBeenCalledTimes(2);

    await user.click(
      within(popover).getByRole("button", { name: "View logs" }),
    );
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it("offers Trust Repository and disables Run all while untrusted", async () => {
    const user = userEvent.setup();
    vi.mocked(api.isRepoTrusted).mockResolvedValue(false);
    render(<Harness />);
    await user.click(await screen.findByTestId("workspace-checks-button"));
    const popover = await screen.findByTestId("checks-popover");
    const runAll = within(popover).getByRole("button", {
      name: "Run all checks",
    });
    expect((runAll as HTMLButtonElement).disabled).toBe(true);
    await user.click(
      await within(popover).findByRole("button", { name: "Trust Repository" }),
    );
    expect(api.trustRepo).toHaveBeenCalledWith("/repo");
  });

  it("renders nothing without workflows or PR checks", async () => {
    vi.mocked(api.listWorkflows).mockResolvedValue([]);
    render(<Harness />);
    await screen.findByText(/No workflows found/);
    expect(screen.queryByTestId("workspace-checks-button")).toBeNull();
  });
});

describe("PrWorkspaceChecks", () => {
  it("opens the workspace Logs tab when Logs is enabled", async () => {
    const user = userEvent.setup();
    useFeaturePreviewStore.setState((state) => ({
      flags: { ...state.flags, logs: true },
    }));
    const onOpenWorkspace = vi.fn();
    render(
      <PrWorkspaceChecks
        workspace={WORKSPACE}
        onOpenWorkspace={onOpenWorkspace}
      />,
    );
    await screen.findByRole("button", { name: "Re-run CI" });
    await user.click(screen.getByRole("button", { name: "Open in Logs" }));
    expect(onOpenWorkspace).toHaveBeenCalledWith(3);
    expect(useOpenLogsRequest.getState().workspaceId).toBe(3);
  });
});

describe("CommandPalette checks commands", () => {
  it("runs all checks, one workflow, and the setup script", async () => {
    const user = userEvent.setup();
    vi.mocked(api.rerunWorkspaceSetupScript).mockResolvedValue({
      job_id: "setup",
      steps: [],
      success: true,
    });
    const props = {
      showCommandPalette: true,
      onCommandPaletteChange: vi.fn(),
      workspaces: [WORKSPACE],
      onNavigateToDashboard: vi.fn(),
      onNavigateToSettings: vi.fn(),
      onOpenBranchSwitcher: vi.fn(),
      onOpenFilePicker: vi.fn(),
      onOpenWorkspacePicker: vi.fn(),
      onOpenWorkspaceDeletion: vi.fn(),
      onCreateStackedWorkspace: vi.fn(),
      hasSelectedWorkspace: true,
      showBranchSwitcher: false,
      onBranchSwitcherChange: vi.fn(),
      onBranchChanged: vi.fn(),
      showWorkspaceDeletion: false,
      onWorkspaceDeletionChange: vi.fn(),
      currentWorkspace: WORKSPACE,
      onDeleteWorkspace: vi.fn(),
      showFilePicker: false,
      onFilePickerChange: vi.fn(),
      onFileSelected: vi.fn(),
      selectedWorkspaceId: 3,
      repoPath: "/repo",
    };
    render(<CommandPalette {...props} />);

    await user.click(await screen.findByText("Run checks…"));
    expect(api.runWorkflow).toHaveBeenCalledTimes(2);
    expect(await screen.findByText("Checks passed")).toBeTruthy();

    await user.click(await screen.findByText("Run check: Lint"));
    expect(api.runWorkflow).toHaveBeenLastCalledWith(
      "/repo",
      "lint.yaml",
      3,
      "/ws",
    );

    await user.click(await screen.findByText("Re-run setup script"));
    expect(api.rerunWorkspaceSetupScript).toHaveBeenCalledOnce();
  });
});
