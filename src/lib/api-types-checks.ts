import type { AgentKind } from "./agents";

export interface WorkflowStepInfo {
  name: string;
}
export interface WorkflowJobInfo {
  id: string;
  name: string;
  steps: WorkflowStepInfo[];
}
export interface WorkflowInfo {
  filename: string;
  name: string;
  jobs: WorkflowJobInfo[];
}
export interface StepResult {
  name: string;
  success: boolean;
}
export interface JobResult {
  job_id: string;
  steps: StepResult[];
  success: boolean;
}

export interface RunJobSummary {
  job_id: string;
  status: string;
  steps: StepResult[];
  has_logs: boolean;
}
export interface RunSummary {
  id: number;
  filename: string;
  status: string;
  started_at: string;
  completed_at: string | null;
  jobs: RunJobSummary[];
}
export interface SetupScriptStatus {
  configured: boolean;
  /** "not_configured" | "pending" | "running" | "passed" | "failed" */
  status: string;
  run_id: number | null;
}

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

type AgentChatRole = "user" | "agent";

interface AgentChatMessage {
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
