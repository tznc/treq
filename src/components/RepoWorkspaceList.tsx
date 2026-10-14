import { DragDropContext, Droppable, type DropResult } from "@hello-pangea/dnd";
import { Archive, ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";
import useSWR from "swr";
import { useMergeQueueEnabled } from "../hooks/useMergeQueueStatus";
import { useWorkspaceSidebarRows } from "../hooks/useWorkspaceSidebarRows";
import { useWorkspaceSidebarMultiSelect } from "../hooks/useWorkspaceSidebarMultiSelect";
import type { Workspace } from "../lib/api";
import type { QueueEntryStatus } from "../lib/api-types";
import type { ChangeFilesMoveRequest } from "../lib/change-file-drag";
import { FEATURES } from "../lib/features";
import { supabase } from "../lib/supabase";
import { pollMs } from "../lib/swr-cache";
import { getDescendants, getEntireStack } from "../lib/workspace-tree";
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
  /** Draw a separator above the list; off between repository groups. */
  showSeparator?: boolean;
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
  showSeparator = true,
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
  const {
    workspaces,
    workspacesPending,
    statuses,
    flattenedNodes,
    remoteInfo,
    prStatusesByBranch,
    streamingByBranchWithAgentSession,
  } = useWorkspaceSidebarRows({ repoPath, cacheKey, terminalSessions });

  const { data: queueEnabled } = useMergeQueueEnabled(repoPath);
  const repoFullName = remoteInfo?.full_name;
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

  const workspacesForSelection = statuses.map((s) => s.current);
  const [renameTarget, setRenameTarget] = useState<Workspace | null>(null);

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
      {showSeparator && (groupLabel || workspaces.length > 0) && (
        <SidebarSeparator />
      )}
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
                        prStatusesByBranch[node.status.current.branch_name] ??
                        null
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
