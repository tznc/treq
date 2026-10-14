// @include-parallel
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "../test-utils";
import { createTestRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import type { GpuiSidebarSnapshot } from "../../src/lib/gpui-shell";
import { clearLastOpenedRemoteRepository } from "../../src/lib/remote-repository";
import { emitOnTestBus } from "../test-event-bus";
import { openSavedRemoteRepo } from "../remote-repo";

function lastSnapshot(): GpuiSidebarSnapshot | null | undefined {
  const calls = vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "gpui_sidebar_update");
  return (calls.at(-1)?.[1] as { snapshot: GpuiSidebarSnapshot | null })
    ?.snapshot;
}

describe("Dashboard - GPUI sidebar for a remote repository", () => {
  beforeEach(async () => {
    window.history.replaceState({}, "", "/");
    await clearLastOpenedRemoteRepository();
    const { repoPath } = createTestRepo(false);
    await createWorkspace(repoPath, "feat/remote-ui");
    await openSavedRemoteRepo(repoPath);
    window.__TREQ_GPUI_SHELL__ = true;
  });

  afterEach(() => {
    delete window.__TREQ_GPUI_SHELL__;
  });

  it("draws the remote workspaces and selects one", async () => {
    render(<Dashboard />);
    await waitFor(
      () =>
        expect(
          lastSnapshot()?.repos[0]?.workspaces.map((w) => w.title),
        ).toEqual(["feat/remote-ui"]),
      { timeout: 10_000 },
    );
    expect(screen.queryByTestId("workspace-sidebar")).toBeNull();

    const [{ id }] = lastSnapshot()!.repos[0].workspaces;
    act(() => {
      emitOnTestBus("gpui-sidebar-action", {
        kind: "selectWorkspace",
        id,
        shift: false,
        toggle: false,
      });
    });
    const header = await screen.findByTestId("show-workspace-header");
    expect(await within(header).findByText("feat/remote-ui")).toBeTruthy();
  });
});
