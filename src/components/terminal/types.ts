import type { AgentKind } from "../../lib/agents";
import type { ConsolidatedTerminalHandle } from "../ConsolidatedTerminal";

// Minimum width for each terminal panel (used when multiple terminals)
export const MIN_TERMINAL_WIDTH = 300;

export interface AgentSessionData {
  /**
   * Identifies the session in the terminal pane. The pane can show sessions
   * from several repositories, whose database ids overlap, so this can differ
   * from `dbSessionId`.
   */
  sessionId: number;
  /** The session's id in `repoPath`'s database; defaults to `sessionId`. */
  dbSessionId?: number;
  sessionName: string;
  ptySessionId: string;
  workspacePath: string | null;
  workspaceId?: number | null;
  workingDirectoryOverride?: string;
  repoPath: string;
  workspaceName?: string | null; // Branch name or null for main repo
  pendingPrompt?: string; // Optional prompt to send after agent initializes
  permissionMode?: "plan" | "acceptEdits"; // Permission mode for Claude terminal
  agent?: AgentKind;
}

export interface ShellTerminalData {
  id: string;
  workingDirectory: string;
}

export type TerminalRefsMap = Map<string, ConsolidatedTerminalHandle | null>;

/**
 * Summary of a single terminal (agent or shell), read by the workspace
 * sidebar's spinner and the idle-agent lookup.
 */
export interface TerminalSessionSummary {
  /** Matches the id used in WorkspaceTerminalPane's terminalOrder ("shell-..." or "agent-<sessionId>"). */
  id: string;
  kind: "agent" | "shell";
  name: string;
  /** null when the terminal belongs to the main repo (not a workspace). */
  branchName: string | null;
  /** Repository of an agent terminal; unset for shells. */
  repoPath?: string;
  isMainRepo: boolean;
  agent?: AgentKind;
  /** True while output is actively streaming (shows a spinner). */
  isStreaming: boolean;
}

/** The id to pass to session APIs together with `repoPath`. */
export const dbSessionIdOf = (session: AgentSessionData): number =>
  session.dbSessionId ?? session.sessionId;
