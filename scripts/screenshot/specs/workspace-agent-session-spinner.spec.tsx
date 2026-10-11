import { expect, it, onTestFinished } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  commitWorkspaceFile,
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../../test/utils";
import { installFakeAgents } from "../../../test/fake-agent";
import {
  checkAndRebaseWorkspaces,
  createCommit,
  createWorkspace,
  ensureWorkspaceIndexed,
  getWorkspaces,
} from "../../../src/lib/api";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";

// Verifies the workspace sidebar's right-aligned git status pip: nothing
// when clean, a yellow pip with uncommitted changes, and a red pip when
// conflicted. The pip spins while an agent in the workspace streams output;
// a clean workspace with a working agent shows a gray spinner.
it("shows the right git-state pip for sessioned and session-less workspaces", async () => {
  onTestFinished(installFakeAgents());

  const { repoPath, defaultBranch } = createTestRepo(false);
  openRepo(repoPath);

  const noSessionCleanId = await createWorkspace(repoPath, "feat/no-session-clean");
  const noSessionDirtyId = await createWorkspace(repoPath, "feat/no-session-dirty");
  const noSessionConflictId = await createWorkspace(
    repoPath,
    "feat/no-session-conflict",
  );
  const sessionCleanId = await createWorkspace(repoPath, "feat/session-clean");
  const sessionDirtyId = await createWorkspace(repoPath, "feat/session-dirty");
  const sessionConflictId = await createWorkspace(repoPath, "feat/session-conflict");
  const longBranch =
    "feat/a-very-long-workspace-name-that-should-truncate-at-the-end";
  const sessionLongId = await createWorkspace(repoPath, longBranch);

  const workspaces = await getWorkspaces(repoPath);
  const byId = (id: number) => workspaces.find((w) => w.id === id)!;
  const noSessionDirty = byId(noSessionDirtyId);
  const sessionDirty = byId(sessionDirtyId);
  const conflictRows = [byId(noSessionConflictId), byId(sessionConflictId)];

  // Uncommitted changes on the two "dirty" rows.
  for (const ws of [noSessionDirty, sessionDirty]) {
    const workspacePath = resolveWorkspacePath(repoPath, ws.workspace_path);
    writeWorkspaceFile(workspacePath, "dirty.txt", "uncommitted\n");
  }

  // Real conflicts on the two "conflict" rows.
  for (const ws of conflictRows) {
    const workspacePath = resolveWorkspacePath(repoPath, ws.workspace_path);
    writeWorkspaceFile(workspacePath, "README.md", "workspace side\n");
    await createCommit(repoPath, ws.id, "workspace conflicting change");
  }
  writeWorkspaceFile(repoPath, "README.md", "main side\n");
  await createCommit(repoPath, null, "main conflicting change");
  for (const ws of conflictRows) {
    await checkAndRebaseWorkspaces(repoPath, ws.id, defaultBranch, true);
    await ensureWorkspaceIndexed(
      repoPath,
      ws.id,
      resolveWorkspacePath(repoPath, ws.workspace_path),
    );
  }

  const user = userEvent.setup();
  render(<Dashboard />);

  async function openAgentSession(branchName: string) {
    await user.click(await findSidebarBranchElement(branchName));
    await screen.findByTestId("workspace-terminal-pane");
    await user.keyboard("{Meta>}]{/Meta}");
    await waitFor(() => {
      expect(
        document.querySelector('[data-terminal-id^="agent-"]'),
      ).not.toBeNull();
    });
  }

  await openAgentSession("feat/session-dirty");

  const dirtyIndicator = () =>
    document.querySelector(
      `[data-testid="workspace-status-indicator-${sessionDirtyId}"]`,
    );
  await waitFor(() => {
    expect(dirtyIndicator()).not.toBeNull();
    expect(document.querySelector('[data-terminal-id^="agent-"]')).not.toBeNull();
  });
  await captureDocument(document, {
    name: "workspace-agent-session-spinner-01-active",
    expectations: [
      "feat/session-dirty shows its yellow workspace status indicator while the agent terminal is active.",
      "The workspace sidebar and active agent terminal remain visible together.",
    ],
  });
  await captureDocument(document, {
    name: "workspace-agent-session-spinner-02-idle",
    expectations: [
      "feat/session-dirty keeps its yellow workspace status indicator after terminal startup settles.",
      "The agent terminal pane remains open below the workspace content.",
    ],
  });

  await openAgentSession("feat/session-clean");
  const cleanSpinner = await screen.findByTestId(
    `workspace-status-indicator-${sessionCleanId}`,
  );
  expect(cleanSpinner).toHaveAttribute("aria-label", "Agent working");
  await captureDocument(document, {
    name: "workspace-agent-session-spinner-04-clean-working",
    expectations: [
      "feat/session-clean shows a small gray spinning indicator at its right edge while its new agent starts up.",
      "feat/no-session-clean, the other clean row, has no indicator.",
    ],
  });
  // The spinner stops once the agent goes idle.
  await waitFor(
    () =>
      expect(
        screen.queryByTestId(`workspace-status-indicator-${sessionCleanId}`),
      ).not.toBeInTheDocument(),
    { timeout: 15000 },
  );

  // A long name truncates at its end, and the spinner sits centered on the
  // row rather than dropping below it.
  await openAgentSession(longBranch);
  const longSpinner = await screen.findByTestId(
    `workspace-status-indicator-${sessionLongId}`,
  );
  expect(longSpinner).toHaveAttribute("aria-label", "Agent working");
  await captureDocument(document, {
    name: "workspace-agent-session-spinner-05-long-name",
    clipSelector: '[data-testid="workspace-sidebar"]',
    deviceScaleFactor: 2,
    expectations: [
      "The long feat/a-very-long-workspace-name... row shows the start of its name and ends with an ellipsis.",
      "That row's gray spinner is vertically centered on the row's text, not below it.",
      "The truncated name stops short of the spinner without overlapping it.",
    ],
  });
  await waitFor(
    () =>
      expect(
        screen.queryByTestId(`workspace-status-indicator-${sessionLongId}`),
      ).not.toBeInTheDocument(),
    { timeout: 15000 },
  );

  await openAgentSession("feat/session-conflict");

  // Clean rows show no pip once no agent in them is working.
  expect(
    screen.queryByTestId(`workspace-status-indicator-${noSessionCleanId}`),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByTestId(`workspace-conflict-indicator-${noSessionCleanId}`),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByTestId(`workspace-status-indicator-${sessionCleanId}`),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByTestId(`workspace-conflict-indicator-${sessionCleanId}`),
  ).not.toBeInTheDocument();

  // Dirty rows show the yellow pip, session or not.
  await screen.findByTestId(`workspace-status-indicator-${noSessionDirtyId}`);
  await screen.findByTestId(`workspace-status-indicator-${sessionDirtyId}`);

  // Conflicted rows show the red pip, session or not.
  await screen.findByTestId(
    `workspace-conflict-indicator-${noSessionConflictId}`,
  );
  await screen.findByTestId(`workspace-conflict-indicator-${sessionConflictId}`);

  await captureDocument(document, {
    name: "workspace-agent-session-spinner-03-states",
    expectations: [
      "feat/no-session-clean and feat/session-clean have no pip at their right edge at all.",
      "feat/no-session-dirty and feat/session-dirty each show a small yellow pip at the right edge.",
      "feat/no-session-conflict and feat/session-conflict each show a small red pip at the right edge.",
    ],
  });
}, 120000);
