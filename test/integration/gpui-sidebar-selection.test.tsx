// @include-parallel
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import type {
  GpuiSidebarAction,
  GpuiSidebarSnapshot,
} from "../../src/lib/gpui-shell";
import { emitOnTestBus } from "../test-event-bus";

function lastSnapshot(): GpuiSidebarSnapshot | null | undefined {
  const calls = vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "gpui_sidebar_update");
  return (calls.at(-1)?.[1] as { snapshot: GpuiSidebarSnapshot | null })
    ?.snapshot;
}

function rows() {
  return lastSnapshot()?.repos[0]?.workspaces ?? [];
}

function idOf(title: string) {
  const row = rows().find((r) => r.title === title);
  if (!row) throw new Error(`no "${title}" row`);
  return row.id;
}

function selectedTitles() {
  return rows()
    .filter((r) => r.selected)
    .map((r) => r.title)
    .sort();
}

function send(action: GpuiSidebarAction) {
  act(() => {
    emitOnTestBus("gpui-sidebar-action", action);
  });
}

function click(
  title: string,
  modifiers: { shift?: boolean; toggle?: boolean },
) {
  send({
    kind: "selectWorkspace",
    id: idOf(title),
    shift: modifiers.shift ?? false,
    toggle: modifiers.toggle ?? false,
  });
}

describe("Dashboard - GPUI sidebar selection", () => {
  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
    await createWorkspace(repoPath, "feat/child", "feat/alpha");
    await createWorkspace(repoPath, "feat/beta");
    window.__TREQ_GPUI_SHELL__ = true;
  });

  afterEach(() => {
    delete window.__TREQ_GPUI_SHELL__;
  });

  async function renderRows() {
    render(<Dashboard />);
    await waitFor(() => expect(rows()).toHaveLength(3));
  }

  it("toggles workspaces into the selection with Cmd or Ctrl", async () => {
    await renderRows();
    click("feat/alpha", { toggle: true });
    click("feat/beta", { toggle: true });
    await waitFor(() => expect(lastSnapshot()?.bulkArchive).toBe(2));
    expect(selectedTitles()).toEqual(["feat/alpha", "feat/beta"]);

    click("feat/beta", { toggle: true });
    await waitFor(() => expect(lastSnapshot()?.bulkArchive).toBe(1));
    expect(selectedTitles()).toEqual(["feat/alpha"]);
  });

  it("selects the visible range with Shift", async () => {
    await renderRows();
    const titles = rows().map((r) => r.title);
    click(titles[0], {});
    click(titles[2], { shift: true });
    await waitFor(() => expect(lastSnapshot()?.bulkArchive).toBe(3));
  });

  it("selects a stack on double click, or its descendants with Shift", async () => {
    await renderRows();
    send({ kind: "selectStack", id: idOf("feat/child"), descendants: false });
    await waitFor(() =>
      expect(selectedTitles()).toEqual(["feat/alpha", "feat/child"]),
    );

    send({ kind: "selectStack", id: idOf("feat/child"), descendants: true });
    await waitFor(() => expect(lastSnapshot()?.bulkArchive).toBe(1));
    expect(selectedTitles()).toEqual(["feat/child"]);
  });

  it("clears the selection from a background click", async () => {
    await renderRows();
    click("feat/alpha", { toggle: true });
    click("feat/beta", { toggle: true });
    await waitFor(() => expect(lastSnapshot()?.bulkArchive).toBe(2));

    send({ kind: "clearSelection" });
    await waitFor(() => expect(lastSnapshot()?.bulkArchive).toBeNull());
  });

  it("clears the selection from a home click before navigating", async () => {
    await renderRows();
    click("feat/alpha", {});
    click("feat/beta", { toggle: true });
    await waitFor(() => expect(lastSnapshot()?.bulkArchive).toBe(1));

    const home = {
      kind: "selectHome",
      repoPath: lastSnapshot()!.repos[0].path,
    };
    send(home as GpuiSidebarAction);
    await waitFor(() => expect(lastSnapshot()?.bulkArchive).toBeNull());
    expect(lastSnapshot()?.repos[0].homeSelected).toBe(false);

    send(home as GpuiSidebarAction);
    await waitFor(() =>
      expect(lastSnapshot()?.repos[0].homeSelected).toBe(true),
    );
  });

  it("ignores a stack for a workspace it does not know", async () => {
    await renderRows();
    send({ kind: "selectStack", id: 999_999, descendants: false });
    expect(lastSnapshot()?.bulkArchive).toBeNull();
  });
});
