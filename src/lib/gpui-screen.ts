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

export type GpuiSettingsTab =
  | "repository"
  | "application"
  | "account"
  | "integrations"
  | "skills"
  | "preview";

/**
 * GPUI loads and saves the settings itself
 * (src-tauri/src/gpui_shell/screen/settings.rs); React says which tab shows.
 */
interface GpuiSettingsScreen {
  kind: "settings";
  tab: GpuiSettingsTab;
  repoPath: string | null;
  currentBranch: string | null;
  /** The auth store, while the Account or Integrations tab shows. */
  account: GpuiAccountState | null;
}

/**
 * What the Account and Integrations tabs show of the Supabase session,
 * which only React holds (src-tauri/src/gpui_shell/screen/settings/account.rs).
 * GPUI hands back what needs it: sign-out, the token exchange, the cloud
 * workspace actions and Linear OAuth.
 */
export interface GpuiAccountState {
  loading: boolean;
  signingIn: boolean;
  signInError: string | null;
  /** Set while there is a session. */
  user: { name: string; email: string | null; avatarUrl: string | null } | null;
  subscription: {
    plan: string;
    status: string;
    currentPeriodEnd: string | null;
  } | null;
  devCallback: { loading: boolean; error: string | null };
  cloud: {
    instance: {
      status: string;
      providerResourceId: string | null;
      diskQuotaGb: number;
    } | null;
    endpoint: { username: string; hostname: string; port: number } | null;
    provisioningStage: string | null;
    provisioningError: string | null;
    usage: {
      repositoryCount: number;
      workspaceCount: number;
      diskUsedBytes: number;
    } | null;
    usageError: string | null;
    submitting: boolean;
  } | null;
  /** The account's GitHub repositories, while signed in on Integrations. */
  github: {
    loading: boolean;
    failed: boolean;
    repositories: { id: number; fullName: string; private: boolean }[];
  } | null;
  linearOauthLoading: boolean;
}

export type GpuiWorkspaceTab = "overview" | "commits" | "changes" | "logs";

export type GpuiPrState = "OPEN" | "CLOSED" | "MERGED";

/**
 * ShowWorkspace's header and tab row, which GPUI reads from the backend
 * itself (src-tauri/src/gpui_shell/screen/workspace_data.rs). React says
 * which workspace shows and the state of the flows it still runs, and GPUI
 * places the webview below, where React draws only the tab's content (its
 * "framed" layout).
 */
export interface GpuiWorkspaceScreen {
  kind: "workspace";
  repoPath: string;
  /** null for the home repository. */
  workspaceId: number | null;
  /** The home repository's checked-out branch. */
  homeBranch: string;
  tab: GpuiWorkspaceTab;
  /** false: React draws the tab row in the frame, as the Browser view's address bar lives there. */
  tabRow: boolean;
  /** The target branch is being changed. */
  retargeting: boolean;
  /** The Changes tab is refreshing its files. */
  refreshing: boolean;
  creatingPr: boolean;
  /** Workflow files started from this window that have not reported yet. */
  runningChecks: string[];
  mergeQueue: {
    label: string;
    secondary: boolean;
    disabled: boolean;
    tooltip: string;
  } | null;
  /** Bumped whenever React refreshes its caches, so GPUI reads again. */
  revision: number;
  /** GPUI draws the tab: Commits, or Code while React's file browser is closed. */
  code: boolean;
  /** The change the Commits tab should scroll to and open, until it says it did. */
  scrollToCommit: string | null;
  /** How tall the terminal pane GPUI frames below the Code tab is, once measured. */
  frameHeight: number | null;
  /** The Changes tab's "Show" toggle for committed files, null when not offered. */
  showCommitted: boolean | null;
  /** Where the Changes tab can move files (`useChangeFilesMove`). */
  moveTargets: { branch: string; label: string }[];
  /** Bumped when the workspace's files change or the window gains focus. */
  changesRevision: number;
  /** The tab row offers the Diff/Browser switcher (browser preview, Diff shown). */
  reviewSwitcher: boolean;
  /** The Logs tab's source while it shows (`LogSource`). */
  logsSource: string | null;
  /** Bumped by "open in Logs" links, so GPUI's Logs tab starts over on `logsSource`. */
  logsKey: number;
}

/**
 * MergePreviewPage, which GPUI reads and merges itself
 * (src-tauri/src/gpui_shell/screen/merge.rs). It presses
 * "mergePreview.cancel" to leave and "mergePreview.merged" once merged.
 */
interface GpuiMergePreviewScreen {
  kind: "mergePreview";
  repoPath: string;
  workspaceId: number;
}

export type GpuiScreenView =
  /** GPUI reads its data from the backend and opens repositories itself. */
  | { kind: "onboarding" }
  | GpuiSettingsScreen
  | GpuiWorkspaceScreen
  | GpuiMergePreviewScreen;

export type GpuiScreenSnapshot = GpuiScreenView & { dark: boolean };

export type GpuiScreenAction =
  | { kind: "openRepo" }
  | { kind: "openRemote" }
  | { kind: "recheckPrerequisites" }
  | { kind: "openUrl"; url: string }
  /** A control's new value; `field` names it, e.g. "app.theme". */
  | {
      kind: "field";
      field: string;
      value: string | number | boolean | Record<string, unknown>;
    }
  /** A slider released at `value`. */
  | { kind: "commit"; field: string; value: number }
  /** A button; `id` names it, e.g. "settings.save". */
  | { kind: "press"; id: string };

const GPUI_SCREEN_ACTION_EVENT = "gpui-screen-action";

/** Push a screen to GPUI; null shows the webview. Failures are logged, not thrown. */
async function updateGpuiScreen(
  snapshot: GpuiScreenSnapshot | null,
): Promise<void> {
  try {
    await gpuiScreenUpdate(snapshot);
  } catch (error) {
    console.error("gpui_screen_update failed:", error);
  }
}

/** Listen for GPUI screen clicks sent to this webview window only. */
function listenGpuiScreenActions(
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
): boolean {
  const host = isGpuiShellHost() && window.__TREQ_WEB_UI__ !== true;
  const dark = useThemeStore((s) => s.actualTheme()) === "dark";
  const modal = useWindowModalBackdrop(host) !== null;
  const json =
    host && view && !modal ? JSON.stringify({ ...view, dark }) : null;

  // Starts at null so a screen that never drew sends no null, which would
  // clear the screen another component pushed.
  const lastSent = useRef<string | null>(null);
  useEffect(() => {
    if (!host || json === lastSent.current) return;
    lastSent.current = json;
    void updateGpuiScreen(json === null ? null : JSON.parse(json));
  }, [host, json]);

  useEffect(() => {
    if (!host) return;
    return () => {
      if (lastSent.current === null) return;
      lastSent.current = null;
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
  return json !== null;
}
