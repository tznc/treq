import { userEvent } from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "../../../test/test-utils";
import * as api from "../../lib/api";
import type { RunSummary, WorkflowInfo } from "../../lib/api-types";
import { useWorkspaceChecks } from "../../hooks/useWorkspaceChecks";
import { CheckWorkflowFilter, LogsChecksActions } from "./LogsChecksControls";

vi.mock("../../lib/api", async () => {
  const actual =
    await vi.importActual<typeof import("../../lib/api")>("../../lib/api");
  return {
    ...actual,
    isRepoTrusted: vi.fn(),
    trustRepo: vi.fn(),
    listWorkflows: vi.fn(),
    listWorkflowRuns: vi.fn(),
    getWorkspaceSetupStatus: vi.fn(),
    runWorkflow: vi.fn(),
    rerunWorkspaceSetupScript: vi.fn(),
  };
});

const CI: WorkflowInfo = { filename: "ci.yaml", name: "CI", jobs: [] };
const DOCS: WorkflowInfo = { filename: "docs.yaml", name: "Docs", jobs: [] };
const CI_PASSED: RunSummary = {
  id: 4,
  filename: "ci.yaml",
  status: "passed",
  started_at: "2026-01-01T00:00:00Z",
  completed_at: "2026-01-01T00:00:05Z",
  jobs: [],
};
const TARGET = { repoPath: "/repo", workspaceId: 3, workspacePath: "/ws" };

function Harness() {
  const [workflow, setWorkflow] = useState("");
  const checks = useWorkspaceChecks(TARGET);
  return (
    <>
      <CheckWorkflowFilter
        workflows={checks.workflows}
        value={workflow}
        onChange={setWorkflow}
      />
      <LogsChecksActions checks={checks} workflow={workflow} />
    </>
  );
}

beforeEach(() => {
  vi.mocked(api.isRepoTrusted).mockResolvedValue(true);
  vi.mocked(api.trustRepo).mockResolvedValue(undefined);
  vi.mocked(api.listWorkflows).mockResolvedValue([CI, DOCS]);
  vi.mocked(api.listWorkflowRuns).mockImplementation(async (_r, _w, file) =>
    file === "ci.yaml" ? [CI_PASSED] : [],
  );
  vi.mocked(api.getWorkspaceSetupStatus).mockResolvedValue({
    configured: true,
    status: "passed",
    run_id: 1,
  });
  vi.mocked(api.runWorkflow).mockReset().mockResolvedValue([]);
  vi.mocked(api.rerunWorkspaceSetupScript).mockReset();
});

describe("Logs checks controls", () => {
  it("runs every workflow while All checks is selected", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    const filter = (await screen.findByRole("combobox", {
      name: "Filter by check",
    })) as HTMLSelectElement;
    expect(filter.value).toBe("");
    await screen.findByRole("option", { name: "Docs" });
    await user.click(screen.getByRole("button", { name: "Run all checks" }));
    expect(api.runWorkflow).toHaveBeenCalledTimes(2);
  });

  it("runs only the selected workflow and shows its status", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await screen.findByRole("option", { name: "CI" });
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Filter by check" }),
      "ci.yaml",
    );
    await user.click(await screen.findByRole("button", { name: "Re-run CI" }));
    expect(api.runWorkflow).toHaveBeenCalledOnce();
    expect(api.runWorkflow).toHaveBeenCalledWith("/repo", "ci.yaml", 3, "/ws");
    expect(
      within(screen.getByTestId("selected-check-status")).getByTestId(
        "run-status-passed",
      ),
    ).toBeTruthy();

    await user.selectOptions(
      screen.getByRole("combobox", { name: "Filter by check" }),
      "docs.yaml",
    );
    expect(screen.getByRole("button", { name: "Run Docs" })).toBeTruthy();
  });

  it("asks to trust the repo and re-runs the setup script", async () => {
    const user = userEvent.setup();
    vi.mocked(api.isRepoTrusted).mockResolvedValue(false);
    vi.mocked(api.rerunWorkspaceSetupScript).mockResolvedValue({
      job_id: "setup",
      steps: [],
      success: true,
    });
    render(<Harness />);

    await user.click(
      await screen.findByRole("button", { name: "Trust Repository" }),
    );
    expect(api.trustRepo).toHaveBeenCalledWith("/repo");
    expect(
      (
        screen.getByRole("button", {
          name: "Run all checks",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);

    await user.click(
      screen.getByRole("button", { name: "Re-run setup script" }),
    );
    expect(api.rerunWorkspaceSetupScript).toHaveBeenCalledWith(
      "/repo",
      3,
      "/ws",
    );
  });
});
