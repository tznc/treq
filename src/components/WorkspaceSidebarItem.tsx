import { Draggable } from "@hello-pangea/dnd";
import {
  Archive,
  Bot,
  GitBranch,
  Layers2,
  Link,
  Loader2,
  Pencil,
  Terminal,
} from "lucide-react";
import { useState } from "react";
import { type QueueEntryStatus, type Workspace } from "../lib/api";
import type { PrInfo } from "../lib/api-types";
import { useRemoteCapabilities } from "../lib/active-repository-context";
import { cn, getFullWorkspacePath } from "../lib/utils";
import type { FlattenedWorkspaceNode } from "../lib/workspace-tree";
import {
  getWorkspaceStatusIndicator,
  WORKSPACE_STATUS_DOT_BG_CLASS,
  WORKSPACE_STATUS_DOT_TEXT_CLASS,
} from "../lib/workspace-status-indicator";
import {
  getWorkspaceTitle as getWorkspaceTitleFromUtils,
} from "../lib/workspace-utils";
import {
  getChangeFilesDragData,
  isChangeFilesDrag,
  type ChangeFilesMoveRequest,
} from "../lib/change-file-drag";
import { Button } from "./ui/button";
import { SidebarMenuItem } from "./ui/sidebar";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "./ui/context-menu";
import { TruncatedText } from "./TruncatedText";
import { Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";
import { useWorkspaceRowPointerHandlers } from "../hooks/useWorkspaceSidebarMultiSelect";
import { PathContextMenuItems } from "./WorkspacePathContextMenu";

interface WorkspaceSidebarItemProps {
  node: FlattenedWorkspaceNode;
  index: number;
  repoPath?: string;
  selectedWorkspaceId?: number | null;
  selectedWorkspaceIds?: Set<number>;
  onWorkspaceClick?: (workspace: Workspace) => void;
  onWorkspaceMultiSelect?: (
    workspace: Workspace,
    event: React.MouseEvent | React.PointerEvent,
    index: number,
  ) => void;
  onAddAfter?: (workspace: Workspace) => void;
  onStartAgent?: (workspace: Workspace) => void;
  onStartShell?: (workspace: Workspace) => void;
  onArchiveWorkspace?: (workspace: Workspace) => void;
  archiving?: boolean;
  exiting?: boolean;
  onRenameWorkspace: (workspace: Workspace) => void;
  onDoubleClick?: (workspace: Workspace, event: React.MouseEvent) => void;
  queueStatus?: QueueEntryStatus;
  /** Cached PR info from the Rust background poller (null = no PR / not yet known). */
  prInfo?: PrInfo | null;
  /** Whether the repo has a GitHub remote (gates PR icon coloring). */
  hasRemote?: boolean;
  onDropChangeFiles?: (request: ChangeFilesMoveRequest) => void;
  /** Whether at least one agent terminal session is currently open on this workspace. */
  hasActiveAgentSession?: boolean;
  /** Whether any open agent session on this workspace is actively streaming. */
  isAgentSessionStreaming?: boolean;
}

function prIconStyle(prInfo: PrInfo): { color: string; label: string } {
  const state = prInfo.state.toUpperCase();
  if (prInfo.is_draft && state === "OPEN") {
    return { color: "text-muted-foreground", label: "Draft PR" };
  }
  if (state === "OPEN") {
    return {
      color: "text-green-600 dark:text-green-400",
      label: "Open PR",
    };
  }
  if (state === "MERGED") {
    return {
      color: "text-purple-600 dark:text-purple-400",
      label: "PR merged",
    };
  }
  return { color: "text-red-600 dark:text-red-400", label: "PR closed" };
}

function queueStatusDot(status: QueueEntryStatus): {
  color: string;
  label: string;
} {
  switch (status) {
    case "queued":
      return { color: "bg-yellow-400", label: "In merge queue" };
    case "testing":
      return {
        color: "bg-blue-400 animate-pulse",
        label: "CI running in merge queue",
      };
    case "merging":
      return {
        color: "bg-green-400 animate-pulse",
        label: "Passed CI, merging now",
      };
    case "merged":
      return { color: "bg-green-600", label: "Merged via queue" };
    case "failed":
      return { color: "bg-red-500", label: "Failed in merge queue" };
    default:
      return { color: "bg-muted-foreground", label: status };
  }
}

export const WorkspaceSidebarItem: React.FC<WorkspaceSidebarItemProps> = ({
  node,
  index,
  repoPath,
  selectedWorkspaceId,
  selectedWorkspaceIds,
  onWorkspaceClick,
  onWorkspaceMultiSelect,
  onAddAfter,
  onStartAgent,
  onStartShell,
  onArchiveWorkspace,
  archiving = false,
  exiting = false,
  onRenameWorkspace,
  onDoubleClick,
  queueStatus,
  prInfo = null,
  hasRemote = false,
  onDropChangeFiles,
  hasActiveAgentSession = false,
  isAgentSessionStreaming = false,
}) => {
  const caps = useRemoteCapabilities();
  const workspace = node.status.current;
  const isSelected =
    selectedWorkspaceIds?.has(workspace.id) ||
    selectedWorkspaceId === workspace.id;
  const indentStyle = {
    paddingLeft: `${16 + (node.depth - 1) * 6}px`,
  };
  const isConflicted = node.status.has_conflicts;
  const statusIndicator = getWorkspaceStatusIndicator({
    isConflicted,
    hasChanges: node.status.has_changes ?? false,
    isAgentSessionStreaming: hasActiveAgentSession && isAgentSessionStreaming,
  });
  const workspaceTitle = getWorkspaceTitleFromUtils(workspace);
  const prStatus = hasRemote && prInfo ? prIconStyle(prInfo) : null;
  const [isChangeDropTarget, setIsChangeDropTarget] = useState(false);
  const { onPointerDown, onClick } = useWorkspaceRowPointerHandlers({
    onSelect: (event) => {
      if (archiving || exiting) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      if (onWorkspaceMultiSelect) {
        onWorkspaceMultiSelect(workspace, event, index);
        return;
      }
      onWorkspaceClick?.(workspace);
    },
  });

  return (
    <SidebarMenuItem>
      <Draggable draggableId={String(workspace.id)} index={index}>
        {(dragProvided, dragSnapshot) => (
          <div
            ref={dragProvided.innerRef}
            {...dragProvided.draggableProps}
            {...dragProvided.dragHandleProps}
          >
            <ContextMenu>
              <Tooltip>
                <ContextMenuTrigger asChild>
                  <TooltipTrigger asChild>
                    <div
                      data-testid={`workspace-sidebar-item-${workspace.branch_name}`}
                      data-sidebar-index={index}
                      style={indentStyle}
                      className={cn(
                        "group/workspace relative flex h-8 items-center tracking-wide rounded-sm transition-[colors,opacity,max-height,padding] duration-200 ease-out cursor-pointer py-1 pr-2 max-h-8 overflow-hidden",
                        {
                          "bg-primary/20": isSelected && !archiving && !exiting,
                          "hover:bg-muted/50":
                            !isSelected && !archiving && !exiting,
                          "bg-primary/10":
                            dragSnapshot.combineTargetFor || isChangeDropTarget,
                          "opacity-50": dragSnapshot.isDragging,
                          "text-destructive": isConflicted,
                          "opacity-50 pointer-events-none cursor-not-allowed":
                            archiving,
                          "opacity-0 max-h-0 py-0 pointer-events-none": exiting,
                        },
                      )}
                      aria-busy={archiving || exiting}
                      aria-disabled={archiving || exiting}
                      onPointerDown={onPointerDown}
                      onClick={onClick}
                      onDoubleClick={(e) => onDoubleClick?.(workspace, e)}
                      onDragOver={(e) => {
                        if (
                          !onDropChangeFiles ||
                          !isChangeFilesDrag(e.dataTransfer)
                        )
                          return;
                        e.preventDefault();
                        e.dataTransfer.dropEffect = "move";
                        setIsChangeDropTarget(true);
                      }}
                      onDragLeave={() => setIsChangeDropTarget(false)}
                      onDrop={(e) => {
                        setIsChangeDropTarget(false);
                        if (!onDropChangeFiles) return;
                        const payload = getChangeFilesDragData(e.dataTransfer);
                        if (!payload) return;
                        e.preventDefault();
                        e.stopPropagation();
                        if (payload.sourceBranch === workspace.branch_name)
                          return;
                        onDropChangeFiles({
                          files: payload.files,
                          sourceBranch: payload.sourceBranch,
                          destinationBranch: workspace.branch_name,
                          destinationLabel: workspaceTitle,
                        });
                      }}
                    >
                      {node.depth > 0 && (
                        <span
                          aria-hidden="true"
                          className="pointer-events-none absolute inset-y-0 border-l-2 border-blue-900 dark:border-blue-300"
                          style={{
                            left: `${8 + Math.max(node.depth - 2, 0) * 6}px`,
                          }}
                        />
                      )}
                      {archiving ? (
                        <Loader2
                          data-testid="workspace-archive-spinner"
                          className="w-3 h-3 mr-1 shrink-0 animate-spin text-muted-foreground"
                          aria-label="Archiving workspace"
                        />
                      ) : (
                        <GitBranch
                          data-testid={`workspace-pr-icon-${workspace.id}`}
                          aria-label={prStatus ? prStatus.label : undefined}
                          className={`w-3 h-3 mr-1 shrink-0 -scale-y-100 ${
                            prStatus
                              ? prStatus.color
                              : isSelected
                                ? "text-primary"
                                : "text-muted-foreground"
                          }`}
                        />
                      )}
                      <TruncatedText
                        text={workspaceTitle}
                        className={cn(
                          "flex-1 min-w-0 font-mono",
                          statusIndicator && "pr-7",
                          isSelected
                            ? "text-primary font-medium"
                            : "text-muted-foreground",
                        )}
                      />
                      {statusIndicator &&
                        (() => {
                          const testId =
                            statusIndicator.color === "red"
                              ? `workspace-conflict-indicator-${workspace.id}`
                              : `workspace-status-indicator-${workspace.id}`;
                          // Spinner centers via inset-y-0 my-auto: animate-spin's
                          // transform would override a -translate-y-1/2.
                          return statusIndicator.spin ? (
                            <Loader2
                              data-testid={testId}
                              className={cn(
                                "w-3.5 h-3.5 shrink-0 animate-spin absolute right-3 inset-y-0 my-auto group-hover/workspace:hidden group-focus-within/workspace:hidden",
                                WORKSPACE_STATUS_DOT_TEXT_CLASS[
                                  statusIndicator.color
                                ],
                              )}
                              aria-label={statusIndicator.label}
                            />
                          ) : (
                            <span
                              data-testid={testId}
                              className={cn(
                                "w-2 h-2 rounded-full shrink-0 absolute right-3 top-1/2 -translate-y-1/2 group-hover/workspace:hidden group-focus-within/workspace:hidden",
                                WORKSPACE_STATUS_DOT_BG_CLASS[
                                  statusIndicator.color
                                ],
                              )}
                              aria-label={statusIndicator.label}
                            />
                          );
                        })()}
                      {queueStatus && (
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <span
                              className={`w-2 h-2 rounded-full shrink-0 mr-1 ${queueStatusDot(queueStatus).color}`}
                              aria-label={queueStatusDot(queueStatus).label}
                            />
                          </TooltipTrigger>
                          <TooltipContent side="right">
                            {queueStatusDot(queueStatus).label}
                          </TooltipContent>
                        </Tooltip>
                      )}
                      <div
                        data-workspace-actions
                        className="hidden items-center gap-1 shrink-0 mr-1 group-hover/workspace:flex group-focus-within/workspace:flex"
                      >
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              size="icon-xs"
                              variant="ghost"
                              className="text-foreground"
                              aria-label="Start agent"
                              disabled={!caps.agentPty.supported}
                              title={caps.agentPty.reason}
                              onClick={(e) => {
                                e.stopPropagation();
                                if (!caps.agentPty.supported) return;
                                onStartAgent?.(workspace);
                              }}
                            >
                              <Bot className="w-4 h-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent side="bottom">
                            Start agent
                          </TooltipContent>
                        </Tooltip>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              size="icon-xs"
                              variant="ghost"
                              className="text-foreground"
                              aria-label="Open shell"
                              disabled={!caps.shell.supported}
                              title={caps.shell.reason}
                              onClick={(e) => {
                                e.stopPropagation();
                                if (!caps.shell.supported) return;
                                onStartShell?.(workspace);
                              }}
                            >
                              <Terminal className="w-4 h-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent side="bottom">
                            Open shell
                          </TooltipContent>
                        </Tooltip>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              size="icon-xs"
                              variant="ghost"
                              className="text-foreground"
                              aria-label="Stack a workspace"
                              onClick={(e) => {
                                e.stopPropagation();
                                onAddAfter?.(workspace);
                              }}
                            >
                              <Layers2 className="w-4 h-4" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent side="bottom">
                            Stack workspace
                          </TooltipContent>
                        </Tooltip>
                      </div>
                    </div>
                  </TooltipTrigger>
                </ContextMenuTrigger>
                {!dragSnapshot.isDragging && (
                  <TooltipContent side="right" className="font-mono">
                    <div className="flex items-center gap-1.5">
                      <GitBranch className="w-3 h-3" />
                      <span>{workspaceTitle}</span>
                    </div>
                    {isConflicted && (
                      <div className="font-sans mt-1 text-destructive">
                        Conflicts detected
                      </div>
                    )}
                    {prStatus && (
                      <div className={`font-sans mt-1 ${prStatus.color}`}>
                        {prStatus.label}
                      </div>
                    )}
                  </TooltipContent>
                )}
              </Tooltip>
              <ContextMenuContent>
                <ContextMenuItem
                  onClick={() => navigator.clipboard.writeText(workspaceTitle)}
                >
                  <GitBranch className="w-4 h-4 mr-2" />
                  Copy branch name
                </ContextMenuItem>
                {prInfo && (
                  <ContextMenuItem
                    onClick={() => navigator.clipboard.writeText(prInfo.url)}
                  >
                    <Link className="w-4 h-4 mr-2" />
                    Copy link to GitHub PR
                  </ContextMenuItem>
                )}
                <ContextMenuItem onClick={() => onRenameWorkspace(workspace)}>
                  <Pencil className="w-4 h-4 mr-2" />
                  Rename Workspace
                </ContextMenuItem>
                <ContextMenuSeparator />
                <PathContextMenuItems
                  relativePath={
                    workspace.workspace_path.startsWith("/")
                      ? repoPath &&
                        workspace.workspace_path.startsWith(repoPath)
                        ? workspace.workspace_path.slice(repoPath.length + 1)
                        : workspace.workspace_path
                      : `.treq/workspaces/${workspace.workspace_path}`
                  }
                  fullPath={getFullWorkspacePath(workspace)}
                  additionalItems={
                    <>
                      <ContextMenuSeparator />
                      <ContextMenuItem
                        className="text-destructive focus:text-destructive"
                        onClick={() => onArchiveWorkspace?.(workspace)}
                      >
                        <Archive className="w-4 h-4 mr-2" />
                        Archive Workspace
                      </ContextMenuItem>
                    </>
                  }
                />
              </ContextMenuContent>
            </ContextMenu>
            {dragSnapshot.combineTargetFor && (
              <div className="relative h-0">
                <div className="absolute left-0 right-0 top-0 h-[2px] bg-primary rounded-full" />
              </div>
            )}
          </div>
        )}
      </Draggable>
    </SidebarMenuItem>
  );
};
