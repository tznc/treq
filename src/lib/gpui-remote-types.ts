import type { PtyLaunchSpec, SshEndpoint } from "./api-types-remote";
import { isGpuiShellHost } from "./gpui-shell";

/**
 * What Dashboard draws above a remote repository's workspace view, which
 * GPUI draws in its place (src-tauri/src/gpui_shell/screen/remote_chrome.rs).
 * It presses "remote.terminal", "remote.refresh", "remote.wake" and
 * "remote.reconnect".
 */
export interface GpuiRemoteChrome {
  displayName: string;
  /** An SSH endpoint is connected, so the strip offers Terminal. */
  terminal: boolean;
  /** `RemoteConnectionState`. */
  state: string;
  cutOff: boolean;
  /** `RemoteCapabilityNotice`'s lines. */
  notices: string[];
}

/**
 * Dashboard's explicit-endpoint repository form, drawn under GPUI's
 * onboarding card (src-tauri/src/gpui_shell/screen/onboarding_remote.rs).
 * It presses "remoteRepo.probe", "remoteRepo.open", "remoteRepo.clone",
 * "remoteRepo.init" and "remoteRepo.reauthenticate", and sets the
 * "remoteRepo.path", "remoteRepo.cloneUrl", "remoteRepo.confirmInit" and
 * "remoteRepo.select" fields.
 */
export interface GpuiRemoteRepoForm {
  /** The endpoint's cutoff reason, while cut off. */
  cutoff: string | null;
  /** `RemoteRepositorySelector` shows. */
  selector: boolean;
  saved: { id: string; display_name: string; canonical_remote_path: string }[];
  selectedId: string | null;
  path: string;
  probe: { is_repo: boolean; needs_clone: boolean } | null;
  cloneUrl: string;
  confirmInit: boolean;
  busy: boolean;
  error: string | null;
}

/** A persistent remote session GPUI's terminal pane attaches to. */
export interface GpuiRemoteTerminalTarget {
  endpoint: SshEndpoint;
  repositoryId: string;
  workspaceId: string;
  remoteWorkingDirectory: string;
  label: string;
  launch?: PtyLaunchSpec;
  reattach: boolean;
}

/** Whether GPUI may draw a remote repository's workspace view. */
export function gpuiDrawsRemoteWorkspace(): boolean {
  return window.__TREQ_REACT_REMOTE_WORKSPACE__ !== true;
}

/**
 * Whether Rust's window model runs the remote flows, restoring the last
 * remote repository among them (src-tauri/src/gpui_shell/window/remote.rs):
 * in every GPUI window except a `TREQ_WEB_UI` one, and except in the tests
 * that run them in React.
 */
export function rustRunsRemote(): boolean {
  return (
    isGpuiShellHost() &&
    window.__TREQ_WEB_UI__ !== true &&
    window.__TREQ_REACT_REMOTE__ !== true
  );
}
