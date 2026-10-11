export interface Workspace {
  id: number;
  repo_path: string;
  workspace_name: string;
  workspace_path: string;
  branch_name: string;
  created_at: string;
  metadata?: string;
  target_branch?: string | null;
  title: string;
  description?: string | null;
  not_on_remote: boolean;
  sparse_patterns?: string[] | null;
  /** RFC3339. Hidden in the sidebar until this time. */
  /** True when the workspace directory was archived; the DB row is kept. */
  archived?: boolean;
}

type RemoteSyncStatus =
  | { type: "NotOnRemote" }
  | { type: "InSync" }
  | { type: "Ahead"; data: { count: number } }
  | { type: "Behind"; data: { count: number } }
  | { type: "Diverged"; data: { ahead: number; behind: number } };

interface WorkspacePartialStatus {
  current: Workspace;
  has_conflicts: boolean;
  has_changes: boolean;
  commits_ahead: number;
}

export interface WorkspaceSidebarStatus {
  current: Workspace;
  has_conflicts: boolean;
  /** Tip / working-copy activity time (RFC3339). Used for sibling sidebar ordering. */
  last_activity_at?: string | null;
}

interface WorkspaceNode {
  status: WorkspacePartialStatus;
  parent_id: number | null;
  child_ids: number[];
  depth: number;
}

export interface WorkspaceStatus {
  current: Workspace;
  has_conflicts: boolean;
  has_changes: boolean;
  conflicted_files: string[];
  remote_sync: RemoteSyncStatus;
  target: Workspace | null;
  children: Workspace[];
  dag_nodes: WorkspaceNode[];
  conflicted_workspace_ids: number[];
  commits_ahead_of_target: {
    hash: string;
    timestamp: string;
    message: string;
  }[];
}

export interface RepoBranch {
  current_branch: string | null;
  display_ref: string;
  is_detached: boolean;
}

export interface Session {
  id: number;
  workspace_id: number | null;
  name: string;
  created_at: string;
  last_accessed: string;
  model?: string | null;
}

export interface PromptHistoryEntry {
  id: number;
  workspace_id: number | null;
  session_id: number | null;
  prompt_text: string;
  agent: string | null;
  created_at: string;
  /** Display label for the associated workspace (title or branch name), if any. */
  workspace_label: string | null;
}

/** Immutable stashed change set (local gist of file changes). */
export interface StashEntry {
  id: number;
  workspace_id: number | null;
  workspace_label: string;
  commit_id: string;
  change_id: string;
  short_commit_id: string;
  bookmark_name: string;
  created_at: string;
  additions: number;
  deletions: number;
  files_changed: string[];
}

export interface JjDiffHunk {
  id: string;
  header: string;
  lines: string[];
  patch: string;
  conflict_style?: ConflictStyle;
  conflict_regions?: ConflictRegion[];
}

export interface JjFileChange {
  path: string;
  status: string;
  previous_path?: string | null;
  changed_line_count: number;
  diff_deferred: boolean;
}

export interface JjFileLines {
  lines: string[];
  start_line: number;
  end_line: number;
}

export interface JjRebaseResult {
  success: boolean;
  message: string;
}

interface BookmarkConflictCommit {
  commit_id: string;
  short_commit_id: string;
  change_id: string;
  description: string;
  author_name: string;
  timestamp: string;
  diff_summary: string;
}

interface WorkspaceBookmarkConflict {
  workspace_id: number;
  workspace_name: string;
  workspace_path: string;
  branch_name: string;
  bookmark: string;
  commits: BookmarkConflictCommit[];
}

export interface JjLogCommit {
  commit_id: string;
  short_id: string;
  change_id: string;
  description: string;
  author_name: string;
  timestamp: string;
  parent_ids: string[];
  is_working_copy: boolean;
  workspace_label?: string | null;
  bookmarks: string[];
  is_immutable: boolean;
  insertions: number;
  deletions: number;
  /** True when this commit belongs to the target branch but not the current branch */
  on_target_only?: boolean;
  /** True when this commit's tree still contains unresolved jj conflicts */
  has_conflicts?: boolean;
}

interface ResolveTarget {
  change_id: string;
  commit_id: string;
  description: string;
  conflicted_files: string[];
  resolve_path: string;
  workspace_id: number;
  workspace_name: string;
}

export interface ResolveConflictsSession {
  targets: ResolveTarget[];
  /** Absolute path the agent should start in: `.treq/resolve/<workspace-slug>/`. */
  agent_cwd: string;
  source_workspace_id: number | null;
  workspace_slug: string;
}

export interface ResolveCommitResult {
  success: boolean;
  message: string;
  change_id: string;
  remaining_conflicts: string[];
}

export interface JjTentativeWorkingCopy {
  workspace_label: string;
  commit: JjLogCommit;
}

export interface JjLogResult {
  /** Workspace/current-branch commits first, then target-only commits (`on_target_only`). */
  commits: JjLogCommit[];
  tentative_working_copy?: JjTentativeWorkingCopy | null;
  target_branch: string;
  workspace_branch: string;
  /** @deprecated Target-only commits are included in `commits` with `on_target_only = true`. */
  target_branch_commits?: JjLogCommit[];
  /** Commit ID of the common ancestor (merge base) for non-default home-repo branch views */
  merge_base_id?: string | null;
}

export interface HomeRebaseDryRunResult {
  would_conflict: boolean;
  conflicted_files: string[];
}

export interface JjCommitsAhead {
  commits: JjLogCommit[];
  total_count: number;
}

export type MergeStrategy = "merge" | "squash" | "rebase";

export interface JjFileDiff {
  path: string;
  hunks: JjDiffHunk[];
  conflict_style?: ConflictStyle;
  conflict_regions?: ConflictRegion[];
}

export interface JjRevisionDiff {
  committed_files: JjFileChange[];
  hunks_by_file: JjFileDiff[];
  uncommitted_files?: JjFileChange[];
  conflicted_files?: string[];
  too_large_to_render: boolean;
  render_block_reason?: string | null;
}

export interface DirectoryEntry {
  name: string;
  path: string;
  is_directory: boolean;
  modified_at?: string | null;
  submodule_pin?: string | null;
  submodule_synced?: boolean | null;
  /** "conflict" | "workingCopy" | "committed", populated only by lsWorkspaceWithStatus. */
  status?: string | null;
}

export interface FileSearchResult {
  file_path: string;
  relative_path: string;
}

export interface EditorAppsResponse {
  cursor: boolean;
  vscode: boolean;
  zed: boolean;
}

/** Whether one CLI the first-run checklist needs is on PATH. */
export interface PrerequisiteStatus {
  binary: string;
  installed: boolean;
}

export interface JjBranch {
  name: string;
  is_current: boolean;
}

export interface PullWorkspaceResult {
  success: boolean;
  message: string;
  was_diverged: boolean;
  commits_rebased: number;
  /** True when pull/rebase left unresolved conflicts for local resolution. */
  has_conflicts?: boolean;
}

export interface BranchStatus {
  local_exists: boolean;
  remote_exists: boolean;
  remote_name?: string;
  remote_ref?: string;
}

export interface RenameWorkspaceResult {
  success: boolean;
  message: string;
  workspace: Workspace | null;
  updated_children_ids: number[];
}

export interface SingleRebaseResult {
  rebased: boolean;
  success: boolean;
  message: string;
  bookmark_conflicts?: WorkspaceBookmarkConflict[];
}

export interface DiffCacheEntry {
  data: string;
  timestamp: number;
}

export interface LineComment {
  id: string;
  file_path: string;
  hunk_id: string;
  start_line: number;
  end_line: number;
  line_content: string[];
  text: string;
  created_at: string;
  line_side?: "old" | "new";
  /** Set when this comment was seeded by quoting a GitHub review comment. */
  source?: "github";
  github_author?: string;
  github_avatar_url?: string;
  github_comment_url?: string;
}

export interface PendingReview {
  id: number;
  workspace_id: number;
  comments: LineComment[];
  viewed_files: string[];
  summary_text: string | null;
  conflict_comments: import("./api-types-review").ConflictCommentRecord[];
  created_at: string;
  updated_at: string;
}

/** Pending review session for the FileBrowser, kept separate from PendingReview. */
export interface FileBrowserPendingReview {
  id: number;
  workspace_id: number;
  comments: LineComment[];
  summary_text: string | null;
  created_at: string;
  updated_at: string;
}

export interface GitRemoteInfo {
  owner: string;
  repo: string;
  full_name: string;
}

export * from "./api-types-github";

export interface PrInfo {
  number: number;
  title: string;
  /** "OPEN" | "CLOSED" | "MERGED" */
  state: string;
  url: string;
  head_ref_name: string;
  base_ref_name: string;
  merge_state_status: string | null;
  is_draft?: boolean;
}

export interface PrCheckEntry {
  name: string;
  /** "pass" | "fail" | "pending" | "skipping" | "cancel" */
  bucket: string;
  link: string;
  /** Elapsed seconds for the check run, when timestamps are available. */
  duration_secs?: number | null;
}

export interface PrCiStatus {
  /** "success" | "failure" | "pending" */
  state: string;
  total: number;
  passed: number;
  failed: number;
  pending: number;
  checks: PrCheckEntry[];
}

/** Mirrors the merge_queue_entry_status enum in 003_merge_queue.sql. */
export type QueueEntryStatus =
  | "queued"
  | "testing"
  | "merging"
  | "merged"
  | "failed"
  | "dequeued";

export interface WorkspaceQueueStatus {
  entry_id: string;
  pr_number: number;
  status: QueueEntryStatus;
  position: number;
  target_branch: string;
  lane_number: number | null;
  ci_run_status: string | null;
  failure_reason: string | null;
  enqueued_at: string;
  merged_at: string | null;
}

export type ConflictStyle =
  | "jj_diff"
  | "jj_snapshot"
  | "jj_git_diff3"
  | "git_merge"
  | "git_diff3"
  | "unknown";

type ConflictLineKind = "marker" | "content";
export type ConflictLineRole =
  | "start"
  | "base"
  | "separator"
  | "end"
  | "left"
  | "right"
  | "unknown";

interface ConflictLineView {
  raw: string;
  kind: ConflictLineKind;
  role: ConflictLineRole;
  file_line: number;
}

interface ConflictComparisonView {
  left_line_indexes: number[];
  base_line_indexes: number[];
  right_line_indexes: number[];
}

export interface ConflictRegion {
  id: string;
  file_path: string;
  conflict_number: number;
  total_conflicts: number;
  start_line: number;
  end_line: number;
  content: string;
  marker_style: ConflictStyle;
  lines: ConflictLineView[];
  line_map: number[];
  comparison: ConflictComparisonView;
}

export * from "./api-types-checks";
export * from "./api-types-remote";
