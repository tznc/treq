// @include-parallel
import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { ask } from "@tauri-apps/plugin-dialog";
import { render, screen, waitFor } from "../../test-utils";
import {
  commitWorkspaceFile,
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
  resolveWorkspacePath,
} from "../../utils";
import * as api from "../../../src/lib/api";
import { Dashboard } from "../../../src/components/Dashboard";
import fs from "node:fs";
import path from "node:path";

describe("Commits tab - abandon undo", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    vi.mocked(ask).mockResolvedValue(true);
  });

  it("restores an abandoned commit from the toast Undo action", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    const workspaceId = await api.createWorkspace(
      repoPath,
      "feat/abandon-undo",
    );
    const workspace = (await api.getWorkspaces(repoPath)).find(
      (candidate) => candidate.id === workspaceId,
    );
    expect(workspace).toBeTruthy();

    await commitWorkspaceFile(
      repoPath,
      { id: workspace!.id, path: workspace!.workspace_path },
      "abandon-undo.txt",
      "keep this",
      "Commit to abandon then undo",
    );

    const workspacePath = resolveWorkspacePath(
      repoPath,
      workspace!.workspace_path,
    );

    render(<Dashboard />);
    await user.click(await findSidebarBranchElement("feat/abandon-undo"));
    await user.click(await screen.findByRole("tab", { name: /^Commits/ }));

    const commitButton = await screen.findByRole("button", {
      name: /Commit to abandon then undo/i,
    });
    await user.click(commitButton);
    await user.click(
      await screen.findByRole("button", { name: /delete commit/i }),
    );

    await screen.findByText("Commit deleted");
    expect(fs.existsSync(path.join(workspacePath, "abandon-undo.txt"))).toBe(
      false,
    );

    await user.click(await screen.findByRole("button", { name: "Undo" }));

    await screen.findByText("Restored");
    await waitFor(() => {
      expect(
        screen.getAllByText(/Commit to abandon then undo/i).length,
      ).toBeGreaterThan(0);
    });
    expect(fs.existsSync(path.join(workspacePath, "abandon-undo.txt"))).toBe(
      true,
    );
  });
});
