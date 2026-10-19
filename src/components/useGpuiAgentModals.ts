import type { StashEntry, Workspace } from "../lib/api";
import { toAgentKind } from "../lib/agents";
import {
  type GpuiAgentPromptStart,
  type GpuiModal,
  type GpuiScreenAction,
  useGpuiModal,
  useGpuiModalsAvailable,
} from "../lib/gpui-screen";
import type { IssueAttachment } from "../lib/promptAttachments";
import { getFullWorkspacePath } from "../lib/utils";

interface Handlers {
  closeAgentPrompt: () => void;
  /** The prompt dialog's Edit or Plan: React starts the session. */
  startAgent: (start: GpuiAgentPromptStart) => void;
  closePromptHistory: () => void;
  runPrompt: (prompt: string, workspaceId: number | null) => void;
  closeStash: () => void;
  applyStashToNew: (entry: StashEntry) => void;
}

/**
 * Draws the agent prompt dialog, the prompt history and the stash list in
 * GPUI over its screen (src-tauri/src/gpui_shell/screen/modal.rs), while GPUI
 * has a screen up. React keeps the state of whether each is open and starts
 * the session the prompt dialog asks for. Returns whether GPUI draws them, so
 * React leaves its own dialogs closed.
 */
export function useGpuiAgentModals({
  repoPath,
  agentPrompt,
  promptHistory,
  stash,
  handlers,
}: {
  repoPath: string;
  agentPrompt: {
    open: boolean;
    defaultBranch: string;
    workspaces: Workspace[];
    initialPrompt?: string;
    initialWorkspaceId: number | null;
    issue: IssueAttachment | null;
    starting: boolean;
  };
  promptHistory: { open: boolean; focusId: number | null };
  stash: {
    open: boolean;
    /** Activity-sorted, for the apply picker. */
    workspaces: Workspace[];
  };
  handlers: Handlers;
}): boolean {
  const available = useGpuiModalsAvailable();

  let modal: GpuiModal | null = null;
  if (agentPrompt.open) {
    modal = {
      kind: "agentPrompt",
      repoPath,
      defaultBranch: agentPrompt.defaultBranch,
      workspaces: agentPrompt.workspaces.map((ws) => ({
        id: ws.id,
        branchName: ws.branch_name,
        path: getFullWorkspacePath(ws),
      })),
      initialPrompt: agentPrompt.initialPrompt ?? null,
      initialWorkspaceId: agentPrompt.workspaces.some(
        (ws) => ws.id === agentPrompt.initialWorkspaceId,
      )
        ? agentPrompt.initialWorkspaceId
        : null,
      issue: agentPrompt.issue && {
        source: agentPrompt.issue.source,
        key: agentPrompt.issue.key,
        title: agentPrompt.issue.title,
        includeSubItems: agentPrompt.issue.includeSubItems,
      },
      starting: agentPrompt.starting,
    };
  } else if (stash.open) {
    modal = {
      kind: "stash",
      repoPath,
      workspaces: stash.workspaces.map((ws) => ({
        branch: ws.branch_name,
        label: ws.title?.trim() || ws.branch_name,
      })),
    };
  } else if (promptHistory.open) {
    modal = {
      kind: "promptHistory",
      repoPath,
      initialSelectedId: promptHistory.focusId,
    };
  }

  const onAction = (action: GpuiScreenAction) => {
    if (action.kind === "press") {
      switch (action.id) {
        case "agentPrompt.close":
          return handlers.closeAgentPrompt();
        case "promptHistory.close":
          return handlers.closePromptHistory();
        case "stash.close":
          return handlers.closeStash();
        default:
          return;
      }
    }
    if (action.kind !== "field") return;
    switch (action.field) {
      case "agentPrompt.start": {
        const start = action.value as unknown as GpuiAgentPromptStart;
        if (toAgentKind(start.agent)) handlers.startAgent(start);
        return;
      }
      case "promptHistory.run": {
        const { prompt, workspaceId } = action.value as {
          prompt: string;
          workspaceId: number | null;
        };
        return handlers.runPrompt(prompt, workspaceId);
      }
      case "stash.applyToNew":
        return handlers.applyStashToNew(action.value as unknown as StashEntry);
      default:
    }
  };

  useGpuiModal(available ? modal : null, onAction);
  return available;
}
