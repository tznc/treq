// @include-parallel
import * as React from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { createWorkspace } from "../../src/lib/api";
import {
  findGpuiOverlay,
  gpuiOverlay,
  sendGpuiOverlayAction,
} from "../gpui-overlay";
import { findGpuiScreen, sendGpuiScreenAction } from "../gpui-screen";
import { clickGpuiRow } from "../gpui-sidebar";
import { render, screen, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

const press = (id: string) => sendGpuiOverlayAction({ kind: "press", id });

describe("Dashboard dialogs drawn by GPUI", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    window.__TREQ_REACT_OVERLAYS__ = false;
    user = userEvent.setup();
  });

  async function openPalette() {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    await user.keyboard("{Control>}k{/Control}");
    return findGpuiOverlay("commandPalette");
  }

  it("shows the command palette in GPUI over the workspace screen", async () => {
    const palette = await openPalette();
    expect(palette.repoPath).toBe(repoPath);
    expect(palette.stash).toBe(true);
    expect(palette.hasSelectedWorkspace).toBe(false);
    expect(palette.workspace).toBeNull();
    expect(typeof palette.dark).toBe("boolean");
    expect(screen.queryByText("Go to Home")).not.toBeInTheDocument();
    await findGpuiScreen("workspace");
  });

  it("opens the palette for Cmd+K pressed in GPUI", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    press("overlay.commandPalette");
    await findGpuiOverlay("commandPalette");
  });

  it("closes the dialog GPUI asks to close", async () => {
    await openPalette();
    press("overlay.close");
    await waitFor(() => expect(gpuiOverlay()).toBeNull());
  });

  it("runs a palette command and shows the dialog it opens", async () => {
    await openPalette();
    press("palette.switch-branch");
    const switcher = await findGpuiOverlay("branchSwitcher");
    expect(switcher.repoPath).toBe(repoPath);
    expect(screen.queryByPlaceholderText("Search branches...")).toBeNull();

    sendGpuiOverlayAction({
      kind: "field",
      field: "branchSwitcher.changed",
      value: "main",
    });
    await waitFor(() => expect(gpuiOverlay()).toBeNull());
  });

  it("runs a palette command that opens a React dialog", async () => {
    await openPalette();
    press("palette.search-files");
    await waitFor(() => expect(gpuiOverlay()).toBeNull());
    await screen.findByPlaceholderText("Search files...");
  });

  it("opens GPUI's branch switcher from the workspace header", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");

    sendGpuiScreenAction({ kind: "press", id: "workspace.branchSwitcher" });
    await findGpuiOverlay("branchSwitcher");
    await findGpuiScreen("workspace");
    expect(screen.queryByTestId("modal")).toBeNull();
  });

  it("shows the keyboard shortcuts on ?", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    (document.activeElement as HTMLElement | null)?.blur();
    await user.click(document.body);
    await user.keyboard("?");
    await findGpuiOverlay("keyboardShortcuts");
    expect(screen.queryByTestId("keyboard-shortcuts-modal")).toBeNull();

    press("overlay.close");
    await waitFor(() => expect(gpuiOverlay()).toBeNull());
  });

  describe("with a workspace", () => {
    let alphaId: number;

    beforeEach(async () => {
      alphaId = await createWorkspace(repoPath, "feat/alpha");
    });

    it("opens the workspace picked in GPUI", async () => {
      await openPalette();
      press("palette.go-to-workspace");
      const picker = await findGpuiOverlay("workspacePicker");
      expect(picker.repoPath).toBe(repoPath);

      sendGpuiOverlayAction({
        kind: "field",
        field: "workspacePicker.select",
        value: alphaId,
      });
      await waitFor(() => expect(gpuiOverlay()).toBeNull());
      await findGpuiScreen("workspace", (s) => {
        expect(s.workspaceId).toBe(alphaId);
      });
    });

    it("sends the selected workspace for its checks and PR", async () => {
      render(<Dashboard />);
      await clickGpuiRow("feat/alpha");
      await findGpuiScreen("workspace", (s) => {
        expect(s.workspaceId).toBe(alphaId);
      });
      await user.keyboard("{Control>}k{/Control}");
      const palette = await findGpuiOverlay("commandPalette");
      expect(palette.hasSelectedWorkspace).toBe(true);
      expect(palette.workspace).toEqual({
        id: alphaId,
        repoPath,
        branch: "feat/alpha",
      });
    });
  });
});
