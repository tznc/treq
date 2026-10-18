import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useRef } from "react";
import { useThemeStore } from "../stores/themeStore";
import { gpuiScreenUpdate } from "./api";
import { isGpuiShellHost } from "./gpui-shell";
import { useWindowModalBackdrop } from "./window-modals";

/**
 * Screens GPUI draws in place of the webview (src-tauri/src/gpui_shell/screen).
 * React keeps rendering its own version underneath: GPUI hides the webview
 * while a screen is pushed, and shows it again for null.
 */

export interface GpuiChecklistTool {
  binary: string;
  label: string;
  note: string | null;
  required: boolean;
  /** null while the check is still running. */
  installed: boolean | null;
  installUrl: string;
}

export interface GpuiChecklist {
  title: string;
  validating: boolean;
  groups: { label: string; error: boolean; tools: GpuiChecklistTool[] }[];
}

export type GpuiScreenView = {
  kind: "onboarding";
  remoteLabel: string | null;
  checklist: GpuiChecklist;
};

export type GpuiScreenSnapshot = GpuiScreenView & { dark: boolean };

export type GpuiScreenAction =
  | { kind: "openRepo" }
  | { kind: "openRemote" }
  | { kind: "recheckPrerequisites" }
  | { kind: "openUrl"; url: string };

const GPUI_SCREEN_ACTION_EVENT = "gpui-screen-action";

/** Push a screen to GPUI; null shows the webview. Failures are logged, not thrown. */
export async function updateGpuiScreen(
  snapshot: GpuiScreenSnapshot | null,
): Promise<void> {
  try {
    await gpuiScreenUpdate(snapshot);
  } catch (error) {
    console.error("gpui_screen_update failed:", error);
  }
}

/** Listen for GPUI screen clicks sent to this webview window only. */
export function listenGpuiScreenActions(
  handler: (action: GpuiScreenAction) => void,
): Promise<() => void> {
  return getCurrentWebviewWindow().listen<GpuiScreenAction>(
    GPUI_SCREEN_ACTION_EVENT,
    (event) => handler(event.payload),
  );
}

/**
 * Draws `view` in GPUI while the calling component is mounted, and runs
 * `onAction` for its clicks. A webview modal (a dialog GPUI does not draw
 * yet) brings the webview back while it is open, so it stays usable.
 */
export function useGpuiScreen(
  view: GpuiScreenView | null,
  onAction: (action: GpuiScreenAction) => void,
): void {
  const host = isGpuiShellHost();
  const dark = useThemeStore((s) => s.actualTheme()) === "dark";
  const modal = useWindowModalBackdrop(host) !== null;
  const json =
    host && view && !modal ? JSON.stringify({ ...view, dark }) : null;

  const lastSent = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    if (!host || json === lastSent.current) return;
    lastSent.current = json;
    void updateGpuiScreen(json === null ? null : JSON.parse(json));
  }, [host, json]);

  useEffect(() => {
    if (!host) return;
    return () => {
      lastSent.current = undefined;
      void updateGpuiScreen(null);
    };
  }, [host]);

  const latest = useRef(onAction);
  latest.current = onAction;
  useEffect(() => {
    if (!host) return;
    const unlisten = listenGpuiScreenActions((action) =>
      latest.current(action),
    );
    return () => {
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [host]);
}
