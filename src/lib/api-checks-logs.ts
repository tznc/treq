import { invoke } from "@tauri-apps/api/core";
import type {
  AgentChat,
  AgentChatSummary,
  LogBucket,
  LogRecordView,
  SqlResult,
} from "./api-types";
import { assertLocalOperation } from "./repository-adapter";
import { invalidateQueries, setQueryData } from "./swr-cache";

// Checks / logs API

const CHECKS_AND_LOGS = "Checks and logs";

export const getRepoLogs = async (
  repoPath: string,
  options?: {
    levels?: string[];
    search?: string;
    limit?: number;
    offset?: number;
  },
): Promise<LogRecordView[]> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("get_repo_logs", {
    repoPath,
    levels: options?.levels ?? null,
    search: options?.search ?? null,
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
  options?: { levels?: string[]; search?: string; bucketSeconds?: number },
): Promise<LogBucket[]> => {
  assertLocalOperation(repoPath, CHECKS_AND_LOGS);
  return invoke("get_log_timeseries", {
    repoPath,
    levels: options?.levels ?? null,
    search: options?.search ?? null,
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
