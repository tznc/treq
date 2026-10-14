import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { gpuiSidebarUpdate } from "./api";
import type { PrInfo } from "./api-types";
import { getWorkspaceStatusIndicator } from "./workspace-status-indicator";
import { getWorkspaceTitle } from "./workspace-utils";
import type { FlattenedWorkspaceNode } from "./workspace-tree";

/**
 * With the `gpuiSidebar` preview on, the window is drawn by GPUI and this app
 * runs in a child webview. The Rust side marks that webview with this global.
 */
declare global {
  interface Window {
    __TREQ_GPUI_SHELL__?: boolean;
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
  status: { color: "red" | "yellow" | "neutral"; spin: boolean } | null;
  pr: "draft" | "open" | "merged" | "closed" | null;
}

export interface GpuiSidebarSnapshot {
  repoName: string;
  homeBranch: string | null;
  homeSelected: boolean;
  settingsOpen: boolean;
  /** The GitHub nav row; true while its page is open. */
  github: boolean;
  /** The Linear nav row, or null when the Linear preview is off. */
  linear: boolean | null;
  dark: boolean;
  width: number;
  loading: boolean;
  workspaces: GpuiSidebarRow[];
}

export type GpuiSidebarAction =
  | { kind: "selectHome" }
  | { kind: "selectWorkspace"; id: number }
  | { kind: "openSettings" }
  | { kind: "openCommandPalette" }
  | { kind: "openGithub" }
  | { kind: "openLinear" };

const GPUI_SIDEBAR_ACTION_EVENT = "gpui-sidebar-action";

function prState(prInfo: PrInfo): GpuiSidebarRow["pr"] {
  const state = prInfo.state.toUpperCase();
  if (prInfo.is_draft && state === "OPEN") return "draft";
  if (state === "OPEN") return "open";
  if (state === "MERGED") return "merged";
  return "closed";
}

export function buildGpuiSidebarSnapshot(params: {
  /** The window's main repository path. */
  repoPath: string | undefined;
  nodes: FlattenedWorkspaceNode[];
  /** Branches with an open agent terminal, mapped to "is streaming". */
  streamingByBranchWithAgentSession: ReadonlyMap<string, boolean>;
  prStatusesByBranch: Record<string, PrInfo | null>;
  hasRemote: boolean;
  selectedWorkspaceId: number | null;
  /** Same values Dashboard passes to WorkspaceSidebar's `currentPage`. */
  currentPage: string | undefined;
  homeBranch: string | null;
  linearEnabled: boolean;
  dark: boolean;
  width: number;
  loading: boolean;
}): GpuiSidebarSnapshot {
  const {
    repoPath,
    nodes,
    streamingByBranchWithAgentSession,
    prStatusesByBranch,
    hasRemote,
    selectedWorkspaceId,
    currentPage,
    homeBranch,
    linearEnabled,
    dark,
    width,
    loading,
  } = params;
  const workspaceSelectionActive =
    currentPage !== "github" &&
    currentPage !== "linear" &&
    currentPage !== "settings";
  return {
    repoName: repoPath
      ? repoPath.split("/").filter(Boolean).pop() || "Repository"
      : "Repository",
    homeBranch,
    homeSelected: workspaceSelectionActive && selectedWorkspaceId === null,
    settingsOpen: currentPage === "settings",
    github: currentPage === "github",
    linear: linearEnabled ? currentPage === "linear" : null,
    dark,
    width,
    loading,
    workspaces: nodes.map((node) => {
      const workspace = node.status.current;
      const branch = workspace.branch_name;
      const hasAgentSession = streamingByBranchWithAgentSession.has(branch);
      const indicator = getWorkspaceStatusIndicator({
        isConflicted: node.status.has_conflicts,
        hasChanges: node.status.has_changes ?? false,
        isAgentSessionStreaming:
          hasAgentSession &&
          (streamingByBranchWithAgentSession.get(branch) ?? false),
      });
      const prInfo = prStatusesByBranch[branch] ?? null;
      return {
        id: workspace.id,
        title: getWorkspaceTitle(workspace),
        depth: node.depth,
        selected:
          workspaceSelectionActive && selectedWorkspaceId === workspace.id,
        status: indicator
          ? { color: indicator.color, spin: indicator.spin }
          : null,
        pr: hasRemote && prInfo ? prState(prInfo) : null,
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
