// @include-parallel
import * as React from "react";
import { act, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import * as api from "../../src/lib/api";
import { createWorkspace } from "../../src/lib/api";
import { dispatchRefreshWorkspaceChanges } from "../../src/lib/change-file-drag";
import type { GpuiNativeEvent } from "../../src/lib/gpui-native";
import { popupNativeMenu } from "../../src/lib/native-context-menu";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { clickGpuiRow, findGpuiToast, gpuiMenuLabels } from "../gpui-sidebar";
import { emitOnTestBus } from "../test-event-bus";
import { render, screen } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

const press = (id: string) => sendGpuiScreenAction({ kind: "press", id });

function sendNativeEvent(event: GpuiNativeEvent) {
  act(() => {
    emitOnTestBus("gpui-native-event", event);
  });
}

describe("GPUI workspace header", () => {
  let repoPath: string;
  let defaultBranch: string;

  beforeEach(() => {
    ({ repoPath, defaultBranch } = createTestRepo(false));
    openRepo(repoPath);
  });

  it("asks GPUI for the home repository's route", async () => {
    render(<Dashboard />);

    const home = await findGpuiScreen("workspace", (s) => {
      expect(s.homeBranch).toBe(defaultBranch);
    });
    expect(home.repoPath).toBe(repoPath);
    expect(home.workspaceId).toBeNull();
    expect(home.tab).toBe("overview");
    expect(home.tabRow).toBe(true);
    expect(home.retargeting).toBe(false);
    expect(home.runningChecks).toEqual([]);
  });

  it("asks GPUI to read again after a push or sync", async () => {
    render(<Dashboard />);
    const { revision } = await findGpuiScreen("workspace");

    sendNativeEvent({ kind: "workspaceChanged", repoPath });
    await findGpuiScreen("workspace", (s) => {
      expect(s.revision).toBeGreaterThan(revision);
    });
  });

  it("shows the toasts GPUI sends", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");

    sendNativeEvent({
      kind: "toast",
      title: "Synced with conflicts",
      description: "Fetched and pushed; resolve remaining conflicts locally",
      tone: "warning",
    });
    await findGpuiToast("Synced with conflicts");
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
      expect(s.tab).toBe("commits");
    });
  });

  it("draws the Changes tab in GPUI", async () => {
    render(<Dashboard />);
    const home = await findGpuiScreen("workspace");
    expect(home.code).toBe(true);

    sendGpuiScreenAction({
      kind: "field",
      field: "workspace.tab",
      value: "changes",
    });
    const changes = await findGpuiScreen("workspace", (s) => {
      expect(s.tab).toBe("changes");
      expect(s.code).toBe(true);
    });
    expect(changes.showCommitted).toBeNull();
    expect(changes.moveTargets).toEqual([]);
  });

  it("asks GPUI's Changes tab to read again when files change", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    sendGpuiScreenAction({
      kind: "field",
      field: "workspace.tab",
      value: "changes",
    });
    const { changesRevision } = await findGpuiScreen("workspace", (s) => {
      expect(s.tab).toBe("changes");
    });

    act(() => {
      dispatchRefreshWorkspaceChanges();
    });
    await findGpuiScreen("workspace", (s) => {
      expect(s.changesRevision).toBeGreaterThan(changesRevision);
    });
  });

  it("shows the refresh GPUI's Changes tab reports", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    sendGpuiScreenAction({
      kind: "field",
      field: "workspace.tab",
      value: "changes",
    });
    await findGpuiScreen("workspace", (s) => {
      expect(s.tab).toBe("changes");
    });

    sendGpuiScreenAction({
      kind: "field",
      field: "changes.refreshing",
      value: true,
    });
    await findGpuiScreen("workspace", (s) => {
      expect(s.refreshing).toBe(true);
    });
    sendGpuiScreenAction({
      kind: "field",
      field: "changes.refreshing",
      value: false,
    });
    await findGpuiScreen("workspace", (s) => {
      expect(s.refreshing).toBe(false);
    });
  });

  it("opens a commit from the Code tab's history", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");

    sendGpuiScreenAction({
      kind: "field",
      field: "code.commit",
      value: "zzzzzzzz",
    });
    await findGpuiScreen("workspace", (s) => {
      expect(s.tab).toBe("commits");
      expect(s.code).toBe(true);
      expect(s.scrollToCommit).toBe("zzzzzzzz");
    });

    sendGpuiScreenAction({
      kind: "field",
      field: "commits.scrolled",
      value: null,
    });
    await findGpuiScreen("workspace", (s) => {
      expect(s.tab).toBe("commits");
      expect(s.scrollToCommit).toBeNull();
    });
  });

  it("shows the Changes tab when the Commits tab asks for it", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    sendGpuiScreenAction({
      kind: "field",
      field: "workspace.tab",
      value: "commits",
    });
    await findGpuiScreen("workspace", (s) => {
      expect(s.code).toBe(true);
    });
    sendGpuiScreenAction({
      kind: "field",
      field: "workspace.tab",
      value: "changes",
    });
    await findGpuiScreen("workspace", (s) => {
      expect(s.tab).toBe("changes");
      expect(s.code).toBe(true);
    });
  });

  it("opens the file browser in React for a Code tab entry", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace", (s) => {
      expect(s.code).toBe(true);
    });

    sendGpuiScreenAction({
      kind: "field",
      field: "code.open",
      value: { name: "README.md", isDirectory: false },
    });
    await findGpuiScreen("workspace", (s) => {
      expect(s.tab).toBe("overview");
      expect(s.code).toBe(false);
    });
  });

  describe("with GPUI's file browser", () => {
    beforeEach(() => {
      window.__TREQ_REACT_FILE_BROWSER__ = false;
    });

    async function openReadme() {
      render(<Dashboard />);
      await findGpuiScreen("workspace", (s) => {
        expect(s.code).toBe(true);
      });
      sendGpuiScreenAction({
        kind: "field",
        field: "code.open",
        value: { name: "README.md", isDirectory: false },
      });
      return findGpuiScreen("workspace", (s) => {
        expect(s.fileBrowser).not.toBeNull();
      });
    }

    it("draws the file browser in GPUI for a Code tab entry", async () => {
      const s = await openReadme();
      expect(s.tab).toBe("overview");
      expect(s.code).toBe(true);
      expect(s.fileBrowser).toEqual({
        selectedFile: expect.stringMatching(/\/README\.md$/),
        expandedDir: null,
      });
      expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
    });

    it("opens a folder entry expanded", async () => {
      render(<Dashboard />);
      await findGpuiScreen("workspace");
      sendGpuiScreenAction({
        kind: "field",
        field: "code.open",
        value: { name: "src", isDirectory: true },
      });
      await findGpuiScreen("workspace", (s) => {
        expect(s.fileBrowser).toEqual({
          selectedFile: null,
          expandedDir: expect.stringMatching(/\/src$/),
        });
      });
    });

    it("goes back to the Code tab's file list", async () => {
      await openReadme();
      press("fileBrowser.back");
      await findGpuiScreen("workspace", (s) => {
        expect(s.fileBrowser).toBeNull();
        expect(s.code).toBe(true);
      });
    });

    it("asks GPUI's file browser to read again when files change", async () => {
      const { changesRevision } = await openReadme();
      act(() => {
        dispatchRefreshWorkspaceChanges();
      });
      await findGpuiScreen("workspace", (s) => {
        expect(s.changesRevision).toBeGreaterThan(changesRevision);
      });
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
        expect(s.workspaceId).not.toBeNull();
      });
    }

    it("asks GPUI for the workspace's route", async () => {
      const alpha = await openAlpha();
      expect(alpha.repoPath).toBe(repoPath);
      expect(alpha.creatingPr).toBe(false);
    });

    it("steps aside while React's details popover is open", async () => {
      await openAlpha();

      press("workspace.details");
      await screen.findByTestId("workspace-details-popover");
      await waitFor(() => expect(gpuiScreen()).toBeNull());
    });

    it("opens the target branch picker in React", async () => {
      await openAlpha();

      await waitFor(() => {
        press("workspace.target");
        expect(screen.getByPlaceholderText("Search branches...")).toBeTruthy();
      });
      await waitFor(() => expect(gpuiScreen()).toBeNull());
    });

    it("starts an agent with the review GPUI's Changes tab finished", async () => {
      const alpha = await openAlpha();
      const createSession = vi.spyOn(api, "createSession");

      sendGpuiScreenAction({
        kind: "field",
        field: "changes.review",
        value: {
          markdown: "# Review\n\nTighten this",
          mode: "plan",
          useIdleAgent: false,
        },
      });
      await waitFor(() => {
        expect(createSession).toHaveBeenCalledWith(
          repoPath,
          alpha.workspaceId,
          "Code Review",
        );
      });
    });

    it("starts an agent with the review GPUI's file browser finished", async () => {
      window.__TREQ_REACT_FILE_BROWSER__ = false;
      const alpha = await openAlpha();
      const createSession = vi.spyOn(api, "createSession");

      sendGpuiScreenAction({
        kind: "field",
        field: "fileBrowser.review",
        value: {
          markdown: "# Review\n\nRename this",
          mode: "acceptEdits",
          useIdleAgent: false,
        },
      });
      await waitFor(() => {
        expect(createSession).toHaveBeenCalledWith(
          repoPath,
          alpha.workspaceId,
          "Code Review",
        );
      });
    });

    it("starts an agent with the agent comment GPUI's Changes tab sends", async () => {
      const alpha = await openAlpha();
      const createSession = vi.spyOn(api, "createSession");

      sendGpuiScreenAction({
        kind: "field",
        field: "changes.agentComment",
        value: {
          id: "ag",
          repo_path: repoPath,
          target_type: "workspace_diff",
          target_id: String(alpha.workspaceId),
          file_path: "a.rs",
          hunk_id: null,
          start_line: 2,
          end_line: 2,
          side: "new",
          comment_text: "Use one line",
          suggested_replacement: null,
          status: "open",
          source: "local-agent",
          created_at: "",
          resolved_at: null,
        },
      });
      await waitFor(() => {
        expect(createSession).toHaveBeenCalledWith(
          repoPath,
          alpha.workspaceId,
          "Review Comment",
        );
      });
    });

    it("starts an agent with the log lines GPUI's Logs tab sends", async () => {
      const alpha = await openAlpha();
      const createSession = vi.spyOn(api, "createSession");

      sendGpuiScreenAction({
        kind: "field",
        field: "logs.sendToAgent",
        value: "Here are 1 log line from my check logs.",
      });
      await waitFor(() => {
        expect(createSession).toHaveBeenCalledWith(
          repoPath,
          alpha.workspaceId,
          "Logs",
        );
      });
    });

    it("opens the merge preview in GPUI and comes back", async () => {
      const alpha = await openAlpha();

      press("workspace.merge");
      const preview = await findGpuiScreen("mergePreview");
      expect(preview).toEqual({
        kind: "mergePreview",
        dark: false,
        repoPath,
        workspaceId: alpha.workspaceId,
      });

      press("mergePreview.cancel");
      await findGpuiScreen("workspace", (s) => {
        expect(s.workspaceId).toBe(alpha.workspaceId);
      });
    });

    it("leaves the merge preview once GPUI merged", async () => {
      await openAlpha();
      press("workspace.merge");
      await findGpuiScreen("mergePreview");

      press("mergePreview.merged");
      await findGpuiScreen("workspace");
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
