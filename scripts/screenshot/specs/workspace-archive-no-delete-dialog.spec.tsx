/**
 * Regression spec: Archive Workspace must not open the Delete Workspace dialog.
 * Archiving removes the workspace directory and hides the row, keeping the DB record.
 */

import * as React from "react";
import { expect, it, vi } from "vitest";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { findGpuiRow, pickGpuiRowMenuItem, waitForNoGpuiRow } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/archive-no-delete-dialog";

it("archives a workspace from the sidebar without the delete dialog", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  await createWorkspace(repoPath, BRANCH_NAME);
  const workspaces = await getWorkspaces(repoPath);
  const workspace = workspaces.find((w) => w.branch_name === BRANCH_NAME);
  if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);

  const askSpy = vi.mocked(ask);
  render(<Dashboard />);

  await findGpuiRow(BRANCH_NAME);
  await captureDocument(document, {
    name: "workspace-archive-no-delete-dialog-01-before",
    expectations: [
      "The sidebar lists the feat/archive-no-delete-dialog workspace.",
      "No delete-workspace dialog or warning banner is visible.",
    ],
  });

  await pickGpuiRowMenuItem(BRANCH_NAME, "Archive Workspace");
  await waitForNoGpuiRow(BRANCH_NAME);

  expect(askSpy).not.toHaveBeenCalled();
  expect(screen.queryByText("Delete Workspace")).not.toBeInTheDocument();

  await captureDocument(document, {
    name: "workspace-archive-no-delete-dialog-02-after",
    expectations: [
      "The archived workspace is gone from the sidebar.",
      "No Delete Workspace dialog is on screen.",
    ],
  });
}, 60000);
