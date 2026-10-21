/**
 * Verifies the Linear "Kick off" flow from the Dashboard: Kanban view shows
 * a Kick off button per issue, and clicking it opens the agent prompt dialog
 * with the Linear issue attached as a chip.
 *
 * Linear's HTTP API is mocked at the api-linear boundary (there is no local
 * Linear server); everything else is the real Dashboard + Rust dispatch.
 */

import * as React from "react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import type {
  LinearIssue,
  LinearTeam,
  LinearUser,
} from "../../../src/lib/api-linear";
import {
  createWorkspace,
  getSessions,
  getWorkspaces,
} from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import {
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { installFakeAgents } from "../../../test/fake-agent";
import { captureDocument } from "../capture";
import { findGpuiRow, sendGpuiAction } from "../../../test/gpui-sidebar";

const {
  mockLinearListTeams,
  mockLinearListIssues,
  mockLinearGetViewer,
  mockLinearKickoff,
} = vi.hoisted(() => ({
  mockLinearListTeams: vi.fn(),
  mockLinearListIssues: vi.fn(),
  mockLinearGetViewer: vi.fn(),
  mockLinearKickoff: vi.fn(),
}));

vi.mock("../../../src/lib/api-linear", async () => {
  const actual = await vi.importActual<
    typeof import("../../../src/lib/api-linear")
  >("../../../src/lib/api-linear");
  return {
    ...actual,
    linearListTeams: mockLinearListTeams,
    linearListIssues: mockLinearListIssues,
    linearGetViewer: mockLinearGetViewer,
    linearOpenOrCreateWorkspaceFromIssue: mockLinearKickoff,
  };
});

const TEAMS: LinearTeam[] = [{ id: "team-1", key: "ENG", name: "Engineering" }];
const VIEWER: LinearUser = { id: "user-me", name: "Ty" };

const ISSUES: LinearIssue[] = [
  {
    id: "issue-1",
    identifier: "ENG-101",
    title: "Rework the ranking pipeline",
    description: "Ranking is slow.",
    state: { name: "In Progress", type: "started" },
    labels: ["backend"],
    branch_name: "eng-101",
    parent_id: null,
    sub_issue_ids: ["issue-2"],
    url: "https://linear.app/treq/issue/ENG-101",
    assignee: VIEWER,
    priority: 1,
    priority_label: "Urgent",
    project: null,
  },
  {
    id: "issue-2",
    identifier: "ENG-102",
    title: "Cache ranking features",
    description: "",
    state: { name: "In Progress", type: "started" },
    labels: [],
    branch_name: "eng-102",
    parent_id: "issue-1",
    sub_issue_ids: [],
    url: "https://linear.app/treq/issue/ENG-102",
    assignee: null,
    priority: 2,
    priority_label: "High",
    project: null,
  },
  {
    id: "issue-3",
    identifier: "ENG-103",
    title: "Investigate invoice rounding error",
    description: "",
    state: { name: "Todo", type: "unstarted" },
    labels: ["bug"],
    branch_name: "eng-103",
    parent_id: null,
    sub_issue_ids: [],
    url: "https://linear.app/treq/issue/ENG-103",
    assignee: null,
    priority: 3,
    priority_label: "Medium",
    project: null,
  },
];

it("kicks off an agent prompt from a Linear issue in Kanban view", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  mockLinearListTeams.mockResolvedValue(TEAMS);
  mockLinearListIssues.mockResolvedValue(ISSUES);
  mockLinearGetViewer.mockResolvedValue(VIEWER);

  const user = userEvent.setup();
  render(<Dashboard />);

  sendGpuiAction({ kind: "openLinear" });
  await screen.findByText("Rework the ranking pipeline");
  expect(
    await screen.findAllByRole("button", { name: "Kick off" }),
  ).toHaveLength(3);

  await captureDocument(document, {
    name: "linear-kickoff-01-list-view",
    expectations: [
      "The Linear panel is open in List view with ENG-101 and ENG-103 as root rows.",
      "Sub-issue ENG-102 is indented under ENG-101 with a left border.",
      "Every row (ENG-101, ENG-102, ENG-103) has a Kick off button at its right edge.",
    ],
  });

  await user.click(screen.getByRole("tab", { name: "Kanban" }));
  await screen.findAllByRole("button", { name: "Kick off" });

  await captureDocument(document, {
    name: "linear-kickoff-02-kanban-view",
    expectations: [
      'Kanban columns "In Progress" and "Todo" are shown side by side.',
      "Each card (ENG-101, its sub-issue ENG-102, ENG-103) has a full-width Kick off button.",
    ],
  });

  const card = screen
    .getByText("Rework the ranking pipeline")
    .closest("div") as HTMLElement;
  await user.click(within(card).getByRole("button", { name: "Kick off" }));
  await screen.findByText("Start a new agent session");
  const chip = await screen.findByTestId("issue-chip");
  expect(chip.textContent).toContain("ENG-101");

  await captureDocument(document, {
    name: "linear-kickoff-03-prompt-dialog",
    expectations: [
      'The "Start a new agent session" dialog is open over the Linear panel.',
      "A 'Linear ENG-101' chip with a violet issue icon and a remove (x) button sits above the prompt textarea.",
      'In place of the branch picker, a muted line reads "Opens a workspace for ENG-101 and its sub-issues".',
    ],
  });

  await user.click(screen.getByRole("button", { name: "Remove Linear issue" }));
  await waitFor(() => {
    if (screen.queryByText(/Opens a workspace for/))
      throw new Error("picker not restored yet");
  });

  await captureDocument(document, {
    name: "linear-kickoff-04-issue-removed",
    expectations: [
      "The ENG-101 chip is gone and the branch picker combobox is back above the prompt textarea.",
    ],
  });
}, 60000);

async function openKickoffDialog(
  user: ReturnType<typeof userEvent.setup>,
  issueTitle: string,
) {
  sendGpuiAction({ kind: "openLinear" });
  await user.click(await screen.findByRole("tab", { name: "Kanban" }));
  const card = (await screen.findByText(issueTitle)).closest(
    "div",
  ) as HTMLElement;
  await user.click(within(card).getByRole("button", { name: "Kick off" }));
  return (
    await screen.findByRole("heading", { name: "Start a new agent session" })
  ).closest('[data-testid="modal"]') as HTMLElement;
}

it("starts an agent session in the Linear issue's workspace on submit", async () => {
  const restoreAgents = installFakeAgents();
  try {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    mockLinearListTeams.mockResolvedValue(TEAMS);
    mockLinearListIssues.mockResolvedValue(ISSUES);
    mockLinearGetViewer.mockResolvedValue(VIEWER);
    // Stands in for the Linear lookup only: the workspace itself is created
    // by the real backend, under the issue's branch name.
    mockLinearKickoff.mockImplementation(async (path: string) => {
      const workspaceId = await createWorkspace(path, "eng-103");
      return { issue_id: "issue-3", workspace_id: workspaceId, created: true };
    });

    const user = userEvent.setup();
    render(<Dashboard />);

    const dialog = await openKickoffDialog(
      user,
      "Investigate invoice rounding error",
    );
    await user.type(
      within(dialog).getByPlaceholderText("Describe a task..."),
      "Fix the rounding",
    );
    await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));

    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "Start a new agent session" }),
      ).not.toBeInTheDocument(),
    );
    expect(mockLinearKickoff).toHaveBeenCalledWith(repoPath, "issue-3");
    await waitFor(async () => {
      const sessions = await getSessions(repoPath);
      expect(sessions).toHaveLength(1);
      expect(sessions[0].workspace_id).not.toBeNull();
    });
    await screen.findByTestId("linear-panel");
    expect(
      (await getWorkspaces(repoPath)).some((w) => w.branch_name === "eng-103"),
    ).toBe(true);
    await findGpuiRow("eng-103");

    await captureDocument(document, {
      name: "linear-kickoff-05-submitted",
      expectations: [
        "The prompt dialog is closed and the Linear panel is still the page on screen (no navigation).",
        "The sidebar's Workspaces list now includes the new 'eng-103' workspace.",
      ],
    });
  } finally {
    restoreAgents();
  }
}, 120000);

it("keeps the dialog open and shows an error toast when the Linear workspace can't be created", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  mockLinearListTeams.mockResolvedValue(TEAMS);
  mockLinearListIssues.mockResolvedValue(ISSUES);
  mockLinearGetViewer.mockResolvedValue(VIEWER);
  // Real backend: with no Linear credentials configured, the command fails.
  const actual = await vi.importActual<
    typeof import("../../../src/lib/api-linear")
  >("../../../src/lib/api-linear");
  mockLinearKickoff.mockImplementation(
    actual.linearOpenOrCreateWorkspaceFromIssue,
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  const dialog = await openKickoffDialog(
    user,
    "Investigate invoice rounding error",
  );
  await user.type(
    within(dialog).getByPlaceholderText("Describe a task..."),
    "Fix the rounding",
  );
  await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));

  await screen.findByText("Failed to create task");
  expect(
    screen.getByRole("heading", { name: "Start a new agent session" }),
  ).toBeInTheDocument();
  expect(within(dialog).getByTestId("issue-chip")).toHaveTextContent("ENG-103");
  expect(await getSessions(repoPath)).toHaveLength(0);

  await captureDocument(document, {
    name: "linear-kickoff-06-submit-error",
    expectations: [
      "A red error toast titled 'Failed to create task' is visible.",
      "The prompt dialog is still open with the ENG-103 chip and the typed prompt 'Fix the rounding' intact.",
    ],
  });
}, 60000);
