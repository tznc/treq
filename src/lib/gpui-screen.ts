import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useRef, useSyncExternalStore } from "react";
import { create } from "zustand";
import { useThemeStore } from "../stores/themeStore";
import { gpuiModalUpdate, gpuiScreenUpdate } from "./api";
import type { SshEndpoint } from "./api-types-remote";
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
  /** GPUI draws the terminal pane below the tab; null while React does (remote). */
  terminal: GpuiTerminalRoute | null;
}

/** A Dashboard request of the terminal pane (its imperative handle). */
export interface GpuiTerminalCommand {
  /** Increases per command, so GPUI runs each once. */
  nonce: number;
  kind:
    | "toggleCollapse"
    | "toggleMaximize"
    | "newShell"
    | "newAgent"
    | "closeWorkspace"
    | "send";
  dir?: string | null;
  sessionId?: number | null;
  text?: string | null;
}

/** What GPUI's terminal pane (src-tauri/src/gpui_shell/screen/terminal.rs) needs from React. */
export interface GpuiTerminalRoute {
  /** The active agent session's pane id, while the workspace view shows. */
  activeSessionId: number | null;
  /** New sessions' starts (`pendingSessionData`), by pane id. */
  pending: {
    sessionId: number;
    pendingPrompt?: string | null;
    permissionMode?: string | null;
    agent?: string | null;
    workspacePath?: string | null;
  }[];
  /** Changes with the session list, so GPUI reads it again. */
  sessionsRevision: number;
  commands: GpuiTerminalCommand[];
  /**
   * `treq send` attachments (`treqSendStore`), which GPUI draws over the
   * terminals; it sends "terminal.dismissAsset" with an id to drop one.
   */
  sendAssets: {
    id: string;
    repo: string;
    ptySessionId: string | null;
    mediaType: string;
    path: string;
    title: string;
  }[];
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

/** `CloudWorkspaceCard`'s state, which GPUI draws in the settings page and the remote setup dialog. */
export type GpuiCloudState = NonNullable<GpuiAccountState["cloud"]>;

/**
 * The remote repository dialogs Dashboard opens over any screen, which GPUI
 * draws (src-tauri/src/gpui_shell/screen/remote.rs). Each is null while
 * closed. GPUI presses "remote.*" and sets "remote.registerUserManaged" and
 * "remote.terminal.open" (useGpuiRemoteDialogs).
 */
export interface GpuiRemoteRoute {
  /** RemoteSetupDialog. */
  setup: {
    allowUserManaged: boolean;
    cloud: GpuiCloudState;
    /** React is registering the endpoint the form confirmed. */
    registering: boolean;
  } | null;
  /** RemoteTerminalDialog. */
  terminal: {
    endpoint: SshEndpoint;
    repositoryId: string;
    workspaceId: string;
  } | null;
  /** RemoteAmbiguousMutationDialog: why the change could not be verified. */
  ambiguous: string | null;
}

interface GpuiRemoteStore {
  /** What the remote dialogs show, null with none open. */
  route: GpuiRemoteRoute | null;
  /** How many mounted screens GPUI draws now. */
  drawing: number;
}

/** Dashboard's remote dialogs, and whether a GPUI screen is there to draw them. */
export const useGpuiRemoteStore = create<GpuiRemoteStore>(() => ({
  route: null,
  drawing: 0,
}));

/** Whether GPUI may draw the remote dialogs. */
export function gpuiDrawsRemoteDialogs(): boolean {
  return (
    isGpuiShellHost() &&
    window.__TREQ_WEB_UI__ !== true &&
    window.__TREQ_REACT_REMOTE_DIALOGS__ !== true
  );
}

/**
 * Whether GPUI draws the remote dialogs now: it does over any screen it
 * draws, and React draws them over its own views.
 */
export function useGpuiDrawsRemoteDialogs(): boolean {
  const drawing = useGpuiRemoteStore((s) => s.drawing > 0);
  return gpuiDrawsRemoteDialogs() && drawing;
}
/** A workspace as the GPUI dialogs need it. */
export interface GpuiDialogWorkspace {
  id: number;
  branchName: string;
  workspacePath: string;
}

/**
 * Dashboard's dialogs that GPUI draws over whichever screen shows
 * (src-tauri/src/gpui_shell/screen/modal.rs). React keeps which one is
 * open; GPUI does the work through the backend and hands back closing,
 * the workspace it made, and the workspace to delete.
 */
export type GpuiWorkspaceModal =
  /** `UnifiedWorkspaceDialog`; `key` changes with each open. */
  | {
      kind: "workspaceDialog";
      repoPath: string;
      key: number;
      defaults: {
        targetBranch?: string;
        source:
          | { kind: "create" }
          | { kind: "home" }
          | {
              kind: "workspace";
              id: number;
              branchName: string;
              targetBranch: string | null;
            };
        preSelectedCommits?: string[];
        preSelectedFiles?: string[];
        description?: string;
        title?: string;
        branchName?: string;
        activeTab?: "commits" | "changes";
        applyStashId?: number;
        applyStashCommit?: { hash: string; message: string; timestamp: string };
      };
    }
  /** `RenameWorkspaceDialog`. */
  | {
      kind: "renameWorkspace";
      repoPath: string;
      workspaceId: number;
      branchName: string;
    }
  /** The command palette's `WorkspaceDeletion`. */
  | {
      kind: "workspaceDeletion";
      repoPath: string;
      workspaces: GpuiDialogWorkspace[];
      currentWorkspace: GpuiDialogWorkspace | null;
    }
  /** The `AlertDialog` confirming a move of dropped files. */
  | {
      kind: "confirmChangeMove";
      repoPath: string;
      files: string[];
      sourceBranch: string;
      destinationBranch: string;
      destinationLabel: string;
    };

/** The open GPUI dialog, and how many mounted screens GPUI is asked to draw. */
const useGpuiWorkspaceModalStore = create<{
  modal: GpuiWorkspaceModal | null;
  screens: number;
}>(() => ({ modal: null, screens: 0 }));
export type GpuiScreenSnapshot = GpuiScreenView & {
  dark: boolean;
  /** The remote dialogs open over the screen; absent with none. */
  remote?: GpuiRemoteRoute;
  /** The workspace dialog GPUI draws over the screen, while one is open. */
  modal?: GpuiWorkspaceModal;
};

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

/**
 * Dialogs GPUI draws over its screen (src-tauri/src/gpui_shell/screen/modal.rs).
 * React still owns whether one is open and what it starts with; GPUI reads
 * the prompt history and stashes itself, and hands back what only React can
 * do as `press` and `field` actions named after the dialog.
 */
interface GpuiAgentPromptModal {
  kind: "agentPrompt";
  repoPath: string;
  defaultBranch: string;
  workspaces: { id: number; branchName: string; path: string }[];
  initialPrompt: string | null;
  initialWorkspaceId: number | null;
  /** While attached, the session runs in the issue's own workspace. */
  issue: {
    source: "github" | "linear";
    key: string;
    title: string;
    includeSubItems: boolean;
  } | null;
  /** React is starting the session. */
  starting: boolean;
}

interface GpuiPromptHistoryModal {
  kind: "promptHistory";
  repoPath: string;
  initialSelectedId: number | null;
}

interface GpuiStashModal {
  kind: "stash";
  repoPath: string;
  /** Where a stash applies to besides the home repo, most active first. */
  workspaces: { branch: string; label: string }[];
}

export type GpuiModal =
  | GpuiAgentPromptModal
  | GpuiPromptHistoryModal
  | GpuiStashModal;

/** What the agent prompt dialog hands React when "Edit" or "Plan" is pressed. */
export interface GpuiAgentPromptStart {
  prompt: string;
  agent: string;
  mode: "plan" | "acceptEdits";
  workspaceId: number | null;
  saveAsRepoDefault: boolean;
  /** The attached issue is still attached. */
  issue: boolean;
}

const GPUI_SCREEN_ACTION_EVENT = "gpui-screen-action";

/** The mounted `useGpuiScreen`s that draw a screen now. */
const drawnScreens = new Set<object>();
const drawnScreenListeners = new Set<() => void>();

function setScreenDrawn(token: object, drawn: boolean): void {
  if (drawn === drawnScreens.has(token)) return;
  if (drawn) drawnScreens.add(token);
  else drawnScreens.delete(token);
  drawnScreenListeners.forEach((listener) => listener());
}

function subscribeDrawnScreens(listener: () => void): () => void {
  drawnScreenListeners.add(listener);
  return () => void drawnScreenListeners.delete(listener);
}

/** Pushes a dialog to GPUI; null closes it. Failures are logged, not thrown. */
async function updateGpuiModal(modal: GpuiModal | null): Promise<void> {
  try {
    await gpuiModalUpdate(modal);
  } catch (error) {
    console.error("gpui_modal_update failed:", error);
  }
}

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
): boolean {
  const host = isGpuiShellHost() && window.__TREQ_WEB_UI__ !== true;
  const dark = useThemeStore((s) => s.actualTheme()) === "dark";
  const domModal = useWindowModalBackdrop(host) !== null;
  const remote = useGpuiRemoteStore((s) => s.route);
  const modal = useGpuiWorkspaceModalStore((s) => s.modal);
  // A GPUI dialog covers the window, so a webview modal closing under it
  // (the command palette opening Delete Workspace) does not drop the screen.
  const json =
    host && view && (!domModal || modal)
      ? JSON.stringify({
          ...view,
          dark,
          ...(remote ? { remote } : {}),
          ...(modal ? { modal } : {}),
        })
      : null;

  // Tells Dashboard a screen is there for GPUI to draw its dialogs over.
  const drawing = json !== null;
  useEffect(() => {
    if (!drawing) return;
    useGpuiRemoteStore.setState((s) => ({ drawing: s.drawing + 1 }));
    return () =>
      useGpuiRemoteStore.setState((s) => ({ drawing: s.drawing - 1 }));
  }, [drawing]);

  const wants = host && view !== null;
  useEffect(() => {
    if (!wants) return;
    useGpuiWorkspaceModalStore.setState((s) => ({ screens: s.screens + 1 }));
    return () =>
      useGpuiWorkspaceModalStore.setState((s) => ({ screens: s.screens - 1 }));
  }, [wants]);

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

  // Lets dialogs know GPUI has a screen to draw them over.
  const token = useRef({});
  const drawn = json !== null;
  useEffect(() => {
    const { current } = token;
    setScreenDrawn(current, drawn);
    return () => setScreenDrawn(current, false);
  }, [drawn]);
  return drawn;
}

/** Whether GPUI is drawing a screen, which dialogs can then open over. */
function useGpuiScreenDrawn(): boolean {
  return useSyncExternalStore(
    subscribeDrawnScreens,
    () => drawnScreens.size > 0,
    () => false,
  );
}

/**
 * Whether GPUI draws the agent prompt, prompt history and stash dialogs:
 * inside the shell, over a GPUI screen (the webview cannot be covered, so
 * React's own dialog stays where React draws the window). React skips its
 * dialog while this is true.
 */
export function useGpuiModalsAvailable(): boolean {
  const drawn = useGpuiScreenDrawn();
  return (
    isGpuiShellHost() &&
    window.__TREQ_WEB_UI__ !== true &&
    window.__TREQ_REACT_MODALS__ !== true &&
    drawn
  );
}

/**
 * Draws `modal` in GPUI while the caller is mounted (null closes it), and
 * runs `onAction` for its `press` and `field` actions.
 */
export function useGpuiModal(
  modal: GpuiModal | null,
  onAction: (action: GpuiScreenAction) => void,
): void {
  const host = isGpuiShellHost();
  const json = host && modal ? JSON.stringify(modal) : null;
  const lastSent = useRef<string | null>(null);
  useEffect(() => {
    if (!host || json === lastSent.current) return;
    lastSent.current = json;
    void updateGpuiModal(json === null ? null : JSON.parse(json));
  }, [host, json]);
  useEffect(() => {
    if (!host) return;
    return () => {
      if (lastSent.current === null) return;
      lastSent.current = null;
      void updateGpuiModal(null);
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

/**
 * Opens `modal` over the GPUI screen while it is non-null and GPUI is
 * drawing a screen, and runs `onAction` for the screen's clicks. Returns
 * whether GPUI draws Dashboard's dialogs now; React draws its own when not
 * (no GPUI screen shows, or the integration tests' React dialogs).
 */
export function useGpuiWorkspaceModal(
  enabled: boolean,
  modal: GpuiWorkspaceModal | null,
  onAction: (action: GpuiScreenAction) => void,
): boolean {
  const host =
    enabled &&
    isGpuiShellHost() &&
    window.__TREQ_WEB_UI__ !== true &&
    window.__TREQ_REACT_WORKSPACE_DIALOGS__ !== true;
  const screens = useGpuiWorkspaceModalStore((s) => s.screens);
  const draws = host && screens > 0;
  const json = draws && modal ? JSON.stringify(modal) : null;

  useEffect(() => {
    useGpuiWorkspaceModalStore.setState({
      modal: json === null ? null : (JSON.parse(json) as GpuiWorkspaceModal),
    });
  }, [json]);
  useEffect(
    () => () => useGpuiWorkspaceModalStore.setState({ modal: null }),
    [],
  );

  const latest = useRef(onAction);
  latest.current = onAction;
  useEffect(() => {
    if (!draws) return;
    const unlisten = listenGpuiScreenActions((action) =>
      latest.current(action),
    );
    return () => {
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [draws]);
  return draws;
}

/** Whether GPUI may draw the terminal pane. */
export function gpuiDrawsTerminalPane(): boolean {
  return window.__TREQ_REACT_TERMINAL__ !== true;
}
