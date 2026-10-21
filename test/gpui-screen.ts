/**
 * Drives GPUI screens the way the native window does: reads the snapshot
 * React pushes with `gpui_screen_update`, and emits `gpui-screen-action`
 * events on the test bus.
 */
import { invoke } from "@tauri-apps/api/core";
import { act, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import type {
  GpuiModal,
  GpuiScreenAction,
  GpuiScreenSnapshot,
} from "../src/lib/gpui-screen";
import { emitOnTestBus } from "./test-event-bus";

/** The screen GPUI draws now; null while the webview shows. */
export function gpuiScreen(): GpuiScreenSnapshot | null | undefined {
  const calls = vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "gpui_screen_update");
  return (calls.at(-1)?.[1] as { snapshot: GpuiScreenSnapshot | null })
    ?.snapshot;
}

/** Waits for GPUI to draw a screen of `kind`, and returns it. */
export async function findGpuiScreen<K extends GpuiScreenSnapshot["kind"]>(
  kind: K,
  check?: (screen: Extract<GpuiScreenSnapshot, { kind: K }>) => void,
): Promise<Extract<GpuiScreenSnapshot, { kind: K }>> {
  return waitFor(() => {
    const screen = gpuiScreen();
    if (screen?.kind !== kind) {
      throw new Error(
        `GPUI shows ${screen ? `"${screen.kind}"` : "no screen"}, not "${kind}"`,
      );
    }
    const typed = screen as Extract<GpuiScreenSnapshot, { kind: K }>;
    check?.(typed);
    return typed;
  });
}

/** The dialog GPUI draws over its screen now; null once closed. */
export function gpuiModal(): GpuiModal | null | undefined {
  const calls = vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "gpui_modal_update");
  return (calls.at(-1)?.[1] as { modal: GpuiModal | null })?.modal;
}

/** Waits for GPUI to draw a dialog of `kind`, and returns it. */
export async function findGpuiModal<K extends GpuiModal["kind"]>(
  kind: K,
): Promise<Extract<GpuiModal, { kind: K }>> {
  return waitFor(() => {
    const modal = gpuiModal();
    if (modal?.kind !== kind) {
      throw new Error(`GPUI dialog is ${modal ? `"${modal.kind}"` : "closed"}`);
    }
    return modal as Extract<GpuiModal, { kind: K }>;
  });
}

/** Emits one action, as a click on a GPUI screen does. */
export function sendGpuiScreenAction(action: GpuiScreenAction): void {
  act(() => {
    emitOnTestBus("gpui-screen-action", action);
  });
}
