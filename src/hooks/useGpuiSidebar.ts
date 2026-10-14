import { useEffect, useRef } from "react";
import {
  buildGpuiSidebarSnapshot,
  type GpuiSidebarAction,
  type GpuiSidebarSnapshot,
  isGpuiShellHost,
  listenGpuiSidebarActions,
  updateGpuiSidebar,
} from "../lib/gpui-shell";
import type { Workspace } from "../lib/api";
import type { TerminalSessionSummary } from "../components/terminal/types";
import { useWorkspaceSidebarRows } from "./useWorkspaceSidebarRows";

interface UseGpuiSidebarParams {
  /** Single local repository open: no SSH, no supporting repos, past onboarding. */
  supported: boolean;
  repoPath: string | undefined;
  cacheKey: string | undefined;
  terminalSessions: TerminalSessionSummary[];
  selectedWorkspaceId: number | null;
  currentPage: string | undefined;
  homeBranch: string | null;
  linearEnabled: boolean;
  dark: boolean;
  width: number;
  onSelectWorkspace: (workspace: Workspace | null) => void;
  onOpenSettings: () => void;
  onOpenCommandPalette: () => void;
  onOpenGithub: () => void;
  onOpenLinear: () => void;
}

/**
 * Mirrors the workspace sidebar into the GPUI shell and routes its clicks
 * back. Returns whether GPUI draws the sidebar, so the caller can skip its own.
 */
export function useGpuiSidebar(params: UseGpuiSidebarParams): boolean {
  const active = isGpuiShellHost() && params.supported;
  const rows = useWorkspaceSidebarRows({
    repoPath: active ? params.repoPath : undefined,
    cacheKey: active ? params.cacheKey : undefined,
    terminalSessions: params.terminalSessions,
  });

  const snapshot: GpuiSidebarSnapshot | null = active
    ? buildGpuiSidebarSnapshot({
        repoPath: params.repoPath,
        nodes: rows.flattenedNodes,
        streamingByBranchWithAgentSession:
          rows.streamingByBranchWithAgentSession,
        prStatusesByBranch: rows.prStatusesByBranch,
        hasRemote: !!rows.remoteInfo,
        selectedWorkspaceId: params.selectedWorkspaceId,
        currentPage: params.currentPage,
        homeBranch: params.homeBranch,
        linearEnabled: params.linearEnabled,
        dark: params.dark,
        width: params.width,
        loading: rows.workspacesPending && rows.flattenedNodes.length === 0,
      })
    : null;

  const lastSent = useRef<string | null>(null);
  const snapshotJson = JSON.stringify(snapshot);
  useEffect(() => {
    if (!active || snapshotJson === lastSent.current) return;
    lastSent.current = snapshotJson;
    void updateGpuiSidebar(JSON.parse(snapshotJson));
  }, [active, snapshotJson]);

  // Hide the GPUI sidebar when the window goes away or the case stops being supported.
  useEffect(() => {
    if (!active) return;
    return () => {
      lastSent.current = null;
      void updateGpuiSidebar(null);
    };
  }, [active]);

  // Handlers change every render; the listener reads the latest through a ref.
  const latest = useRef({ params, workspaces: rows.workspaces });
  latest.current = { params, workspaces: rows.workspaces };
  useEffect(() => {
    if (!active) return;
    const handle = (action: GpuiSidebarAction) => {
      const { params: p, workspaces } = latest.current;
      switch (action.kind) {
        case "selectHome":
          p.onSelectWorkspace(null);
          break;
        case "selectWorkspace": {
          const workspace = workspaces.find((w) => w.id === action.id);
          if (workspace) p.onSelectWorkspace(workspace);
          break;
        }
        case "openSettings":
          p.onOpenSettings();
          break;
        case "openCommandPalette":
          p.onOpenCommandPalette();
          break;
        case "openGithub":
          p.onOpenGithub();
          break;
        case "openLinear":
          p.onOpenLinear();
          break;
        default:
          break;
      }
    };
    const unlisten = listenGpuiSidebarActions(handle);
    return () => {
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [active]);

  return active;
}
