// @include-parallel
import { clickGpuiRow } from "../../gpui-sidebar";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createTestRepo, openRepo } from "../../utils";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";
import { useZoomSettingsStore } from "../../../src/stores/zoomSettingsStore";
import { useTerminalSettingsStore } from "../../../src/stores/terminalSettingsStore";

async function setupWorkspace(branchName: string) {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspaces = await getWorkspaces(repoPath);
  const workspace = workspaces.find((w) => w.id === workspaceId);
  if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);
  return { workspace };
}

describe("WorkspaceTerminalPane integration", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("creates new agent and shell terminals via keyboard shortcuts", async () => {
    const { workspace } = await setupWorkspace("feat/terminal-shortcuts");

    render(<Dashboard />);
    await clickGpuiRow(workspace.branch_name);
    await screen.findByTestId("workspace-terminal-pane");
    expect(screen.queryByText("Terminals")).not.toBeInTheDocument();
    expect(screen.getByTestId("terminal-pane-controls")).toBeInTheDocument();
    expect(screen.getByLabelText(/Expand terminal/i)).toBeInTheDocument();

    await user.keyboard("{Meta>}]{/Meta}");
    await waitFor(() => {
      expect(
        document.querySelector('[data-terminal-id^="agent-"]'),
      ).not.toBeNull();
    });

    await user.keyboard("{Meta>}\\{/Meta}");
    await waitFor(() => {
      expect(
        document.querySelector('[data-terminal-id^="shell-"]'),
      ).not.toBeNull();
    });
  });

  it("minimizes, maximizes, and restores the terminal pane", async () => {
    const { workspace } = await setupWorkspace("feat/terminal-resize");

    render(<Dashboard />);
    await clickGpuiRow(workspace.branch_name);
    await screen.findByTestId("workspace-terminal-pane");

    await user.keyboard("{Meta>}]{/Meta}");
    await waitFor(() => {
      expect(
        document.querySelector('[data-terminal-id^="agent-"]'),
      ).not.toBeNull();
    });

    await screen.findByLabelText(/Collapse terminal/i);

    await user.click(screen.getByLabelText(/Collapse terminal/i));
    await screen.findByLabelText(/Expand terminal/i);

    await user.keyboard("{Meta>}{Control>}j{/Control}{/Meta}");
    await waitFor(() => {
      expect(screen.getByLabelText(/Restore terminal/i)).toBeInTheDocument();
    });
    expect(screen.queryByLabelText(/Expand terminal/i)).not.toBeInTheDocument();

    await user.keyboard("{Meta>}{Control>}j{/Control}{/Meta}");
    await waitFor(() => {
      expect(screen.getByLabelText(/Maximize terminal/i)).toBeInTheDocument();
    });
    expect(
      screen.queryByLabelText(/Restore terminal/i),
    ).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Expand terminal/i)).not.toBeInTheDocument();

    await user.keyboard("{Meta>}{Control>}j{/Control}{/Meta}");
    await waitFor(() => {
      expect(screen.getByLabelText(/Restore terminal/i)).toBeInTheDocument();
    });
    expect(
      screen.queryByLabelText(/Maximize terminal/i),
    ).not.toBeInTheDocument();
  });

  it("closes an agent terminal", async () => {
    const { workspace } = await setupWorkspace("feat/terminal-close");

    render(<Dashboard />);
    await clickGpuiRow(workspace.branch_name);
    await screen.findByTestId("workspace-terminal-pane");

    await user.keyboard("{Meta>}]{/Meta}");

    const terminalPanel = await waitFor(() => {
      const el = document.querySelector('[data-terminal-id^="agent-"]');
      expect(el).not.toBeNull();
      return el as Element;
    });

    const closeButton = within(terminalPanel).getByLabelText(/close session/i);
    await user.click(closeButton);

    await waitFor(() => {
      expect(
        document.querySelector('[data-terminal-id^="agent-"]'),
      ).not.toBeInTheDocument();
    });
  });

  it("double-clicking a terminal scrolls it fully into view", async () => {
    const { workspace } = await setupWorkspace("feat/terminal-dblclick-scroll");

    render(<Dashboard />);
    await clickGpuiRow(workspace.branch_name);
    await screen.findByTestId("workspace-terminal-pane");

    await user.keyboard("{Meta>}]{/Meta}");
    await user.keyboard("{Meta>}\\{/Meta}");

    const shellPanel = await waitFor(() => {
      const el = document.querySelector('[data-terminal-id^="shell-"]');
      expect(el).not.toBeNull();
      return el as HTMLElement;
    });

    const scrollContainer = screen.getByTestId("terminal-scroll-container");
    expect(scrollContainer.contains(shellPanel)).toBe(true);

    const scrollBy = vi.fn();
    scrollContainer.scrollBy = scrollBy;

    vi.spyOn(scrollContainer, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      top: 0,
      left: 0,
      bottom: 200,
      right: 400,
      width: 400,
      height: 200,
      toJSON: () => ({}),
    });
    vi.spyOn(shellPanel, "getBoundingClientRect").mockReturnValue({
      x: 250,
      y: 0,
      top: 0,
      left: 250,
      bottom: 200,
      right: 650,
      width: 400,
      height: 200,
      toJSON: () => ({}),
    });

    await user.dblClick(within(shellPanel).getByText("Shell"));

    expect(scrollBy).toHaveBeenCalledWith({
      left: 250,
      behavior: "smooth",
    });
  });

  it("puts the workspace name on each terminal header instead of grouping", async () => {
    const { workspace } = await setupWorkspace("feat/terminal-header-label");

    render(<Dashboard />);
    await clickGpuiRow(workspace.branch_name);
    await screen.findByTestId("workspace-terminal-pane");

    await user.keyboard("{Meta>}]{/Meta}");
    const agentPanel = await waitFor(() => {
      const el = document.querySelector('[data-terminal-id^="agent-"]');
      expect(el).not.toBeNull();
      return el as HTMLElement;
    });

    await user.keyboard("{Meta>}\\{/Meta}");
    const shellPanel = await waitFor(() => {
      const el = document.querySelector('[data-terminal-id^="shell-"]');
      expect(el).not.toBeNull();
      return el as HTMLElement;
    });

    expect(document.querySelector("[data-workspace-group]")).toBeNull();
    expect(
      within(agentPanel).getByTestId("terminal-workspace-label"),
    ).toHaveTextContent("feat/terminal-header-label");
    expect(
      within(shellPanel).getByTestId("terminal-workspace-label"),
    ).toHaveTextContent("feat/terminal-header-label");
    expect(within(shellPanel).getByText("Shell")).toBeInTheDocument();
  });

  it("scales a newly opened terminal's font size by the current UI zoom", async () => {
    const { workspace } = await setupWorkspace("feat/terminal-zoom-scale");
    useTerminalSettingsStore.getState().hydrateFontSize(14);
    await useZoomSettingsStore.getState().setZoom(150);

    render(<Dashboard />);
    await clickGpuiRow(workspace.branch_name);
    await screen.findByTestId("workspace-terminal-pane");

    await user.keyboard("{Meta>}\\{/Meta}");
    const shellPanel = await waitFor(() => {
      const el = document.querySelector('[data-terminal-id^="shell-"]');
      expect(el).not.toBeNull();
      return el as HTMLElement;
    });

    await waitFor(() => {
      const styledElements = Array.from(shellPanel.querySelectorAll("*"));
      const measureEl = styledElements.find(
        (el) => (el as HTMLElement).style.fontSize !== "",
      ) as HTMLElement | undefined;
      expect(measureEl).not.toBeUndefined();
      expect(measureEl!.style.fontSize).toBe("21px");
    });

    expect(document.documentElement.style.fontSize).toBe("");
  }, 20000);
});
