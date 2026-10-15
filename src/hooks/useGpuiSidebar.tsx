import * as React from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  buildGpuiSidebarSnapshot,
  type GpuiSidebarAction,
  type GpuiSidebarRepo,
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
import { getDescendants, getEntireStack } from "../lib/workspace-tree";
import { idsInVisibleRange } from "./useWorkspaceSidebarMultiSelect";
import { useEditorAppsStore } from "../stores/editorAppsStore";
import {
  dismissTopWindowModal,
  setWindowOffsetLeft,
  useWindowModalBackdrop,
} from "../lib/window-modals";
import type { RemoteCapabilities } from "../lib/remote-capabilities";
import type { TerminalSessionSummary } from "../components/terminal/types";
import { useWorkspaceSidebarRows } from "./useWorkspaceSidebarRows";
import {
  GpuiRepoRowsSource,
  type GpuiRepoLookup,
} from "../components/GpuiRepoRowsSource";
import {
  readCollapsedRepos,
  writeCollapsedRepos,
} from "../lib/collapsed-repos";
import { repoDisplayLabels } from "../lib/repo-labels";

interface UseGpuiSidebarParams {
  /** A repository is open in this window. */
  supported: boolean;
  /** The repository on screen. */
  repoPath: string | undefined;
  /** The window's main repository; undefined for a remote one. */
  mainRepoPath: string | undefined;
  /** Repositories linked to the main one; empty for a single repository. */
  supportingRepos: { path: string; exists: boolean }[];
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
  /** A workspace click with its modifiers; null clears the selection. */
  onWorkspaceMultiSelect: (
    workspace: Workspace | null,
    modifiers: { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean },
  ) => void;
  onSelectStack: (workspaceIds: Set<number>) => void;
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
  onRenameWorkspace: (workspace: Workspace, repoPath: string) => void;
  onOpenRepoSettings: (repoPath: string) => void;
  onLocateRepo: (repoPath: string) => void;
  onRemoveRepo: (repoPath: string) => void;
  onArchiveWorkspace: (workspace: Workspace) => void;
  /** Stacks `workspace` on `targetBranch`, or on the home branch for null. */
  onMoveWorkspace: (workspace: Workspace, targetBranch: string | null) => void;
  /** The resize handle was let go at `width`. */
  onResizeSidebar: (width: number) => void;
  onResetSidebarWidth: () => void;
}

/**
 * Mirrors the workspace sidebar into the GPUI shell and routes its clicks
 * back. Returns whether GPUI draws the sidebar, so the caller can skip its
 * own, and `sources`, which the caller renders to load the rows of
 * repositories that are not on screen.
 */
export function useGpuiSidebar(params: UseGpuiSidebarParams): {
  active: boolean;
  sources: React.ReactNode;
} {
  const active = isGpuiShellHost() && params.supported;
  const { mainRepoPath, supportingRepos, terminalSessions } = params;
  const multi = Boolean(mainRepoPath) && supportingRepos.length > 0;
  const order = multi
    ? [mainRepoPath ?? "", ...supportingRepos.map((repo) => repo.path)]
    : [params.repoPath ?? ""];
  const labels = multi ? repoDisplayLabels(order) : null;
  const missingRepos = new Set(
    supportingRepos.filter((repo) => !repo.exists).map((repo) => repo.path),
  );
  // Agents carry their repository; shells belong to the one on screen.
  const sessionsFor = (path: string) =>
    multi
      ? terminalSessions.filter((session) =>
          session.repoPath
            ? session.repoPath === path
            : path === params.repoPath,
        )
      : terminalSessions;

  const rows = useWorkspaceSidebarRows({
    repoPath: active ? params.repoPath : undefined,
    cacheKey: active ? params.cacheKey : undefined,
    terminalSessions: sessionsFor(params.repoPath ?? ""),
  });

  // Folded groups, stored per main repository as the React sidebar does.
  const [collapsedState, setCollapsedState] = useState(() => ({
    mainRepoPath,
    repos: readCollapsedRepos(mainRepoPath),
  }));
  const collapsedRepos =
    collapsedState.mainRepoPath === mainRepoPath
      ? collapsedState.repos
      : readCollapsedRepos(mainRepoPath);

  // Repositories off screen report their sections and rows from `sources`.
  const [sections, setSections] = useState<Record<string, string>>({});
  const onSection = useCallback((path: string, section: string | null) => {
    setSections((prev) => {
      if (section === null) {
        if (!(path in prev)) return prev;
        const rest = { ...prev };
        delete rest[path];
        return rest;
      }
      return prev[path] === section ? prev : { ...prev, [path]: section };
    });
  }, []);
  const lookupsRef = useRef(new Map<string, GpuiRepoLookup>());
  const onLookup = useCallback(
    (path: string, lookup: GpuiRepoLookup | null) => {
      if (lookup) lookupsRef.current.set(path, lookup);
      else lookupsRef.current.delete(path);
    },
    [],
  );
  const offScreen = active
    ? order.filter(
        (path) => path !== params.repoPath && !missingRepos.has(path),
      )
    : [];
  const sources = offScreen.map((path) => (
    <GpuiRepoRowsSource
      key={path}
      path={path}
      terminalSessions={sessionsFor(path)}
      onSection={onSection}
      onLookup={onLookup}
    />
  ));

  const modal = useWindowModalBackdrop(active);
  const built: GpuiSidebarSnapshot | null =
    active && params.repoPath
      ? buildGpuiSidebarSnapshot({
          repoPath: mainRepoPath || params.repoPath,
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
          modal,
        })
      : null;
  const snapshot: GpuiSidebarSnapshot | null = built && {
    ...built,
    repos: order.map((path): GpuiSidebarRepo => {
      const missing = missingRepos.has(path);
      const reported = sections[path];
      const section: GpuiSidebarRepo =
        path === params.repoPath
          ? built.repos[0]
          : reported
            ? (JSON.parse(reported) as GpuiSidebarRepo)
            : {
                path,
                homeRef: null,
                homeSelected: false,
                active: false,
                loading: !missing,
                workspaces: [],
                label: null,
                missing: false,
                collapsed: false,
              };
      return {
        ...section,
        label: labels?.get(path) ?? null,
        missing,
        collapsed: multi && !missing && collapsedRepos.has(path),
        workspaces: missing ? [] : section.workspaces,
        loading: missing ? false : section.loading,
      };
    }),
  };

  const lastSent = useRef<string | null>(null);
  const snapshotJson = JSON.stringify(snapshot);
  useEffect(() => {
    if (!active || snapshotJson === lastSent.current) return;
    lastSent.current = snapshotJson;
    void updateGpuiSidebar(JSON.parse(snapshotJson));
  }, [active, snapshotJson]);

  // Modals centre in the window, whose sidebar lies left of this webview.
  const { width } = params;
  useEffect(() => {
    if (!active) return;
    setWindowOffsetLeft(width);
    return () => setWindowOffsetLeft(null);
  }, [active, width]);

  // Hide the GPUI sidebar when the window goes away or the case stops being supported.
  useEffect(() => {
    if (!active) return;
    return () => {
      lastSent.current = null;
      void updateGpuiSidebar(null);
    };
  }, [active]);

  // Handlers change every render; the listener reads the latest through a ref.
  const toggleRepo = (path: string) => {
    const next = new Set(collapsedRepos);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    writeCollapsedRepos(mainRepoPath, next);
    setCollapsedState({ mainRepoPath, repos: next });
  };
  const latest = useRef({
    params,
    workspaces: rows.workspaces,
    nodes: rows.flattenedNodes,
    prStatusesByBranch: rows.prStatusesByBranch,
    missingRepos,
    toggleRepo,
  });
  latest.current = {
    params,
    workspaces: rows.workspaces,
    nodes: rows.flattenedNodes,
    prStatusesByBranch: rows.prStatusesByBranch,
    missingRepos,
    toggleRepo,
  };
  // The last plain or toggled row, where a Shift range starts.
  const lastSelectedIndexRef = useRef<number | null>(null);
  useEffect(() => {
    if (!active) return;
    const handle = (action: GpuiSidebarAction) => {
      const {
        params: p,
        workspaces,
        nodes,
        prStatusesByBranch,
        missingRepos: missing,
        toggleRepo: toggle,
      } = latest.current;
      const findIn = (path: string, id: number) =>
        lookupsRef.current.get(path)?.workspaces.find((w) => w.id === id) ??
        null;
      const find = (id: number | null) =>
        id === null ? null : (workspaces.find((w) => w.id === id) ?? null);
      switch (action.kind) {
        case "selectHome":
          if (action.repoPath !== p.repoPath) {
            if (!missing.has(action.repoPath)) {
              p.onSelectRepoHome(action.repoPath);
            }
          } else if (p.selectedWorkspaceIds.size > 0) {
            // As in the React sidebar, a click on home first drops the selection.
            lastSelectedIndexRef.current = null;
            p.onWorkspaceMultiSelect(null, {
              shiftKey: false,
              metaKey: false,
              ctrlKey: false,
            });
          } else {
            p.onSelectWorkspace(null);
          }
          break;
        case "selectWorkspace": {
          const workspace = find(action.id);
          const index = nodes.findIndex(
            (n) => n.status.current.id === action.id,
          );
          if (!workspace || index === -1) break;
          if (action.shift && lastSelectedIndexRef.current !== null) {
            p.onSelectStack(
              idsInVisibleRange(nodes, lastSelectedIndexRef.current, index),
            );
            break;
          }
          lastSelectedIndexRef.current = index;
          p.onWorkspaceMultiSelect(workspace, {
            shiftKey: action.shift,
            metaKey: action.toggle,
            ctrlKey: false,
          });
          break;
        }
        case "selectStack": {
          const workspace = find(action.id);
          if (!workspace) break;
          const all = nodes.map((n) => n.status.current);
          const stack = action.descendants
            ? [workspace, ...getDescendants(all, workspace.branch_name)]
            : getEntireStack(all, workspace.branch_name);
          p.onSelectStack(new Set(stack.map((w) => w.id)));
          break;
        }
        case "clearSelection":
          if (p.selectedWorkspaceIds.size > 0) {
            lastSelectedIndexRef.current = null;
            p.onWorkspaceMultiSelect(null, {
              shiftKey: false,
              metaKey: false,
              ctrlKey: false,
            });
          }
          break;
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
        case "moveWorkspace": {
          const workspace = find(action.id);
          const onto = find(action.onto);
          if (!workspace || (action.onto !== null && !onto)) break;
          p.onMoveWorkspace(workspace, onto?.branch_name ?? null);
          break;
        }
        case "resizeSidebar":
          p.onResizeSidebar(action.width);
          break;
        case "resetSidebarWidth":
          p.onResetSidebarWidth();
          break;
        case "dismissModal":
          dismissTopWindowModal();
          break;
        case "homeMenu": {
          const path = action.repoPath;
          const supporting = p.supportingRepos.some((r) => r.path === path);
          const isMissing = missing.has(path);
          void popupNativeMenu(
            homeMenuEntries({
              repoPath: path,
              missing: isMissing,
              editorApps: useEditorAppsStore.getState(),
              onOpenBranchSwitcher:
                path === p.repoPath ? p.onOpenBranchSwitcher : undefined,
              onAddRepository: supporting ? undefined : p.onAddRepository,
              onOpenRepoSettings:
                supporting && !isMissing
                  ? () => p.onOpenRepoSettings(path)
                  : undefined,
              onLocateRepo:
                supporting && isMissing
                  ? () => p.onLocateRepo(path)
                  : undefined,
              onRemoveRepo: supporting ? () => p.onRemoveRepo(path) : undefined,
            }),
          );
          break;
        }
        case "openRepoWorkspace": {
          const workspace = findIn(action.repoPath, action.id);
          if (workspace) p.onSelectWorkspace(workspace);
          break;
        }
        case "repoWorkspaceMenu": {
          const path = action.repoPath;
          const workspace = findIn(path, action.id);
          if (!workspace) break;
          const prs = lookupsRef.current.get(path)?.prStatusesByBranch ?? {};
          void popupNativeMenu(
            workspaceMenuEntries({
              workspace,
              title: getWorkspaceTitle(workspace),
              repoPath: path,
              prUrl: prs[workspace.branch_name]?.url ?? null,
              editorApps: useEditorAppsStore.getState(),
              onRename: () => p.onRenameWorkspace(workspace, path),
            }),
          );
          break;
        }
        case "toggleRepo":
          toggle(action.repoPath);
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
              onRename: () => p.onRenameWorkspace(workspace, p.repoPath ?? ""),
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

  return { active, sources };
}
