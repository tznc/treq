import { invoke } from "@tauri-apps/api/core";
import { getRepoSetting, setRepoSetting } from "./api";

export type LinearState = {
  name: string;
  type: string;
};

export type LinearUser = {
  id: string;
  name: string;
};

export type LinearProjectRef = {
  id: string;
  name: string;
};

export type LinearIssue = {
  id: string;
  identifier: string;
  title: string;
  description?: string;
  state: LinearState;
  labels: string[];
  branch_name: string;
  parent_id: string | null;
  sub_issue_ids: string[];
  url: string;
  assignee?: LinearUser | null;
  priority?: number;
  priority_label?: string;
  project?: LinearProjectRef | null;
};

export type LinearTeam = {
  id: string;
  name: string;
  key: string;
};

export type LinearProject = {
  id: string;
  name: string;
  description?: string;
  state: string;
  target_date?: string | null;
  progress: number;
  url: string;
  lead?: LinearUser | null;
};

export type LinearDocument = {
  id: string;
  title: string;
  content?: string | null;
  url: string;
  updated_at: string;
};

export type LinearComment = {
  id: string;
  body: string;
  user?: LinearUser | null;
  created_at: string;
  quoted_text?: string | null;
};

export type LinearKickoffResult = {
  issue_id: string;
  workspace_id: number;
  created: boolean;
};

export const linearListTeams = (repoPath: string): Promise<LinearTeam[]> =>
  invoke("linear_list_teams", { repoPath });

export const linearListIssues = (
  repoPath: string,
  teamFilter?: string,
): Promise<LinearIssue[]> =>
  invoke("linear_list_issues", { repoPath, teamFilter });

/** Opens or creates the workspace for one issue. Sub-issues are separate calls. */
export const linearOpenOrCreateWorkspaceFromIssue = (
  repoPath: string,
  issueId: string,
): Promise<LinearKickoffResult> =>
  invoke("linear_open_or_create_workspace_from_issue", { repoPath, issueId });

export const getLinearApiKey = (repoPath: string): Promise<string | null> =>
  getRepoSetting(repoPath, "linear_api_key");

export const setLinearApiKey = (
  repoPath: string,
  apiKey: string,
): Promise<void> => setRepoSetting(repoPath, "linear_api_key", apiKey);

export const getLinearAutoKickoffLabel = (
  repoPath: string,
): Promise<string | null> =>
  getRepoSetting(repoPath, "linear_auto_kickoff_label");

export const setLinearAutoKickoffLabel = (
  repoPath: string,
  label: string,
): Promise<void> =>
  setRepoSetting(repoPath, "linear_auto_kickoff_label", label);

export const linearStartAutoKickoffPolling = (
  repoPath: string,
): Promise<void> => invoke("linear_start_auto_kickoff_polling", { repoPath });

/**
 * Gives the Rust Linear client the Supabase session it sends to the
 * `linear-proxy` Edge Function; `null` clears it on sign-out. Used by
 * `src/lib/linear-proxy-auth.ts`.
 */
export const linearSetProxySession = (
  supabaseUrl: string | null,
  accessToken: string | null,
): Promise<void> =>
  invoke("linear_set_proxy_session", { supabaseUrl, accessToken });

export const linearGetViewer = (repoPath: string): Promise<LinearUser> =>
  invoke("linear_get_viewer", { repoPath });

export const linearListProjects = (
  repoPath: string,
): Promise<LinearProject[]> => invoke("linear_list_projects", { repoPath });

export const linearListProjectDocuments = (
  repoPath: string,
  projectId: string,
): Promise<LinearDocument[]> =>
  invoke("linear_list_project_documents", { repoPath, projectId });

export const linearListIssueComments = (
  repoPath: string,
  issueId: string,
): Promise<LinearComment[]> =>
  invoke("linear_list_issue_comments", { repoPath, issueId });

export const linearListProjectComments = (
  repoPath: string,
  projectId: string,
): Promise<LinearComment[]> =>
  invoke("linear_list_project_comments", { repoPath, projectId });

export const linearListDocumentComments = (
  repoPath: string,
  documentId: string,
): Promise<LinearComment[]> =>
  invoke("linear_list_document_comments", { repoPath, documentId });

/** What a Linear review covers, as written to the agent's snapshot. */
export type LinearReviewSnapshot = {
  title: string;
  url: string;
  body: string;
  comments: {
    id: string;
    author?: string | null;
    created_at?: string | null;
    quoted_text?: string | null;
    body: string;
  }[];
};

/** Writes the review snapshot and returns its directory. */
export const linearPrepareReview = (
  repoPath: string,
  {
    targetType,
    targetId,
    snapshot,
  }: { targetType: string; targetId: string; snapshot: LinearReviewSnapshot },
): Promise<string> =>
  invoke("linear_prepare_review", {
    repoPath,
    targetType,
    targetId,
    snapshot,
  });

/** Replaces the reviewed passage in Linear with the comment's suggestion. */
export const linearApplyReviewSuggestion = (
  repoPath: string,
  commentId: string,
): Promise<void> =>
  invoke("linear_apply_review_suggestion", { repoPath, commentId });
