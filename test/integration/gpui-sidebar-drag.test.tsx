// @include-parallel
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace, getSetting } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import type {
  GpuiSidebarAction,
  GpuiSidebarSnapshot,
} from "../../src/lib/gpui-shell";
import {
  DEFAULT_SIDEBAR_WIDTH,
  useSidebarWidthStore,
} from "../../src/stores/sidebarWidthStore";
import { emitOnTestBus } from "../test-event-bus";

function lastSnapshot(): GpuiSidebarSnapshot | null | undefined {
  const calls = vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "gpui_sidebar_update");
  return (calls.at(-1)?.[1] as { snapshot: GpuiSidebarSnapshot | null })
    ?.snapshot;
}

function rowOf(title: string) {
  const row = lastSnapshot()?.repos[0]?.workspaces.find(
    (r) => r.title === title,
  );
  if (!row) throw new Error(`no "${title}" row`);
  return row;
}

function send(action: GpuiSidebarAction) {
  act(() => {
    emitOnTestBus("gpui-sidebar-action", action);
  });
}

describe("Dashboard - GPUI sidebar drag and resize", () => {
  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
    await createWorkspace(repoPath, "feat/beta");
    useSidebarWidthStore.setState({ width: DEFAULT_SIDEBAR_WIDTH });
    window.__TREQ_GPUI_SHELL__ = true;
  });

  afterEach(() => {
    delete window.__TREQ_GPUI_SHELL__;
  });

  async function renderRows() {
    render(<Dashboard />);
    await waitFor(() =>
      expect(lastSnapshot()?.repos[0]?.workspaces).toHaveLength(2),
    );
  }

  it("stacks a dropped workspace on its target, then moves it home", async () => {
    await renderRows();
    send({
      kind: "moveWorkspace",
      id: rowOf("feat/beta").id,
      onto: rowOf("feat/alpha").id,
    });
    await waitFor(() => expect(rowOf("feat/beta").depth).toBe(1), {
      timeout: 10_000,
    });

    send({ kind: "moveWorkspace", id: rowOf("feat/beta").id, onto: null });
    await waitFor(() => expect(rowOf("feat/beta").depth).toBe(0), {
      timeout: 10_000,
    });
  });

  it("ignores a drop onto a workspace it does not know", async () => {
    await renderRows();
    send({ kind: "moveWorkspace", id: rowOf("feat/beta").id, onto: 999_999 });
    expect(rowOf("feat/beta").depth).toBe(0);
  });

  it("saves the width the edge was dragged to, and resets it", async () => {
    await renderRows();
    send({ kind: "resizeSidebar", width: 360 });
    await waitFor(() => expect(lastSnapshot()?.width).toBe(360));
    await waitFor(async () =>
      expect(await getSetting("workspace_sidebar_width")).toBe("360"),
    );

    send({ kind: "resetSidebarWidth" });
    await waitFor(() =>
      expect(lastSnapshot()?.width).toBe(DEFAULT_SIDEBAR_WIDTH),
    );
    await waitFor(async () =>
      expect(await getSetting("workspace_sidebar_width")).toBe(
        String(DEFAULT_SIDEBAR_WIDTH),
      ),
    );
  });
});
