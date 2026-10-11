// @include-parallel
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { createWorkspace } from "../../src/lib/api";
import type { LinearIssue } from "../../src/lib/api-linear";
import { installFakeAgents } from "../fake-agent";
import { render, screen, waitFor, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

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

const ISSUE: LinearIssue = {
  id: "issue-1",
  identifier: "ENG-101",
  title: "Rework the ranking pipeline",
  description: "",
  state: { name: "Todo", type: "unstarted" },
  labels: [],
  branch_name: "eng-101-rework-the-ranking-pipeline",
  parent_id: null,
  sub_issue_ids: [],
  url: "https://linear.app/acme/issue/ENG-101",
};

async function openPromptDialog() {
  return (
    await screen.findByRole("heading", { name: "Start a new agent session" })
  ).closest('[data-testid="modal"]') as HTMLElement;
}

describe("session started feedback", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let repoPath: string;
  let restoreAgents: () => void;

  beforeEach(() => {
    restoreAgents = installFakeAgents();
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
    linearApi.linearListTeams.mockResolvedValue([]);
    linearApi.linearListIssues.mockResolvedValue([ISSUE]);
    linearApi.linearGetViewer.mockResolvedValue({ id: "me", name: "Me" });
    linearApi.linearOpenOrCreateWorkspaceFromIssue.mockImplementation(
      async (path: string) => ({
        issue_id: ISSUE.id,
        workspace_id: await createWorkspace(path, ISSUE.branch_name),
        created: true,
      }),
    );
  });

  afterEach(() => {
    restoreAgents();
  });

  it("starts a Linear session without showing a toast", async () => {
    render(<Dashboard />);
    await user.click(await screen.findByTestId("linear-sidebar-item"));
    await user.click(await screen.findByRole("button", { name: "Kick off" }));
    const dialog = await openPromptDialog();
    await user.type(
      within(dialog).getByPlaceholderText("Describe a task..."),
      "Rework it",
    );
    await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));

    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    expect(screen.getByTestId("linear-panel")).toBeInTheDocument();
    expect(screen.queryByText("Session started")).not.toBeInTheDocument();
    await user.click(
      await screen.findByTestId(`workspace-sidebar-item-${ISSUE.branch_name}`),
    );
    const pane = await screen.findByTestId("workspace-terminal-pane");
    expect(
      await within(pane).findByText(
        "prompt: Rework it",
        {},
        { timeout: 15000 },
      ),
    ).toBeInTheDocument();
  }, 60000);

  it("starts a session in another workspace without showing a toast", async () => {
    await createWorkspace(repoPath, "feat/elsewhere");
    render(<Dashboard />);
    await screen.findByTestId("show-workspace-header");
    await user.keyboard("{Meta>}i{/Meta}");
    const dialog = await openPromptDialog();
    await user.click(
      dialog.querySelector('button[role="combobox"]') as HTMLElement,
    );
    await user.click(
      await screen.findByRole("option", { name: "feat/elsewhere" }),
    );
    await user.type(
      within(dialog).getByPlaceholderText("Describe a task..."),
      "Do the thing",
    );
    await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));

    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    expect(screen.queryByText("Session started")).not.toBeInTheDocument();
    await user.click(
      await screen.findByTestId("workspace-sidebar-item-feat/elsewhere"),
    );
    const pane = await screen.findByTestId("workspace-terminal-pane");
    expect(
      await within(pane).findByText(
        "prompt: Do the thing",
        {},
        { timeout: 15000 },
      ),
    ).toBeInTheDocument();
  }, 60000);

  it("stays quiet when the new session's terminal is already on screen", async () => {
    render(<Dashboard />);
    await screen.findByTestId("show-workspace-header");
    await user.keyboard("{Meta>}i{/Meta}");
    const dialog = await openPromptDialog();
    await user.type(
      within(dialog).getByPlaceholderText("Describe a task..."),
      "Right here",
    );
    await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));

    const pane = await screen.findByTestId("workspace-terminal-pane");
    await within(pane).findByText("prompt: Right here", {}, { timeout: 15000 });
    await waitFor(() =>
      expect(screen.queryByText("Session started")).not.toBeInTheDocument(),
    );
  }, 60000);
});
