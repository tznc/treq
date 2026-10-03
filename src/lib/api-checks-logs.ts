import { invoke } from "@tauri-apps/api/core";
import type {
  AgentChat,
  AgentChatSummary,
  JobResult,
  LogBucket,
  LogRecordView,
  RunSummary,
  SetupScriptStatus,
  SqlResult,
  WorkflowInfo,
} from "./api-types";
import { invalidateQueries, setQueryData } from "./swr-cache";

// Checks / logs API

export const listWorkflows = (repoPath: string): Promise<WorkflowInfo[]> =>
  invoke("list_workflows", { repoPath });

export const runWorkflowJob = (
  repoPath: string,
  filename: string,
  jobId: string,
  workspaceId: number,
  workspacePath: string,
): Promise<JobResult> =>
  invoke("run_workflow_job", {
    repoPath,
    filename,
    jobId,
    workspaceId,
    workspacePath,
  });

export const runWorkflow = (
  repoPath: string,
  filename: string,
  workspaceId: number,
  workspacePath: string,
): Promise<JobResult[]> =>
  invoke("run_workflow", { repoPath, filename, workspaceId, workspacePath });

export const isRepoTrusted = (repoPath: string): Promise<boolean> =>
  invoke("is_repo_trusted", { repoPath });

export const trustRepo = (repoPath: string): Promise<void> =>
  invoke("trust_repo", { repoPath });

export const listWorkflowRuns = (
  repoPath: string,
  workspaceId: number,
  filename: string,
  limit?: number,
): Promise<RunSummary[]> =>
  invoke("list_workflow_runs", {
    repoPath,
    workspaceId,
    filename,
    limit: limit ?? null,
  });

export const getWorkspaceSetupStatus = (
  repoPath: string,
  workspaceId: number,
): Promise<SetupScriptStatus> =>
  invoke("get_workspace_setup_status", { repoPath, workspaceId });

export const rerunWorkspaceSetupScript = (
  repoPath: string,
  workspaceId: number,
  workspacePath: string,
): Promise<JobResult> =>
  invoke("rerun_workspace_setup_script", {
    repoPath,
    workspaceId,
    workspacePath,
  });

export const getRepoLogs = (
  repoPath: string,
  options?: {
    levels?: string[];
    search?: string;
    limit?: number;
    offset?: number;
  },
): Promise<LogRecordView[]> =>
  invoke("get_repo_logs", {
    repoPath,
    levels: options?.levels ?? null,
    search: options?.search ?? null,
    limit: options?.limit ?? null,
    offset: options?.offset ?? null,
  });

/** Output of every application setup script run, newest run first. */
export const getAppSetupLogs = (options?: {
  levels?: string[];
  search?: string;
}): Promise<LogRecordView[]> =>
  invoke("get_app_setup_logs", {
    levels: options?.levels ?? null,
    search: options?.search ?? null,
  });

export const getLogTimeseries = (
  repoPath: string,
  options?: { levels?: string[]; search?: string; bucketSeconds?: number },
): Promise<LogBucket[]> =>
  invoke("get_log_timeseries", {
    repoPath,
    levels: options?.levels ?? null,
    search: options?.search ?? null,
    bucketSeconds: options?.bucketSeconds ?? null,
  });

export const runLogsSql = (
  repoPath: string,
  sql: string,
  maxRows?: number,
): Promise<SqlResult> =>
  invoke("run_logs_sql", { repoPath, sql, maxRows: maxRows ?? null });

export const registerAgentChat = (
  repoPath: string,
  sessionId: number,
  ptySessionId: string,
  name: string,
  agent: string,
  workspaceId: number | null,
  initialPrompt?: string,
): Promise<AgentChat> =>
  invoke("register_agent_chat", {
    repoPath,
    sessionId,
    ptySessionId,
    name,
    agent,
    workspaceId,
    initialPrompt: initialPrompt ?? null,
  }).then((chat) => cacheAgentChat(repoPath, chat as AgentChat));

export const recordAgentChatUserMessage = (
  repoPath: string,
  sessionId: number,
  screenBefore: string,
  text: string,
): Promise<AgentChat> =>
  invoke("record_agent_chat_user_message", {
    repoPath,
    sessionId,
    screenBefore,
    text,
  }).then((chat) => cacheAgentChat(repoPath, chat as AgentChat));

export const recordAgentChatScreen = (
  repoPath: string,
  sessionId: number,
  screen: string,
): Promise<AgentChat> =>
  invoke("record_agent_chat_screen", {
    repoPath,
    sessionId,
    screen,
  }).then((chat) => cacheAgentChat(repoPath, chat as AgentChat));

async function cacheAgentChat(
  repoPath: string,
  chat: AgentChat,
): Promise<AgentChat> {
  await Promise.all([
    setQueryData(["agent-chat", repoPath, chat.session_id], chat),
    invalidateQueries(["agent-chats", repoPath]),
  ]);
  return chat;
}

export const listAgentChats = (repoPath: string): Promise<AgentChatSummary[]> =>
  invoke("list_agent_chats", { repoPath });

export const getAgentChat = (
  repoPath: string,
  sessionId: number,
): Promise<AgentChat | null> =>
  invoke("get_agent_chat", { repoPath, sessionId });
