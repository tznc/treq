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
import { assertLocalOperation } from "./repository-adapter";
import { invalidateQueries, setQueryData } from "./swr-cache";

// Checks / logs API

const CHECKS_AND_LOGS = "Checks and logs";

export const listWorkflows = async (
  repoPath: string,
): Promise<WorkflowInfo[]> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("list_workflows", { repoPath });
};

export const runWorkflowJob = async (
  repoPath: string,
  filename: string,
  jobId: string,
  workspaceId: number,
  workspacePath: string,
): Promise<JobResult> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("run_workflow_job", {
    repoPath,
    filename,
    jobId,
    workspaceId,
    workspacePath,
  });
};

export const runWorkflow = async (
  repoPath: string,
  filename: string,
  workspaceId: number,
  workspacePath: string,
): Promise<JobResult[]> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("run_workflow", {
    repoPath,
    filename,
    workspaceId,
    workspacePath,
  });
};

export const isRepoTrusted = async (repoPath: string): Promise<boolean> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("is_repo_trusted", { repoPath });
};

export const trustRepo = async (repoPath: string): Promise<void> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("trust_repo", { repoPath });
};

export const listWorkflowRuns = async (
  repoPath: string,
  workspaceId: number,
  filename: string,
  limit?: number,
): Promise<RunSummary[]> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("list_workflow_runs", {
    repoPath,
    workspaceId,
    filename,
    limit: limit ?? null,
  });
};

export const getWorkspaceSetupStatus = async (
  repoPath: string,
  workspaceId: number,
): Promise<SetupScriptStatus> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("get_workspace_setup_status", { repoPath, workspaceId });
};

export const rerunWorkspaceSetupScript = async (
  repoPath: string,
  workspaceId: number,
  workspacePath: string,
): Promise<JobResult> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("rerun_workspace_setup_script", {
    repoPath,
    workspaceId,
    workspacePath,
  });
};

export const getRepoLogs = async (
  repoPath: string,
  options?: {
    levels?: string[];
    search?: string;
    /** Workflow filenames; keeps logs from the runs of any of them. Empty means all. */
    workflows?: string[];
    limit?: number;
    offset?: number;
  },
): Promise<LogRecordView[]> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("get_repo_logs", {
    repoPath,
    levels: options?.levels ?? null,
    search: options?.search ?? null,
    workflows: options?.workflows ?? null,
    limit: options?.limit ?? null,
    offset: options?.offset ?? null,
  });
};

/** Output of every application setup script run, newest run first. */
export const getAppSetupLogs = (options?: {
  levels?: string[];
  search?: string;
}): Promise<LogRecordView[]> =>
  invoke("get_app_setup_logs", {
    levels: options?.levels ?? null,
    search: options?.search ?? null,
  });

export const getLogTimeseries = async (
  repoPath: string,
  options?: {
    levels?: string[];
    search?: string;
    workflows?: string[];
    bucketSeconds?: number;
  },
): Promise<LogBucket[]> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("get_log_timeseries", {
    repoPath,
    levels: options?.levels ?? null,
    search: options?.search ?? null,
    workflows: options?.workflows ?? null,
    bucketSeconds: options?.bucketSeconds ?? null,
  });
};

export const runLogsSql = async (
  repoPath: string,
  sql: string,
  maxRows?: number,
): Promise<SqlResult> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("run_logs_sql", { repoPath, sql, maxRows: maxRows ?? null });
};

// Agent chat logs are files under the repository's `.treq` directory.
const AGENT_CHAT_LOGS = "Agent chat logs";

export const registerAgentChat = async (
  repoPath: string,
  sessionId: number,
  ptySessionId: string,
  name: string,
  agent: string,
  workspaceId: number | null,
  initialPrompt?: string,
): Promise<AgentChat> => {
  assertLocalOperation(repoPath, AGENT_CHAT_LOGS);
  return invoke("register_agent_chat", {
    repoPath,
    sessionId,
    ptySessionId,
    name,
    agent,
    workspaceId,
    initialPrompt: initialPrompt ?? null,
  }).then((chat) => cacheAgentChat(repoPath, chat as AgentChat));
};

export const recordAgentChatUserMessage = async (
  repoPath: string,
  sessionId: number,
  screenBefore: string,
  text: string,
): Promise<AgentChat> => {
  assertLocalOperation(repoPath, AGENT_CHAT_LOGS);
  return invoke("record_agent_chat_user_message", {
    repoPath,
    sessionId,
    screenBefore,
    text,
  }).then((chat) => cacheAgentChat(repoPath, chat as AgentChat));
};

export const recordAgentChatScreen = async (
  repoPath: string,
  sessionId: number,
  screen: string,
): Promise<AgentChat> => {
  assertLocalOperation(repoPath, AGENT_CHAT_LOGS);
  return invoke("record_agent_chat_screen", {
    repoPath,
    sessionId,
    screen,
  }).then((chat) => cacheAgentChat(repoPath, chat as AgentChat));
};

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

export const listAgentChats = async (
  repoPath: string,
): Promise<AgentChatSummary[]> => {
  assertLocalOperation(repoPath, AGENT_CHAT_LOGS);
  return invoke("list_agent_chats", { repoPath });
};

export const getAgentChat = async (
  repoPath: string,
  sessionId: number,
): Promise<AgentChat | null> => {
  assertLocalOperation(repoPath, AGENT_CHAT_LOGS);
  return invoke("get_agent_chat", { repoPath, sessionId });
};
