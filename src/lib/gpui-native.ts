import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useRef } from "react";
import { isGpuiShellHost } from "./gpui-shell";

/**
 * What a GPUI screen did through the backend itself
 * (src-tauri/src/gpui_shell/native.rs), so React updates the views it still
 * draws. React never repeats the change.
 */
export type GpuiNativeEvent =
  | { kind: "repoOpened"; path: string }
  | { kind: "settingsChanged" }
  | { kind: "repoSettingsChanged"; repoPath: string }
  /** A push, sync or rebase changed the repository at `repoPath`. */
  | { kind: "workspaceChanged"; repoPath: string }
  | {
      kind: "toast";
      title: string;
      description: string;
      tone: "success" | "warning" | "error";
      /** A button whose click GPUI handles: `gpuiToastAction`. */
      action?: { label: string; id: number };
    };

const GPUI_NATIVE_EVENT = "gpui-native-event";

/** Runs `onEvent` for each native event while mounted in the GPUI shell. */
export function useGpuiNativeEvents(
  onEvent: (event: GpuiNativeEvent) => void,
): void {
  const latest = useRef(onEvent);
  useEffect(() => {
    latest.current = onEvent;
  });
  useEffect(() => {
    if (!isGpuiShellHost()) return;
    const unlisten = getCurrentWebviewWindow().listen<GpuiNativeEvent>(
      GPUI_NATIVE_EVENT,
      (event) => latest.current(event.payload),
    );
    return () => {
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);
}
