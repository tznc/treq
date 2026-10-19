/**
 * Drives GPUI's window-wide dialogs the way the native window does: reads
 * the snapshot React pushes with `gpui_overlay_update`, and emits
 * `gpui-overlay-action` events on the test bus.
 */
import { invoke } from "@tauri-apps/api/core";
import { act, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import type { GpuiOverlaySnapshot } from "../src/lib/gpui-overlay";
import type { GpuiScreenAction } from "../src/lib/gpui-screen";
import { emitOnTestBus } from "./test-event-bus";

/** The dialog GPUI shows now; null once closed, undefined before any. */
export function gpuiOverlay(): GpuiOverlaySnapshot | null | undefined {
  const calls = vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "gpui_overlay_update");
  return (calls.at(-1)?.[1] as { snapshot: GpuiOverlaySnapshot | null })
    ?.snapshot;
}

/** Waits for GPUI to show a dialog of `kind`, and returns it. */
export async function findGpuiOverlay<K extends GpuiOverlaySnapshot["kind"]>(
  kind: K,
  check?: (overlay: Extract<GpuiOverlaySnapshot, { kind: K }>) => void,
): Promise<Extract<GpuiOverlaySnapshot, { kind: K }>> {
  return waitFor(() => {
    const overlay = gpuiOverlay();
    if (overlay?.kind !== kind) {
      throw new Error(
        `GPUI shows ${overlay ? `"${overlay.kind}"` : "no dialog"}, not "${kind}"`,
      );
    }
    const typed = overlay as Extract<GpuiOverlaySnapshot, { kind: K }>;
    check?.(typed);
    return typed;
  });
}

/** Emits one action, as a pick in a GPUI dialog does. */
export function sendGpuiOverlayAction(action: GpuiScreenAction): void {
  act(() => {
    emitOnTestBus("gpui-overlay-action", action);
  });
}
