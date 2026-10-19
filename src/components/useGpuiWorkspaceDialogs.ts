import { useRef } from "react";
import type { Workspace } from "../lib/api";
import {
  type GpuiWorkspaceModal,
  type GpuiDialogWorkspace,
  type GpuiScreenAction,
  useGpuiWorkspaceModal,
} from "../lib/gpui-screen";
import type { WorkspaceDialogDefaults } from "./UnifiedWorkspaceDialog";

interface ChangeMove {
  files: string[];
  sourceBranch: string;
  destinationBranch: string;
  destinationLabel: string;
}

interface Handlers {
  /** `UnifiedWorkspaceDialog`'s `onOpenChange(false)`. */
  closeWorkspaceDialog: () => void;
  /** `UnifiedWorkspaceDialog`'s `onSuccess`. */
  workspaceCreated: (workspaceId: number) => void;
  closeRename: () => void;
  closeDeletion: () => void;
  /** `WorkspaceDeletion`'s `onDeleteWorkspace`. */
  deleteWorkspace: (workspace: Workspace) => void;
  /** The files were moved; GPUI has shown the toast. */
  changeMoved: () => void;
  cancelChangeMove: () => void;
}

function modalWorkspace(workspace: Workspace): GpuiDialogWorkspace {
  return {
    id: workspace.id,
    branchName: workspace.branch_name,
    workspacePath: workspace.workspace_path,
  };
}

function dialogSource(
  source: Workspace | null | undefined,
): Extract<
  GpuiWorkspaceModal,
  { kind: "workspaceDialog" }
>["defaults"]["source"] {
  if (source === undefined) return { kind: "create" };
  if (source === null) return { kind: "home" };
  return {
    kind: "workspace",
    id: source.id,
    branchName: source.branch_name,
    targetBranch: source.target_branch ?? null,
  };
}

/**
 * Has GPUI draw Dashboard's workspace dialogs (`UnifiedWorkspaceDialog`,
 * `RenameWorkspaceDialog`, `WorkspaceDeletion` and the change-move
 * `AlertDialog`) over its screen, from the state Dashboard keeps for them.
 * GPUI creates, moves and renames through the backend itself. Returns the
 * kind of dialog GPUI draws, which React then leaves out; null while React
 * draws them all (no GPUI screen shows, or a remote repository).
 */
export function useGpuiWorkspaceDialogs({
  enabled,
  repoPath,
  changeMoveRepoPath,
  workspaces,
  currentWorkspace,
  workspaceDialog,
  rename,
  deletion,
  changeMove,
  handlers,
}: {
  enabled: boolean;
  /** The repository the dialogs list and create workspaces in. */
  repoPath: string;
  /** The repository dropped files move in. */
  changeMoveRepoPath: string;
  workspaces: Workspace[];
  currentWorkspace: Workspace | null;
  workspaceDialog: WorkspaceDialogDefaults | null;
  rename: { workspace: Workspace; repoPath: string } | null;
  deletion: boolean;
  changeMove: ChangeMove | null;
  handlers: Handlers;
}): GpuiWorkspaceModal["kind"] | null {
  // Each open of the workspace dialog starts it fresh.
  const opened = useRef<{
    defaults: WorkspaceDialogDefaults | null;
    key: number;
  }>({ defaults: null, key: 0 });
  if (workspaceDialog && workspaceDialog !== opened.current.defaults) {
    opened.current = {
      defaults: workspaceDialog,
      key: opened.current.key + 1,
    };
  }

  let modal: GpuiWorkspaceModal | null = null;
  if (workspaceDialog) {
    const { sourceWorkspace, ...rest } = workspaceDialog;
    modal = {
      kind: "workspaceDialog",
      repoPath,
      key: opened.current.key,
      defaults: { ...rest, source: dialogSource(sourceWorkspace) },
    };
  } else if (rename) {
    modal = {
      kind: "renameWorkspace",
      repoPath: rename.repoPath,
      workspaceId: rename.workspace.id,
      branchName: rename.workspace.branch_name,
    };
  } else if (deletion && repoPath) {
    modal = {
      kind: "workspaceDeletion",
      repoPath,
      workspaces: workspaces.map(modalWorkspace),
      currentWorkspace: currentWorkspace
        ? modalWorkspace(currentWorkspace)
        : null,
    };
  } else if (changeMove && changeMoveRepoPath) {
    modal = {
      kind: "confirmChangeMove",
      repoPath: changeMoveRepoPath,
      ...changeMove,
    };
  }

  const onAction = (action: GpuiScreenAction) => {
    if (action.kind === "press") {
      switch (action.id) {
        case "workspaceDialog.close":
          return handlers.closeWorkspaceDialog();
        case "renameWorkspace.close":
          return handlers.closeRename();
        case "workspaceDeletion.close":
          return handlers.closeDeletion();
        case "changeMove.done":
          return handlers.changeMoved();
        case "changeMove.cancel":
          return handlers.cancelChangeMove();
        default:
          return;
      }
    }
    if (action.kind !== "field" || typeof action.value !== "number") return;
    if (action.field === "workspaceDialog.created") {
      handlers.workspaceCreated(action.value);
      handlers.closeWorkspaceDialog();
    } else if (action.field === "workspaceDeletion.delete") {
      const workspace = workspaces.find((w) => w.id === action.value);
      if (workspace) handlers.deleteWorkspace(workspace);
      handlers.closeDeletion();
    }
  };

  const draws = useGpuiWorkspaceModal(enabled, modal, onAction);
  return draws && modal ? modal.kind : null;
}
