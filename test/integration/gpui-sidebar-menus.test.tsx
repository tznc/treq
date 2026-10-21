// @include-parallel
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace, getWorkspaces } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import type { GpuiSidebarSnapshot } from "../../src/lib/gpui-shell";
import { popupNativeMenu } from "../../src/lib/native-context-menu";
import { emitOnTestBus } from "../test-event-bus";

vi.mock("../../src/lib/native-context-menu", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../src/lib/native-context-menu")>();
  return { ...actual, popupNativeMenu: vi.fn() };
});

function lastSnapshot(): GpuiSidebarSnapshot | null | undefined {
  const calls = vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "gpui_sidebar_update");
  return (calls.at(-1)?.[1] as { snapshot: GpuiSidebarSnapshot | null })
    ?.snapshot;
}

async function pick(
  action:
    | { kind: "homeMenu"; repoPath: string }
    | {
        kind: "workspaceMenu";
        id: number;
      },
  label: string,
) {
  vi.mocked(popupNativeMenu).mockClear();
  act(() => {
    emitOnTestBus("gpui-sidebar-action", action);
  });
  await waitFor(() => expect(popupNativeMenu).toHaveBeenCalledOnce());
  const [[entries]] = vi.mocked(popupNativeMenu).mock.calls;
  const entry = entries.find((e) => e.kind === "item" && e.text === label);
  if (entry?.kind !== "item") throw new Error(`no "${label}" item`);
  act(() => entry.onSelect());
}

describe("Dashboard - GPUI sidebar context menus", () => {
  let repoPath: string;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
    window.__TREQ_GPUI_SHELL__ = true;
    vi.mocked(popupNativeMenu).mockClear();
  });

  afterEach(() => {
    delete window.__TREQ_GPUI_SHELL__;
  });

  async function alphaId() {
    render(<Dashboard />);
    await waitFor(() =>
      expect(lastSnapshot()?.repos[0]?.workspaces).toHaveLength(1),
    );
    return lastSnapshot()!.repos[0].workspaces[0].id;
  }

  it("switches branch from the home row menu", async () => {
    await alphaId();
    await pick({ kind: "homeMenu", repoPath }, "Switch Branch...");
    expect(
      await screen.findByPlaceholderText("Search branches..."),
    ).toBeInTheDocument();
  });

  it("renames a workspace from its menu", async () => {
    const id = await alphaId();
    await pick({ kind: "workspaceMenu", id }, "Rename Workspace");
    expect(
      await screen.findByText("Rename the branch for this workspace"),
    ).toBeInTheDocument();
  });

  it("archives a workspace from its menu", async () => {
    const id = await alphaId();
    await pick({ kind: "workspaceMenu", id }, "Archive Workspace");
    await waitFor(
      async () => {
        expect((await getWorkspaces(repoPath)).map((w) => w.id)).not.toContain(
          id,
        );
      },
      { timeout: 10_000 },
    );
    await waitFor(() =>
      expect(lastSnapshot()?.repos[0].workspaces).toHaveLength(0),
    );
  });

  it("ignores a menu for a workspace it does not know", async () => {
    await alphaId();
    act(() => {
      emitOnTestBus("gpui-sidebar-action", {
        kind: "workspaceMenu",
        id: 999_999,
      });
    });
    expect(popupNativeMenu).not.toHaveBeenCalled();
  });
});
