import * as React from "react";
import { expect, it } from "vitest";
import {
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { useTerminalSettingsStore } from "../../../src/stores/terminalSettingsStore";
import { captureDocument } from "../capture";
import { clickGpuiRow, openGpuiShell } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/terminal-font-size";

it("renders the terminal pane at the default 14px font size", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  await createWorkspace(repoPath, BRANCH_NAME);
  const workspaces = await getWorkspaces(repoPath);
  const workspace = workspaces.find((w) => w.branch_name === BRANCH_NAME);
  if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);

  render(<Dashboard />);

  await clickGpuiRow(workspace.branch_name);
  await screen.findByTestId("show-workspace-header");

  await openGpuiShell(BRANCH_NAME);

  const terminalPanel = await waitFor(() => {
    const el = document.querySelector('[data-terminal-id^="shell-"]');
    expect(el).not.toBeNull();
    return el as HTMLElement;
  });

  expect(within(terminalPanel).getByText("Shell")).toBeVisible();
  expect(useTerminalSettingsStore.getState().fontSize).toBe(14);

  await waitFor(() => {
    expect(terminalPanel.querySelector(".xterm")).not.toBeNull();
  });

  await captureDocument(document, {
    name: "terminal-pane-font-size-01-shell",
    expectations: [
      "The bottom terminal pane is expanded with a Shell panel filling the pane.",
      "The shell terminal uses a readable monospace size matching the 14px default, not a cramped ~10px glyph size.",
      "The Shell header and pane chrome remain the same layout; only the terminal text scale is larger.",
    ],
  });
}, 60000);
