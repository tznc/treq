import * as React from "react";
import type userEvent from "@testing-library/user-event";
import { expect } from "vitest";
import * as api from "../../../src/lib/api";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../utils";
import { render, screen } from "../../test-utils";

export async function createTwoWorkspacesWithDirtySource() {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const sourceId = await api.createWorkspace(repoPath, "feat/source");
  const destId = await api.createWorkspace(repoPath, "feat/dest");
  const workspaces = await api.getWorkspaces(repoPath);
  const source = workspaces.find((w) => w.id === sourceId)!;
  const dest = workspaces.find((w) => w.id === destId)!;
  expect(source).toBeTruthy();
  expect(dest).toBeTruthy();

  const sourcePath = resolveWorkspacePath(repoPath, source.workspace_path);
  const destPath = resolveWorkspacePath(repoPath, dest.workspace_path);
  const fileName = "move-me.txt";
  writeWorkspaceFile(sourcePath, fileName, "content to move\n");

  return { repoPath, source, dest, sourcePath, destPath, fileName };
}

export async function openReviewForBranch(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
) {
  render(<Dashboard />);
  await user.click(await findSidebarBranchElement(branchName));
  const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
  await user.click(reviewTab);
  await screen.findByRole("tab", { name: /^Changes/, selected: true });
}

export async function waitForChangedFileText(fileName: string) {
  await screen.findAllByText(fileName);
}
