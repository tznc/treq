/**
 * The standardised issue kickoff: GitHub and Linear both open
 * the same prompt dialog (issue chip, "Opens a workspace for ..." in place
 * of the workspace picker) and start the session in the issue's workspace.
 * Only the external tracker APIs are stubbed.
 */
import userEvent from "@testing-library/user-event";
import { expect, it, onTestFinished, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  getSessions,
  getWorkspaces,
} from "../../../src/lib/api";
import type { LinearIssue } from "../../../src/lib/api-linear";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { installFakeAgents } from "../../../test/fake-agent";
import { captureDocument } from "../capture";
import {
  createTestRepo,
  openRepo,
  setOriginUrl,
} from "../../../test/utils";
import { findGpuiRow, sendGpuiAction } from "../../../test/gpui-sidebar";

// Only the external tracker APIs are stubbed. Workspaces are created by the
// real backend: GitHub through its own command, the others through
// createWorkspace from the stubbed kickoff calls.
const external = vi.hoisted(() => ({
  ghListIssues: vi.fn(),
  ghViewIssue: vi.fn(),
  linearListIssues: vi.fn(),
  linearListTeams: vi.fn(),
  linearGetViewer: vi.fn(),
  linearOpenOrCreateWorkspaceFromIssue: vi.fn(),
}));

vi.mock("../../../src/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../src/lib/api")>()),
  ghListIssues: external.ghListIssues,
  ghViewIssue: external.ghViewIssue,
  ghListPrs: vi.fn().mockResolvedValue({ items: [], hasMore: false }),
  getCachedPrInfo: vi.fn().mockResolvedValue(null),
  startPrStatusPolling: vi.fn(async () => undefined),
  refreshPrStatuses: vi.fn(async () => undefined),
}));
vi.mock("../../../src/lib/api-linear", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../src/lib/api-linear")>()),
  linearListIssues: external.linearListIssues,
  linearListTeams: external.linearListTeams,
  linearGetViewer: external.linearGetViewer,
  linearOpenOrCreateWorkspaceFromIssue:
    external.linearOpenOrCreateWorkspaceFromIssue,
}));
const GITHUB_ISSUE = {
  number: 42,
  title: "Fix the login redirect",
  state: "OPEN",
  url: "https://github.com/acme/treq/issues/42",
  body: "Users land on /dashboard.",
  author: { login: "alice", avatar_url: null },
  labels: [],
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  comments: null,
};

const LINEAR_ISSUE: LinearIssue = {
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

type Case = {
  source: string;
  label: string;
  key: string;
  branch: string;
  setup: (repoPath: string) => void;
  open: (user: ReturnType<typeof userEvent.setup>) => Promise<void>;
};

const CASES: Case[] = [
  {
    source: "GitHub",
    label: "GitHub",
    key: "#42",
    branch: "github-42-fix-the-login-redirect",
    setup: (repoPath) => {
      setOriginUrl(repoPath, "https://github.com/acme/treq.git");
      external.ghListIssues.mockResolvedValue({
        items: [GITHUB_ISSUE],
        hasMore: false,
      });
      external.ghViewIssue.mockResolvedValue(GITHUB_ISSUE);
    },
    open: async (user) => {
      sendGpuiAction({ kind: "openGithub" });
      await user.click(await screen.findByText(GITHUB_ISSUE.title));
      await user.click(await screen.findByRole("button", { name: /^agent/i }));
    },
  },
  {
    source: "Linear",
    label: "Linear",
    key: "ENG-101",
    branch: LINEAR_ISSUE.branch_name,
    setup: () => {
      external.linearListTeams.mockResolvedValue([]);
      external.linearListIssues.mockResolvedValue([LINEAR_ISSUE]);
      external.linearGetViewer.mockResolvedValue({ id: "me", name: "Me" });
      external.linearOpenOrCreateWorkspaceFromIssue.mockImplementation(
        async (path: string) => ({
          issue_id: LINEAR_ISSUE.id,
          workspace_id: await createWorkspace(path, LINEAR_ISSUE.branch_name),
          created: true,
        }),
      );
    },
    open: async (user) => {
      sendGpuiAction({ kind: "openLinear" });
      await user.click(await screen.findByRole("button", { name: "Kick off" }));
    },
  },
];

for (const c of CASES) {
  it(`captures the ${c.source} kickoff dialog and submit`, async () => {
    onTestFinished(installFakeAgents());
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    c.setup(repoPath);
    const user = userEvent.setup();
    render(<Dashboard />);
    await c.open(user);

    const dialog = (
      await screen.findByRole("heading", { name: "Start a new agent session" })
    ).closest('[data-testid="modal"]') as HTMLElement;
    expect(await within(dialog).findByTestId("issue-chip")).toHaveTextContent(
      `${c.label} ${c.key}`,
    );
    const slug = c.source.toLowerCase();
    await captureDocument(document, {
      name: `issue-kickoff-flow-${slug}-01-dialog`,
      expectations: [
        `The 'Start a new agent session' dialog shows a '${c.label} ${c.key}' chip above the 'Describe a task...' textarea.`,
        `In place of the branch picker, a muted line reads 'Opens a workspace for ${c.key}'.`,
        "The dialog looks the same as the other trackers' kickoff dialogs apart from the chip text and icon color.",
      ],
    });

    await user.type(
      within(dialog).getByPlaceholderText("Describe a task..."),
      "Implement it",
    );
    await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));
    await findGpuiRow(c.branch);
    await waitFor(async () => {
      const workspace = (await getWorkspaces(repoPath)).find(
        (ws) => ws.branch_name === c.branch,
      );
      expect(
        (await getSessions(repoPath)).map((s) => s.workspace_id),
      ).toContain(workspace?.id);
    });

    await captureDocument(document, {
      name: `issue-kickoff-flow-${slug}-02-submitted`,
      expectations: [
        "The prompt dialog is closed and the tracker panel is still the page on screen.",
        `The sidebar's Workspaces list includes '${c.branch}'.`,
        `A 'Session started' toast in the bottom-left corner reads 'in ${c.branch}' and has an 'Open' link.`,
      ],
    });
  }, 120000);
}
