/** Mirrors ConflictComment in changes-diff-viewer/types.ts. */
export interface ConflictCommentRecord {
  id: string;
  conflictId: string;
  filePath: string;
  conflictNumber: number;
  text: string;
  createdAt: string;
}

/** What an agent review targets. Open-ended so other content types can be
 * added later without changing the storage or the UI contract. */
export type AgentReviewTargetType = string;

export const AGENT_REVIEW_TARGET_WORKSPACE_DIFF = "workspace_diff";
export const AGENT_REVIEW_TARGET_FILE_BROWSER_FILE = "file_browser_file";
export const AGENT_REVIEW_TARGET_LINEAR_ISSUE = "linear_issue";
export const AGENT_REVIEW_TARGET_LINEAR_PROJECT = "linear_project";
export const AGENT_REVIEW_TARGET_LINEAR_DOCUMENT = "linear_document";

export type LinearReviewTargetType =
  | typeof AGENT_REVIEW_TARGET_LINEAR_ISSUE
  | typeof AGENT_REVIEW_TARGET_LINEAR_PROJECT
  | typeof AGENT_REVIEW_TARGET_LINEAR_DOCUMENT;

/** Snapshot file a Linear review comment on the entity's own text uses. */
export const LINEAR_REVIEW_BODY_FILE = "body.md";

/** Snapshot file a Linear review comment on one Linear comment uses. */
export const linearReviewCommentFile = (commentId: string) =>
  `comments/${commentId}.md`;

/**
 * A local-only review comment written by a review agent. Never synced to
 * GitHub, and deliberately separate from the read-only `GhReviewThread` /
 * `GhReviewComment` types in api-types.ts so the two can never be confused.
 */
export interface AgentReviewComment {
  id: string;
  repo_path: string;
  target_type: AgentReviewTargetType;
  target_id: string;
  file_path: string;
  hunk_id: string | null;
  start_line: number;
  end_line: number;
  side: "old" | "new" | null;
  comment_text: string;
  /** Raw suggested text, without the ```suggestion fence. */
  suggested_replacement: string | null;
  status: "open" | "resolved";
  /** Where the comment came from; always "local-agent" today. */
  source: string;
  created_at: string;
  resolved_at: string | null;
  /** Exact reviewed text of the line range; set for Linear targets. */
  quoted_text?: string | null;
}
