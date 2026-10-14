// @include-parallel
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import type { LinearIssue } from "../../src/lib/api-linear";
import { render, screen, within } from "../test-utils";
import { createWorkspace } from "../../src/lib/api";
import { createTestRepo, openRepo } from "../utils";
import { findGpuiRow, sendGpuiAction } from "../gpui-sidebar";

const linearApi = vi.hoisted(() => ({
  linearListIssues: vi.fn(),
  linearListTeams: vi.fn(),
  linearGetViewer: vi.fn(),
  linearOpenOrCreateWorkspaceFromIssue: vi.fn(),
}));

vi.mock("../../src/lib/api-linear", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../src/lib/api-linear")>()),
  ...linearApi,
}));

const issue: LinearIssue = {
  id: "issue-id",
  identifier: "TREQ-281",
  title: "Linear integration should CRUD issues",
  description: "Implement the issue workflow",
  state: { name: "Todo", type: "unstarted" },
  labels: [],
  branch_name: "ty/treq-281-linear-integration",
  parent_id: null,
  sub_issue_ids: [],
  url: "https://linear.app/treq/issue/TREQ-281",
};

describe("Linear issue kickoff", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let repoPath: string;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
    linearApi.linearListTeams.mockResolvedValue([]);
    linearApi.linearListIssues.mockResolvedValue([issue]);
    linearApi.linearGetViewer.mockResolvedValue({
      id: "viewer-id",
      name: "Viewer",
    });
  });

  it("opens the agent prompt dialog with the Linear issue attached", async () => {
    render(<Dashboard />);

    sendGpuiAction({ kind: "openLinear" });
    await user.click(await screen.findByRole("tab", { name: "Kanban" }));
    await user.click(await screen.findByRole("button", { name: "Kick off" }));

    const dialog = (
      await screen.findByRole("heading", { name: "Start a new agent session" })
    ).closest('[data-testid="modal"]') as HTMLElement;
    expect(dialog).toBeTruthy();

    const chip = await within(dialog).findByTestId("issue-chip");
    expect(chip).toHaveTextContent("TREQ-281");
    expect(
      within(dialog).getByRole("button", { name: /^edit$/i }),
    ).toBeEnabled();
  }, 60000);

  it("lists the Linear issue's new workspace in the sidebar after submit", async () => {
    linearApi.linearOpenOrCreateWorkspaceFromIssue.mockImplementation(
      async (path: string) => {
        const workspaceId = await createWorkspace(path, issue.branch_name);
        return { issue_id: issue.id, workspace_id: workspaceId, created: true };
      },
    );
    render(<Dashboard />);

    sendGpuiAction({ kind: "openLinear" });
    await user.click(await screen.findByRole("tab", { name: "Kanban" }));
    await user.click(await screen.findByRole("button", { name: "Kick off" }));
    const dialog = (
      await screen.findByRole("heading", { name: "Start a new agent session" })
    ).closest('[data-testid="modal"]') as HTMLElement;
    await user.type(
      within(dialog).getByPlaceholderText("Describe a task..."),
      "Implement CRUD",
    );
    await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));

    expect(linearApi.linearOpenOrCreateWorkspaceFromIssue).toHaveBeenCalledWith(
      repoPath,
      issue.id,
    );
    await findGpuiRow(issue.branch_name);
  }, 60000);
});
