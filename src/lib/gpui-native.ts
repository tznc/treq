import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useRef } from "react";
import { isGpuiShellHost } from "./gpui-shell";
import type { CutoffReason } from "./remote-cert-lifecycle";
import type { SessionCreationInfo } from "../types/sessions";

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
      tone: "success" | "warning" | "error" | "info";
      /** A button whose click GPUI handles: `gpuiToastAction`. */
      action?: { label: string; id: number };
    }
  /** How a change GPUI sent to the remote repository ended. */
  | {
      kind: "remoteMutation";
      status: "applied" | "already_applied" | "ambiguous";
      reason?: string;
    }
  /** A remote command found the endpoint's credential cut off. */
  | { kind: "remoteCutoff"; endpointId: string; reason: CutoffReason }
  /**
   * The backend started an agent session (src-tauri/src/gpui_shell/launch.rs):
   * it already created the session, recorded its prompt and keeps its start
   * for GPUI's terminal pane. React makes it the active session, and with
   * `select` shows its workspace.
   */
  | {
      kind: "agentSessionStarted";
      session: SessionCreationInfo;
      paneSessionId: number;
      select: boolean;
    }
  /** GPUI deleted or archived these workspaces: leave them if they show. */
  | { kind: "workspacesRemoved"; repoPath: string; ids: number[] }
  /** A GPUI menu or button opens something React still routes. */
  | { kind: "open"; what: GpuiOpenRequest };

/** What GPUI asks React to open (src-tauri/src/gpui_shell/native.rs). */
export type GpuiOpenRequest =
  | { kind: "branchSwitcher" }
  | { kind: "addRepository" }
  | { kind: "repoSettings"; repoPath: string }
  | { kind: "locateRepo"; repoPath: string }
  | { kind: "removeRepo"; repoPath: string }
  | { kind: "renameWorkspace"; repoPath: string; workspaceId: number }
  | { kind: "prReview"; number: number; state: string };

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
