/**
 * A session started away from the visible terminal is confirmed with a
 * "Session started" toast; its Open action shows the session's workspace
 * and the fake agent's terminal output.
 */
import userEvent from "@testing-library/user-event";
import { expect, it, onTestFinished, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import type { LinearIssue } from "../../../src/lib/api-linear";
import { installFakeAgents } from "../../../test/fake-agent";
import { render, screen, within } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import { captureDocument } from "../capture";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

const linearApi = vi.hoisted(() => ({
  linearListIssues: vi.fn(),
  linearListTeams: vi.fn(),
  linearGetViewer: vi.fn(),
  linearOpenOrCreateWorkspaceFromIssue: vi.fn(),
}));

vi.mock("../../../src/lib/api-linear", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../src/lib/api-linear")>()),
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

it("captures the session-started toast and its Open action", async () => {
  onTestFinished(installFakeAgents());
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
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
  const user = userEvent.setup();
  render(<Dashboard />);

  sendGpuiAction({ kind: "openLinear" });
  await user.click(await screen.findByRole("button", { name: "Kick off" }));
  const dialog = (
    await screen.findByRole("heading", { name: "Start a new agent session" })
  ).closest('[data-testid="modal"]') as HTMLElement;
  await user.type(
    within(dialog).getByPlaceholderText("Describe a task..."),
    "Rework it",
  );
  await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));

  const toast = (await screen.findByText("Session started")).closest(
    '[role="status"]',
  ) as HTMLElement;
  expect(toast).toHaveTextContent(`in ${ISSUE.branch_name}`);
  await captureDocument(document, {
    name: "session-started-toast-01-linear",
    expectations: [
      "The Linear panel is still the page on screen; the app did not navigate away.",
      `A toast in the bottom-left corner with a green check reads 'Session started' and 'in ${ISSUE.branch_name}', with an 'Open' link below.`,
    ],
  });

  await user.click(within(toast).getByRole("button", { name: "Open" }));
  expect(await screen.findByTestId("show-workspace-header")).toHaveTextContent(
    ISSUE.branch_name,
  );
  await within(screen.getByTestId("workspace-terminal-pane")).findByText(
    /prompt: Rework it$/,
    {},
    { timeout: 15000 },
  );
  await captureDocument(document, {
    name: "session-started-toast-02-opened",
    expectations: [
      `The workspace header shows '${ISSUE.branch_name}'.`,
      "The agent terminal shows the fake agent's output, including 'fake-agent: claude' and 'prompt: Rework it'.",
      "The 'Session started' toast is gone.",
    ],
  });
}, 120000);
