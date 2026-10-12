// @include-serial
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen, waitFor } from "../../test-utils";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../utils";
import * as api from "../../../src/lib/api";
import { Dashboard } from "../../../src/components/Dashboard";
import fs from "node:fs";
import path from "node:path";

async function createDirtyWorkspace(branchName: string) {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await api.createWorkspace(repoPath, branchName);
  const workspace = (await api.getWorkspaces(repoPath)).find(
    (candidate) => candidate.id === workspaceId,
  );
  expect(workspace).toBeTruthy();

  writeWorkspaceFile(
    resolveWorkspacePath(repoPath, workspace!.workspace_path),
    "tentative-actions.txt",
    "dirty working copy content\n",
  );

  return { repoPath, workspace: workspace! };
}

async function openWorkspaceCommitsTab(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
) {
  render(<Dashboard />);
  await user.click(await findSidebarBranchElement(branchName));
  await user.click(await screen.findByRole("tab", { name: /^Commits/ }));
}

describe("ShowWorkspace - tentative working copy actions", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("expands the tentative row and switches to Review from View changes", async () => {
    await createDirtyWorkspace("feat/tentative-view");
    await openWorkspaceCommitsTab(user, "feat/tentative-view");

    const workingCopyButton = await screen.findByRole("button", {
      name: /working copy/i,
    });
    await user.click(workingCopyButton);

    expect(
      await screen.findByRole("button", { name: /view changes/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /move changes/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /delete changes/i }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /view changes/i }));
    await screen.findByRole("tab", { name: /Changes/, selected: true });
  });

  it("opens the move dialog with the tentative commit selected", async () => {
    await createDirtyWorkspace("feat/tentative-move");
    await openWorkspaceCommitsTab(user, "feat/tentative-move");

    const workingCopyButton = await screen.findByRole("button", {
      name: /working copy/i,
    });
    await user.click(workingCopyButton);

    await user.click(screen.getByRole("button", { name: /move changes/i }));
    await user.click(
      await screen.findByRole("menuitem", { name: /Move to New Workspace/i }),
    );

    await screen.findByText(/Stack a new Workspace/i);
    expect(
      screen.getByRole("button", { name: /Split 1 item/i }),
    ).toBeInTheDocument();
  });

  it("restores discarded tentative changes from the toast Undo action", async () => {
    const { repoPath, workspace } = await createDirtyWorkspace(
      "feat/tentative-undo",
    );
    await openWorkspaceCommitsTab(user, "feat/tentative-undo");

    const workspacePath = resolveWorkspacePath(
      repoPath,
      workspace.workspace_path,
    );

    const workingCopyButton = await screen.findByRole("button", {
      name: /working copy/i,
    });
    await user.click(workingCopyButton);
    await user.click(screen.getByRole("button", { name: /delete changes/i }));

    await waitFor(() => {
      expect(
        screen.queryAllByRole("button", { name: /working copy/i }),
      ).toHaveLength(0);
    });

    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await screen.findByText("Working copy changes were restored");
    expect(
      fs.existsSync(path.join(workspacePath, "tentative-actions.txt")),
    ).toBe(true);
    await waitFor(() => {
      expect(
        screen.getAllByRole("button", { name: /working copy/i }).length,
      ).toBeGreaterThan(0);
    });
  });
});
