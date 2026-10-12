import type { Ref } from "react";
import {
  type AgentSessionData,
  type TerminalSessionSummary,
} from "../terminal/types";
import type { RemoteTerminalTarget } from "../RemoteTerminalPanel";

export interface ShellTerminalData {
  id: string;
  workingDirectory: string;
  remoteHost?: string;
  /**
   * Set for a shell or agent running in a persistent session on a remote
   * host. Rendered with the remote PTY panel instead of a local PTY.
   */
  remote?: RemoteTerminalTarget;
}

export interface WorkspaceTerminalPaneProps {
  ref?: Ref<WorkspaceTerminalPaneHandle>;
  workingDirectory: string;
  /** Local repository whose environment setting new terminals follow. */
  repoPath?: string;
  remoteHost?: string;
  onSessionError?: (message: string) => void;
  currentBranch?: string | null;
  agentSessions?: AgentSessionData[];
  activeAgentSessionId?: number | null;
  onActiveSessionChange?: (sessionId: number | null) => void;
  onCreateNewSession?: (activeWorkspacePath?: string | null) => void;
  onCloseSession?: (sessionId: number) => void;
  onNavigateToWorkspace?: (workspaceKey: string, isMainRepo: boolean) => void;
  /** Full workspace path -> branch name, used to resolve shell terminal branches for the sidebar list. */
  workspaceBranchByPath?: Map<string, string>;
  /** Reports the up-to-date list of terminal sessions for the sidebar's terminal list. */
  onTerminalsChange?: (summaries: TerminalSessionSummary[]) => void;
  /**
   * Set while a remote repository is active. New shells are opened on the
   * remote host in the session this returns, never as a local PTY. Returning
   * `null` cancels the shell (the caller reports why).
   */
  resolveRemoteShell?: (
    workingDirectory: string,
  ) => RemoteTerminalTarget | null;
  className?: string;
}

export interface WorkspaceTerminalPaneHandle {
  toggleCollapse: () => void;
  toggleMaximize: () => void;
  createShellSession: (workingDir?: string) => void;
  /** Opens (or reattaches to) a persistent remote shell or agent session in the pane. */
  openRemoteSession: (target: RemoteTerminalTarget) => void;
  closeTerminalsForWorkspace: (workspaceKey: string) => void;
  sendToTerminal: (id: string, text: string) => void;
}

export type TerminalEntry =
  | { type: "shell"; data: ShellTerminalData }
  | { type: "agent"; data: AgentSessionData };

export interface TerminalWithWorkspace {
  terminal: TerminalEntry;
  workspace: {
    workspaceKey: string;
    workspaceName: string;
    isMainRepo: boolean;
  };
}
