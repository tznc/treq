import type { AgentKind } from "./agents";

export interface LogRecordView {
  timestamp: string;
  severity_number: number;
  severity_text: string;
  body: string;
  trace_id: string;
  span_id: string;
  run_id: number;
  job_id: string;
  step_index: number;
  step_name: string;
  stream: string;
}

export interface LogBucket {
  bucket: string;
  severity_text: string;
  count: number;
}
export interface SqlResult {
  columns: string[];
  rows: (string | null)[][];
  row_count: number;
}

export type AgentChatRole = "user" | "agent";

export interface AgentChatMessage {
  id: number;
  role: AgentChatRole;
  message: string;
  time: string;
}

export interface AgentChat {
  session_id: number;
  pty_session_id: string;
  name: string;
  agent: AgentKind;
  workspace_id: number | null;
  created_at: string;
  screen_before_last_user_message: string;
  messages: AgentChatMessage[];
}

export interface AgentChatSummary {
  session_id: number;
  pty_session_id: string;
  name: string;
  agent: string;
  workspace_id: number | null;
  created_at: string;
  message_count: number;
}
