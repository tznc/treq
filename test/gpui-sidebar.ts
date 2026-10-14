/**
 * Drives the GPUI sidebar the way the native window does: reads the snapshot
 * React pushes with `gpui_sidebar_update`, and emits `gpui-sidebar-action`
 * events on the test bus. Context menus are recorded by the
 * `popupNativeMenu` mock in setup.integration.ts.
 */
import { invoke } from "@tauri-apps/api/core";
import { act, waitFor } from "@testing-library/react";
import { expect, vi } from "vitest";
import type {
  GpuiSidebarAction,
  GpuiSidebarRepo,
  GpuiSidebarRow,
  GpuiSidebarSnapshot,
} from "../src/lib/gpui-shell";
import {
  popupNativeMenu,
  type NativeMenuEntry,
} from "../src/lib/native-context-menu";
import { emitOnTestBus } from "./test-event-bus";

/** The snapshot the sidebar draws now; null once it is hidden. */
export function gpuiSnapshot(): GpuiSidebarSnapshot | null | undefined {
  const calls = vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "gpui_sidebar_update");
  return (calls.at(-1)?.[1] as { snapshot: GpuiSidebarSnapshot | null })
    ?.snapshot;
}

/** A repository's section: the given path, or the one on screen. */
export function gpuiRepo(repoPath?: string): GpuiSidebarRepo | undefined {
  const repos = gpuiSnapshot()?.repos ?? [];
  return repoPath === undefined
    ? repos.find((r) => r.active)
    : repos.find((r) => r.path === repoPath);
}

/** The rows of a repository's section, in sidebar order. */
export function gpuiRows(repoPath?: string): GpuiSidebarRow[] {
  return gpuiRepo(repoPath)?.workspaces ?? [];
}

/** Titles of the rows of a repository's section, in sidebar order. */
export function gpuiTitles(repoPath?: string): string[] {
  return gpuiRows(repoPath).map((r) => r.title);
}

/** Waits for a row titled `title` (a branch name or workspace title). */
export async function findGpuiRow(
  title: string,
  options: { repoPath?: string; timeout?: number } = {},
): Promise<GpuiSidebarRow> {
  return waitFor(
    () => {
      const row = gpuiRows(options.repoPath).find((r) => r.title === title);
      if (!row) {
        throw new Error(
          `no "${title}" row in the GPUI sidebar (rows: ${JSON.stringify(gpuiTitles(options.repoPath))})`,
        );
      }
      return row;
    },
    options.timeout === undefined ? undefined : { timeout: options.timeout },
  );
}

/** Waits until no row titled `title` is drawn. */
export async function waitForNoGpuiRow(
  title: string,
  options: { repoPath?: string; timeout?: number } = {},
): Promise<void> {
  await waitFor(
    () => expect(gpuiTitles(options.repoPath)).not.toContain(title),
    options.timeout === undefined ? undefined : { timeout: options.timeout },
  );
}

/** Emits one action, as a click in the GPUI sidebar does. */
export function sendGpuiAction(action: GpuiSidebarAction): void {
  act(() => {
    emitOnTestBus("gpui-sidebar-action", action);
  });
}

/**
 * Clicks a row. `toggle` is Cmd/Ctrl. A row of a repository off screen
 * opens that repository, as in the native sidebar.
 */
export async function clickGpuiRow(
  title: string,
  options: {
    repoPath?: string;
    shift?: boolean;
    toggle?: boolean;
    timeout?: number;
  } = {},
): Promise<GpuiSidebarRow> {
  const row = await findGpuiRow(title, options);
  const repo = gpuiRepo(options.repoPath);
  if (repo && !repo.active) {
    sendGpuiAction({
      kind: "openRepoWorkspace",
      repoPath: repo.path,
      id: row.id,
    });
  } else {
    sendGpuiAction({
      kind: "selectWorkspace",
      id: row.id,
      shift: options.shift ?? false,
      toggle: options.toggle ?? false,
    });
  }
  return row;
}

/** Double-clicks a row: selects its stack, or its descendants with Shift. */
export async function doubleClickGpuiRow(
  title: string,
  options: { shift?: boolean } = {},
): Promise<void> {
  const row = await findGpuiRow(title);
  sendGpuiAction({
    kind: "selectStack",
    id: row.id,
    descendants: options.shift ?? false,
  });
}

/** Clicks a repository's home row (the one on screen by default). */
export async function clickGpuiHome(repoPath?: string): Promise<void> {
  const repo = await waitFor(() => {
    const r = gpuiRepo(repoPath);
    if (!r) throw new Error("no repository section in the GPUI sidebar");
    return r;
  });
  sendGpuiAction({ kind: "selectHome", repoPath: repo.path });
}

/** Clicks a row's "Start agent" hover action; null is the home row's. */
export async function startGpuiAgent(title: string | null): Promise<void> {
  const id = title === null ? null : (await findGpuiRow(title)).id;
  sendGpuiAction({ kind: "startAgent", id });
}

/** Clicks a row's "Open shell" hover action; null is the home row's. */
export async function openGpuiShell(title: string | null): Promise<void> {
  const id = title === null ? null : (await findGpuiRow(title)).id;
  sendGpuiAction({ kind: "openShell", id });
}

/** Clicks a row's "Stack" hover action; null is the home row's. */
export async function stackGpui(title: string | null): Promise<void> {
  const id = title === null ? null : (await findGpuiRow(title)).id;
  sendGpuiAction({ kind: "stack", id });
}

/** Drops row `title` onto row `onto`, or onto the list (root) when null. */
export async function moveGpuiRow(
  title: string,
  onto: string | null,
): Promise<void> {
  const row = await findGpuiRow(title);
  const target = onto === null ? null : (await findGpuiRow(onto)).id;
  sendGpuiAction({ kind: "moveWorkspace", id: row.id, onto: target });
}

/** Opens a context menu and returns its entries. */
export async function openGpuiMenu(
  action: Extract<
    GpuiSidebarAction,
    { kind: "homeMenu" | "workspaceMenu" | "repoWorkspaceMenu" }
  >,
): Promise<NativeMenuEntry[]> {
  vi.mocked(popupNativeMenu).mockClear();
  sendGpuiAction(action);
  await waitFor(() => expect(popupNativeMenu).toHaveBeenCalledOnce());
  return vi.mocked(popupNativeMenu).mock.calls[0][0];
}

/** Item labels of a menu, submenus included by their own label. */
export function gpuiMenuLabels(entries: NativeMenuEntry[]): string[] {
  return entries.flatMap((e) => (e.kind === "separator" ? [] : [e.text]));
}

/** Opens a row's context menu and picks `label`. */
export async function pickGpuiRowMenuItem(
  title: string,
  label: string,
): Promise<void> {
  const row = await findGpuiRow(title);
  await pickGpuiMenuItem({ kind: "workspaceMenu", id: row.id }, label);
}

/** Opens a home row's context menu (the one on screen by default) and picks `label`. */
export async function pickGpuiHomeMenuItem(
  label: string,
  repoPath?: string,
): Promise<void> {
  const repo = gpuiRepo(repoPath);
  if (!repo) throw new Error("no repository section in the GPUI sidebar");
  await pickGpuiMenuItem({ kind: "homeMenu", repoPath: repo.path }, label);
}

async function pickGpuiMenuItem(
  action: Parameters<typeof openGpuiMenu>[0],
  label: string,
): Promise<void> {
  const entries = await openGpuiMenu(action);
  const entry = entries.find((e) => e.kind === "item" && e.text === label);
  if (entry?.kind !== "item") {
    throw new Error(
      `no "${label}" item (menu: ${JSON.stringify(gpuiMenuLabels(entries))})`,
    );
  }
  act(() => entry.onSelect());
}
