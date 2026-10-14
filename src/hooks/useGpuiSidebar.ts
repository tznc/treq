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
import {
  homeMenuEntries,
  popupNativeMenu,
  workspaceMenuEntries,
} from "../lib/native-context-menu";
import { getWorkspaceTitle } from "../lib/workspace-utils";
import { useEditorAppsStore } from "../stores/editorAppsStore";
import type { RemoteCapabilities } from "../lib/remote-capabilities";
import type { TerminalSessionSummary } from "../components/terminal/types";
import { useWorkspaceSidebarRows } from "./useWorkspaceSidebarRows";

interface UseGpuiSidebarParams {
  /** Single local repository open: no SSH, no supporting repos, past onboarding. */
  supported: boolean;
  repoPath: string | undefined;
  cacheKey: string | undefined;
  terminalSessions: TerminalSessionSummary[];
  selectedWorkspaceId: number | null;
  selectedWorkspaceIds: ReadonlySet<number>;
  archivingWorkspaceIds: ReadonlySet<number>;
  exitingWorkspaceIds: ReadonlySet<number>;
  currentPage: string | undefined;
  homeRef: string | null;
  linearEnabled: boolean;
  dark: boolean;
  width: number;
  capabilities: RemoteCapabilities;
  onSelectWorkspace: (workspace: Workspace | null) => void;
  onSelectRepoHome: (repoPath: string) => void;
  onOpenSettings: () => void;
  onOpenCommandPalette: () => void;
  onOpenGithub: () => void;
  onOpenLinear: () => void;
  /** null starts an agent at the repository home. */
  onStartAgent: (workspaceId: number | null) => void;
  onOpenShell: (workspace: Workspace | null) => void;
  onStack: (workspace: Workspace | null) => void;
  onBulkArchive: () => void;
  onOpenBranchSwitcher: () => void;
  /** Adds a supporting repository; undefined drops the menu item. */
  onAddRepository?: () => void;
  onRenameWorkspace: (workspace: Workspace) => void;
  onArchiveWorkspace: (workspace: Workspace) => void;
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

  const snapshot: GpuiSidebarSnapshot | null =
    active && params.repoPath
      ? buildGpuiSidebarSnapshot({
          repoPath: params.repoPath,
          repos: [
            {
              path: params.repoPath,
              homeRef: params.homeRef,
              active: true,
              loading:
                rows.workspacesPending && rows.flattenedNodes.length === 0,
              nodes: rows.flattenedNodes,
              streamingByBranchWithAgentSession:
                rows.streamingByBranchWithAgentSession,
              prStatusesByBranch: rows.prStatusesByBranch,
              hasRemote: !!rows.remoteInfo,
              queueStatusesByBranch: rows.branchQueueStatuses,
            },
          ],
          selectedWorkspaceId: params.selectedWorkspaceId,
          selectedWorkspaceIds: params.selectedWorkspaceIds,
          archivingWorkspaceIds: params.archivingWorkspaceIds,
          exitingWorkspaceIds: params.exitingWorkspaceIds,
          currentPage: params.currentPage,
          linearEnabled: params.linearEnabled,
          dark: params.dark,
          width: params.width,
          capabilities: params.capabilities,
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
  const latest = useRef({
    params,
    workspaces: rows.workspaces,
    prStatusesByBranch: rows.prStatusesByBranch,
  });
  latest.current = {
    params,
    workspaces: rows.workspaces,
    prStatusesByBranch: rows.prStatusesByBranch,
  };
  useEffect(() => {
    if (!active) return;
    const handle = (action: GpuiSidebarAction) => {
      const { params: p, workspaces, prStatusesByBranch } = latest.current;
      const find = (id: number | null) =>
        id === null ? null : (workspaces.find((w) => w.id === id) ?? null);
      switch (action.kind) {
        case "selectHome":
          if (action.repoPath === p.repoPath) p.onSelectWorkspace(null);
          else p.onSelectRepoHome(action.repoPath);
          break;
        case "selectWorkspace": {
          const workspace = find(action.id);
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
        case "startAgent":
          if (action.id === null || find(action.id)) p.onStartAgent(action.id);
          break;
        case "openShell": {
          const workspace = find(action.id);
          if (action.id === null || workspace) p.onOpenShell(workspace);
          break;
        }
        case "stack": {
          const workspace = find(action.id);
          if (action.id === null || workspace) p.onStack(workspace);
          break;
        }
        case "bulkArchive":
          p.onBulkArchive();
          break;
        case "homeMenu":
          void popupNativeMenu(
            homeMenuEntries({
              repoPath: action.repoPath,
              editorApps: useEditorAppsStore.getState(),
              onOpenBranchSwitcher: p.onOpenBranchSwitcher,
              onAddRepository: p.onAddRepository,
            }),
          );
          break;
        case "workspaceMenu": {
          const workspace = find(action.id);
          if (!workspace) break;
          void popupNativeMenu(
            workspaceMenuEntries({
              workspace,
              title: getWorkspaceTitle(workspace),
              repoPath: p.repoPath,
              prUrl: prStatusesByBranch[workspace.branch_name]?.url ?? null,
              editorApps: useEditorAppsStore.getState(),
              onRename: () => p.onRenameWorkspace(workspace),
              onArchive: () => p.onArchiveWorkspace(workspace),
            }),
          );
          break;
        }
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
