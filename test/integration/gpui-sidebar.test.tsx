// @include-parallel
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import type { GpuiSidebarSnapshot } from "../../src/lib/gpui-shell";
import { emitOnTestBus } from "../test-event-bus";

function lastSnapshot(): GpuiSidebarSnapshot | null | undefined {
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
      emitOnTestBus("gpui-sidebar-action", { kind: "selectHome" });
    });
    await waitFor(() => expect(lastSnapshot()?.homeSelected).toBe(true));
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
