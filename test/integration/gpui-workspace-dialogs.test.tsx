// @include-parallel
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { createWorkspace, getWorkspaces } from "../../src/lib/api";
import type { GpuiWorkspaceModal } from "../../src/lib/gpui-screen";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { clickGpuiRow, pickGpuiRowMenuItem } from "../gpui-sidebar";
import { render, screen, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

const press = (id: string) => sendGpuiScreenAction({ kind: "press", id });

async function findGpuiModal<K extends GpuiWorkspaceModal["kind"]>(
  kind: K,
): Promise<Extract<GpuiWorkspaceModal, { kind: K }>> {
  const shown = await findGpuiScreen("workspace", (s) => {
    expect(s.modal?.kind).toBe(kind);
  });
  return shown.modal as Extract<GpuiWorkspaceModal, { kind: K }>;
}

async function waitForNoGpuiModal() {
  await waitFor(() => expect(gpuiScreen()?.modal).toBeUndefined());
}

describe("GPUI workspace dialogs", () => {
  let repoPath: string;
  let defaultBranch: string;
  let alphaId: number;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    window.__TREQ_REACT_WORKSPACE_DIALOGS__ = false;
    ({ repoPath, defaultBranch } = createTestRepo(false));
    openRepo(repoPath);
    alphaId = await createWorkspace(repoPath, "feat/alpha");
    user = userEvent.setup();
  });

  it("draws the stack dialog over the screen instead of React", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");

    press("workspace.stack");
    const dialog = await findGpuiModal("workspaceDialog");
    expect(dialog.repoPath).toBe(repoPath);
    expect(dialog.defaults).toEqual({
      targetBranch: defaultBranch,
      source: { kind: "home" },
    });
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument();

    press("workspaceDialog.close");
    await waitForNoGpuiModal();
  });

  it("stacks on the open workspace, and starts fresh on each open", async () => {
    render(<Dashboard />);
    await clickGpuiRow("feat/alpha");
    await findGpuiScreen("workspace", (s) => {
      expect(s.workspaceId).toBe(alphaId);
    });

    press("workspace.stack");
    const first = await findGpuiModal("workspaceDialog");
    expect(first.defaults).toEqual({
      targetBranch: "feat/alpha",
      source: {
        kind: "workspace",
        id: alphaId,
        branchName: "feat/alpha",
        targetBranch: defaultBranch,
      },
    });
    press("workspaceDialog.close");
    await waitForNoGpuiModal();

    press("workspace.stack");
    const second = await findGpuiModal("workspaceDialog");
    expect(second.key).toBeGreaterThan(first.key);
  });

  it("opens the workspace GPUI created", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    press("workspace.stack");
    await findGpuiModal("workspaceDialog");

    const created = await createWorkspace(repoPath, "feat/created");
    sendGpuiScreenAction({
      kind: "field",
      field: "workspaceDialog.created",
      value: created,
    });

    await findGpuiScreen("workspace", (s) => {
      expect(s.workspaceId).toBe(created);
      expect(s.modal).toBeUndefined();
    });
  });

  it("draws the rename dialog", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");

    await pickGpuiRowMenuItem("feat/alpha", "Rename Workspace");
    expect(await findGpuiModal("renameWorkspace")).toEqual({
      kind: "renameWorkspace",
      repoPath,
      workspaceId: alphaId,
      branchName: "feat/alpha",
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    press("renameWorkspace.close");
    await waitForNoGpuiModal();
  });

  it("draws Delete Workspace and confirms the deletion natively", async () => {
    const { ask } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(ask).mockResolvedValue(false);
    render(<Dashboard />);
    await findGpuiScreen("workspace");

    await user.keyboard("{Control>}k{/Control}");
    await user.click(await screen.findByText("Delete Workspace"));
    const deletion = await findGpuiModal("workspaceDeletion");
    expect(deletion.repoPath).toBe(repoPath);
    expect(deletion.currentWorkspace).toBeNull();
    expect(deletion.workspaces.map((w) => w.branchName)).toEqual([
      "feat/alpha",
    ]);

    sendGpuiScreenAction({
      kind: "field",
      field: "workspaceDeletion.delete",
      value: alphaId,
    });
    await waitForNoGpuiModal();
    await waitFor(() =>
      expect(ask).toHaveBeenCalledWith("Delete workspace feat/alpha?", {
        title: "Delete Workspace",
        kind: "warning",
      }),
    );
    expect((await getWorkspaces(repoPath)).map((w) => w.id)).toEqual([alphaId]);
  });

  it("confirms moving dropped files", async () => {
    render(<Dashboard />);
    await clickGpuiRow("feat/alpha");
    await findGpuiScreen("workspace", (s) => {
      expect(s.workspaceId).toBe(alphaId);
    });

    sendGpuiScreenAction({
      kind: "field",
      field: "changes.moveFiles",
      value: {
        files: ["a.txt", "b.txt"],
        sourceBranch: "feat/alpha",
        destinationBranch: defaultBranch,
        destinationLabel: "Home",
      },
    });
    expect(await findGpuiModal("confirmChangeMove")).toEqual({
      kind: "confirmChangeMove",
      repoPath,
      files: ["a.txt", "b.txt"],
      sourceBranch: "feat/alpha",
      destinationBranch: defaultBranch,
      destinationLabel: "Home",
    });
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();

    press("changeMove.done");
    await waitForNoGpuiModal();
  });

  it("leaves the dialogs to React when the tests ask for them", async () => {
    window.__TREQ_REACT_WORKSPACE_DIALOGS__ = true;
    render(<Dashboard />);
    await findGpuiScreen("workspace");

    press("workspace.stack");
    await screen.findByTestId("modal");
    expect(gpuiScreen()?.modal).toBeUndefined();
  });
});
