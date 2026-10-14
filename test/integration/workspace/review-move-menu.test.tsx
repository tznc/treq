// @include-serial
import fs from "node:fs";
import path from "node:path";
import { fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import * as api from "../../../src/lib/api";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../utils";
import { screen, waitFor } from "../../test-utils";
import {
  createTwoWorkspacesWithDirtySource,
  openReviewForBranch,
  waitForChangedFileText,
} from "./move-changes-helpers";

describe("Review tab - Move to workspace menu", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("moves a file from its Move to workspace menu", async () => {
    const { source, dest, sourcePath, destPath, fileName } =
      await createTwoWorkspacesWithDirtySource();
    await openReviewForBranch(user, source.branch_name);

    await waitForChangedFileText(fileName);

    const [sidebarFile] = screen.getAllByText(fileName);
    fireEvent.contextMenu(sidebarFile);
    await user.hover(await screen.findByTestId("move-to-workspace"));
    await screen.findByText(dest.branch_name, { selector: "[role=menuitem]" });
    expect(
      screen.queryByText(source.branch_name, { selector: "[role=menuitem]" }),
    ).toBeNull();
    fireEvent.click(
      screen.getByText(dest.branch_name, { selector: "[role=menuitem]" }),
    );

    await screen.findByText(/Move 1 file to feat\/dest\?/);
    await user.click(await screen.findByRole("button", { name: /^Move$/ }));

    await waitFor(() => {
      expect(fs.existsSync(path.join(sourcePath, fileName))).toBe(false);
      expect(fs.existsSync(path.join(destPath, fileName))).toBe(true);
    });
  });

  it("moves the full selection from one selected file's menu", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    const sourceId = await api.createWorkspace(repoPath, "feat/menu-src");
    const destId = await api.createWorkspace(repoPath, "feat/menu-dest");
    const workspaces = await api.getWorkspaces(repoPath);
    const source = workspaces.find((w) => w.id === sourceId)!;
    const dest = workspaces.find((w) => w.id === destId)!;
    const sourcePath = resolveWorkspacePath(repoPath, source.workspace_path);
    const destPath = resolveWorkspacePath(repoPath, dest.workspace_path);

    writeWorkspaceFile(sourcePath, "a.txt", "aaa\n");
    writeWorkspaceFile(sourcePath, "b.txt", "bbb\n");

    await openReviewForBranch(user, source.branch_name);

    await waitForChangedFileText("a.txt");
    await waitForChangedFileText("b.txt");

    await user.click(screen.getByTitle(/Select all/i));

    const [bFile] = screen.getAllByText("b.txt");
    fireEvent.contextMenu(bFile);
    await user.hover(await screen.findByTestId("move-to-workspace"));
    fireEvent.click(
      await screen.findByText(dest.branch_name, {
        selector: "[role=menuitem]",
      }),
    );

    await screen.findByText(/Move 2 files to feat\/menu-dest\?/);
    await user.click(await screen.findByRole("button", { name: /^Move$/ }));

    await waitFor(() => {
      expect(fs.existsSync(path.join(sourcePath, "a.txt"))).toBe(false);
      expect(fs.existsSync(path.join(sourcePath, "b.txt"))).toBe(false);
      expect(fs.existsSync(path.join(destPath, "a.txt"))).toBe(true);
      expect(fs.existsSync(path.join(destPath, "b.txt"))).toBe(true);
    });
    await waitFor(() => {
      expect(screen.queryAllByText("a.txt")).toHaveLength(0);
      expect(screen.queryAllByText("b.txt")).toHaveLength(0);
    });

    expect(fs.existsSync(path.join(sourcePath, "a.txt"))).toBe(false);
    expect(fs.existsSync(path.join(sourcePath, "b.txt"))).toBe(false);
    expect(fs.existsSync(path.join(destPath, "a.txt"))).toBe(true);
    expect(fs.existsSync(path.join(destPath, "b.txt"))).toBe(true);
  });
});
