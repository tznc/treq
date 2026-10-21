import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useRef } from "react";
import { useThemeStore } from "../stores/themeStore";
import { gpuiOverlayUpdate } from "./api";
import { isGpuiShellHost } from "./gpui-shell";
import type { GpuiScreenAction } from "./gpui-screen";

/**
 * Dashboard's window-wide dialogs, which GPUI draws over the whole window
 * (src-tauri/src/gpui_shell/overlay). Dashboard keeps which one is open;
 * GPUI reads what they list from the backend and switches branches itself,
 * and sends back what runs a Dashboard handler.
 */
export type GpuiOverlayView =
  | {
      kind: "commandPalette";
      repoPath: string;
      /** "View Stashed Changes" is offered. */
      stash: boolean;
      hasSelectedWorkspace: boolean;
      /** The current workspace, for its checks and pull request. */
      workspace: { id: number; repoPath: string; branch: string } | null;
    }
  | { kind: "keyboardShortcuts" }
  | { kind: "workspacePicker"; repoPath: string }
  | { kind: "branchSwitcher"; repoPath: string };

export type GpuiOverlaySnapshot = GpuiOverlayView & { dark: boolean };

const GPUI_OVERLAY_ACTION_EVENT = "gpui-overlay-action";

/** Whether GPUI draws Dashboard's dialogs in this window. */
export function gpuiDrawsOverlays(): boolean {
  return (
    isGpuiShellHost() &&
    window.__TREQ_WEB_UI__ !== true &&
    window.__TREQ_REACT_OVERLAYS__ !== true
  );
}

/**
 * Shows `view` in GPUI while it is set and closes it for null, running
 * `onAction` for what GPUI sends back. A no-op outside the GPUI shell.
 */
export function useGpuiOverlay(
  view: GpuiOverlayView | null,
  onAction: (action: GpuiScreenAction) => void,
): void {
  const host = isGpuiShellHost();
  const dark = useThemeStore((s) => s.actualTheme()) === "dark";
  const json = host && view ? JSON.stringify({ ...view, dark }) : null;

  // Starts at null, so a window that never showed a dialog sends nothing.
  const lastSent = useRef<string | null>(null);
  useEffect(() => {
    if (!host || json === lastSent.current) return;
    lastSent.current = json;
    void gpuiOverlayUpdate(json === null ? null : JSON.parse(json)).catch(
      (error: unknown) => console.error("gpui_overlay_update failed:", error),
    );
  }, [host, json]);

  useEffect(() => {
    if (!host) return;
    return () => {
      if (lastSent.current === null) return;
      lastSent.current = null;
      void gpuiOverlayUpdate(null).catch(() => {});
    };
  }, [host]);

  const latest = useRef(onAction);
  latest.current = onAction;
  useEffect(() => {
    if (!host) return;
    const unlisten = getCurrentWebviewWindow().listen<GpuiScreenAction>(
      GPUI_OVERLAY_ACTION_EVENT,
      (event) => latest.current(event.payload),
    );
    return () => {
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [host]);
}
