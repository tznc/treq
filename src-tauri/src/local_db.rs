use crate::lock_ext::LockExt;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::agent_dispatch::{InstanceWindowSnapshot, RegisteredInstance};
use crate::jj::DiscoveredWorkspace;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Workspace {
  pub id: i64,
  pub repo_path: String,
  pub workspace_name: String,
  pub workspace_path: String,
  pub branch_name: String,
  pub created_at: String,
  pub refreshed_at: Option<String>,
  pub metadata: Option<String>,
  pub target_branch: Option<String>,
  pub title: String,
  pub description: Option<String>,
  pub moved_files: Option<Vec<String>>,
  pub not_on_remote: bool,
  pub sparse_patterns: Option<Vec<String>>,
  /// RFC3339 timestamp. The workspace is hidden in the sidebar until this time.
  #[serde(default)]
  pub hidden_until: Option<String>,
  /// True when the workspace directory was archived; the DB row is kept.
  #[serde(default)]
  pub archived: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Session {
  pub id: i64,
  pub workspace_id: Option<i64>,
  pub name: String,
  pub created_at: String,
  pub last_accessed: String,
  pub model: Option<String>,
}

static INITIALIZED_DBS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn workspace_from_row(repo_path: &str, row: &Row<'_>) -> rusqlite::Result<Workspace> {
  let moved_files_json: Option<String> = row.get(10)?;
  let moved_files =
    moved_files_json.and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok());
  let sparse_patterns_json: Option<String> = row.get(12)?;
  let sparse_patterns =
    sparse_patterns_json.and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok());
  Ok(Workspace {
    id: row.get(0)?,
    repo_path: repo_path.to_string(),
    workspace_name: row.get(1)?,
    workspace_path: row.get(2)?,
    branch_name: row.get(3)?,
    created_at: row.get(4)?,
    refreshed_at: row.get(5)?,
    metadata: row.get(6)?,
    target_branch: row.get(7)?,
    not_on_remote: row.get::<_, i64>(8)? != 0,
    title: row.get(9)?,
    description: row.get(11)?,
    moved_files,
    sparse_patterns,
    hidden_until: row.get(13)?,
    archived: row.get::<_, i64>(14)? != 0,
  })
}

/// Cached file information for workspace file indexing
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CachedWorkspaceFile {
  pub id: i64,
  pub workspace_id: Option<i64>,
  pub file_path: String,
  pub relative_path: String,
  pub is_directory: bool,
  pub parent_path: Option<String>,
  pub cached_at: String,
  /// File modification time (unix timestamp)
  pub mtime: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PendingReview {
  pub id: i64,
  pub workspace_id: i64,
  pub comments: String,             // JSON string
  pub viewed_files: Option<String>, // JSON string
  pub summary_text: Option<String>,
  pub conflict_comments: Option<String>, // JSON string
  pub created_at: String,
  pub updated_at: String,
}

/// A local-only review comment anchored to a line range of some reviewed
/// target. These are never pushed to GitHub and are structurally distinct from
/// the read-only `GhReviewThread`/`GhReviewComment` types on the frontend.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentReviewComment {
  pub id: String,
  pub repo_path: String,
  /// What was reviewed, e.g. `"workspace_diff"` or `"file_browser_file"`.
  pub target_type: String,
  /// Identifier within `target_type`, e.g. a workspace id or an absolute path.
  pub target_id: String,
  pub file_path: String,
  pub hunk_id: Option<String>,
  pub start_line: i64,
  pub end_line: i64,
  /// `"old"` or `"new"`, matching the frontend's `LineComment.line_side`.
  pub side: Option<String>,
  pub comment_text: String,
  /// Raw suggested text, without the ```suggestion fence.
  pub suggested_replacement: Option<String>,
  /// `"open"` or `"resolved"`.
  pub status: String,
  /// Where the comment came from; always `"local-agent"` today.
  pub source: String,
  pub created_at: String,
  pub resolved_at: Option<String>,
}

/// Pending review session for the FileBrowser, kept separate from `PendingReview`
/// (the Review/Changes-tab session) — see `file_browser_reviews` table.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileBrowserPendingReview {
  pub id: i64,
  pub workspace_id: i64,
  pub comments: String, // JSON string
  pub summary_text: Option<String>,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PromptHistoryEntry {
  pub id: i64,
  pub workspace_id: Option<i64>,
  pub session_id: Option<i64>,
  pub prompt_text: String,
  pub agent: Option<String>,
  pub created_at: String,
  /// Display label for the associated workspace (title or branch name), if any.
  pub workspace_label: Option<String>,
}

/// An immutable stashed change set (local gist of file changes).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StashEntry {
  pub id: i64,
  pub workspace_id: Option<i64>,
  /// Workspace title/branch at stash time (or "Home repo").
  pub workspace_label: String,
  pub commit_id: String,
  pub change_id: String,
  pub short_commit_id: String,
  pub bookmark_name: String,
  pub created_at: String,
  pub additions: u32,
  pub deletions: u32,
  pub files_changed: Vec<String>,
}

/// A pending in-app-browser page review: element comments left while
/// reviewing a live page, plus an overall summary, before it's sent to an
/// agent terminal. One row per workspace, mirroring `PendingReview`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PendingPageReview {
  pub id: i64,
  pub workspace_id: i64,
  pub url: String,
  pub comments: String, // JSON string
  pub summary_text: Option<String>,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CachedCommitDiffStat {
  pub commit_id: String,
  pub insertions: u32,
  pub deletions: u32,
  pub updated_at: String,
  pub change_id: Option<String>,
  pub description: Option<String>,
  pub author_name: Option<String>,
  pub timestamp: Option<String>,
}

pub fn get_local_db_path(repo_path: &str) -> PathBuf {
  Path::new(repo_path).join(".treq").join("local.db")
}

/// Initialize the local database for a repository.
///
/// Creates tables for workspaces, sessions, changed_files, and workspace_files.
/// Handles schema migrations for backward compatibility.
///
/// # Returns
/// The path to the created/opened database file.
pub fn init_local_db(repo_path: &str) -> Result<PathBuf, String> {
  let db_path = get_local_db_path(repo_path);
  if let Some(parent) = db_path.parent() {
    fs::create_dir_all(parent).map_err(|e| format!("Failed to create .treq directory: {}", e))?;
  }

  let conn = Connection::open(&db_path).map_err(|e| format!("Failed to open local db: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS workspaces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_name TEXT NOT NULL,
            workspace_path TEXT NOT NULL UNIQUE,
            branch_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            metadata TEXT,
            target_branch TEXT
        )",
      [],
    )
    .map_err(|e| format!("Failed to create workspaces table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_workspaces_branch ON workspaces(branch_name)",
      [],
    )
    .map_err(|e| format!("Failed to create workspaces branch index: {}", e))?;

  let _ = conn.execute("ALTER TABLE workspaces ADD COLUMN target_branch TEXT", []);
  let _ = conn.execute(
    "ALTER TABLE workspaces ADD COLUMN has_conflicts BOOLEAN DEFAULT 0",
    [],
  );
  let _ = conn.execute(
    "ALTER TABLE workspaces ADD COLUMN archived BOOLEAN DEFAULT 0",
    [],
  );
  let _ = conn.execute(
    "ALTER TABLE workspaces ADD COLUMN not_on_remote BOOLEAN DEFAULT 0",
    [],
  );
  let _ = conn.execute("ALTER TABLE workspaces ADD COLUMN title TEXT", []);
  let _ = conn.execute("ALTER TABLE workspaces ADD COLUMN description TEXT", []);
  let _ = conn.execute("ALTER TABLE workspaces ADD COLUMN moved_files TEXT", []);
  let _ = conn.execute("ALTER TABLE workspaces ADD COLUMN refreshed_at TEXT", []);
  let _ = conn.execute("ALTER TABLE workspaces ADD COLUMN sparse_patterns TEXT", []);
  let _ = conn.execute("ALTER TABLE workspaces ADD COLUMN hidden_until TEXT", []);

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            last_accessed TEXT NOT NULL,
            model TEXT,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        )",
      [],
    )
    .map_err(|e| format!("Failed to create sessions table: {}", e))?;

  let has_worktree_col: Result<i64, _> = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name='worktree_id'",
    [],
    |row| row.get(0),
  );

  if let Ok(count) = has_worktree_col {
    if count > 0 {
      conn
        .execute(
          "CREATE TABLE sessions_new (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workspace_id INTEGER,
                    name TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    last_accessed TEXT NOT NULL,
                    model TEXT,
                    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
                )",
          [],
        )
        .map_err(|e| format!("Failed to create sessions_new table: {}", e))?;

      conn.execute(
                "INSERT INTO sessions_new SELECT id, worktree_id, name, created_at, last_accessed, model FROM sessions",
                [],
            )
            .map_err(|e| format!("Failed to migrate sessions data: {}", e))?;

      conn
        .execute("DROP TABLE sessions", [])
        .map_err(|e| format!("Failed to drop old sessions table: {}", e))?;

      conn
        .execute("ALTER TABLE sessions_new RENAME TO sessions", [])
        .map_err(|e| format!("Failed to rename sessions_new to sessions: {}", e))?;
    }
  }

  let _ = conn.execute("ALTER TABLE sessions ADD COLUMN model TEXT", []);

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_sessions_workspace ON sessions(workspace_id)",
      [],
    )
    .map_err(|e| format!("Failed to create sessions workspace index: {}", e))?;

  let _ = conn.execute("DROP TABLE IF EXISTS git_file_hunks", []);
  let _ = conn.execute("DROP TABLE IF EXISTS git_changed_files", []);
  let _ = conn.execute("DROP INDEX IF EXISTS idx_git_file_hunks_workspace", []);
  let _ = conn.execute("DROP INDEX IF EXISTS idx_git_changed_files_workspace", []);

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS changed_files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            file_path TEXT NOT NULL,
            workspace_status TEXT,
            is_untracked INTEGER NOT NULL DEFAULT 0,
            hunks_json TEXT,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
            UNIQUE(workspace_id, file_path)
        )",
      [],
    )
    .map_err(|e| format!("Failed to create changed_files table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_changed_files_workspace ON changed_files(workspace_id)",
      [],
    )
    .map_err(|e| format!("Failed to create changed_files workspace index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS workspace_files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            file_path TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            is_directory INTEGER NOT NULL DEFAULT 0,
            parent_path TEXT,
            cached_at TEXT NOT NULL,
            mtime INTEGER,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
            UNIQUE(workspace_id, file_path)
        )",
      [],
    )
    .map_err(|e| format!("Failed to create workspace_files table: {}", e))?;

  let _ = conn.execute("ALTER TABLE workspace_files ADD COLUMN mtime INTEGER", []);

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_workspace_files_workspace ON workspace_files(workspace_id)",
      [],
    )
    .map_err(|e| format!("Failed to create workspace_files workspace index: {}", e))?;

  conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_files_parent ON workspace_files(workspace_id, parent_path)",
        [],
    )
    .map_err(|e| format!("Failed to create workspace_files parent index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS pending_reviews (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER NOT NULL UNIQUE,
            comments TEXT NOT NULL,
            viewed_files TEXT,
            summary_text TEXT,
            conflict_comments TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        )",
      [],
    )
    .map_err(|e| format!("Failed to create pending_reviews table: {}", e))?;

  // Migration: add conflict_comments column to pending_reviews if missing
  // (covers DBs created before conflict comments were persisted).
  let has_conflict_comments_column: Result<i64, _> = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('pending_reviews') WHERE name = 'conflict_comments'",
    [],
    |row| row.get(0),
  );
  if matches!(has_conflict_comments_column, Ok(0)) {
    conn
      .execute(
        "ALTER TABLE pending_reviews ADD COLUMN conflict_comments TEXT",
        [],
      )
      .map_err(|e| format!("Failed to add conflict_comments column: {}", e))?;
  }

  // Drop any prior schema of commit_diff_stats — keyed cache, safe to recreate.
  let needs_rebuild: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('commit_diff_stats') WHERE name IN ('tree_key', 'parent_ids')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
  if needs_rebuild {
    let _ = conn.execute("DROP TABLE IF EXISTS commit_diff_stats", []);
  }

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS commit_diff_stats (
            commit_id TEXT NOT NULL PRIMARY KEY,
            insertions INTEGER NOT NULL,
            deletions INTEGER NOT NULL,
            updated_at TEXT NOT NULL,
            change_id TEXT,
            description TEXT,
            author_name TEXT,
            timestamp TEXT
        )",
      [],
    )
    .map_err(|e| format!("Failed to create commit_diff_stats table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_pending_reviews_workspace ON pending_reviews(workspace_id)",
      [],
    )
    .map_err(|e| format!("Failed to create pending_reviews workspace index: {}", e))?;

  // Separate pending-review session for the FileBrowser's "review any file" flow —
  // intentionally not shared with pending_reviews (the Review/Changes-tab session).
  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS file_browser_reviews (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER NOT NULL UNIQUE,
            comments TEXT NOT NULL,
            summary_text TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        )",
      [],
    )
    .map_err(|e| format!("Failed to create file_browser_reviews table: {}", e))?;

  conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_file_browser_reviews_workspace ON file_browser_reviews(workspace_id)",
        [],
    )
    .map_err(|e| format!("Failed to create file_browser_reviews workspace index: {}", e))?;

  // Local-only review comments left by a review agent. Deliberately never
  // synced to GitHub — `source` records where a row came from so other local
  // producers can be added later, and `target_type`/`target_id` keep the table
  // generic over what was reviewed (a workspace diff, a FileBrowser file, and
  // later other content types).
  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS agent_review_comments (
            id TEXT PRIMARY KEY,
            repo_path TEXT NOT NULL,
            target_type TEXT NOT NULL,
            target_id TEXT NOT NULL,
            file_path TEXT NOT NULL,
            hunk_id TEXT,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            side TEXT,
            comment_text TEXT NOT NULL,
            suggested_replacement TEXT,
            status TEXT NOT NULL,
            source TEXT NOT NULL,
            created_at TEXT NOT NULL,
            resolved_at TEXT
        )",
      [],
    )
    .map_err(|e| format!("Failed to create agent_review_comments table: {}", e))?;

  // Migration: add `source` to agent_review_comments if missing (covers DBs
  // created before other local comment producers were anticipated).
  let has_agent_review_source_column: Result<i64, _> = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('agent_review_comments') WHERE name = 'source'",
    [],
    |row| row.get(0),
  );
  if matches!(has_agent_review_source_column, Ok(0)) {
    conn
      .execute(
        "ALTER TABLE agent_review_comments ADD COLUMN source TEXT NOT NULL DEFAULT 'local-agent'",
        [],
      )
      .map_err(|e| format!("Failed to add agent_review_comments source column: {}", e))?;
  }

  conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_review_comments_target ON agent_review_comments(repo_path, target_type, target_id)",
        [],
    )
    .map_err(|e| format!("Failed to create agent_review_comments target index: {}", e))?;

  // Separate pending-review session for the in-app browser's page-review flow —
  // one row per workspace, keyed independently of pending_reviews/file_browser_reviews.
  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS pending_page_reviews (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER NOT NULL UNIQUE,
            url TEXT NOT NULL,
            comments TEXT NOT NULL,
            summary_text TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        )",
      [],
    )
    .map_err(|e| format!("Failed to create pending_page_reviews table: {}", e))?;

  conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_pending_page_reviews_workspace ON pending_page_reviews(workspace_id)",
        [],
    )
    .map_err(|e| format!("Failed to create pending_page_reviews workspace index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS instance_registry (
            instance_id TEXT NOT NULL PRIMARY KEY,
            pid INTEGER NOT NULL,
            started_at INTEGER NOT NULL,
            last_heartbeat_at INTEGER NOT NULL,
            endpoint TEXT NOT NULL,
            windows_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
      [],
    )
    .map_err(|e| format!("Failed to create instance_registry table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_instance_registry_last_heartbeat_at
         ON instance_registry(last_heartbeat_at)",
      [],
    )
    .map_err(|e| format!("Failed to create instance_registry heartbeat index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS prompt_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            session_id INTEGER,
            prompt_text TEXT NOT NULL,
            agent TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )",
      [],
    )
    .map_err(|e| format!("Failed to create prompt_history table: {}", e))?;

  let prompt_history_foreign_keys_without_cascade: i64 = conn
    .query_row(
      "SELECT COUNT(*) FROM pragma_foreign_key_list('prompt_history') WHERE on_delete != 'CASCADE'",
      [],
      |row| row.get(0),
    )
    .map_err(|e| format!("Failed to inspect prompt_history foreign keys: {}", e))?;
  if prompt_history_foreign_keys_without_cascade > 0 {
    conn
      .execute_batch(
        "CREATE TABLE prompt_history_new (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              workspace_id INTEGER,
              session_id INTEGER,
              prompt_text TEXT NOT NULL,
              agent TEXT,
              created_at TEXT NOT NULL,
              FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
              FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
          );
          INSERT INTO prompt_history_new
            (id, workspace_id, session_id, prompt_text, agent, created_at)
            SELECT id, workspace_id, session_id, prompt_text, agent, created_at
            FROM prompt_history;
          DROP TABLE prompt_history;
          ALTER TABLE prompt_history_new RENAME TO prompt_history;",
      )
      .map_err(|e| format!("Failed to migrate prompt_history cascade deletes: {}", e))?;
  }

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_prompt_history_workspace ON prompt_history(workspace_id)",
      [],
    )
    .map_err(|e| format!("Failed to create prompt_history workspace index: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_prompt_history_created_at ON prompt_history(created_at)",
      [],
    )
    .map_err(|e| format!("Failed to create prompt_history created_at index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS stashes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            workspace_label TEXT NOT NULL,
            commit_id TEXT NOT NULL,
            change_id TEXT NOT NULL,
            short_commit_id TEXT NOT NULL,
            bookmark_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            additions INTEGER NOT NULL DEFAULT 0,
            deletions INTEGER NOT NULL DEFAULT 0,
            files_changed TEXT NOT NULL DEFAULT '[]',
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
        )",
      [],
    )
    .map_err(|e| format!("Failed to create stashes table: {}", e))?;

  let stash_foreign_keys_without_set_null: i64 = conn
    .query_row(
      "SELECT COUNT(*) FROM pragma_foreign_key_list('stashes')
       WHERE \"from\" = 'workspace_id' AND on_delete != 'SET NULL'",
      [],
      |row| row.get(0),
    )
    .map_err(|e| format!("Failed to inspect stashes foreign keys: {}", e))?;
  if stash_foreign_keys_without_set_null > 0 {
    conn
      .execute_batch(
        "BEGIN;
         CREATE TABLE stashes_new (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           workspace_id INTEGER,
           workspace_label TEXT NOT NULL,
           commit_id TEXT NOT NULL,
           change_id TEXT NOT NULL,
           short_commit_id TEXT NOT NULL,
           bookmark_name TEXT NOT NULL,
           created_at TEXT NOT NULL,
           additions INTEGER NOT NULL DEFAULT 0,
           deletions INTEGER NOT NULL DEFAULT 0,
           files_changed TEXT NOT NULL DEFAULT '[]',
           FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
         );
         INSERT INTO stashes_new
           (id, workspace_id, workspace_label, commit_id, change_id, short_commit_id,
            bookmark_name, created_at, additions, deletions, files_changed)
           SELECT id, workspace_id, workspace_label, commit_id, change_id, short_commit_id,
                  bookmark_name, created_at, additions, deletions, files_changed
           FROM stashes;
         DROP TABLE stashes;
         ALTER TABLE stashes_new RENAME TO stashes;
         COMMIT;",
      )
      .map_err(|e| format!("Failed to migrate stashes delete behavior: {}", e))?;
  }

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_stashes_created_at ON stashes(created_at)",
      [],
    )
    .map_err(|e| format!("Failed to create stashes created_at index: {}", e))?;

  // Migration: replace the old per-job workflow_runs shape with per-invocation.
  let has_legacy_workflow_runs: i64 = conn
    .query_row(
      "SELECT COUNT(*) FROM pragma_table_info('workflow_runs') WHERE name = 'job_id'",
      [],
      |row| row.get(0),
    )
    .unwrap_or(0);
  if has_legacy_workflow_runs > 0 {
    conn
      .execute("DROP TABLE workflow_runs", [])
      .map_err(|e| format!("Failed to drop legacy workflow_runs table: {}", e))?;
  }

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS workflow_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            filename TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL,
            completed_at TEXT
        )",
      [],
    )
    .map_err(|e| format!("Failed to create workflow_runs table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_workflow_runs_lookup
         ON workflow_runs(workspace_id, filename, id DESC)",
      [],
    )
    .map_err(|e| format!("Failed to create workflow_runs index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS workflow_job_results (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id INTEGER NOT NULL,
            job_id TEXT NOT NULL,
            position INTEGER NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT,
            completed_at TEXT,
            steps_json TEXT NOT NULL,
            log_path TEXT
        )",
      [],
    )
    .map_err(|e| format!("Failed to create workflow_job_results table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_workflow_job_results_run
         ON workflow_job_results(run_id, position)",
      [],
    )
    .map_err(|e| format!("Failed to create workflow_job_results index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS repo_trust (
            id INTEGER PRIMARY KEY,
            trusted_at TEXT NOT NULL
        )",
      [],
    )
    .map_err(|e| format!("Failed to create repo_trust table: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS supporting_repos (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            repo_path TEXT NOT NULL UNIQUE,
            added_at TEXT NOT NULL
        )",
      [],
    )
    .map_err(|e| format!("Failed to create supporting_repos table: {}", e))?;

  // Migration: rename pending_reviews columns from old schema to new schema.
  let has_old_columns: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('pending_reviews') WHERE name IN ('comments_json', 'overall_comment', 'viewed_files_json')",
        [],
        |row| row.get(0),
    );

  if let Ok(count) = has_old_columns {
    if count > 0 {
      // Create new table with correct schema
      conn
        .execute(
          "CREATE TABLE pending_reviews_new (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workspace_id INTEGER NOT NULL UNIQUE,
                    comments TEXT NOT NULL,
                    viewed_files TEXT,
                    summary_text TEXT,
                    conflict_comments TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
                )",
          [],
        )
        .map_err(|e| format!("Failed to create pending_reviews_new table: {}", e))?;

      // Migrate old pending_reviews column names to new names.
      conn.execute(
                "INSERT INTO pending_reviews_new (id, workspace_id, comments, viewed_files, summary_text, created_at, updated_at)
                 SELECT id, workspace_id, comments_json, viewed_files_json, overall_comment, created_at, updated_at
                 FROM pending_reviews",
                [],
            )
            .map_err(|e| format!("Failed to migrate pending_reviews data: {}", e))?;

      // Drop old table
      conn
        .execute("DROP TABLE pending_reviews", [])
        .map_err(|e| format!("Failed to drop old pending_reviews table: {}", e))?;

      // Rename new table to original name
      conn
        .execute(
          "ALTER TABLE pending_reviews_new RENAME TO pending_reviews",
          [],
        )
        .map_err(|e| {
          format!(
            "Failed to rename pending_reviews_new to pending_reviews: {}",
            e
          )
        })?;

      // Recreate the index (was dropped with old table)
      conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_pending_reviews_workspace ON pending_reviews(workspace_id)",
                [],
            )
            .map_err(|e| format!("Failed to recreate pending_reviews workspace index: {}", e))?;
    }
  }

  Ok(db_path)
}

/// Get a database connection for a repository.
///
/// Ensures the database is initialized before returning the connection.
/// Uses a cache to avoid re-initializing databases that have already been set up in this session.
fn get_connection(repo_path: &str) -> Result<Connection, String> {
  let initialized = INITIALIZED_DBS.get_or_init(|| Mutex::new(HashSet::new()));
  let db_key = repo_path.to_string();

  {
    let guard = initialized.lock_or_recover();
    if !guard.contains(&db_key) {
      drop(guard);
      init_local_db(repo_path)?;
      initialized.lock_or_recover().insert(db_key);
    }
  }

  let db_path = get_local_db_path(repo_path);
  let conn = Connection::open(db_path).map_err(|e| format!("Failed to open local db: {}", e))?;
  conn
    .pragma_update(None, "foreign_keys", true)
    .map_err(|e| format!("Failed to enable foreign key enforcement: {}", e))?;
  // Multiple treq instances/windows can share this db file (see
  // `upsert_instance_registry`). WAL lets readers and a writer proceed
  // concurrently instead of blocking, and busy_timeout makes a genuine
  // writer-writer conflict retry for a bit instead of immediately failing
  // with "database is locked".
  conn
    .pragma_update(None, "journal_mode", "WAL")
    .map_err(|e| format!("Failed to enable WAL mode: {}", e))?;
  conn
    .busy_timeout(std::time::Duration::from_secs(5))
    .map_err(|e| format!("Failed to set busy timeout: {}", e))?;
  Ok(conn)
}

pub fn upsert_instance_registry(
  repo_path: &str,
  instance: RegisteredInstance,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  let windows_json = serde_json::to_string(&instance.windows)
    .map_err(|e| format!("Failed to serialize instance windows: {}", e))?;
  let updated_at = Utc::now().to_rfc3339();

  conn
    .execute(
      "INSERT INTO instance_registry
            (instance_id, pid, started_at, last_heartbeat_at, endpoint, windows_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(instance_id) DO UPDATE SET
            pid = excluded.pid,
            started_at = excluded.started_at,
            last_heartbeat_at = excluded.last_heartbeat_at,
            endpoint = excluded.endpoint,
            windows_json = excluded.windows_json,
            updated_at = excluded.updated_at",
      params![
        instance.instance_id,
        instance.pid as i64,
        instance.started_at as i64,
        instance.last_heartbeat_at as i64,
        instance.endpoint,
        windows_json,
        updated_at,
      ],
    )
    .map_err(|e| format!("Failed to upsert instance registry row: {}", e))?;

  Ok(())
}

pub fn list_instance_registry(repo_path: &str) -> Result<Vec<RegisteredInstance>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(
      "SELECT instance_id, pid, started_at, last_heartbeat_at, endpoint, windows_json
             FROM instance_registry",
    )
    .map_err(|e| format!("Failed to prepare instance registry query: {}", e))?;

  let mut rows = stmt
    .query([])
    .map_err(|e| format!("Failed to list instance registry rows: {}", e))?;
  let mut instances = Vec::new();
  while let Some(row) = rows
    .next()
    .map_err(|e| format!("Failed to read instance registry row: {}", e))?
  {
    let windows_json: String = row
      .get(5)
      .map_err(|e| format!("Failed to read instance windows payload: {}", e))?;
    let windows: Vec<InstanceWindowSnapshot> = serde_json::from_str(&windows_json)
      .map_err(|e| format!("Failed to parse instance windows payload: {}", e))?;
    instances.push(RegisteredInstance {
      instance_id: row
        .get(0)
        .map_err(|e| format!("Failed to read instance_id: {}", e))?,
      pid: row
        .get::<_, i64>(1)
        .map_err(|e| format!("Failed to read pid: {}", e))? as u32,
      started_at: row
        .get::<_, i64>(2)
        .map_err(|e| format!("Failed to read started_at: {}", e))? as u64,
      last_heartbeat_at: row
        .get::<_, i64>(3)
        .map_err(|e| format!("Failed to read last_heartbeat_at: {}", e))?
        as u64,
      endpoint: row
        .get(4)
        .map_err(|e| format!("Failed to read endpoint: {}", e))?,
      windows,
    });
  }
  Ok(instances)
}

pub fn prune_stale_instance_registry(
  repo_path: &str,
  now: u64,
  timeout_ms: u64,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  let threshold = now.saturating_sub(timeout_ms) as i64;
  conn
    .execute(
      "DELETE FROM instance_registry WHERE last_heartbeat_at < ?1",
      params![threshold],
    )
    .map_err(|e| format!("Failed to prune stale instance registry rows: {}", e))?;
  Ok(())
}

const WORKSPACE_SELECT_COLUMNS: &str = "id, workspace_name, workspace_path, branch_name, created_at, refreshed_at, metadata, target_branch, COALESCE(not_on_remote, 0), COALESCE(title, branch_name), moved_files, description, sparse_patterns, hidden_until, COALESCE(archived, 0)";

pub fn get_workspaces(repo_path: &str) -> Result<Vec<Workspace>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
        .prepare(&format!("SELECT {WORKSPACE_SELECT_COLUMNS} FROM workspaces WHERE COALESCE(archived, 0) = 0 ORDER BY branch_name COLLATE NOCASE ASC"))
        .map_err(|e| format!("Failed to prepare workspaces query: {}", e))?;

  let workspaces = stmt
    .query_map([], |row| workspace_from_row(repo_path, row))
    .map_err(|e| format!("Failed to query workspaces: {}", e))?;

  workspaces
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

pub fn get_workspace_by_id(repo_path: &str, id: i64) -> Result<Option<Workspace>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(&format!(
      "SELECT {WORKSPACE_SELECT_COLUMNS} FROM workspaces WHERE id = ?1"
    ))
    .map_err(|e| format!("Failed to prepare workspace query: {}", e))?;

  let workspace = stmt
    .query_row([id], |row| workspace_from_row(repo_path, row))
    .optional()
    .map_err(|e| format!("Failed to query workspace: {}", e))?;

  Ok(workspace)
}

pub fn get_workspace_by_path(
  repo_path: &str,
  workspace_path: &str,
) -> Result<Option<Workspace>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(&format!(
      "SELECT {WORKSPACE_SELECT_COLUMNS} FROM workspaces WHERE workspace_path = ?1"
    ))
    .map_err(|e| format!("Failed to prepare workspace query: {}", e))?;

  let workspace = stmt
    .query_row([workspace_path], |row| workspace_from_row(repo_path, row))
    .optional()
    .map_err(|e| format!("Failed to query workspace: {}", e))?;

  Ok(workspace)
}

pub fn get_workspace_by_branch(
  repo_path: &str,
  branch_name: &str,
) -> Result<Option<Workspace>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(&format!(
      "SELECT {WORKSPACE_SELECT_COLUMNS} FROM workspaces WHERE branch_name = ?1 AND COALESCE(archived, 0) = 0"
    ))
    .map_err(|e| format!("Failed to prepare workspace query: {}", e))?;

  let workspace = stmt
    .query_row([branch_name], |row| workspace_from_row(repo_path, row))
    .optional()
    .map_err(|e| format!("Failed to query workspace by branch: {}", e))?;

  Ok(workspace)
}

/// Archived rows keep their record but no longer own a directory. Move such a
/// row off `workspace_path` so a new workspace can take the same slot without
/// hitting the UNIQUE constraint.
fn release_archived_workspace_path(conn: &Connection, workspace_path: &str) -> Result<(), String> {
  conn
    .execute(
      "UPDATE workspaces SET workspace_path = workspace_path || '#archived-' || id
       WHERE workspace_path = ?1 AND COALESCE(archived, 0) = 1",
      [workspace_path],
    )
    .map_err(|e| format!("Failed to release archived workspace path: {}", e))?;
  Ok(())
}

pub fn add_workspace(
  repo_path: &str,
  workspace_name: String,
  workspace_path: String,
  branch_name: String,
  description: Option<String>,
  moved_files: Option<Vec<String>>,
  sparse_patterns: Option<Vec<String>>,
) -> Result<i64, String> {
  let conn = get_connection(repo_path)?;
  let created_at = Utc::now().to_rfc3339();
  let refreshed_at = created_at.clone();

  // Convert moved_files Vec to JSON string for storage, store NULL if empty
  let moved_files_json = moved_files.and_then(|files| {
    if files.is_empty() {
      None
    } else {
      serde_json::to_string(&files).ok()
    }
  });
  let sparse_patterns_json = sparse_patterns.and_then(|patterns| {
    if patterns.is_empty() {
      None
    } else {
      serde_json::to_string(&patterns).ok()
    }
  });

  release_archived_workspace_path(&conn, &workspace_path)?;
  conn.execute(
        "INSERT INTO workspaces (workspace_name, workspace_path, branch_name, created_at, refreshed_at, description, moved_files, sparse_patterns)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            workspace_name,
            workspace_path,
            branch_name,
            created_at,
            refreshed_at,
            description,
            moved_files_json,
            sparse_patterns_json
        ],
    )
    .map_err(|e| format!("Failed to insert workspace: {}", e))?;

  Ok(conn.last_insert_rowid())
}

pub fn delete_workspace(repo_path: &str, id: i64) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute("DELETE FROM workspaces WHERE id = ?1", [id])
    .map_err(|e| format!("Failed to delete workspace: {}", e))?;
  Ok(())
}

pub fn archive_workspace(repo_path: &str, id: i64) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute("UPDATE workspaces SET archived = 1 WHERE id = ?1", [id])
    .map_err(|e| format!("Failed to archive workspace: {}", e))?;
  Ok(())
}

pub fn update_workspace_description(
  repo_path: &str,
  id: i64,
  description: &str,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE workspaces SET description = ?1 WHERE id = ?2",
      params![description, id],
    )
    .map_err(|e| format!("Failed to update workspace description: {}", e))?;
  Ok(())
}

pub fn update_workspace_title(repo_path: &str, id: i64, title: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE workspaces SET title = ?1 WHERE id = ?2",
      params![title, id],
    )
    .map_err(|e| format!("Failed to update workspace title: {}", e))?;
  Ok(())
}

/// Drop a custom title so the workspace shows its branch name again.
pub fn clear_workspace_title(repo_path: &str, id: i64) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute("UPDATE workspaces SET title = NULL WHERE id = ?1", [id])
    .map_err(|e| format!("Failed to clear workspace title: {}", e))?;
  Ok(())
}

pub fn clear_workspace_description(repo_path: &str, id: i64) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE workspaces SET description = NULL WHERE id = ?1",
      [id],
    )
    .map_err(|e| format!("Failed to clear workspace description: {}", e))?;
  Ok(())
}

pub fn update_workspace_hidden_until(
  repo_path: &str,
  id: i64,
  hidden_until: Option<&str>,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE workspaces SET hidden_until = ?1 WHERE id = ?2",
      params![hidden_until, id],
    )
    .map_err(|e| format!("Failed to update workspace hidden_until: {}", e))?;
  Ok(())
}

pub fn update_workspace_target_branch(
  repo_path: &str,
  id: i64,
  target_branch: &str,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE workspaces SET target_branch = ?1 WHERE id = ?2",
      params![target_branch, id],
    )
    .map_err(|e| format!("Failed to update workspace target branch: {}", e))?;
  Ok(())
}

pub fn update_workspace_branch_name(
  repo_path: &str,
  id: i64,
  branch_name: &str,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE workspaces SET branch_name = ?1 WHERE id = ?2",
      params![branch_name, id],
    )
    .map_err(|e| format!("Failed to update workspace branch name: {}", e))?;
  Ok(())
}

pub fn get_cached_commit_diff_stats_batch(
  repo_path: &str,
  commit_ids: &[String],
) -> Result<std::collections::HashMap<String, CachedCommitDiffStat>, String> {
  use std::collections::HashMap;
  if commit_ids.is_empty() {
    return Ok(HashMap::new());
  }
  let conn = get_connection(repo_path)?;
  let placeholders = std::iter::repeat_n("?", commit_ids.len())
    .collect::<Vec<_>>()
    .join(",");
  let sql = format!(
    "SELECT commit_id, insertions, deletions, updated_at,
                change_id, description, author_name, timestamp
         FROM commit_diff_stats
         WHERE commit_id IN ({})",
    placeholders
  );
  let mut stmt = conn
    .prepare(&sql)
    .map_err(|e| format!("Failed to prepare commit diff stat batch query: {}", e))?;
  let params_vec: Vec<&dyn rusqlite::ToSql> = commit_ids
    .iter()
    .map(|s| s as &dyn rusqlite::ToSql)
    .collect();
  let rows = stmt
    .query_map(params_vec.as_slice(), |row| {
      Ok(CachedCommitDiffStat {
        commit_id: row.get(0)?,
        insertions: row.get(1)?,
        deletions: row.get(2)?,
        updated_at: row.get(3)?,
        change_id: row.get(4)?,
        description: row.get(5)?,
        author_name: row.get(6)?,
        timestamp: row.get(7)?,
      })
    })
    .map_err(|e| format!("Failed to query commit diff stat cache: {}", e))?;
  let mut out = HashMap::with_capacity(commit_ids.len());
  for row in rows {
    let row = row.map_err(|e| format!("Failed to read commit diff stat row: {}", e))?;
    out.insert(row.commit_id.clone(), row);
  }
  Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub fn cache_commit_row(
  repo_path: &str,
  commit_id: &str,
  insertions: u32,
  deletions: u32,
  change_id: &str,
  description: &str,
  author_name: &str,
  timestamp: &str,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "INSERT INTO commit_diff_stats
             (commit_id, insertions, deletions, updated_at,
              change_id, description, author_name, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(commit_id) DO UPDATE SET
            insertions = excluded.insertions,
            deletions = excluded.deletions,
            updated_at = excluded.updated_at,
            change_id = excluded.change_id,
            description = excluded.description,
            author_name = excluded.author_name,
            timestamp = excluded.timestamp",
      params![
        commit_id,
        insertions,
        deletions,
        Utc::now().to_rfc3339(),
        change_id,
        description,
        author_name,
        timestamp,
      ],
    )
    .map_err(|e| format!("Failed to cache commit row: {}", e))?;
  Ok(())
}

/// Get all workspaces targeting a specific branch
pub fn get_workspaces_by_target_branch(
  repo_path: &str,
  target_branch: &str,
) -> Result<Vec<Workspace>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
        .prepare(&format!("SELECT {WORKSPACE_SELECT_COLUMNS} FROM workspaces WHERE target_branch = ?1 AND COALESCE(archived, 0) = 0 ORDER BY branch_name COLLATE NOCASE ASC"))
        .map_err(|e| format!("Failed to prepare workspaces query: {}", e))?;

  let workspaces = stmt
    .query_map([target_branch], |row| workspace_from_row(repo_path, row))
    .map_err(|e| format!("Failed to query workspaces: {}", e))?;

  workspaces
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

/// Update the not_on_remote flag for a workspace
pub fn update_workspace_not_on_remote(
  repo_path: &str,
  id: i64,
  not_on_remote: bool,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE workspaces SET not_on_remote = ?1 WHERE id = ?2",
      params![not_on_remote as i64, id],
    )
    .map_err(|e| format!("Failed to update workspace not_on_remote: {}", e))?;
  Ok(())
}

pub fn upsert_workspace_discovery(
  repo_path: &str,
  workspace_name: &str,
  workspace_path: &str,
  branch_name: &str,
  refreshed_at: &str,
) -> Result<i64, String> {
  let conn = get_connection(repo_path)?;
  let created_at = refreshed_at.to_string();

  release_archived_workspace_path(&conn, workspace_path)?;
  conn.execute(
        "INSERT INTO workspaces (workspace_name, workspace_path, branch_name, created_at, refreshed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(workspace_path) DO UPDATE SET
             workspace_name = excluded.workspace_name,
             branch_name = excluded.branch_name,
             refreshed_at = excluded.refreshed_at",
        params![
            workspace_name,
            workspace_path,
            branch_name,
            created_at,
            refreshed_at,
        ],
    )
    .map_err(|e| format!("Failed to upsert discovered workspace: {}", e))?;

  conn
    .query_row(
      "SELECT id FROM workspaces WHERE workspace_path = ?1",
      [workspace_path],
      |row| row.get(0),
    )
    .map_err(|e| format!("Failed to fetch upserted workspace id: {}", e))
}

pub fn sync_discovered_workspaces(
  repo_path: &str,
  discovered: &[DiscoveredWorkspace],
  refreshed_at: &str,
) -> Result<Vec<Workspace>, String> {
  if discovered.is_empty() {
    return Ok(Vec::new());
  }

  let mut conn = get_connection(repo_path)?;
  let tx = conn
    .transaction()
    .map_err(|e| format!("Failed to start discovery transaction: {}", e))?;
  for workspace in discovered {
    // Prefer a discovered canonical bookmark (e.g. feat/foo) over a prior
    // sanitized fallback (feat-foo), even while the bookmark is conflicted.
    // Never replace a known name with the sanitized fallback: discovery falls
    // back to the directory name whenever no bookmark matches it, which is
    // always the case after a rename (the directory keeps its old name).
    let discovered_has_canonical_bookmark = workspace.branch_name != workspace.workspace_name;
    let update_branch_name = discovered_has_canonical_bookmark;
    let update_workspace_name = !workspace.has_conflicts;
    release_archived_workspace_path(&tx, &workspace.workspace_path)?;
    tx.execute(
            "INSERT INTO workspaces (workspace_name, workspace_path, branch_name, created_at, refreshed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(workspace_path) DO UPDATE SET
                 workspace_name = CASE WHEN ?6 THEN excluded.workspace_name ELSE workspaces.workspace_name END,
                 branch_name = CASE WHEN ?7 THEN excluded.branch_name ELSE workspaces.branch_name END,
                 refreshed_at = excluded.refreshed_at",
            params![
                workspace.workspace_name,
                workspace.workspace_path,
                workspace.branch_name,
                refreshed_at,
                refreshed_at,
                update_workspace_name,
                update_branch_name,
            ],
        )
        .map_err(|e| format!("Failed to upsert discovered workspace: {}", e))?;
  }
  tx.commit()
    .map_err(|e| format!("Failed to commit discovery transaction: {}", e))?;

  let discovered_paths: HashSet<&str> = discovered
    .iter()
    .map(|workspace| workspace.workspace_path.as_str())
    .collect();
  let mut stmt = conn
    .prepare(&format!(
      "SELECT {WORKSPACE_SELECT_COLUMNS} FROM workspaces WHERE COALESCE(archived, 0) = 0 ORDER BY branch_name COLLATE NOCASE ASC"
    ))
    .map_err(|e| format!("Failed to prepare synced workspaces query: {}", e))?;

  let workspaces = stmt
    .query_map([], |row| workspace_from_row(repo_path, row))
    .map_err(|e| format!("Failed to query synced workspaces: {}", e))?;

  workspaces
    .filter_map(|workspace| match workspace {
      Ok(workspace) if discovered_paths.contains(workspace.workspace_path.as_str()) => {
        Some(Ok(workspace))
      }
      Ok(_) => None,
      Err(err) => Some(Err(err)),
    })
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

/// Get last rebased commit from workspace metadata
pub fn get_workspace_last_rebased_commit(
  repo_path: &str,
  id: i64,
) -> Result<Option<String>, String> {
  let workspaces = get_workspaces(repo_path)?;
  let workspace = workspaces.iter().find(|w| w.id == id);

  if let Some(ws) = workspace {
    if let Some(metadata) = &ws.metadata {
      if let Ok(json) = serde_json::from_str::<serde_json::Value>(metadata) {
        return Ok(
          json
            .get("last_rebased_target_commit")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        );
      }
    }
  }
  Ok(None)
}

/// Update last rebased commit in workspace metadata.
///
/// Retrieves the current metadata, parses it as JSON, updates the
/// last_rebased_target_commit field, and serializes it back to the database.
pub fn update_workspace_last_rebased_commit(
  repo_path: &str,
  id: i64,
  commit_id: &str,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;

  let current_metadata: Option<String> = conn
    .query_row(
      "SELECT metadata FROM workspaces WHERE id = ?1",
      [id],
      |row| row.get(0),
    )
    .ok();

  let mut meta: serde_json::Value = current_metadata
    .and_then(|m| serde_json::from_str(&m).ok())
    .unwrap_or(serde_json::json!({}));

  meta["last_rebased_target_commit"] = serde_json::Value::String(commit_id.to_string());

  let new_metadata =
    serde_json::to_string(&meta).map_err(|e| format!("Failed to serialize metadata: {}", e))?;

  conn
    .execute(
      "UPDATE workspaces SET metadata = ?1 WHERE id = ?2",
      params![new_metadata, id],
    )
    .map_err(|e| format!("Failed to update metadata: {}", e))?;

  Ok(())
}

pub fn set_workspace_metadata(repo_path: &str, id: i64, metadata: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE workspaces SET metadata = ?1 WHERE id = ?2",
      params![metadata, id],
    )
    .map_err(|e| format!("Failed to set workspace metadata: {}", e))?;
  Ok(())
}

pub fn get_sessions(repo_path: &str) -> Result<Vec<Session>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
        .prepare("SELECT id, workspace_id, name, created_at, last_accessed, model FROM sessions ORDER BY created_at ASC")
        .map_err(|e| format!("Failed to prepare sessions query: {}", e))?;

  let sessions = stmt
    .query_map([], |row| {
      Ok(Session {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        name: row.get(2)?,
        created_at: row.get(3)?,
        last_accessed: row.get(4)?,
        model: row.get(5)?,
      })
    })
    .map_err(|e| format!("Failed to query sessions: {}", e))?;

  sessions
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

pub fn add_session(
  repo_path: &str,
  workspace_id: Option<i64>,
  name: String,
) -> Result<i64, String> {
  let conn = get_connection(repo_path)?;
  let now = Utc::now().to_rfc3339();

  conn
    .execute(
      "INSERT INTO sessions (workspace_id, name, created_at, last_accessed, model)
         VALUES (?1, ?2, ?3, ?4, ?5)",
      params![workspace_id, name, now, now, None::<String>],
    )
    .map_err(|e| format!("Failed to insert session: {}", e))?;

  Ok(conn.last_insert_rowid())
}

pub fn update_session_access(repo_path: &str, id: i64) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  let now = Utc::now().to_rfc3339();

  conn
    .execute(
      "UPDATE sessions SET last_accessed = ?1 WHERE id = ?2",
      params![now, id],
    )
    .map_err(|e| format!("Failed to update session access time: {}", e))?;

  Ok(())
}

pub fn update_session_name(repo_path: &str, id: i64, name: String) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE sessions SET name = ?1 WHERE id = ?2",
      params![name, id],
    )
    .map_err(|e| format!("Failed to update session name: {}", e))?;

  Ok(())
}

pub fn delete_session(repo_path: &str, id: i64) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute("DELETE FROM sessions WHERE id = ?1", [id])
    .map_err(|e| format!("Failed to delete session: {}", e))?;
  Ok(())
}

pub fn get_session_model(repo_path: &str, id: i64) -> Result<Option<String>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare("SELECT model FROM sessions WHERE id = ?1")
    .map_err(|e| format!("Failed to prepare query: {}", e))?;

  let model: Option<String> = stmt
    .query_row([id], |row| row.get(0))
    .map_err(|e| format!("Failed to get session model: {}", e))?;

  Ok(model)
}

pub fn set_session_model(repo_path: &str, id: i64, model: Option<String>) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE sessions SET model = ?1 WHERE id = ?2",
      params![model, id],
    )
    .map_err(|e| format!("Failed to update session model: {}", e))?;

  Ok(())
}

// Workspace Files Cache Functions

/// Get cached directory listing for a specific parent path
pub fn get_cached_directory_listing(
  repo_path: &str,
  workspace_id: Option<i64>,
  parent_path: &str,
) -> Result<Vec<CachedWorkspaceFile>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
        .prepare(
            "SELECT id, workspace_id, file_path, relative_path, is_directory, parent_path, cached_at, mtime
             FROM workspace_files
             WHERE workspace_id IS ?1 AND parent_path IS ?2
             ORDER BY is_directory DESC, relative_path",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

  let files = stmt
    .query_map(params![workspace_id, parent_path], |row| {
      Ok(CachedWorkspaceFile {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        file_path: row.get(2)?,
        relative_path: row.get(3)?,
        is_directory: row.get::<_, i64>(4)? != 0,
        parent_path: row.get(5)?,
        cached_at: row.get(6)?,
        mtime: row.get(7)?,
      })
    })
    .map_err(|e| format!("Failed to query cached files: {}", e))?;

  files
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

fn escape_like_character(character: char, pattern: &mut String) {
  if matches!(character, '%' | '_' | '^') {
    pattern.push('^');
  }
  pattern.push(character);
}

fn fuzzy_like_pattern(query: &str) -> String {
  let mut pattern = String::from("%");
  for character in query.chars().filter(|character| !character.is_whitespace()) {
    escape_like_character(character.to_ascii_lowercase(), &mut pattern);
    pattern.push('%');
  }
  pattern
}

fn literal_like_pattern(query: &str) -> String {
  let mut pattern = String::from("%");
  for character in query.chars().filter(|character| !character.is_whitespace()) {
    escape_like_character(character.to_ascii_lowercase(), &mut pattern);
  }
  pattern.push('%');
  pattern
}

/// Search workspace files by filename or path.
///
/// Returns files (not directories) using a case-insensitive, ordered-character
/// fuzzy match against the relative path. Whitespace in the query is ignored.
/// Results are ordered by:
/// - Exact filename matches first
/// - Then contiguous substring matches
/// - Then by path length (shorter = more relevant)
pub fn search_workspace_files(
  repo_path: &str,
  workspace_id: Option<i64>,
  query: &str,
  limit: usize,
) -> Result<Vec<CachedWorkspaceFile>, String> {
  let conn = get_connection(repo_path)?;

  // Empty query: return files ordered by path length (shortest/top-level first)
  if query.trim().is_empty() {
    let mut stmt = conn
            .prepare(
                "SELECT id, workspace_id, file_path, relative_path, is_directory, parent_path, cached_at, mtime
                 FROM workspace_files
                 WHERE workspace_id IS ?1
                   AND is_directory = 0
                 ORDER BY LENGTH(relative_path)
                 LIMIT ?2",
            )
            .map_err(|e| format!("Failed to prepare search query: {}", e))?;

    let files = stmt
      .query_map(params![workspace_id, limit as i64], |row| {
        Ok(CachedWorkspaceFile {
          id: row.get(0)?,
          workspace_id: row.get(1)?,
          file_path: row.get(2)?,
          relative_path: row.get(3)?,
          is_directory: row.get::<_, i64>(4)? != 0,
          parent_path: row.get(5)?,
          cached_at: row.get(6)?,
          mtime: row.get(7)?,
        })
      })
      .map_err(|e| format!("Failed to search files: {}", e))?;

    return files
      .collect::<Result<Vec<_>, _>>()
      .map_err(|e| e.to_string());
  }

  let normalized_query = query
    .chars()
    .filter(|character| !character.is_whitespace())
    .flat_map(char::to_lowercase)
    .collect::<String>();
  let search_pattern = fuzzy_like_pattern(query);
  let literal_pattern = literal_like_pattern(query);

  let mut stmt = conn
        .prepare(
            "SELECT id, workspace_id, file_path, relative_path, is_directory, parent_path, cached_at, mtime
             FROM workspace_files
             WHERE workspace_id IS ?1
               AND is_directory = 0
               AND LOWER(relative_path) LIKE ?2 ESCAPE '^'
             ORDER BY
               CASE
                 WHEN LOWER(relative_path) = ?3 OR LOWER(relative_path) LIKE ?4 ESCAPE '^' THEN 0
                 WHEN LOWER(relative_path) LIKE ?5 ESCAPE '^' THEN 1
                 ELSE 2
               END,
               LENGTH(relative_path)
             LIMIT ?6",
        )
        .map_err(|e| format!("Failed to prepare search query: {}", e))?;

  let filename_pattern = format!("%/{}", literal_pattern.trim_start_matches('%'));

  let files = stmt
    .query_map(
      params![
        workspace_id,
        search_pattern,
        normalized_query,
        filename_pattern,
        literal_pattern,
        limit as i64
      ],
      |row| {
        Ok(CachedWorkspaceFile {
          id: row.get(0)?,
          workspace_id: row.get(1)?,
          file_path: row.get(2)?,
          relative_path: row.get(3)?,
          is_directory: row.get::<_, i64>(4)? != 0,
          parent_path: row.get(5)?,
          cached_at: row.get(6)?,
          mtime: row.get(7)?,
        })
      },
    )
    .map_err(|e| format!("Failed to search files: {}", e))?;

  files
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

/// Batch update all cached files for a workspace.
///
/// Deletes all existing entries for the workspace and inserts the provided files.
/// This is an all-or-nothing replacement operation performed within a transaction.
pub fn sync_workspace_files(
  repo_path: &str,
  workspace_id: Option<i64>,
  files: Vec<CachedWorkspaceFile>,
) -> Result<(), String> {
  let mut conn = get_connection(repo_path)?;
  let tx = conn
    .transaction()
    .map_err(|e| format!("Failed to start transaction: {}", e))?;

  tx.execute(
    "DELETE FROM workspace_files WHERE workspace_id IS ?1",
    params![workspace_id],
  )
  .map_err(|e| format!("Failed to delete existing files: {}", e))?;

  for file in &files {
    tx.execute(
      "INSERT INTO workspace_files
             (workspace_id, file_path, relative_path, is_directory, parent_path, cached_at, mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
      params![
        workspace_id,
        &file.file_path,
        &file.relative_path,
        if file.is_directory { 1 } else { 0 },
        &file.parent_path,
        &file.cached_at,
        &file.mtime,
      ],
    )
    .map_err(|e| format!("Failed to insert file: {}", e))?;
  }

  tx.commit()
    .map_err(|e| format!("Failed to commit transaction: {}", e))?;

  Ok(())
}

// Pending Review Functions

/// Get pending review for a workspace
pub fn get_pending_review(
  repo_path: &str,
  workspace_id: i64,
) -> Result<Option<PendingReview>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(
      "SELECT id, workspace_id, comments, viewed_files, summary_text, conflict_comments, created_at, updated_at
             FROM pending_reviews
             WHERE workspace_id = ?1",
    )
    .map_err(|e| format!("Failed to prepare query: {}", e))?;

  let review = stmt
    .query_row([workspace_id], |row| {
      Ok(PendingReview {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        comments: row.get(2)?,
        viewed_files: row.get(3)?,
        summary_text: row.get(4)?,
        conflict_comments: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
      })
    })
    .optional()
    .map_err(|e| format!("Failed to get pending review: {}", e))?;

  Ok(review)
}

/// Save or update pending review for a workspace
pub fn save_pending_review(
  repo_path: &str,
  workspace_id: i64,
  comments: &str,
  viewed_files: Option<&str>,
  summary_text: Option<&str>,
  conflict_comments: Option<&str>,
) -> Result<i64, String> {
  let conn = get_connection(repo_path)?;
  let now = Utc::now().to_rfc3339();

  // Use INSERT OR REPLACE to handle both insert and update
  conn.execute(
        "INSERT OR REPLACE INTO pending_reviews (workspace_id, comments, viewed_files, summary_text, conflict_comments, created_at, updated_at)
         VALUES (
             ?1,
             ?2,
             ?3,
             ?4,
             ?5,
             COALESCE((SELECT created_at FROM pending_reviews WHERE workspace_id = ?1), ?6),
             ?6
         )",
        params![workspace_id, comments, viewed_files, summary_text, conflict_comments, now],
    )
    .map_err(|e| format!("Failed to save pending review: {}", e))?;

  Ok(conn.last_insert_rowid())
}

/// Clear pending review for a workspace
pub fn clear_pending_review(repo_path: &str, workspace_id: i64) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "DELETE FROM pending_reviews WHERE workspace_id = ?1",
      [workspace_id],
    )
    .map_err(|e| format!("Failed to clear pending review: {}", e))?;
  Ok(())
}

// Agent Review Comment Functions

const AGENT_REVIEW_COMMENT_SELECT: &str =
  "SELECT id, repo_path, target_type, target_id, file_path, hunk_id, start_line, end_line, side,
            comment_text, suggested_replacement, status, source, created_at, resolved_at
     FROM agent_review_comments";

fn agent_review_comment_from_row(row: &Row<'_>) -> rusqlite::Result<AgentReviewComment> {
  Ok(AgentReviewComment {
    id: row.get(0)?,
    repo_path: row.get(1)?,
    target_type: row.get(2)?,
    target_id: row.get(3)?,
    file_path: row.get(4)?,
    hunk_id: row.get(5)?,
    start_line: row.get(6)?,
    end_line: row.get(7)?,
    side: row.get(8)?,
    comment_text: row.get(9)?,
    suggested_replacement: row.get(10)?,
    status: row.get(11)?,
    source: row.get(12)?,
    created_at: row.get(13)?,
    resolved_at: row.get(14)?,
  })
}

/// Insert one local review comment. Returns the stored row.
#[allow(clippy::too_many_arguments)]
pub fn create_agent_review_comment(
  repo_path: &str,
  target_type: &str,
  target_id: &str,
  file_path: &str,
  hunk_id: Option<&str>,
  start_line: i64,
  end_line: i64,
  side: Option<&str>,
  comment_text: &str,
  suggested_replacement: Option<&str>,
  source: &str,
) -> Result<AgentReviewComment, String> {
  let conn = get_connection(repo_path)?;
  let comment = AgentReviewComment {
    id: uuid::Uuid::new_v4().to_string(),
    repo_path: repo_path.to_string(),
    target_type: target_type.to_string(),
    target_id: target_id.to_string(),
    file_path: file_path.to_string(),
    hunk_id: hunk_id.map(|s| s.to_string()),
    start_line,
    end_line,
    side: side.map(|s| s.to_string()),
    comment_text: comment_text.to_string(),
    suggested_replacement: suggested_replacement.map(|s| s.to_string()),
    status: "open".to_string(),
    source: source.to_string(),
    created_at: Utc::now().to_rfc3339(),
    resolved_at: None,
  };

  conn.execute(
        "INSERT INTO agent_review_comments (id, repo_path, target_type, target_id, file_path, hunk_id, start_line, end_line, side, comment_text, suggested_replacement, status, source, created_at, resolved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            comment.id,
            comment.repo_path,
            comment.target_type,
            comment.target_id,
            comment.file_path,
            comment.hunk_id,
            comment.start_line,
            comment.end_line,
            comment.side,
            comment.comment_text,
            comment.suggested_replacement,
            comment.status,
            comment.source,
            comment.created_at,
            comment.resolved_at,
        ],
    )
    .map_err(|e| format!("Failed to insert agent review comment: {}", e))?;

  Ok(comment)
}

/// All local review comments for one reviewed target, oldest first.
pub fn list_agent_review_comments(
  repo_path: &str,
  target_type: &str,
  target_id: &str,
) -> Result<Vec<AgentReviewComment>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(&format!(
      "{AGENT_REVIEW_COMMENT_SELECT}
             WHERE repo_path = ?1 AND target_type = ?2 AND target_id = ?3
             ORDER BY file_path ASC, start_line ASC, created_at ASC"
    ))
    .map_err(|e| format!("Failed to prepare query: {}", e))?;

  let comments = stmt
    .query_map(params![repo_path, target_type, target_id], |row| {
      agent_review_comment_from_row(row)
    })
    .map_err(|e| format!("Failed to list agent review comments: {}", e))?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| format!("Failed to read agent review comments: {}", e))?;

  Ok(comments)
}

/// Fetch one local review comment by id.
pub fn get_agent_review_comment(
  repo_path: &str,
  id: &str,
) -> Result<Option<AgentReviewComment>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(&format!("{AGENT_REVIEW_COMMENT_SELECT} WHERE id = ?1"))
    .map_err(|e| format!("Failed to prepare query: {}", e))?;

  let comment = stmt
    .query_row([id], agent_review_comment_from_row)
    .optional()
    .map_err(|e| format!("Failed to get agent review comment: {}", e))?;

  Ok(comment)
}

/// Mark one local review comment resolved. No-op if it is already resolved.
pub fn resolve_agent_review_comment(repo_path: &str, id: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  let now = Utc::now().to_rfc3339();
  conn
    .execute(
      "UPDATE agent_review_comments SET status = 'resolved', resolved_at = ?2 WHERE id = ?1",
      params![id, now],
    )
    .map_err(|e| format!("Failed to resolve agent review comment: {}", e))?;
  Ok(())
}

/// Delete one local review comment.
pub fn delete_agent_review_comment(repo_path: &str, id: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute("DELETE FROM agent_review_comments WHERE id = ?1", [id])
    .map_err(|e| format!("Failed to delete agent review comment: {}", e))?;
  Ok(())
}

// Prompt History Functions

const PROMPT_HISTORY_SELECT: &str = "SELECT ph.id, ph.workspace_id, ph.session_id, ph.prompt_text, ph.agent, ph.created_at, COALESCE(w.title, w.branch_name)
     FROM prompt_history ph
     LEFT JOIN workspaces w ON w.id = ph.workspace_id";

fn prompt_history_from_row(row: &Row<'_>) -> rusqlite::Result<PromptHistoryEntry> {
  Ok(PromptHistoryEntry {
    id: row.get(0)?,
    workspace_id: row.get(1)?,
    session_id: row.get(2)?,
    prompt_text: row.get(3)?,
    agent: row.get(4)?,
    created_at: row.get(5)?,
    workspace_label: row.get(6)?,
  })
}

/// Record a prompt that was sent to an agent, so it can be reviewed later.
pub fn add_prompt_history(
  repo_path: &str,
  workspace_id: Option<i64>,
  session_id: Option<i64>,
  prompt_text: &str,
  agent: Option<&str>,
) -> Result<i64, String> {
  let conn = get_connection(repo_path)?;
  let created_at = Utc::now().to_rfc3339();

  conn
    .execute(
      "INSERT INTO prompt_history (workspace_id, session_id, prompt_text, agent, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
      params![workspace_id, session_id, prompt_text, agent, created_at],
    )
    .map_err(|e| format!("Failed to insert prompt history: {}", e))?;

  Ok(conn.last_insert_rowid())
}

/// Get every prompt ever sent in this repository, most recent first.
pub fn get_prompt_history(repo_path: &str) -> Result<Vec<PromptHistoryEntry>, String> {
  let conn = get_connection(repo_path)?;
  let sql = format!(
    "{} ORDER BY ph.created_at DESC, ph.id DESC",
    PROMPT_HISTORY_SELECT
  );
  let mut stmt = conn
    .prepare(&sql)
    .map_err(|e| format!("Failed to prepare prompt history query: {}", e))?;

  let entries = stmt
    .query_map([], prompt_history_from_row)
    .map_err(|e| format!("Failed to query prompt history: {}", e))?;

  entries
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

/// Persist a newly created stash entry.
#[allow(clippy::too_many_arguments)]
pub fn add_stash(
  repo_path: &str,
  workspace_id: Option<i64>,
  workspace_label: &str,
  commit_id: &str,
  change_id: &str,
  short_commit_id: &str,
  bookmark_name: &str,
  additions: u32,
  deletions: u32,
  files_changed: &[String],
) -> Result<StashEntry, String> {
  let conn = get_connection(repo_path)?;
  let created_at = Utc::now().to_rfc3339();
  let files_json = serde_json::to_string(files_changed)
    .map_err(|e| format!("Failed to serialize stash files: {}", e))?;

  conn
    .execute(
      "INSERT INTO stashes (
            workspace_id, workspace_label, commit_id, change_id, short_commit_id,
            bookmark_name, created_at, additions, deletions, files_changed
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
      params![
        workspace_id,
        workspace_label,
        commit_id,
        change_id,
        short_commit_id,
        bookmark_name,
        created_at,
        additions,
        deletions,
        files_json,
      ],
    )
    .map_err(|e| format!("Failed to insert stash: {}", e))?;

  let id = conn.last_insert_rowid();
  Ok(StashEntry {
    id,
    workspace_id,
    workspace_label: workspace_label.to_string(),
    commit_id: commit_id.to_string(),
    change_id: change_id.to_string(),
    short_commit_id: short_commit_id.to_string(),
    bookmark_name: bookmark_name.to_string(),
    created_at,
    additions,
    deletions,
    files_changed: files_changed.to_vec(),
  })
}

fn stash_from_row(row: &Row<'_>) -> rusqlite::Result<StashEntry> {
  let files_json: String = row.get(10)?;
  let files_changed: Vec<String> = serde_json::from_str(&files_json).unwrap_or_default();
  Ok(StashEntry {
    id: row.get(0)?,
    workspace_id: row.get(1)?,
    workspace_label: row.get(2)?,
    commit_id: row.get(3)?,
    change_id: row.get(4)?,
    short_commit_id: row.get(5)?,
    bookmark_name: row.get(6)?,
    created_at: row.get(7)?,
    additions: row.get::<_, i64>(8)? as u32,
    deletions: row.get::<_, i64>(9)? as u32,
    files_changed,
  })
}

const STASH_SELECT: &str = "SELECT id, workspace_id, workspace_label, commit_id, change_id,
        short_commit_id, bookmark_name, created_at, additions, deletions, files_changed
     FROM stashes";

/// List all stashes for a repository, most recent first.
pub fn get_stashes(repo_path: &str) -> Result<Vec<StashEntry>, String> {
  let conn = get_connection(repo_path)?;
  let sql = format!("{} ORDER BY created_at DESC, id DESC", STASH_SELECT);
  let mut stmt = conn
    .prepare(&sql)
    .map_err(|e| format!("Failed to prepare stash query: {}", e))?;
  let entries = stmt
    .query_map([], stash_from_row)
    .map_err(|e| format!("Failed to query stashes: {}", e))?;
  entries
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

/// Look up a single stash by id.
pub fn get_stash_by_id(repo_path: &str, stash_id: i64) -> Result<Option<StashEntry>, String> {
  let conn = get_connection(repo_path)?;
  let sql = format!("{} WHERE id = ?1", STASH_SELECT);
  let mut stmt = conn
    .prepare(&sql)
    .map_err(|e| format!("Failed to prepare stash lookup: {}", e))?;
  let mut rows = stmt
    .query_map([stash_id], stash_from_row)
    .map_err(|e| format!("Failed to query stash: {}", e))?;
  match rows.next() {
    Some(Ok(entry)) => Ok(Some(entry)),
    Some(Err(e)) => Err(e.to_string()),
    None => Ok(None),
  }
}

/// Delete a stash row by id.
pub fn delete_stash(repo_path: &str, stash_id: i64) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  let changed = conn
    .execute("DELETE FROM stashes WHERE id = ?1", [stash_id])
    .map_err(|e| format!("Failed to delete stash: {}", e))?;
  if changed == 0 {
    return Err(format!("Stash not found: {}", stash_id));
  }
  Ok(())
}

/// Get the earliest prompt sent for a given workspace (its "starting prompt").
pub fn get_workspace_starting_prompt(
  repo_path: &str,
  workspace_id: i64,
) -> Result<Option<PromptHistoryEntry>, String> {
  let conn = get_connection(repo_path)?;
  let sql = format!(
    "{} WHERE ph.workspace_id = ?1 ORDER BY ph.created_at ASC, ph.id ASC LIMIT 1",
    PROMPT_HISTORY_SELECT
  );
  let mut stmt = conn
    .prepare(&sql)
    .map_err(|e| format!("Failed to prepare starting prompt query: {}", e))?;

  let entry = stmt
    .query_row([workspace_id], prompt_history_from_row)
    .optional()
    .map_err(|e| format!("Failed to get workspace starting prompt: {}", e))?;

  Ok(entry)
}

// FileBrowser Pending Review Functions

/// Get the FileBrowser's pending review for a workspace
pub fn get_file_browser_review(
  repo_path: &str,
  workspace_id: i64,
) -> Result<Option<FileBrowserPendingReview>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(
      "SELECT id, workspace_id, comments, summary_text, created_at, updated_at
             FROM file_browser_reviews
             WHERE workspace_id = ?1",
    )
    .map_err(|e| format!("Failed to prepare query: {}", e))?;

  let review = stmt
    .query_row([workspace_id], |row| {
      Ok(FileBrowserPendingReview {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        comments: row.get(2)?,
        summary_text: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
      })
    })
    .optional()
    .map_err(|e| format!("Failed to get file browser review: {}", e))?;

  Ok(review)
}

/// Save or update the FileBrowser's pending review for a workspace
pub fn save_file_browser_review(
  repo_path: &str,
  workspace_id: i64,
  comments: &str,
  summary_text: Option<&str>,
) -> Result<i64, String> {
  let conn = get_connection(repo_path)?;
  let now = Utc::now().to_rfc3339();

  conn.execute(
        "INSERT OR REPLACE INTO file_browser_reviews (workspace_id, comments, summary_text, created_at, updated_at)
         VALUES (
             ?1,
             ?2,
             ?3,
             COALESCE((SELECT created_at FROM file_browser_reviews WHERE workspace_id = ?1), ?4),
             ?4
         )",
        params![workspace_id, comments, summary_text, now],
    )
    .map_err(|e| format!("Failed to save file browser review: {}", e))?;

  Ok(conn.last_insert_rowid())
}

/// Clear the FileBrowser's pending review for a workspace
pub fn clear_file_browser_review(repo_path: &str, workspace_id: i64) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "DELETE FROM file_browser_reviews WHERE workspace_id = ?1",
      [workspace_id],
    )
    .map_err(|e| format!("Failed to clear file browser review: {}", e))?;
  Ok(())
}

// Pending Page Review Functions (in-app browser)

/// Get pending page review for a workspace
pub fn get_pending_page_review(
  repo_path: &str,
  workspace_id: i64,
) -> Result<Option<PendingPageReview>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(
      "SELECT id, workspace_id, url, comments, summary_text, created_at, updated_at
             FROM pending_page_reviews
             WHERE workspace_id = ?1",
    )
    .map_err(|e| format!("Failed to prepare query: {}", e))?;

  let review = stmt
    .query_row([workspace_id], |row| {
      Ok(PendingPageReview {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        url: row.get(2)?,
        comments: row.get(3)?,
        summary_text: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
      })
    })
    .optional()
    .map_err(|e| format!("Failed to get pending page review: {}", e))?;

  Ok(review)
}

/// Save or update pending page review for a workspace
pub fn save_pending_page_review(
  repo_path: &str,
  workspace_id: i64,
  url: &str,
  comments: &str,
  summary_text: Option<&str>,
) -> Result<i64, String> {
  let conn = get_connection(repo_path)?;
  let now = Utc::now().to_rfc3339();

  conn.execute(
        "INSERT OR REPLACE INTO pending_page_reviews (workspace_id, url, comments, summary_text, created_at, updated_at)
         VALUES (
             ?1,
             ?2,
             ?3,
             ?4,
             COALESCE((SELECT created_at FROM pending_page_reviews WHERE workspace_id = ?1), ?5),
             ?5
         )",
        params![workspace_id, url, comments, summary_text, now],
    )
    .map_err(|e| format!("Failed to save pending page review: {}", e))?;

  Ok(conn.last_insert_rowid())
}

/// Clear pending page review for a workspace
pub fn clear_pending_page_review(repo_path: &str, workspace_id: i64) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "DELETE FROM pending_page_reviews WHERE workspace_id = ?1",
      [workspace_id],
    )
    .map_err(|e| format!("Failed to clear pending page review: {}", e))?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  fn cached_file(relative_path: &str) -> CachedWorkspaceFile {
    CachedWorkspaceFile {
      id: 0,
      workspace_id: None,
      file_path: relative_path.to_string(),
      relative_path: relative_path.to_string(),
      is_directory: false,
      parent_path: None,
      cached_at: "2026-08-26T00:00:00Z".to_string(),
      mtime: None,
    }
  }

  #[test]
  fn migrates_legacy_stashes_foreign_key_to_set_null() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();
    let db_path = get_local_db_path(repo_path);
    fs::create_dir_all(db_path.parent().unwrap()).expect("create .treq directory");
    let conn = Connection::open(&db_path).expect("open legacy database");
    conn
      .execute_batch(
        "CREATE TABLE workspaces (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           workspace_name TEXT NOT NULL,
           workspace_path TEXT NOT NULL UNIQUE,
           branch_name TEXT NOT NULL,
           created_at TEXT NOT NULL,
           metadata TEXT,
           target_branch TEXT
         );
         INSERT INTO workspaces
           (id, workspace_name, workspace_path, branch_name, created_at)
           VALUES (7, 'legacy', 'legacy', 'feat/legacy', '2026-01-01T00:00:00Z');
         CREATE TABLE stashes (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           workspace_id INTEGER,
           workspace_label TEXT NOT NULL,
           commit_id TEXT NOT NULL,
           change_id TEXT NOT NULL,
           short_commit_id TEXT NOT NULL,
           bookmark_name TEXT NOT NULL,
           created_at TEXT NOT NULL,
           additions INTEGER NOT NULL DEFAULT 0,
           deletions INTEGER NOT NULL DEFAULT 0,
           files_changed TEXT NOT NULL DEFAULT '[]',
           FOREIGN KEY (workspace_id) REFERENCES workspaces(id)
         );
         INSERT INTO stashes
           (id, workspace_id, workspace_label, commit_id, change_id, short_commit_id,
            bookmark_name, created_at, additions, deletions, files_changed)
           VALUES (11, 7, 'Legacy workspace', 'commit', 'change', 'short',
                   'treq-stash-11', '2026-01-02T00:00:00Z', 3, 4, '[\"src/lib.rs\"]');",
      )
      .expect("create legacy schema");
    drop(conn);

    init_local_db(repo_path).expect("migrate legacy database");

    let conn = Connection::open(&db_path).expect("reopen migrated database");
    let on_delete: String = conn
      .query_row(
        "SELECT on_delete FROM pragma_foreign_key_list('stashes') WHERE \"from\" = 'workspace_id'",
        [],
        |row| row.get(0),
      )
      .expect("stash foreign key");
    assert_eq!(on_delete, "SET NULL");

    let stash = get_stash_by_id(repo_path, 11)
      .expect("read migrated stash")
      .expect("stash should survive migration");
    assert_eq!(stash.workspace_id, Some(7));
    assert_eq!(stash.workspace_label, "Legacy workspace");
    assert_eq!(stash.commit_id, "commit");
    assert_eq!(stash.change_id, "change");
    assert_eq!(stash.short_commit_id, "short");
    assert_eq!(stash.bookmark_name, "treq-stash-11");
    assert_eq!(stash.created_at, "2026-01-02T00:00:00Z");
    assert_eq!(stash.additions, 3);
    assert_eq!(stash.deletions, 4);
    assert_eq!(stash.files_changed, vec!["src/lib.rs"]);
  }

  #[test]
  fn deletes_workspace_and_nulls_stash_workspace_id() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();
    let workspace_id = add_workspace(
      repo_path,
      "stash-owner".to_string(),
      "stash-owner".to_string(),
      "feat/stash-owner".to_string(),
      None,
      None,
      None,
    )
    .expect("add workspace");
    let stash = add_stash(
      repo_path,
      Some(workspace_id),
      "Stash owner",
      "commit",
      "change",
      "short",
      "treq-stash-owner",
      5,
      2,
      &["README.md".to_string()],
    )
    .expect("add stash");

    delete_workspace(repo_path, workspace_id).expect("delete workspace with stash");

    assert!(get_workspace_by_id(repo_path, workspace_id)
      .expect("lookup workspace")
      .is_none());
    let preserved = get_stash_by_id(repo_path, stash.id)
      .expect("lookup stash")
      .expect("stash should survive");
    assert_eq!(preserved.workspace_id, None);
    assert_eq!(preserved.workspace_label, "Stash owner");
    assert_eq!(preserved.commit_id, "commit");
    assert_eq!(preserved.bookmark_name, "treq-stash-owner");
  }

  #[test]
  fn searches_workspace_files_ignoring_query_spaces() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    sync_workspace_files(repo_path, None, vec![cached_file("makefile")]).expect("index files");

    let matches =
      search_workspace_files(repo_path, None, "make file", 50).expect("search workspace files");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].relative_path, "makefile");
  }

  #[test]
  fn searches_workspace_files_by_ordered_lazy_characters() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    sync_workspace_files(
      repo_path,
      None,
      vec![
        cached_file("src-tauri/.gitignore"),
        cached_file("src/components/FilePicker.tsx"),
      ],
    )
    .expect("index files");

    let matches =
      search_workspace_files(repo_path, None, "taurgit", 50).expect("search workspace files");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].relative_path, "src-tauri/.gitignore");
  }

  fn add_archived_workspace(repo_path: &str, path: &str, branch: &str) -> i64 {
    let id = add_workspace(
      repo_path,
      path.to_string(),
      path.to_string(),
      branch.to_string(),
      None,
      None,
      None,
    )
    .expect("register workspace");
    archive_workspace(repo_path, id).expect("archive workspace");
    id
  }

  #[test]
  fn get_workspace_by_branch_ignores_archived_rows() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    init_local_db(repo_path).expect("initialize database");
    add_archived_workspace(repo_path, "fix-example", "fix/example");

    assert!(get_workspace_by_branch(repo_path, "fix/example")
      .expect("lookup")
      .is_none());
  }

  #[test]
  fn add_workspace_reuses_path_held_by_archived_row() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    init_local_db(repo_path).expect("initialize database");
    let archived_id = add_archived_workspace(repo_path, "fix-example", "fix/example");

    let live_id = add_workspace(
      repo_path,
      "fix-example".to_string(),
      "fix-example".to_string(),
      "fix/example".to_string(),
      None,
      None,
      None,
    )
    .expect("register workspace at a path held by an archived row");

    assert_ne!(live_id, archived_id);
    let live = get_workspace_by_branch(repo_path, "fix/example")
      .expect("lookup")
      .expect("live workspace");
    assert_eq!(live.id, live_id);
    assert_eq!(live.workspace_path, "fix-example");
    let archived = get_workspace_by_id(repo_path, archived_id)
      .expect("lookup archived")
      .expect("archived row kept");
    assert!(archived.archived);
    assert_ne!(archived.workspace_path, "fix-example");
  }

  #[test]
  fn discovery_does_not_revive_archived_row_at_same_path() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    init_local_db(repo_path).expect("initialize database");
    let archived_id = add_archived_workspace(repo_path, "fix-example", "fix/example");

    let discovered = vec![crate::jj::DiscoveredWorkspace {
      workspace_name: "fix-example".to_string(),
      workspace_path: "fix-example".to_string(),
      branch_name: "fix/example".to_string(),
      has_conflicts: false,
      last_activity_at: None,
    }];
    let workspaces = sync_discovered_workspaces(repo_path, &discovered, "2026-08-08T00:00:00Z")
      .expect("sync discovery");

    assert_eq!(workspaces.len(), 1);
    assert_ne!(workspaces[0].id, archived_id);
    assert!(!workspaces[0].archived);
  }

  #[test]
  fn discovery_keeps_renamed_branch_when_no_bookmark_matches_directory() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    init_local_db(repo_path).expect("initialize database");
    add_workspace(
      repo_path,
      "feat-alpha".to_string(),
      "feat-alpha".to_string(),
      "feat/beta".to_string(),
      None,
      None,
      None,
    )
    .expect("register renamed workspace");

    let discovered = vec![crate::jj::DiscoveredWorkspace {
      workspace_name: "feat-alpha".to_string(),
      workspace_path: "feat-alpha".to_string(),
      branch_name: "feat-alpha".to_string(),
      has_conflicts: false,
      last_activity_at: None,
    }];
    let workspaces = sync_discovered_workspaces(repo_path, &discovered, "2026-08-08T00:00:00Z")
      .expect("sync discovery");

    assert_eq!(workspaces[0].branch_name, "feat/beta");
  }

  #[test]
  fn conflicted_discovery_preserves_canonical_branch_name() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    init_local_db(repo_path).expect("initialize database");
    add_workspace(
      repo_path,
      "fix-example".to_string(),
      "fix-example".to_string(),
      "fix/example".to_string(),
      None,
      None,
      None,
    )
    .expect("register workspace");

    let discovered = vec![crate::jj::DiscoveredWorkspace {
      workspace_name: "fix-example".to_string(),
      workspace_path: "fix-example".to_string(),
      branch_name: "fix-example".to_string(),
      has_conflicts: true,
      last_activity_at: None,
    }];
    let workspaces = sync_discovered_workspaces(repo_path, &discovered, "2026-08-08T00:00:00Z")
      .expect("sync discovery");

    assert_eq!(workspaces[0].branch_name, "fix/example");
  }

  #[test]
  fn conflicted_discovery_recovers_sanitized_branch_name_when_canonical_is_known() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    init_local_db(repo_path).expect("initialize database");
    add_workspace(
      repo_path,
      "fix-example".to_string(),
      "fix-example".to_string(),
      "fix-example".to_string(),
      None,
      None,
      None,
    )
    .expect("register workspace with corrupted dashed branch_name");

    let discovered = vec![crate::jj::DiscoveredWorkspace {
      workspace_name: "fix-example".to_string(),
      workspace_path: "fix-example".to_string(),
      branch_name: "fix/example".to_string(),
      has_conflicts: true,
      last_activity_at: None,
    }];
    let workspaces = sync_discovered_workspaces(repo_path, &discovered, "2026-08-08T00:00:00Z")
      .expect("sync discovery");

    assert_eq!(
      workspaces[0].branch_name, "fix/example",
      "canonical slash bookmark from discovery must repair a prior dashed fallback"
    );
  }

  #[test]
  fn init_local_db_creates_instance_registry_table() {
    use rusqlite::Connection;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let db_path = init_local_db(repo_path).expect("init_local_db should succeed");
    let conn = Connection::open(db_path).expect("Failed to open database");

    let table_exists: i64 = conn
      .query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'instance_registry'",
        [],
        |row| row.get(0),
      )
      .expect("Should query sqlite_master");
    assert_eq!(table_exists, 1, "instance_registry table should exist");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn upsert_and_list_instance_registry_round_trip_windows_json() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let instance = RegisteredInstance {
      instance_id: "inst-1".to_string(),
      pid: 12345,
      started_at: 1000,
      last_heartbeat_at: 2000,
      endpoint: "127.0.0.1:1234".to_string(),
      windows: vec![InstanceWindowSnapshot {
        window_label: "main".to_string(),
        normalized_repo_path: "/tmp/repo-a".to_string(),
        focused: true,
        last_focused_at: Some(1900),
        supporting: false,
      }],
    };

    upsert_instance_registry(repo_path, instance.clone()).expect("upsert should succeed");
    let instances = list_instance_registry(repo_path).expect("list should succeed");
    assert_eq!(instances, vec![instance]);

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn prune_stale_instance_registry_removes_old_rows_only() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    upsert_instance_registry(
      repo_path,
      RegisteredInstance {
        instance_id: "alive".to_string(),
        pid: 1,
        started_at: 100,
        last_heartbeat_at: 95,
        endpoint: "127.0.0.1:1".to_string(),
        windows: vec![],
      },
    )
    .expect("alive upsert should succeed");
    upsert_instance_registry(
      repo_path,
      RegisteredInstance {
        instance_id: "stale".to_string(),
        pid: 2,
        started_at: 100,
        last_heartbeat_at: 10,
        endpoint: "127.0.0.1:2".to_string(),
        windows: vec![],
      },
    )
    .expect("stale upsert should succeed");

    prune_stale_instance_registry(repo_path, 100, 10).expect("prune should succeed");
    let instances = list_instance_registry(repo_path).expect("list should succeed");
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].instance_id, "alive");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_add_workspace_persists_to_db() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let workspace_dir = temp_dir
      .path()
      .join(".treq")
      .join("workspaces")
      .join("test-workspace");
    fs::create_dir_all(&workspace_dir).expect("Failed to create workspace dir");
    let workspace_path = workspace_dir.to_str().unwrap().to_string();

    assert!(workspace_dir.exists(), "Workspace directory should exist");

    let id = add_workspace(
      repo_path,
      "test-workspace".to_string(),
      workspace_path.clone(),
      "test-branch".to_string(),
      Some("test description".to_string()),
      None,
      None,
    )
    .expect("add_workspace should succeed");

    assert!(id > 0, "Workspace ID should be positive");

    let workspaces = get_workspaces(repo_path).expect("get_workspaces should succeed");

    assert_eq!(workspaces.len(), 1, "Should have exactly 1 workspace");
    assert_eq!(workspaces[0].id, id);
    assert_eq!(workspaces[0].workspace_name, "test-workspace");
    assert_eq!(workspaces[0].workspace_path, workspace_path);
    assert_eq!(workspaces[0].branch_name, "test-branch");
    assert_eq!(workspaces[0].metadata, None);
    assert_eq!(
      workspaces[0].description.as_deref(),
      Some("test description")
    );
    assert!(workspaces[0].target_branch.is_none());
    assert!(workspaces[0].hidden_until.is_none());

    let db_path = get_local_db_path(repo_path);
    assert!(
      db_path.exists(),
      "Database file should exist at {:?}",
      db_path
    );

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_add_multiple_workspaces() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let id1 = add_workspace(
      repo_path,
      "workspace-1".to_string(),
      format!("{}/.treq/workspaces/workspace-1", repo_path),
      "branch-a".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace 1 should succeed");

    let id2 = add_workspace(
      repo_path,
      "workspace-2".to_string(),
      format!("{}/.treq/workspaces/workspace-2", repo_path),
      "branch-b".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace 2 should succeed");

    let workspaces = get_workspaces(repo_path).expect("get_workspaces should succeed");

    assert_eq!(workspaces.len(), 2, "Should have 2 workspaces");
    assert_eq!(workspaces[0].id, id1);
    assert_eq!(workspaces[0].workspace_name, "workspace-1");
    assert_eq!(workspaces[1].id, id2);
    assert_eq!(workspaces[1].workspace_name, "workspace-2");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_workspace_persists_after_reload() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let id = add_workspace(
      repo_path,
      "test-workspace".to_string(),
      format!("{}/.treq/workspaces/test-workspace", repo_path),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    let workspaces = get_workspaces(repo_path).expect("get_workspaces should succeed");
    assert_eq!(workspaces.len(), 1, "Workspace should exist initially");
    assert_eq!(workspaces[0].id, id);

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }

    let workspaces_after_reload =
      get_workspaces(repo_path).expect("get_workspaces should succeed after reload");
    assert_eq!(
      workspaces_after_reload.len(),
      1,
      "Workspace should persist after reload (BUG: this fails!)"
    );
    assert_eq!(workspaces_after_reload[0].id, id);
    assert_eq!(workspaces_after_reload[0].workspace_name, "test-workspace");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_get_workspaces_by_target_branch() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let id1 = add_workspace(
      repo_path,
      "workspace-1".to_string(),
      format!("{}/.treq/workspaces/workspace-1", repo_path),
      "branch-a".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace 1 should succeed");
    update_workspace_target_branch(repo_path, id1, "main")
      .expect("update target branch should succeed");

    let id2 = add_workspace(
      repo_path,
      "workspace-2".to_string(),
      format!("{}/.treq/workspaces/workspace-2", repo_path),
      "branch-b".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace 2 should succeed");
    update_workspace_target_branch(repo_path, id2, "develop")
      .expect("update target branch should succeed");

    let id3 = add_workspace(
      repo_path,
      "workspace-3".to_string(),
      format!("{}/.treq/workspaces/workspace-3", repo_path),
      "branch-c".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace 3 should succeed");
    update_workspace_target_branch(repo_path, id3, "main")
      .expect("update target branch should succeed");

    let main_workspaces =
      get_workspaces_by_target_branch(repo_path, "main").expect("query should succeed");

    assert_eq!(main_workspaces.len(), 2);
    assert_eq!(main_workspaces[0].id, id1);
    assert_eq!(main_workspaces[1].id, id3);
    assert_eq!(main_workspaces[0].target_branch, Some("main".to_string()));
    assert_eq!(main_workspaces[1].target_branch, Some("main".to_string()));

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_add_workspace_sets_refreshed_at() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let id = add_workspace(
      repo_path,
      "workspace-refresh".to_string(),
      "workspace-refresh".to_string(),
      "branch-refresh".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    let workspace = get_workspace_by_id(repo_path, id)
      .expect("lookup should succeed")
      .expect("workspace should exist");

    assert!(workspace.refreshed_at.is_some());

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_upsert_workspace_preserves_existing_metadata_and_created_at() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let id = add_workspace(
      repo_path,
      "workspace-upsert".to_string(),
      "workspace-upsert".to_string(),
      "branch-before".to_string(),
      Some("keep me".to_string()),
      Some(vec!["src/main.rs".to_string()]),
      None,
    )
    .expect("add_workspace should succeed");
    update_workspace_target_branch(repo_path, id, "main")
      .expect("update target branch should succeed");
    update_workspace_not_on_remote(repo_path, id, true)
      .expect("update not_on_remote should succeed");

    let original = get_workspace_by_id(repo_path, id)
      .expect("lookup should succeed")
      .expect("workspace should exist");
    let original_created_at = original.created_at.clone();

    let refreshed_at = "2026-05-01T10:00:00Z";
    let upserted_id = upsert_workspace_discovery(
      repo_path,
      "workspace-upsert",
      "workspace-upsert",
      "branch-after",
      refreshed_at,
    )
    .expect("upsert should succeed");

    assert_eq!(upserted_id, id);

    let workspace = get_workspace_by_id(repo_path, id)
      .expect("lookup should succeed")
      .expect("workspace should exist");
    assert_eq!(workspace.branch_name, "branch-after");
    assert_eq!(workspace.target_branch.as_deref(), Some("main"));
    assert_eq!(workspace.description.as_deref(), Some("keep me"));
    assert_eq!(workspace.moved_files, Some(vec!["src/main.rs".to_string()]));
    assert!(workspace.not_on_remote);
    assert_eq!(workspace.created_at, original_created_at);
    assert_eq!(workspace.refreshed_at.as_deref(), Some(refreshed_at));

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_save_and_load_pending_review() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    // Create a workspace first
    let workspace_id = add_workspace(
      repo_path,
      "test".to_string(),
      format!("{}/.treq/workspaces/test", repo_path),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    // Save a pending review
    let comments = r#"[{"id":"c1","filePath":"test.txt","hunkId":"h1","startLine":1,"endLine":1,"lineContent":["+new"],"text":"Comment","createdAt":"2025-01-15T10:00:00Z"}]"#;
    let viewed_files = r#"["file1.txt","file2.txt"]"#;
    let summary = "Overall summary";

    let id = save_pending_review(
      repo_path,
      workspace_id,
      comments,
      Some(viewed_files),
      Some(summary),
      None,
    )
    .expect("save_pending_review should succeed");
    assert!(id > 0, "Review ID should be positive");

    // Load it back
    let review =
      get_pending_review(repo_path, workspace_id).expect("get_pending_review should succeed");
    assert!(review.is_some(), "Review should exist");

    let review = review.unwrap();
    assert_eq!(review.workspace_id, workspace_id);
    assert_eq!(review.comments, comments);
    assert_eq!(review.viewed_files, Some(viewed_files.to_string()));
    assert_eq!(review.summary_text, Some(summary.to_string()));

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_pending_review_unique_constraint() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    // Create a workspace
    let workspace_id = add_workspace(
      repo_path,
      "test".to_string(),
      format!("{}/.treq/workspaces/test", repo_path),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    // Save first review
    let comments1 = r#"[{"id":"c1","text":"First comment"}]"#;
    save_pending_review(repo_path, workspace_id, comments1, None, None, None)
      .expect("First save should succeed");

    // Save again with different data - should update, not create duplicate
    let comments2 = r#"[{"id":"c2","text":"Second comment"}]"#;
    save_pending_review(
      repo_path,
      workspace_id,
      comments2,
      None,
      Some("New summary"),
      None,
    )
    .expect("Second save should succeed");

    // Verify only one review exists with updated data
    let review = get_pending_review(repo_path, workspace_id).expect("get should succeed");
    assert!(review.is_some());

    let review = review.unwrap();
    assert_eq!(review.comments, comments2);
    assert_eq!(review.summary_text, Some("New summary".to_string()));

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_clear_pending_review() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    // Create a workspace
    let workspace_id = add_workspace(
      repo_path,
      "test".to_string(),
      format!("{}/.treq/workspaces/test", repo_path),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    // Save a review
    save_pending_review(
      repo_path,
      workspace_id,
      r#"[{"id":"c1"}]"#,
      None,
      None,
      None,
    )
    .expect("save should succeed");

    // Verify it exists
    let review = get_pending_review(repo_path, workspace_id).expect("get should succeed");
    assert!(review.is_some());

    // Clear it
    clear_pending_review(repo_path, workspace_id).expect("clear should succeed");

    // Verify it's gone
    let review = get_pending_review(repo_path, workspace_id).expect("get should succeed");
    assert!(review.is_none(), "Review should be cleared");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_pending_review_cascade_delete() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    // Create a workspace
    let workspace_id = add_workspace(
      repo_path,
      "test".to_string(),
      format!("{}/.treq/workspaces/test", repo_path),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    // Save a review
    save_pending_review(
      repo_path,
      workspace_id,
      r#"[{"id":"c1"}]"#,
      None,
      None,
      None,
    )
    .expect("save should succeed");

    // Verify it exists
    let review = get_pending_review(repo_path, workspace_id).expect("get should succeed");
    assert!(review.is_some());

    // Delete the workspace
    delete_workspace(repo_path, workspace_id).expect("delete_workspace should succeed");

    // Verify the review is also deleted (CASCADE)
    let review = get_pending_review(repo_path, workspace_id).expect("get should succeed");
    assert!(
      review.is_none(),
      "Review should be deleted when workspace is deleted"
    );

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_pending_review_preserves_created_at_on_update() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    // Create a workspace
    let workspace_id = add_workspace(
      repo_path,
      "test".to_string(),
      format!("{}/.treq/workspaces/test", repo_path),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    // Save first review
    save_pending_review(
      repo_path,
      workspace_id,
      r#"[{"id":"c1"}]"#,
      None,
      None,
      None,
    )
    .expect("save should succeed");

    let first_review = get_pending_review(repo_path, workspace_id)
      .expect("get should succeed")
      .expect("review should exist");
    let first_created_at = first_review.created_at.clone();

    // Wait a moment
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Update the review
    save_pending_review(
      repo_path,
      workspace_id,
      r#"[{"id":"c2"}]"#,
      None,
      None,
      None,
    )
    .expect("update should succeed");

    let updated_review = get_pending_review(repo_path, workspace_id)
      .expect("get should succeed")
      .expect("review should exist");

    // created_at should be the same
    assert_eq!(updated_review.created_at, first_created_at);
    // updated_at should be different
    assert_ne!(updated_review.updated_at, first_created_at);

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_pending_review_migration_from_old_schema() {
    use rusqlite::Connection;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join(".treq").join("old_schema.db");
    fs::create_dir_all(db_path.parent().unwrap()).expect("Failed to create .treq dir");

    // Create database with old schema
    let conn = Connection::open(&db_path).expect("Failed to open database");

    // Create workspaces table (needed for foreign key)
    conn
      .execute(
        "CREATE TABLE workspaces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_name TEXT NOT NULL,
                workspace_path TEXT NOT NULL,
                branch_name TEXT NOT NULL,
                metadata TEXT,
                target_branch TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        [],
      )
      .expect("Failed to create workspaces table");

    let workspace_id = 1;
    conn.execute(
            "INSERT INTO workspaces (workspace_name, workspace_path, branch_name, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)",
            ["test-ws", "/path/to/ws", "test-branch", "2025-01-15T10:00:00Z", "2025-01-15T10:00:00Z"],
        )
        .expect("Failed to insert workspace");

    // Create pending_reviews table with OLD schema
    conn
      .execute(
        "CREATE TABLE pending_reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_id INTEGER NOT NULL UNIQUE,
                comments_json TEXT NOT NULL,
                overall_comment TEXT NOT NULL,
                viewed_files_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            )",
        [],
      )
      .expect("Failed to create old pending_reviews table");

    // Insert test data with old column names
    let test_comments = r#"[{"id":"c1","filePath":"test.txt","text":"Test comment"}]"#;
    let test_viewed_files = r#"["file1.txt","file2.txt"]"#;
    let test_summary = "Overall test comment";
    let test_created = "2025-01-15T10:00:00Z";
    let test_updated = "2025-01-15T11:00:00Z";

    conn.execute(
            "INSERT INTO pending_reviews (workspace_id, comments_json, overall_comment, viewed_files_json, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            [workspace_id.to_string().as_str(), test_comments, test_summary, test_viewed_files, test_created, test_updated],
        )
        .expect("Failed to insert old data");

    drop(conn); // Close connection before calling init_local_db

    // Now rename the database to the expected location
    let repo_path = temp_dir.path().to_str().unwrap();
    let expected_db_path = get_local_db_path(repo_path);
    fs::create_dir_all(expected_db_path.parent().unwrap()).expect("Failed to create .treq dir");
    fs::rename(&db_path, &expected_db_path).expect("Failed to move database");

    // Clear the cache so init_local_db will process the old database
    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }

    // Call init_local_db to trigger migration
    init_local_db(repo_path).expect("init_local_db should succeed");

    // Verify the table has new column names
    let conn = Connection::open(&expected_db_path).expect("Failed to open database");

    // Check that new columns exist
    let mut stmt = conn
            .prepare("SELECT comments, viewed_files, summary_text, created_at, updated_at FROM pending_reviews WHERE workspace_id = 1")
            .expect("Should be able to query new columns");

    let review = stmt
      .query_row([], |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, Option<String>>(1)?,
          row.get::<_, Option<String>>(2)?,
          row.get::<_, String>(3)?,
          row.get::<_, String>(4)?,
        ))
      })
      .expect("Should find the migrated row");

    assert_eq!(review.0, test_comments, "comments should match");
    assert_eq!(
      review.1,
      Some(test_viewed_files.to_string()),
      "viewed_files should match"
    );
    assert_eq!(
      review.2,
      Some(test_summary.to_string()),
      "summary_text should match"
    );
    assert_eq!(review.3, test_created, "created_at should match");
    assert_eq!(review.4, test_updated, "updated_at should match");

    // Verify old columns don't exist
    let old_columns: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('pending_reviews') WHERE name IN ('comments_json', 'overall_comment', 'viewed_files_json')",
                [],
                |row| row.get(0),
            )
            .expect("Should be able to count old columns");
    assert_eq!(
      old_columns, 0,
      "Old columns should not exist after migration"
    );

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_pending_page_review_round_trip() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let workspace_id = add_workspace(
      repo_path,
      "test".to_string(),
      format!("{}/.treq/workspaces/test", repo_path),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    let comments = r##"[{"id":"c1","selector":"#foo","tag":"BUTTON","textPreview":"Submit","text":"Comment","createdAt":"2025-01-15T10:00:00Z"}]"##;
    let url = "http://localhost:3000/checkout";
    let summary = "Overall page feedback";

    let id = save_pending_page_review(repo_path, workspace_id, url, comments, Some(summary))
      .expect("save_pending_page_review should succeed");
    assert!(id > 0, "Review ID should be positive");

    let review = get_pending_page_review(repo_path, workspace_id)
      .expect("get_pending_page_review should succeed");
    assert!(review.is_some(), "Review should exist");

    let review = review.unwrap();
    assert_eq!(review.workspace_id, workspace_id);
    assert_eq!(review.url, url);
    assert_eq!(review.comments, comments);
    assert_eq!(review.summary_text, Some(summary.to_string()));

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_pending_page_review_unique_constraint() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let workspace_id = add_workspace(
      repo_path,
      "test".to_string(),
      format!("{}/.treq/workspaces/test", repo_path),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    save_pending_page_review(
      repo_path,
      workspace_id,
      "http://localhost:3000/",
      r#"[{"id":"c1"}]"#,
      None,
    )
    .expect("First save should succeed");

    save_pending_page_review(
      repo_path,
      workspace_id,
      "http://localhost:3000/cart",
      r#"[{"id":"c2"}]"#,
      Some("New summary"),
    )
    .expect("Second save should succeed");

    let review = get_pending_page_review(repo_path, workspace_id).expect("get should succeed");
    assert!(review.is_some());

    let review = review.unwrap();
    assert_eq!(review.url, "http://localhost:3000/cart");
    assert_eq!(review.comments, r#"[{"id":"c2"}]"#);
    assert_eq!(review.summary_text, Some("New summary".to_string()));

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_clear_pending_page_review() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let workspace_id = add_workspace(
      repo_path,
      "test".to_string(),
      format!("{}/.treq/workspaces/test", repo_path),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    save_pending_page_review(
      repo_path,
      workspace_id,
      "file:///tmp/preview.html",
      r#"[{"id":"c1"}]"#,
      None,
    )
    .expect("save should succeed");

    let review = get_pending_page_review(repo_path, workspace_id).expect("get should succeed");
    assert!(review.is_some());

    clear_pending_page_review(repo_path, workspace_id).expect("clear should succeed");

    let review = get_pending_page_review(repo_path, workspace_id).expect("get should succeed");
    assert!(review.is_none(), "Review should be cleared");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_pending_page_review_missing_returns_none() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let workspace_id = add_workspace(
      repo_path,
      "test".to_string(),
      format!("{}/.treq/workspaces/test", repo_path),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    let review = get_pending_page_review(repo_path, workspace_id).expect("get should succeed");
    assert!(review.is_none());

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_pending_review_migration_idempotent() {
    use rusqlite::Connection;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join(".treq").join("idempotent.db");
    fs::create_dir_all(db_path.parent().unwrap()).expect("Failed to create .treq dir");

    // Create database with old schema
    let conn = Connection::open(&db_path).expect("Failed to open database");

    // Create workspaces table
    conn
      .execute(
        "CREATE TABLE workspaces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_name TEXT NOT NULL,
                workspace_path TEXT NOT NULL,
                branch_name TEXT NOT NULL,
                metadata TEXT,
                target_branch TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        [],
      )
      .expect("Failed to create workspaces table");

    let workspace_id = 1;
    conn.execute(
            "INSERT INTO workspaces (workspace_name, workspace_path, branch_name, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)",
            ["ws1", "/path/to/ws1", "branch1", "2025-01-15T10:00:00Z", "2025-01-15T10:00:00Z"],
        )
        .expect("Failed to insert workspace");

    // Create old schema table
    conn
      .execute(
        "CREATE TABLE pending_reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_id INTEGER NOT NULL UNIQUE,
                comments_json TEXT NOT NULL,
                overall_comment TEXT NOT NULL,
                viewed_files_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            )",
        [],
      )
      .expect("Failed to create old pending_reviews table");

    // Insert test data
    conn.execute(
            "INSERT INTO pending_reviews (workspace_id, comments_json, overall_comment, viewed_files_json, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            [workspace_id.to_string().as_str(), r#"[{"id":"c1"}]"#, "Test", r#"["f1"]"#, "2025-01-15T10:00:00Z", "2025-01-15T11:00:00Z"],
        )
        .expect("Failed to insert data");

    drop(conn);

    // Move database to expected location
    let repo_path = temp_dir.path().to_str().unwrap();
    let expected_db_path = get_local_db_path(repo_path);
    fs::create_dir_all(expected_db_path.parent().unwrap()).expect("Failed to create .treq dir");
    fs::rename(&db_path, &expected_db_path).expect("Failed to move database");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }

    // First migration
    init_local_db(repo_path).expect("First init_local_db should succeed");

    // Get row count after first migration
    let conn = Connection::open(&expected_db_path).expect("Failed to open database");
    let count_after_first: i64 = conn
      .query_row("SELECT COUNT(*) FROM pending_reviews", [], |row| row.get(0))
      .expect("Should count rows");
    assert_eq!(
      count_after_first, 1,
      "Should have exactly 1 row after first migration"
    );

    drop(conn);

    // Second migration (should be idempotent)
    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
    init_local_db(repo_path).expect("Second init_local_db should succeed");

    // Verify data and count after second migration
    let conn = Connection::open(&expected_db_path).expect("Failed to open database");
    let count_after_second: i64 = conn
      .query_row("SELECT COUNT(*) FROM pending_reviews", [], |row| row.get(0))
      .expect("Should count rows");
    assert_eq!(
      count_after_second, 1,
      "Should still have exactly 1 row after second migration"
    );

    // Verify data integrity
    let review = conn
      .query_row(
        "SELECT comments FROM pending_reviews WHERE workspace_id = 1",
        [],
        |row| row.get::<_, String>(0),
      )
      .expect("Should find row");
    assert_eq!(review, r#"[{"id":"c1"}]"#, "Data should be intact");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_pending_review_migration_empty_table() {
    use rusqlite::Connection;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join(".treq").join("empty.db");
    fs::create_dir_all(db_path.parent().unwrap()).expect("Failed to create .treq dir");

    // Create database with old schema but no data
    let conn = Connection::open(&db_path).expect("Failed to open database");

    conn
      .execute(
        "CREATE TABLE workspaces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_name TEXT NOT NULL,
                workspace_path TEXT NOT NULL,
                branch_name TEXT NOT NULL,
                metadata TEXT,
                target_branch TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        [],
      )
      .expect("Failed to create workspaces table");

    // Create old schema table with no data
    conn
      .execute(
        "CREATE TABLE pending_reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_id INTEGER NOT NULL UNIQUE,
                comments_json TEXT NOT NULL,
                overall_comment TEXT NOT NULL,
                viewed_files_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            )",
        [],
      )
      .expect("Failed to create old pending_reviews table");

    drop(conn);

    // Move database to expected location
    let repo_path = temp_dir.path().to_str().unwrap();
    let expected_db_path = get_local_db_path(repo_path);
    fs::create_dir_all(expected_db_path.parent().unwrap()).expect("Failed to create .treq dir");
    fs::rename(&db_path, &expected_db_path).expect("Failed to move database");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }

    // Call init_local_db to trigger migration
    init_local_db(repo_path).expect("init_local_db should succeed with empty table");

    // Verify the table still exists and has new schema
    let conn = Connection::open(&expected_db_path).expect("Failed to open database");

    // Try to query new columns (should succeed with no rows)
    let count: i64 = conn
      .query_row("SELECT COUNT(*) FROM pending_reviews", [], |row| row.get(0))
      .expect("Should be able to count rows");
    assert_eq!(count, 0, "Table should be empty after migration");

    // Verify new columns exist
    let mut stmt = conn
      .prepare("SELECT comments, viewed_files, summary_text FROM pending_reviews LIMIT 0")
      .expect("Should be able to query new columns");
    let _ = stmt
      .query([])
      .expect("Should execute query with new columns");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn test_pending_review_migration_multiple_workspaces() {
    use rusqlite::Connection;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join(".treq").join("multi.db");
    fs::create_dir_all(db_path.parent().unwrap()).expect("Failed to create .treq dir");

    // Create database with old schema
    let conn = Connection::open(&db_path).expect("Failed to open database");

    // Create workspaces table
    conn
      .execute(
        "CREATE TABLE workspaces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_name TEXT NOT NULL,
                workspace_path TEXT NOT NULL,
                branch_name TEXT NOT NULL,
                metadata TEXT,
                target_branch TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        [],
      )
      .expect("Failed to create workspaces table");

    // Create 3 workspaces
    for i in 1..=3 {
      conn.execute(
                "INSERT INTO workspaces (workspace_name, workspace_path, branch_name, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?)",
                [
                    &format!("ws{}", i),
                    &format!("/path/to/ws{}", i),
                    &format!("branch{}", i),
                    "2025-01-15T10:00:00Z",
                    "2025-01-15T10:00:00Z",
                ],
            )
            .expect("Failed to insert workspace");
    }

    // Create old schema table
    conn
      .execute(
        "CREATE TABLE pending_reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_id INTEGER NOT NULL UNIQUE,
                comments_json TEXT NOT NULL,
                overall_comment TEXT NOT NULL,
                viewed_files_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            )",
        [],
      )
      .expect("Failed to create old pending_reviews table");

    // Insert pending reviews for all 3 workspaces
    for i in 1..=3 {
      conn.execute(
                "INSERT INTO pending_reviews (workspace_id, comments_json, overall_comment, viewed_files_json, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
                [
                    &i.to_string(),
                    &format!(r#"[{{"id":"c{}","text":"comment{}"}}]"#, i, i),
                    &format!("Summary {}", i),
                    &format!(r#"["file{}.txt"]"#, i),
                    "2025-01-15T10:00:00Z",
                    "2025-01-15T11:00:00Z",
                ],
            )
            .expect("Failed to insert pending review");
    }

    drop(conn);

    // Move database to expected location
    let repo_path = temp_dir.path().to_str().unwrap();
    let expected_db_path = get_local_db_path(repo_path);
    fs::create_dir_all(expected_db_path.parent().unwrap()).expect("Failed to create .treq dir");
    fs::rename(&db_path, &expected_db_path).expect("Failed to move database");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }

    // Call init_local_db to trigger migration
    init_local_db(repo_path).expect("init_local_db should succeed");

    // Verify all 3 reviews were migrated correctly
    let conn = Connection::open(&expected_db_path).expect("Failed to open database");

    for i in 1..=3 {
      let review = conn
        .query_row(
          "SELECT comments, summary_text, viewed_files FROM pending_reviews WHERE workspace_id = ?",
          [i],
          |row| {
            Ok((
              row.get::<_, String>(0)?,
              row.get::<_, Option<String>>(1)?,
              row.get::<_, Option<String>>(2)?,
            ))
          },
        )
        .unwrap_or_else(|e| panic!("Should find pending review for workspace {}: {}", i, e));

      assert_eq!(
        review.0,
        format!(r#"[{{"id":"c{}","text":"comment{}"}}]"#, i, i)
      );
      assert_eq!(review.1, Some(format!("Summary {}", i)));
      assert_eq!(review.2, Some(format!(r#"["file{}.txt"]"#, i)));
    }

    // Verify total count
    let total_count: i64 = conn
      .query_row("SELECT COUNT(*) FROM pending_reviews", [], |row| row.get(0))
      .expect("Should count rows");
    assert_eq!(total_count, 3, "Should have exactly 3 reviews");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn add_prompt_history_persists_and_lists_most_recent_first() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let workspace_id = add_workspace(
      repo_path,
      "feature-one".to_string(),
      format!("{}/.treq/workspaces/feature-one", repo_path),
      "feature-one".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    let first_id = add_prompt_history(
      repo_path,
      Some(workspace_id),
      None,
      "first prompt",
      Some("claude"),
    )
    .expect("add_prompt_history should succeed");
    std::thread::sleep(std::time::Duration::from_millis(2));
    let second_id = add_prompt_history(repo_path, None, None, "second prompt", None)
      .expect("add_prompt_history should succeed");

    let history = get_prompt_history(repo_path).expect("get_prompt_history should succeed");

    assert_eq!(history.len(), 2, "Should have exactly 2 prompt entries");
    // Most recent first
    assert_eq!(history[0].id, second_id);
    assert_eq!(history[0].prompt_text, "second prompt");
    assert_eq!(history[0].workspace_id, None);
    assert_eq!(history[0].workspace_label, None);
    assert_eq!(history[1].id, first_id);
    assert_eq!(history[1].prompt_text, "first prompt");
    assert_eq!(history[1].agent.as_deref(), Some("claude"));
    assert_eq!(history[1].workspace_id, Some(workspace_id));
    assert_eq!(history[1].workspace_label.as_deref(), Some("feature-one"));

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn deletes_workspace_prompt_history_and_sessions() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let workspace_id = add_workspace(
      repo_path,
      "feature-delete".to_string(),
      format!("{}/.treq/workspaces/feature-delete", repo_path),
      "feature-delete".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");
    let session_id = add_session(repo_path, Some(workspace_id), "Delete me".to_string())
      .expect("add_session should succeed");
    add_prompt_history(
      repo_path,
      Some(workspace_id),
      Some(session_id),
      "delete this prompt",
      Some("codex"),
    )
    .expect("add_prompt_history should succeed");

    delete_workspace(repo_path, workspace_id).expect("delete_workspace should cascade");

    assert!(get_prompt_history(repo_path)
      .expect("get_prompt_history should succeed")
      .is_empty());
    assert!(get_sessions(repo_path)
      .expect("get_sessions should succeed")
      .is_empty());

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn get_workspace_starting_prompt_returns_earliest_prompt_for_workspace() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let workspace_id = add_workspace(
      repo_path,
      "feature-two".to_string(),
      format!("{}/.treq/workspaces/feature-two", repo_path),
      "feature-two".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    add_prompt_history(
      repo_path,
      Some(workspace_id),
      None,
      "the starting prompt",
      Some("claude"),
    )
    .expect("add_prompt_history should succeed");
    std::thread::sleep(std::time::Duration::from_millis(2));
    add_prompt_history(
      repo_path,
      Some(workspace_id),
      None,
      "a later followup prompt",
      Some("claude"),
    )
    .expect("add_prompt_history should succeed");

    let starting_prompt = get_workspace_starting_prompt(repo_path, workspace_id)
      .expect("get_workspace_starting_prompt should succeed")
      .expect("should find a starting prompt");

    assert_eq!(starting_prompt.prompt_text, "the starting prompt");

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }

  #[test]
  fn get_workspace_starting_prompt_returns_none_when_no_prompts_recorded() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap();

    let workspace_id = add_workspace(
      repo_path,
      "feature-three".to_string(),
      format!("{}/.treq/workspaces/feature-three", repo_path),
      "feature-three".to_string(),
      None,
      None,
      None,
    )
    .expect("add_workspace should succeed");

    let starting_prompt = get_workspace_starting_prompt(repo_path, workspace_id)
      .expect("get_workspace_starting_prompt should succeed");

    assert!(starting_prompt.is_none());

    if let Some(initialized) = INITIALIZED_DBS.get() {
      initialized.lock_or_recover().remove(repo_path);
    }
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowRunRecord {
  pub id: i64,
  pub filename: String,
  pub status: String,
  pub started_at: String,
  pub completed_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowJobRecord {
  pub id: i64,
  pub run_id: i64,
  pub job_id: String,
  pub position: i64,
  pub status: String,
  pub started_at: Option<String>,
  pub completed_at: Option<String>,
  pub steps_json: String,
  pub log_path: Option<String>,
}

/// Opens a new run row in the `running` state; jobs are attached as they finish.
pub fn create_workflow_run(
  repo_path: &str,
  workspace_id: i64,
  filename: &str,
) -> Result<i64, String> {
  let conn = get_connection(repo_path)?;
  let started_at = Utc::now().to_rfc3339();
  conn
    .execute(
      "INSERT INTO workflow_runs (workspace_id, filename, status, started_at)
         VALUES (?1, ?2, 'running', ?3)",
      params![workspace_id, filename, started_at],
    )
    .map_err(|e| format!("Failed to insert workflow_run: {}", e))?;
  Ok(conn.last_insert_rowid())
}

#[allow(clippy::too_many_arguments)]
pub fn add_workflow_job_result(
  repo_path: &str,
  run_id: i64,
  job_id: &str,
  position: i64,
  status: &str,
  started_at: &str,
  completed_at: &str,
  steps_json: &str,
  log_path: Option<&str>,
) -> Result<i64, String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "INSERT INTO workflow_job_results
           (run_id, job_id, position, status, started_at, completed_at, steps_json, log_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
      params![
        run_id,
        job_id,
        position,
        status,
        started_at,
        completed_at,
        steps_json,
        log_path
      ],
    )
    .map_err(|e| format!("Failed to insert workflow_job_result: {}", e))?;
  Ok(conn.last_insert_rowid())
}

pub fn finish_workflow_run(repo_path: &str, run_id: i64, status: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  let completed_at = Utc::now().to_rfc3339();
  conn
    .execute(
      "UPDATE workflow_runs SET status = ?1, completed_at = ?2 WHERE id = ?3",
      params![status, completed_at, run_id],
    )
    .map_err(|e| format!("Failed to finish workflow_run: {}", e))?;
  Ok(())
}

pub fn list_workflow_runs(
  repo_path: &str,
  workspace_id: i64,
  filename: &str,
  limit: i64,
) -> Result<Vec<WorkflowRunRecord>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(
      "SELECT id, filename, status, started_at, completed_at
             FROM workflow_runs
             WHERE workspace_id = ?1 AND filename = ?2
             ORDER BY id DESC LIMIT ?3",
    )
    .map_err(|e| format!("Failed to prepare workflow_runs query: {}", e))?;
  let rows = stmt
    .query_map(params![workspace_id, filename, limit], |row| {
      Ok(WorkflowRunRecord {
        id: row.get(0)?,
        filename: row.get(1)?,
        status: row.get(2)?,
        started_at: row.get(3)?,
        completed_at: row.get(4)?,
      })
    })
    .map_err(|e| format!("Failed to query workflow_runs: {}", e))?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| format!("Failed to read workflow_runs: {}", e))
}

pub fn list_workflow_job_results(
  repo_path: &str,
  run_id: i64,
) -> Result<Vec<WorkflowJobRecord>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(
      "SELECT id, run_id, job_id, position, status, started_at, completed_at,
                    steps_json, log_path
             FROM workflow_job_results
             WHERE run_id = ?1
             ORDER BY position ASC",
    )
    .map_err(|e| format!("Failed to prepare job results query: {}", e))?;
  let rows = stmt
    .query_map(params![run_id], |row| {
      Ok(WorkflowJobRecord {
        id: row.get(0)?,
        run_id: row.get(1)?,
        job_id: row.get(2)?,
        position: row.get(3)?,
        status: row.get(4)?,
        started_at: row.get(5)?,
        completed_at: row.get(6)?,
        steps_json: row.get(7)?,
        log_path: row.get(8)?,
      })
    })
    .map_err(|e| format!("Failed to query job results: {}", e))?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| format!("Failed to read job results: {}", e))
}

pub fn get_job_log_path(
  repo_path: &str,
  run_id: i64,
  job_id: &str,
) -> Result<Option<String>, String> {
  let conn = get_connection(repo_path)?;
  conn
    .query_row(
      "SELECT log_path FROM workflow_job_results WHERE run_id = ?1 AND job_id = ?2",
      params![run_id, job_id],
      |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(|opt| opt.flatten())
    .map_err(|e| format!("Failed to query job log path: {}", e))
}

pub fn is_repo_trusted(repo_path: &str) -> bool {
  get_connection(repo_path)
    .and_then(|conn| {
      conn
        .query_row("SELECT COUNT(*) FROM repo_trust", [], |row| {
          row.get::<_, i64>(0)
        })
        .map_err(|e| format!("Failed to query repo_trust: {}", e))
    })
    .map(|count| count > 0)
    .unwrap_or(false)
}

/// Supporting repository paths linked to the main repository at `repo_path`,
/// in the order they were added.
pub fn list_supporting_repo_paths(repo_path: &str) -> Result<Vec<String>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare("SELECT repo_path FROM supporting_repos ORDER BY id")
    .map_err(|e| format!("Failed to prepare supporting_repos query: {}", e))?;
  let rows = stmt
    .query_map([], |row| row.get::<_, String>(0))
    .map_err(|e| format!("Failed to query supporting_repos: {}", e))?;
  rows
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| format!("Failed to read supporting_repos row: {}", e))
}

pub fn add_supporting_repo_path(repo_path: &str, supporting_path: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "INSERT OR IGNORE INTO supporting_repos (repo_path, added_at) VALUES (?1, ?2)",
      params![supporting_path, Utc::now().to_rfc3339()],
    )
    .map_err(|e| format!("Failed to add supporting repo: {}", e))?;
  Ok(())
}

pub fn remove_supporting_repo_path(repo_path: &str, supporting_path: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "DELETE FROM supporting_repos WHERE repo_path = ?1",
      params![supporting_path],
    )
    .map_err(|e| format!("Failed to remove supporting repo: {}", e))?;
  Ok(())
}

pub fn trust_repo(repo_path: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  let trusted_at = Utc::now().to_rfc3339();
  conn
    .execute(
      "INSERT OR IGNORE INTO repo_trust (id, trusted_at) VALUES (1, ?1)",
      params![trusted_at],
    )
    .map_err(|e| format!("Failed to trust repo: {}", e))?;
  Ok(())
}
