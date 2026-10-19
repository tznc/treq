import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { gpuiSidebarUpdate } from "./api";
import type { PrInfo, QueueEntryStatus } from "./api-types";
import type { RemoteCapabilities } from "./remote-capabilities";
import { getWorkspaceStatusIndicator } from "./workspace-status-indicator";
import { getWorkspaceTitle } from "./workspace-utils";
import type { FlattenedWorkspaceNode } from "./workspace-tree";

/**
 * On desktop the window is drawn by GPUI and this app runs in a child webview. The Rust side marks that webview with this global.
 */
declare global {
  interface Window {
    __TREQ_GPUI_SHELL__?: boolean;
    /** `TREQ_WEB_UI` is set: React draws every screen, for comparison. */
    __TREQ_WEB_UI__?: boolean;
    /** React draws the terminal pane; tests of that pane set it. */
    __TREQ_REACT_TERMINAL__?: boolean;
    /** React draws Dashboard's workspace dialogs; tests of them set it. */
    __TREQ_REACT_WORKSPACE_DIALOGS__?: boolean;
  }
}

export function isGpuiShellHost(): boolean {
  return typeof window !== "undefined" && window.__TREQ_GPUI_SHELL__ === true;
}

interface GpuiSidebarRow {
  id: number;
  title: string;
  depth: number;
  selected: boolean;
  conflicted: boolean;
  status: { color: "red" | "yellow" | "neutral"; spin: boolean } | null;
  pr: "draft" | "open" | "merged" | "closed" | null;
  /** The merge queue entry's status, when the merge queue is on. */
  queue: QueueEntryStatus | null;
  archiving: boolean;
  exiting: boolean;
}

export interface GpuiSidebarRepo {
  path: string;
  /** Checked-out branch or detached short hash; null until it loads. */
  homeRef: string | null;
  homeSelected: boolean;
  /** The repository on screen; only it gets workspace actions. */
  active: boolean;
  loading: boolean;
  workspaces: GpuiSidebarRow[];
  /** The repository's name in a window with several; null with one. */
  label: string | null;
  /** A supporting repository whose folder is gone. */
  missing: boolean;
  /** Its workspace group is folded away. */
  collapsed: boolean;
}

export interface GpuiSidebarSnapshot {
  repoName: string;
  settingsOpen: boolean;
  /** The GitHub nav row; true while its page is open. */
  github: boolean;
  /** The Linear nav row, or null when the Linear preview is off. */
  linear: boolean | null;
  dark: boolean;
  width: number;
  /** Why "Start agent" is unavailable, or null when it works. */
  agentUnavailable: string | null;
  /** Why "Open shell" is unavailable, or null when it works. */
  shellUnavailable: string | null;
  /** The "Archive N workspaces" button's count, or null to hide it. */
  bulkArchive: number | null;
  repos: GpuiSidebarRepo[];
  /** An open modal's backdrop opacity, which GPUI draws over the sidebar; null with none. */
  modal: number | null;
}

export type GpuiSidebarAction =
  | { kind: "selectHome"; repoPath: string }
  | { kind: "selectWorkspace"; id: number; shift: boolean; toggle: boolean }
  | { kind: "selectStack"; id: number; descendants: boolean }
  | { kind: "clearSelection" }
  | { kind: "openSettings" }
  | { kind: "openCommandPalette" }
  | { kind: "openGithub" }
  | { kind: "openLinear" }
  | { kind: "startAgent"; id: number | null }
  | { kind: "openShell"; id: number | null }
  | { kind: "stack"; id: number | null }
  | { kind: "bulkArchive" }
  | { kind: "homeMenu"; repoPath: string }
  | { kind: "workspaceMenu"; id: number }
  | { kind: "dismissModal" }
  | { kind: "moveWorkspace"; id: number; onto: number | null }
  | { kind: "resizeSidebar"; width: number }
  | { kind: "resetSidebarWidth" }
  | { kind: "openRepoWorkspace"; repoPath: string; id: number }
  | { kind: "repoWorkspaceMenu"; repoPath: string; id: number }
  | { kind: "toggleRepo"; repoPath: string };

const GPUI_SIDEBAR_ACTION_EVENT = "gpui-sidebar-action";

function prState(prInfo: PrInfo): GpuiSidebarRow["pr"] {
  const state = prInfo.state.toUpperCase();
  if (prInfo.is_draft && state === "OPEN") return "draft";
  if (state === "OPEN") return "open";
  if (state === "MERGED") return "merged";
  return "closed";
}

/** One repository's rows, from the same data its React list draws. */
export interface GpuiSidebarRepoInput {
  path: string;
  homeRef: string | null;
  active: boolean;
  loading: boolean;
  nodes: FlattenedWorkspaceNode[];
  /** Branches with an open agent terminal, mapped to "is streaming". */
  streamingByBranchWithAgentSession: ReadonlyMap<string, boolean>;
  prStatusesByBranch: Record<string, PrInfo | null>;
  hasRemote: boolean;
  queueStatusesByBranch?: ReadonlyMap<string, QueueEntryStatus>;
  label?: string | null;
  missing?: boolean;
  collapsed?: boolean;
}

export function buildGpuiSidebarSnapshot(params: {
  /** The window's main repository path, which names the palette button. */
  repoPath: string | undefined;
  repos: GpuiSidebarRepoInput[];
  selectedWorkspaceId: number | null;
  /** Multi-selected workspaces of the active repository. */
  selectedWorkspaceIds?: ReadonlySet<number>;
  archivingWorkspaceIds?: ReadonlySet<number>;
  exitingWorkspaceIds?: ReadonlySet<number>;
  /** Same values Dashboard passes as `currentPage`. */
  currentPage: string | undefined;
  linearEnabled: boolean;
  dark: boolean;
  width: number;
  capabilities?: RemoteCapabilities;
  modal?: number | null;
}): GpuiSidebarSnapshot {
  const {
    repoPath,
    repos,
    selectedWorkspaceId,
    selectedWorkspaceIds = new Set<number>(),
    archivingWorkspaceIds = new Set<number>(),
    exitingWorkspaceIds = new Set<number>(),
    currentPage,
    linearEnabled,
    dark,
    width,
    capabilities,
    modal = null,
  } = params;
  const workspaceSelectionActive =
    currentPage !== "github" &&
    currentPage !== "linear" &&
    currentPage !== "settings";
  const busy = archivingWorkspaceIds.size > 0 || exitingWorkspaceIds.size > 0;
  return {
    repoName: repoPath
      ? repoPath.split("/").filter(Boolean).pop() || "Repository"
      : "Repository",
    settingsOpen: currentPage === "settings",
    github: currentPage === "github",
    linear: linearEnabled ? currentPage === "linear" : null,
    dark,
    width,
    agentUnavailable:
      capabilities && !capabilities.agentPty.supported
        ? (capabilities.agentPty.reason ?? "Unavailable")
        : null,
    shellUnavailable:
      capabilities && !capabilities.shell.supported
        ? (capabilities.shell.reason ?? "Unavailable")
        : null,
    bulkArchive:
      selectedWorkspaceIds.size > 0 && !busy ? selectedWorkspaceIds.size : null,
    modal,
    repos: repos.map((repo) =>
      buildGpuiRepoSection(repo, {
        selectedWorkspaceId,
        selectedWorkspaceIds,
        archivingWorkspaceIds,
        exitingWorkspaceIds,
        workspaceSelectionActive,
      }),
    ),
  };
}

/** Selection state that only the repository on screen shows. */
interface GpuiSelection {
  selectedWorkspaceId: number | null;
  selectedWorkspaceIds: ReadonlySet<number>;
  archivingWorkspaceIds: ReadonlySet<number>;
  exitingWorkspaceIds: ReadonlySet<number>;
  /** False while GitHub, Linear or Settings is open. */
  workspaceSelectionActive: boolean;
}

/** No selection, for a repository that is not on screen. */
export const NO_GPUI_SELECTION: GpuiSelection = {
  selectedWorkspaceId: null,
  selectedWorkspaceIds: new Set(),
  archivingWorkspaceIds: new Set(),
  exitingWorkspaceIds: new Set(),
  workspaceSelectionActive: false,
};

/** One repository's home row and workspace rows. */
export function buildGpuiRepoSection(
  repo: GpuiSidebarRepoInput,
  selection: GpuiSelection,
): GpuiSidebarRepo {
  const {
    selectedWorkspaceId,
    selectedWorkspaceIds,
    archivingWorkspaceIds,
    exitingWorkspaceIds,
    workspaceSelectionActive,
  } = selection;
  return {
    path: repo.path,
    homeRef: repo.homeRef,
    homeSelected:
      workspaceSelectionActive && repo.active && selectedWorkspaceId === null,
    active: repo.active,
    loading: repo.loading,
    label: repo.label ?? null,
    missing: repo.missing ?? false,
    collapsed: repo.collapsed ?? false,
    workspaces: repo.nodes.map((node) => {
      const workspace = node.status.current;
      const branch = workspace.branch_name;
      const hasAgentSession =
        repo.streamingByBranchWithAgentSession.has(branch);
      const indicator = getWorkspaceStatusIndicator({
        isConflicted: node.status.has_conflicts,
        hasChanges: node.status.has_changes ?? false,
        isAgentSessionStreaming:
          hasAgentSession &&
          (repo.streamingByBranchWithAgentSession.get(branch) ?? false),
      });
      const prInfo = repo.prStatusesByBranch[branch] ?? null;
      const selected =
        repo.active &&
        (selectedWorkspaceIds.has(workspace.id) ||
          (workspaceSelectionActive && selectedWorkspaceId === workspace.id));
      return {
        id: workspace.id,
        title: getWorkspaceTitle(workspace),
        depth: node.depth,
        selected,
        conflicted: node.status.has_conflicts,
        status: indicator
          ? { color: indicator.color, spin: indicator.spin }
          : null,
        pr: repo.hasRemote && prInfo ? prState(prInfo) : null,
        queue: repo.queueStatusesByBranch?.get(branch) ?? null,
        archiving: repo.active && archivingWorkspaceIds.has(workspace.id),
        exiting: repo.active && exitingWorkspaceIds.has(workspace.id),
      };
    }),
  };
}

/** Push the sidebar to GPUI; null hides it. Failures are logged, not thrown. */
export async function updateGpuiSidebar(
  snapshot: GpuiSidebarSnapshot | null,
): Promise<void> {
  try {
    await gpuiSidebarUpdate(snapshot);
  } catch (error) {
    console.error("gpui_sidebar_update failed:", error);
  }
}

/** Listen for GPUI sidebar clicks sent to this webview window only. */
export function listenGpuiSidebarActions(
  handler: (action: GpuiSidebarAction) => void,
): Promise<() => void> {
  return getCurrentWebviewWindow().listen<GpuiSidebarAction>(
    GPUI_SIDEBAR_ACTION_EVENT,
    (event) => handler(event.payload),
  );
}
