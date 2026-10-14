// @include-parallel
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import type { GpuiSidebarSnapshot } from "../../src/lib/gpui-shell";
import { emitOnTestBus } from "../test-event-bus";

function lastSnapshot() {
  const snapshot = lastPushed();
  if (!snapshot) return snapshot;
  const [repo] = snapshot.repos;
  return { ...snapshot, ...repo };
}

function lastPushed(): GpuiSidebarSnapshot | null | undefined {
  const calls = vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "gpui_sidebar_update");
  return (calls.at(-1)?.[1] as { snapshot: GpuiSidebarSnapshot | null })
    ?.snapshot;
}

describe("Dashboard - GPUI sidebar host", () => {
  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
    window.__TREQ_GPUI_SHELL__ = true;
  });

  afterEach(() => {
    delete window.__TREQ_GPUI_SHELL__;
  });

  it("replaces the React sidebar and selects a workspace from a GPUI click", async () => {
    render(<Dashboard />);

    await waitFor(() => {
      expect(lastSnapshot()?.workspaces.map((w) => w.title)).toEqual([
        "feat/alpha",
      ]);
    });
    expect(screen.queryByTestId("workspace-sidebar")).toBeNull();
    expect(lastSnapshot()?.homeSelected).toBe(true);

    const [{ id }] = lastSnapshot()!.workspaces;
    act(() => {
      emitOnTestBus("gpui-sidebar-action", { kind: "selectWorkspace", id });
    });

    await waitFor(() => {
      const snapshot = lastSnapshot();
      expect(snapshot?.homeSelected).toBe(false);
      expect(snapshot?.workspaces[0].selected).toBe(true);
    });

    act(() => {
      emitOnTestBus("gpui-sidebar-action", {
        kind: "selectHome",
        repoPath: lastSnapshot()!.path,
      });
    });
    await waitFor(() => expect(lastSnapshot()?.homeSelected).toBe(true));
  });
});

describe("Dashboard - GPUI sidebar actions", () => {
  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
    window.__TREQ_GPUI_SHELL__ = true;
  });

  afterEach(() => {
    delete window.__TREQ_GPUI_SHELL__;
  });

  it("pushes the home branch and local capabilities", async () => {
    render(<Dashboard />);
    await waitFor(() => expect(lastSnapshot()?.homeRef).toBeTruthy());
    expect(lastSnapshot()).toMatchObject({
      active: true,
      agentUnavailable: null,
      shellUnavailable: null,
      bulkArchive: null,
    });
  });

  it("opens the stack dialog from the home row", async () => {
    render(<Dashboard />);
    await waitFor(() => expect(lastSnapshot()?.homeRef).toBeTruthy());
    act(() => {
      emitOnTestBus("gpui-sidebar-action", { kind: "stack", id: null });
    });
    const dialog = await screen.findByTestId("modal");
    expect(within(dialog).getByLabelText("Branch Name")).toBeTruthy();
  });

  it("opens the stack dialog from a workspace row", async () => {
    render(<Dashboard />);
    await waitFor(() => expect(lastSnapshot()?.workspaces).toHaveLength(1));
    const [{ id }] = lastSnapshot()!.workspaces;
    act(() => {
      emitOnTestBus("gpui-sidebar-action", { kind: "stack", id });
    });
    const dialog = await screen.findByTestId("modal");
    expect(within(dialog).getByLabelText("Branch Name")).toBeTruthy();
  });

  it("ignores actions for workspaces it does not know", async () => {
    render(<Dashboard />);
    await waitFor(() => expect(lastSnapshot()?.workspaces).toHaveLength(1));
    act(() => {
      emitOnTestBus("gpui-sidebar-action", { kind: "stack", id: 999_999 });
      emitOnTestBus("gpui-sidebar-action", {
        kind: "selectWorkspace",
        id: 999_999,
      });
    });
    expect(screen.queryByTestId("modal")).toBeNull();
    expect(lastSnapshot()?.homeSelected).toBe(true);
  });
});

describe("Dashboard - without the GPUI shell", () => {
  it("renders the React sidebar and never pushes a snapshot", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    vi.mocked(invoke).mockClear();
    render(<Dashboard />);
    expect(await screen.findByTestId("workspace-sidebar")).toBeTruthy();
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([cmd]) => cmd === "gpui_sidebar_update"),
    ).toBe(false);
  });
});
