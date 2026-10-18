// @include-parallel
import * as React from "react";
import { waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { createWorkspace } from "../../src/lib/api";
import { popupNativeMenu } from "../../src/lib/native-context-menu";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { clickGpuiRow, gpuiMenuLabels } from "../gpui-sidebar";
import { render, screen } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

const press = (id: string) => sendGpuiScreenAction({ kind: "press", id });

describe("GPUI workspace header", () => {
  let repoPath: string;
  let defaultBranch: string;

  beforeEach(() => {
    ({ repoPath, defaultBranch } = createTestRepo(false));
    openRepo(repoPath);
  });

  it("draws the home repository's header and tab row", async () => {
    render(<Dashboard />);

    const home = await findGpuiScreen("workspace", (s) => {
      expect(s.header.branch).toBe(defaultBranch);
    });
    expect(home.header.home).toBe(true);
    expect(home.header.details).toBe(false);
    expect(home.tabRow?.tab).toBe("overview");
  });

  it("switches tabs through React", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");

    sendGpuiScreenAction({
      kind: "field",
      field: "workspace.tab",
      value: "commits",
    });
    await findGpuiScreen("workspace", (s) => {
      expect(s.tabRow?.tab).toBe("commits");
    });
  });

  it("hands the window back to React for the branch switcher", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");

    press("workspace.branchSwitcher");
    await screen.findByTestId("modal");
    await waitFor(() => expect(gpuiScreen()).toBeNull());
  });

  describe("in a workspace", () => {
    beforeEach(async () => {
      await createWorkspace(repoPath, "feat/alpha");
    });

    async function openAlpha() {
      render(<Dashboard />);
      await clickGpuiRow("feat/alpha");
      return findGpuiScreen("workspace", (s) => {
        expect(s.header.branch).toBe("feat/alpha");
      });
    }

    it("draws the workspace's controls", async () => {
      const alpha = await openAlpha();
      expect(alpha.header.home).toBe(false);
      expect(alpha.header.details).toBe(true);
      expect(alpha.header.more).toEqual({ disabled: false });
      await findGpuiScreen("workspace", (s) => {
        expect(s.header.target?.branch).toBe(defaultBranch);
      });
    });

    it("steps aside while React's details popover is open", async () => {
      await openAlpha();

      press("workspace.details");
      await screen.findByTestId("workspace-details-popover");
      await waitFor(() => expect(gpuiScreen()).toBeNull());
    });

    it("opens the target branch picker in React", async () => {
      await openAlpha();
      await findGpuiScreen("workspace", (s) => {
        expect(s.header.target).not.toBeNull();
      });

      press("workspace.target");
      await screen.findByPlaceholderText("Search branches...");
      await waitFor(() => expect(gpuiScreen()).toBeNull());
    });

    it("opens the more menu with Delete Workspace", async () => {
      await openAlpha();
      vi.mocked(popupNativeMenu).mockClear();

      press("workspace.more");
      await waitFor(() => expect(popupNativeMenu).toHaveBeenCalledOnce());
      const [[entries]] = vi.mocked(popupNativeMenu).mock.calls;
      expect(gpuiMenuLabels(entries)).toEqual(["Delete Workspace"]);
    });
  });
});
