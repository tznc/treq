/**
 * Visual QA: terminal pane is a flat row of terminals. Each header is
 * icon + short title + workspace-name label (no workspace group bars).
 */

import * as React from "react";
import { expect, it } from "vitest";
import {
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow, openGpuiShell, startGpuiAgent } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/terminal-header-label";

it("shows workspace name as a label on each terminal header", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  await createWorkspace(repoPath, BRANCH_NAME);
  const workspaces = await getWorkspaces(repoPath);
  const workspace = workspaces.find((w) => w.branch_name === BRANCH_NAME);
  if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);

  render(<Dashboard />);

  await clickGpuiRow(workspace.branch_name);
  await screen.findByTestId("workspace-terminal-pane");

  await startGpuiAgent(BRANCH_NAME);
  const agentPanel = await waitFor(() => {
    const el = document.querySelector('[data-terminal-id^="agent-"]');
    expect(el).not.toBeNull();
    return el as HTMLElement;
  });

  await openGpuiShell(BRANCH_NAME);
  const shellPanel = await waitFor(() => {
    const el = document.querySelector('[data-terminal-id^="shell-"]');
    expect(el).not.toBeNull();
    return el as HTMLElement;
  });

  expect(document.querySelector("[data-workspace-group]")).toBeNull();
  expect(
    within(agentPanel).getByTestId("terminal-workspace-label"),
  ).toHaveTextContent(BRANCH_NAME);
  expect(
    within(shellPanel).getByTestId("terminal-workspace-label"),
  ).toHaveTextContent(BRANCH_NAME);

  await captureDocument(document, {
    name: "terminal-header-workspace-label-01-flat-pane",
    expectations: [
      "There is no full-width workspace group header bar above the terminals.",
      "The agent terminal header shows its icon, a short session title, and a small workspace-name label.",
      "The shell terminal header shows a terminal icon, the word Shell, and the same workspace-name label.",
    ],
  });
}, 60000);
