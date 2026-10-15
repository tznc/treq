/**
 * Captures the Code tab's (workspace Overview) top-level file list
 * highlighting jj file status via core::ls_workspace_with_status /
 * core::files::annotate_entry_statuses: a red pip for an unresolved
 * conflict, a blue pip for a file changed by an already-committed
 * (not-yet-rebased) ancestor commit, a yellow pip for an uncommitted
 * working-copy change, and a dimmed name (no pip) for untouched files.
 * File names otherwise stay unstyled — only the pip carries status color.
 */

import * as React from "react";
import { expect, it } from "vitest";
import {
  commitWorkspaceFile,
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  checkAndRebaseWorkspaces,
  createCommit,
  createWorkspace,
  ensureWorkspaceIndexed,
  getWorkspaces,
} from "../../../src/lib/api";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";
import { waitForGpuiConflict } from "../gpui-wait";

it("captures conflict/committed/working-copy status pips in the Code tab file list", async () => {
  const branchName = "feat/file-status-pips";
  const { repoPath, defaultBranch } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (candidate) => candidate.id === workspaceId,
  );
  if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);
  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace.workspace_path,
  );

  // Conflict (red): diverge README.md between the workspace and main, then
  // rebase the workspace onto main so the two edits collide.
  writeWorkspaceFile(workspacePath, "README.md", "workspace side\n");
  await createCommit(repoPath, workspaceId, "workspace conflicting change");
  writeWorkspaceFile(repoPath, "README.md", "main side\n");
  await createCommit(repoPath, null, "main conflicting change");
  await checkAndRebaseWorkspaces(repoPath, workspaceId, defaultBranch, true);

  // Committed (blue): a real commit in the workspace, not yet rebased onto
  // the target branch.
  await commitWorkspaceFile(
    repoPath,
    { id: workspace.id, path: workspace.workspace_path },
    "committed.txt",
    "committed content\n",
    "add committed file",
  );

  // Working copy (yellow): an uncommitted edit sitting in @.
  writeWorkspaceFile(workspacePath, "dirty.txt", "uncommitted\n");

  await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);

  render(<Dashboard />);

  await waitForGpuiConflict(workspaceId);
  await clickGpuiRow(branchName);
  await screen.findByTestId("show-workspace-header");

  await screen.findByRole("button", { name: /committed\.txt/ });
  await screen.findByRole("button", { name: /dirty\.txt/ });
  await screen.findByRole("button", { name: /README\.md/ });

  await waitFor(() => {
    const readmeRow = screen.getByRole("button", { name: /README\.md/ });
    expect(readmeRow.querySelector(".bg-red-500")).toBeTruthy();

    const committedRow = screen.getByRole("button", {
      name: /committed\.txt/,
    });
    expect(committedRow.querySelector(".bg-blue-500")).toBeTruthy();

    const dirtyRow = screen.getByRole("button", { name: /dirty\.txt/ });
    expect(dirtyRow.querySelector(".bg-yellow-500")).toBeTruthy();
  });

  await captureDocument(document, {
    name: "filebrowser-status-pips-01-tree",
    expectations: [
      "README.md's row shows a red pip (its file name stays default-colored, not red), signalling an unresolved conflict.",
      "committed.txt's row shows a blue pip, signalling a change already committed but not yet rebased onto the target branch.",
      "dirty.txt's row shows a yellow pip, signalling an uncommitted working-copy change.",
    ],
  });

  // Untouched files (e.g. the repo's other starting files) stay in the list,
  // dimmed and pip-less, rather than being hidden.
  await captureDocument(document, {
    name: "filebrowser-status-pips-02-untouched-dimmed",
    expectations: [
      "At least one other, untouched file (e.g. .gitignore) is visible in the list alongside the highlighted ones.",
      "That untouched file's name renders dimmed/grey with no colored pip, contrasting with README.md/committed.txt/dirty.txt.",
    ],
  });
}, 90000);
