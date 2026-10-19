// @include-parallel
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import type { LinearIssue } from "../../src/lib/api-linear";
import { findGpuiScreen, sendGpuiScreenAction } from "../gpui-screen";
import { sendGpuiAction } from "../gpui-sidebar";
import { render, screen, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

const linearApi = vi.hoisted(() => ({
  linearListIssues: vi.fn(),
  linearListTeams: vi.fn(),
  linearGetViewer: vi.fn(),
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
  sub_issue_ids: ["child-id"],
  url: "https://linear.app/treq/issue/TREQ-281",
};

describe("GPUI Linear screen", () => {
  let repoPath: string;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    linearApi.linearListTeams.mockResolvedValue([]);
    linearApi.linearListIssues.mockResolvedValue([issue]);
    linearApi.linearGetViewer.mockResolvedValue({ id: "v", name: "Viewer" });
  });

  it("asks GPUI for the repository only", async () => {
    render(<Dashboard />);
    sendGpuiAction({ kind: "openLinear" });

    const linear = await findGpuiScreen("linear");
    expect(Object.keys(linear).sort()).toEqual(["dark", "kind", "repoPath"]);
    expect(linear.repoPath).toBe(repoPath);
  });

  it("opens the agent prompt for the issue GPUI kicks off", async () => {
    render(<Dashboard />);
    sendGpuiAction({ kind: "openLinear" });
    await findGpuiScreen("linear");

    sendGpuiScreenAction({
      kind: "field",
      field: "linear.kickoff",
      value: {
        id: issue.id,
        identifier: issue.identifier,
        url: issue.url,
        title: issue.title,
        includeSubissues: true,
        sub_issue_ids: issue.sub_issue_ids,
      },
    });

    const dialog = (
      await screen.findByRole("heading", { name: "Start a new agent session" })
    ).closest('[data-testid="modal"]') as HTMLElement;
    expect(await within(dialog).findByTestId("issue-chip")).toHaveTextContent(
      "TREQ-281",
    );
  }, 60000);

  it("starts the review session GPUI prepared in the home repository", async () => {
    render(<Dashboard />);
    sendGpuiAction({ kind: "openLinear" });
    await findGpuiScreen("linear");

    sendGpuiScreenAction({
      kind: "field",
      field: "linear.startReview",
      value: { prompt: "Review the issue", agent: null },
    });

    const workspace = await findGpuiScreen("workspace");
    expect(workspace.repoPath).toBe(repoPath);
    expect(workspace.workspaceId).toBeNull();
  }, 60000);
});
