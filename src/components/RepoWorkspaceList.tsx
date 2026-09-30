import { DragDropContext, Droppable, type DropResult } from "@hello-pangea/dnd";
import { Archive, ChevronDown, ChevronRight } from "lucide-react";
import { useMemo, useState } from "react";
import useSWR from "swr";
import {
  useGitRemoteInfo,
  useMergeQueueEnabled,
  usePrStatusPolling,
} from "../hooks/useMergeQueueStatus";
import { useWorkspaceSidebarMultiSelect } from "../hooks/useWorkspaceSidebarMultiSelect";
import { useWorkspaceGitDetail } from "../hooks/useWorkspaceGitDetail";
import {
  getWorkspaces,
  listWorkspaceStatuses,
  type Workspace,
} from "../lib/api";
import type { PrInfo, QueueEntryStatus } from "../lib/api-types";
import type { ChangeFilesMoveRequest } from "../lib/change-file-drag";
import { FEATURES } from "../lib/features";
import { supabase } from "../lib/supabase";
import { pollMs } from "../lib/swr-cache";
import {
  buildWorkspaceTree,
  flattenWorkspaceTree,
  getDescendants,
  getEntireStack,
  type WorkspaceSidebarStatusWithAgentDetail,
} from "../lib/workspace-tree";
import { isWorkspaceHidden } from "../lib/workspace-utils";
import { usePreviewFeature } from "../stores/featurePreviewStore";
import { HiddenWorkspacesToggle } from "./HiddenWorkspacesToggle";
import { RenameWorkspaceDialog } from "./RenameWorkspaceDialog";
import type { TerminalSessionSummary } from "./terminal/types";
import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
  SidebarSeparator,
} from "./ui/sidebar";
import { WorkspaceSidebarItem } from "./WorkspaceSidebarItem";

export interface RepoWorkspaceListProps {
  repoPath?: string;
  cacheKey?: string;
  /** Repository label; set when the window shows several repositories. */
  groupLabel?: string;
  collapsed?: boolean;
  onToggleCollapsed?: () => void;
  /** Undefined when the selection is in another repository or page. */
  activeSelectedWorkspaceId?: number | null;
  activeSelectedWorkspaceIds?: Set<number>;
  selectedWorkspaceIds?: Set<number>;
  onWorkspaceClick?: (workspace: Workspace) => void;
  onWorkspaceMultiSelect?: (
    workspace: Workspace | null,
    event: React.MouseEvent,
  ) => void;
  onBulkArchive?: () => void;
  onArchiveWorkspace?: (workspace: Workspace) => void;
  archivingWorkspaceIds?: Set<number>;
  exitingWorkspaceIds?: Set<number>;
  onAddAfter?: (workspace: Workspace) => void;
  onMoveWorkspace?: (workspace: Workspace, targetBranch: string | null) => void;
  onSelectStack?: (workspaceIds: Set<number>) => void;
  onStartAgent?: (workspace: Workspace) => void;
  onStartShell?: (workspace: Workspace) => void;
  terminalSessions?: TerminalSessionSummary[];
  onDropChangeFiles?: (request: ChangeFilesMoveRequest) => void;
}

/** One repository's workspace tree in the sidebar, with its own polling. */
export const RepoWorkspaceList: React.FC<RepoWorkspaceListProps> = ({
  repoPath,
  cacheKey,
  groupLabel,
  collapsed = false,
  onToggleCollapsed,
  activeSelectedWorkspaceId,
  activeSelectedWorkspaceIds,
  selectedWorkspaceIds,
  onWorkspaceClick,
  onWorkspaceMultiSelect,
  onBulkArchive,
  onArchiveWorkspace,
  archivingWorkspaceIds,
  exitingWorkspaceIds,
  onAddAfter,
  onMoveWorkspace,
  onSelectStack,
  onStartAgent,
  onStartShell,
  terminalSessions,
  onDropChangeFiles,
}) => {
  const workspaceScheduling = usePreviewFeature("workspaceScheduling");
  const { data: workspaces = [], isLoading: workspacesPending } = useSWR(
    cacheKey ? ["workspaces", cacheKey] : null,
    () => getWorkspaces(repoPath || ""),
    { keepPreviousData: true },
  );
  const workspacesLoaded = workspacesPending === false && Boolean(repoPath);

  // Branches with an open agent terminal, and whether any of those sessions
  // is actively streaming -- drives the sidebar spinner's spin vs. idle pip.
  const streamingByBranchWithAgentSession = useMemo(() => {
    const branches = new Map<string, boolean>();
    for (const session of terminalSessions ?? []) {
      if (session.kind !== "agent" || !session.branchName) continue;
      branches.set(
        session.branchName,
        (branches.get(session.branchName) ?? false) || session.isStreaming,
      );
    }
    return branches;
  }, [terminalSessions]);

  // Shares its key with Dashboard.tsx's own plain `listWorkspaceStatuses`
  // poll, so this fetcher must stay a plain pass-through of that call --
  // see useWorkspaceGitDetail for the extra per-workspace detail this
  // sidebar needs on top of it.
  const { data: workspaceStatuses = [] } = useSWR(
    cacheKey && workspacesLoaded ? ["workspace-statuses", cacheKey] : null,
    () => listWorkspaceStatuses(repoPath || ""),
    { keepPreviousData: true },
  );
  const gitDetailById = useWorkspaceGitDetail({
    repoPath,
    cacheKey,
    enabled: workspacesLoaded,
    statuses: workspaceStatuses,
  });

  const { data: remoteInfo } = useGitRemoteInfo(repoPath);
  const { data: queueEnabled } = useMergeQueueEnabled(repoPath);
  const repoFullName = remoteInfo?.full_name;
  // Single Rust-backed cache for all workspace PR statuses — no per-row
  // `gh pr view` polling from the WebView.
  const { data: prStatusesByBranch = {} } = usePrStatusPolling(repoPath);
  const { data: branchQueueStatuses } = useSWR(
    FEATURES.mergeQueue && queueEnabled === true && repoFullName
      ? ["repo-branch-queue-statuses", repoFullName]
      : null,
    async () => {
      const { data } = await supabase.rpc("get_repo_branch_queue_statuses", {
        p_repo_full_name: repoFullName!,
      });
      const map = new Map<string, QueueEntryStatus>();
      for (const row of (data ?? []) as {
        branch_name: string;
        status: string;
      }[]) {
        map.set(row.branch_name, row.status as QueueEntryStatus);
      }
      return map;
    },
    { refreshInterval: pollMs(30_000) },
  );

  const statuses: WorkspaceSidebarStatusWithAgentDetail[] = (() => {
    const statusById = new Map(
      (workspaceStatuses ?? []).map((status) => [status.current.id, status]),
    );
    return (workspaces ?? []).map((workspace) => {
      const status = statusById.get(workspace.id);
      const detail = gitDetailById?.get(workspace.id);
      return {
        ...(status ?? { current: workspace, has_conflicts: false }),
        ...detail,
      };
    });
  })();
  const workspacesForSelection = statuses.map((s) => s.current);
  const [renameTarget, setRenameTarget] = useState<Workspace | null>(null);
  const [showHidden, setShowHidden] = useState(false);

  const visibleStatuses = (() => {
    if (!workspaceScheduling || showHidden) return statuses;
    return statuses.filter((status) => !isWorkspaceHidden(status.current));
  })();
  const hiddenCount = workspaceScheduling
    ? statuses.filter((status) => isWorkspaceHidden(status.current)).length
    : 0;

  const flattenedNodes = (() => {
    const isMergedBranch = (branchName: string) =>
      (prStatusesByBranch as Record<string, PrInfo | null>)[branchName]
        ?.state === "MERGED";
    const tree = buildWorkspaceTree(visibleStatuses, isMergedBranch);
    return flattenWorkspaceTree(tree);
  })();

  const { handleItemSelect } = useWorkspaceSidebarMultiSelect({
    flattenedNodes,
    onSelectStack,
    onWorkspaceMultiSelect,
    onWorkspaceClick,
  });

  const handleDoubleClick = (workspace: Workspace, e: React.MouseEvent) => {
    if (!onSelectStack) return;
    e.stopPropagation();

    if (e.shiftKey) {
      const descendants = getDescendants(
        workspacesForSelection,
        workspace.branch_name,
      );
      const ids = new Set([workspace.id, ...descendants.map((w) => w.id)]);
      onSelectStack(ids);
      return;
    }

    const stack = getEntireStack(workspacesForSelection, workspace.branch_name);
    onSelectStack(new Set(stack.map((w) => w.id)));
  };

  const handleDragEnd = (result: DropResult) => {
    if (!onMoveWorkspace) return;

    const draggedId = parseInt(result.draggableId, 10);
    const draggedWorkspace = workspacesForSelection.find(
      (w) => w.id === draggedId,
    );
    if (!draggedWorkspace) return;

    if (result.combine) {
      const targetWorkspace = workspacesForSelection.find(
        (w) => String(w.id) === result.combine!.draggableId,
      );
      if (targetWorkspace && targetWorkspace.id !== draggedWorkspace.id) {
        onMoveWorkspace(draggedWorkspace, targetWorkspace.branch_name);
      }
      return;
    }

    if (result.destination) {
      onMoveWorkspace(draggedWorkspace, null);
    }
  };

  const showBulkArchive = activeSelectedWorkspaceIds !== undefined;

  return (
    <>
      {(groupLabel || workspaces.length > 0) && <SidebarSeparator />}
      <SidebarGroup className="py-0">
        {groupLabel ? (
          <SidebarGroupLabel
            asChild
            className="uppercase tracking-widest cursor-pointer"
          >
            <button
              type="button"
              data-testid="repo-workspace-group-label"
              aria-expanded={!collapsed}
              onClick={onToggleCollapsed}
              title={repoPath}
            >
              {collapsed ? (
                <ChevronRight className="w-3 h-3 mr-1 shrink-0" />
              ) : (
                <ChevronDown className="w-3 h-3 mr-1 shrink-0" />
              )}
              <span className="truncate">{groupLabel}</span>
            </button>
          </SidebarGroupLabel>
        ) : (
          <SidebarGroupLabel className="uppercase tracking-widest">
            Workspaces
          </SidebarGroupLabel>
        )}
        {workspaceScheduling && !collapsed && (
          <HiddenWorkspacesToggle
            showHidden={showHidden}
            hiddenCount={hiddenCount}
            onToggle={() => setShowHidden((value) => !value)}
          />
        )}
        <SidebarGroupContent hidden={collapsed}>
          <DragDropContext onDragEnd={handleDragEnd}>
            <Droppable droppableId="sidebar-root" isCombineEnabled>
              {(droppableProvided) => (
                <SidebarMenu
                  ref={droppableProvided.innerRef}
                  {...droppableProvided.droppableProps}
                >
                  {workspacesPending &&
                    flattenedNodes.length === 0 &&
                    Array.from({ length: 6 }, (_, index) => (
                      <SidebarMenuItem key={`workspace-skeleton-${index}`}>
                        <SidebarMenuSkeleton />
                      </SidebarMenuItem>
                    ))}
                  {flattenedNodes.map((node, index) => (
                    <WorkspaceSidebarItem
                      key={node.status.current.id}
                      node={node}
                      index={index}
                      repoPath={repoPath}
                      selectedWorkspaceId={activeSelectedWorkspaceId}
                      selectedWorkspaceIds={activeSelectedWorkspaceIds}
                      onWorkspaceClick={onWorkspaceClick}
                      onWorkspaceMultiSelect={handleItemSelect}
                      onAddAfter={onAddAfter}
                      onStartAgent={onStartAgent}
                      onStartShell={onStartShell}
                      onArchiveWorkspace={onArchiveWorkspace}
                      archiving={archivingWorkspaceIds?.has(
                        node.status.current.id,
                      )}
                      exiting={exitingWorkspaceIds?.has(node.status.current.id)}
                      onRenameWorkspace={setRenameTarget}
                      onDoubleClick={handleDoubleClick}
                      queueStatus={branchQueueStatuses?.get(
                        node.status.current.branch_name,
                      )}
                      prInfo={
                        (prStatusesByBranch as Record<string, PrInfo | null>)[
                          node.status.current.branch_name
                        ] ?? null
                      }
                      hasRemote={!!remoteInfo}
                      onDropChangeFiles={onDropChangeFiles}
                      hasActiveAgentSession={streamingByBranchWithAgentSession.has(
                        node.status.current.branch_name,
                      )}
                      isAgentSessionStreaming={
                        streamingByBranchWithAgentSession.get(
                          node.status.current.branch_name,
                        ) ?? false
                      }
                    />
                  ))}
                  {droppableProvided.placeholder}
                  {showBulkArchive &&
                    selectedWorkspaceIds &&
                    selectedWorkspaceIds.size > 0 &&
                    !archivingWorkspaceIds?.size &&
                    !exitingWorkspaceIds?.size && (
                      <SidebarMenuItem>
                        <SidebarMenuButton
                          type="button"
                          onClick={onBulkArchive}
                          className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                        >
                          <Archive />
                          <span>
                            Archive {selectedWorkspaceIds.size} workspace
                            {selectedWorkspaceIds.size > 1 ? "s" : ""}
                          </span>
                        </SidebarMenuButton>
                      </SidebarMenuItem>
                    )}
                </SidebarMenu>
              )}
            </Droppable>
          </DragDropContext>
        </SidebarGroupContent>
      </SidebarGroup>
      {renameTarget && repoPath && (
        <RenameWorkspaceDialog
          open={!!renameTarget}
          onOpenChange={(open) => {
            if (!open) setRenameTarget(null);
          }}
          repoPath={repoPath}
          workspace={renameTarget}
          onSuccess={() => setRenameTarget(null)}
        />
      )}
    </>
  );
};
