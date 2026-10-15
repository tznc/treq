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

describe("Dashboard - modals over the GPUI sidebar", () => {
  let repoPath: string;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
    window.__TREQ_GPUI_SHELL__ = true;
  });

  afterEach(() => {
    delete window.__TREQ_GPUI_SHELL__;
  });

  it("centres modals in the window, past the GPUI sidebar", async () => {
    render(<Dashboard />);
    await waitFor(() => expect(lastSnapshot()?.width).toBeGreaterThan(0));
    expect(
      document.documentElement.style.getPropertyValue("--window-offset-left"),
    ).toBe(`${lastSnapshot()!.width}px`);
  });

  it("covers the sidebar while a dialog is open and closes it from there", async () => {
    render(<Dashboard />);
    await waitFor(() =>
      expect(lastSnapshot()?.repos[0]?.workspaces).toHaveLength(1),
    );
    expect(lastSnapshot()?.modal).toBeNull();
    const [{ id }] = lastSnapshot()!.repos[0].workspaces;
    await pick({ kind: "workspaceMenu", id }, "Rename Workspace");
    await screen.findByText("Rename the branch for this workspace");
    await waitFor(() => expect(lastSnapshot()?.modal).toBeGreaterThan(0));

    act(() => {
      emitOnTestBus("gpui-sidebar-action", { kind: "dismissModal" });
    });
    await waitFor(() =>
      expect(
        screen.queryByText("Rename the branch for this workspace"),
      ).toBeNull(),
    );
    await waitFor(() => expect(lastSnapshot()?.modal).toBeNull());
  });

  it("covers the sidebar while the command palette is open and closes it from there", async () => {
    render(<Dashboard />);
    await waitFor(() => expect(lastSnapshot()).toBeTruthy());
    act(() => {
      emitOnTestBus("gpui-sidebar-action", { kind: "openCommandPalette" });
    });
    expect(await screen.findByTestId("modal")).toBeInTheDocument();
    await waitFor(() => expect(lastSnapshot()?.modal).toBeGreaterThan(0));
    act(() => {
      emitOnTestBus("gpui-sidebar-action", { kind: "dismissModal" });
    });
    await waitFor(() => expect(screen.queryByTestId("modal")).toBeNull());
    await waitFor(() => expect(lastSnapshot()?.modal).toBeNull());
  });
});
