use crate::lock_ext::LockExt;
use crate::result_ext::ResultExt;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::agent_dispatch::{InstanceWindowSnapshot, RegisteredInstance};
use crate::jj::DiscoveredWorkspace;

mod migrations;
mod pool;
use pool::{ConnectionPool, PooledConnection};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
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
  /// True when the workspace directory was archived; the DB row is kept.
  #[serde(default)]
  pub archived: bool,
}

/// The PR values last copied into a workspace (`core::pr_sync`). A field is
/// `None` when it does not follow the PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrSyncRecord {
  pub pr_number: u64,
  pub title: Option<String>,
  pub description: Option<String>,
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

/// Held for a whole `init_local_db`: its migrations check for a column, then
/// add it, so two threads migrating one file both add it and the second fails.
static INIT_LOCK: Mutex<()> = Mutex::new(());

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
    archived: row.get::<_, i64>(13)? != 0,
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
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
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
  /// Where the comment came from: `"local-agent"`, or `"local-human"` for a
  /// PR review draft the user wrote.
  pub source: String,
  pub created_at: String,
  pub resolved_at: Option<String>,
  /// The exact text of `start_line..=end_line` when the comment was written.
  /// Set for Linear targets, whose suggestions are applied by text match.
  #[serde(default)]
  pub quoted_text: Option<String>,
  /// Hash of the commented diff lines when the comment was anchored; a PR
  /// draft whose lines no longer hash the same is outdated.
  #[serde(default)]
  pub line_hash: Option<String>,
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

/// Marks a `repo_path` argument as the client-side state scope of a remote
/// repository (its descriptor key follows), not a path on this machine.
pub const REMOTE_STATE_SCOPE_PREFIX: &str = "treq-remote-state:";

/// Where a repository's client-side state database lives.
#[derive(Debug, PartialEq, Eq)]
pub enum RepoStateScope<'a> {
  /// A repository on this machine: `<path>/.treq/local.db`.
  Local(&'a str),
  /// A remote repository, keyed by its descriptor identity. Its state lives
  /// under the app data directory, so nothing is created at the remote path.
  Remote(&'a str),
}

impl<'a> RepoStateScope<'a> {
  pub fn parse(repo_arg: &'a str) -> Self {
    match repo_arg.strip_prefix(REMOTE_STATE_SCOPE_PREFIX) {
      Some(key) => Self::Remote(key),
      None => Self::Local(repo_arg),
    }
  }

  pub fn db_path(&self, app_data_dir: Option<&Path>) -> Result<PathBuf, String> {
    match self {
      Self::Local(path) => Ok(get_local_db_path(path)),
      Self::Remote(key) => {
        let dir = app_data_dir.ok_or("Remote repository state needs the app data directory")?;
        let digest = Sha256::digest(key.as_bytes());
        let name: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(dir.join("remote-state").join(name).join("local.db"))
      }
    }
  }

  /// A remote scope holds review state only: its workspaces live on the
  /// remote host, so no local `workspaces` row exists for them to reference.
  pub fn enforces_foreign_keys(&self) -> bool {
    matches!(self, Self::Local(_))
  }
}

fn resolve_db_path(repo_arg: &str) -> Result<PathBuf, String> {
  RepoStateScope::parse(repo_arg).db_path(crate::core::app_data_dir().as_deref())
}

/// Initialize the local database for a repository.
///
/// Creates tables for workspaces, sessions, changed_files, and workspace_files.
/// Handles schema migrations for backward compatibility.
///
/// # Returns
/// The path to the created/opened database file.
pub fn init_local_db(repo_path: &str) -> Result<PathBuf, String> {
  let _init = INIT_LOCK.lock_or_recover();
  let db_path = resolve_db_path(repo_path)?;
  if let Some(parent) = db_path.parent() {
    fs::create_dir_all(parent).map_err(|e| format!("Failed to create .treq directory: {}", e))?;
  }

  let conn = Connection::open(&db_path).map_err(|e| format!("Failed to open local db: {}", e))?;

  // Every remote request cold-starts `treq` and lands here, so the common
  // case (schema already current) must cost one pragma read, not ~70 statements.
  if migrations::is_current(&conn, migrations::MIGRATIONS)? {
    return Ok(db_path);
  }

  migrations::run(&conn, migrations::MIGRATIONS)?;

  Ok(db_path)
}

/// Idle connections kept open between `get_connection` calls (see `pool`).
/// 16 covers the repos and remote scopes of a few open windows; each one
/// holds two file handles.
static CONNECTION_POOL: ConnectionPool = ConnectionPool::new(16, 4);

/// Get a database connection for a repository.
///
/// Ensures the database is initialized before returning the connection.
/// Uses a cache to avoid re-initializing databases that have already been set up in this session.
/// The connection goes back to `CONNECTION_POOL` when dropped.
fn get_connection(repo_path: &str) -> Result<PooledConnection<'static>, String> {
  let initialized = INITIALIZED_DBS.get_or_init(|| Mutex::new(HashSet::new()));
  let db_key = repo_path.to_string();

  {
    let guard = initialized.lock_or_recover();
    if !guard.contains(&db_key) {
      drop(guard);
      init_local_db(repo_path)?;
      initialized.lock_or_recover().insert(db_key);
      prune_review_history_now(repo_path);
      start_review_history_pruner();
    }
  }

  let db_path = resolve_db_path(repo_path)?;
  CONNECTION_POOL.check_out(&db_path, || open_connection(repo_path, &db_path))
}

fn open_connection(repo_path: &str, db_path: &Path) -> Result<Connection, String> {
  let conn = Connection::open(db_path).map_err(|e| format!("Failed to open local db: {}", e))?;
  let enforce_foreign_keys = RepoStateScope::parse(repo_path).enforces_foreign_keys();
  conn
    .pragma_update(None, "foreign_keys", enforce_foreign_keys)
    .map_err(|e| format!("Failed to enable foreign key enforcement: {}", e))?;
  // Multiple treq instances/windows can share this db file (see
  // `upsert_instance_registry`). WAL lets readers and a writer proceed
  // concurrently instead of blocking, and busy_timeout makes a genuine
  // writer-writer conflict retry for a bit instead of immediately failing
  // with "database is locked".
  conn
    .pragma_update(None, "journal_mode", "WAL")
    .map_err(|e| format!("Failed to enable WAL mode: {}", e))?;
  // NORMAL is crash-safe under WAL (it can lose the last commits on power
  // loss, never corrupt) and avoids an fsync per commit.
  conn
    .pragma_update(None, "synchronous", "NORMAL")
    .map_err(|e| format!("Failed to set synchronous mode: {}", e))?;
  conn
    .busy_timeout(std::time::Duration::from_secs(5))
    .map_err(|e| format!("Failed to set busy timeout: {}", e))?;
  Ok(conn)
}

/// Closes the idle connections to databases under `dir`, so the directory
/// can be moved or deleted (Windows refuses while a file in it is open).
pub fn release_connections_under(dir: &Path) {
  CONNECTION_POOL.release_under(dir);
}

/// Closes every idle connection. The last connection to close a database
/// checkpoints its WAL, so the next process to open it need not replay it.
/// Call before the process exits.
pub fn close_idle_connections() {
  CONNECTION_POOL.release_all();
}

pub fn upsert_instance_registry(
  repo_path: &str,
  instance: RegisteredInstance,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  upsert_instance_registry_row(&conn, instance)
}

fn upsert_instance_registry_row(
  conn: &Connection,
  instance: RegisteredInstance,
) -> Result<(), String> {
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
  prune_stale_instance_registry_rows(&conn, now, timeout_ms)
}

fn prune_stale_instance_registry_rows(
  conn: &Connection,
  now: u64,
  timeout_ms: u64,
) -> Result<(), String> {
  let threshold = now.saturating_sub(timeout_ms) as i64;
  conn
    .execute(
      "DELETE FROM instance_registry WHERE last_heartbeat_at < ?1",
      params![threshold],
    )
    .map_err(|e| format!("Failed to prune stale instance registry rows: {}", e))?;
  Ok(())
}

/// Prunes stale rows and upserts this instance's row in one transaction on
/// one connection, so a heartbeat costs a single commit.
pub fn heartbeat_instance_registry(
  repo_path: &str,
  now: u64,
  timeout_ms: u64,
  instance: RegisteredInstance,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  let tx = conn
    .unchecked_transaction()
    .map_err(|e| format!("Failed to start heartbeat transaction: {}", e))?;
  prune_stale_instance_registry_rows(&tx, now, timeout_ms)?;
  upsert_instance_registry_row(&tx, instance)?;
  tx.commit()
    .map_err(|e| format!("Failed to commit heartbeat transaction: {}", e))
}

const WORKSPACE_SELECT_COLUMNS: &str = "id, workspace_name, workspace_path, branch_name, created_at, refreshed_at, metadata, target_branch, COALESCE(not_on_remote, 0), COALESCE(title, branch_name), moved_files, description, sparse_patterns, COALESCE(archived, 0)";

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
  workspace_by_id(&conn, repo_path, id)
}

fn workspace_by_id(
  conn: &Connection,
  repo_path: &str,
  id: i64,
) -> Result<Option<Workspace>, String> {
  let mut stmt = conn
    .prepare(&format!(
      "SELECT {WORKSPACE_SELECT_COLUMNS} FROM workspaces WHERE id = ?1"
    ))
    .map_err(|e| format!("Failed to prepare workspace query: {}", e))?;

  stmt
    .query_row([id], |row| workspace_from_row(repo_path, row))
    .optional()
    .map_err(|e| format!("Failed to query workspace: {}", e))
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
  // Cached: discovery runs this for every workspace on every poll.
  conn
    .prepare_cached(
      "UPDATE workspaces SET workspace_path = workspace_path || '#archived-' || id
       WHERE workspace_path = ?1 AND COALESCE(archived, 0) = 1",
    )
    .and_then(|mut statement| statement.execute([workspace_path]))
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
  // Releasing the archived row's path and inserting must land together.
  let tx = conn
    .unchecked_transaction()
    .map_err(|e| format!("Failed to start workspace insert: {}", e))?;
  let id = insert_workspace(
    &tx,
    &workspace_name,
    &workspace_path,
    &branch_name,
    description,
    moved_files,
    sparse_patterns,
  )?;
  tx.commit()
    .map_err(|e| format!("Failed to commit workspace insert: {}", e))?;
  Ok(id)
}

/// [`add_workspace`] that also sets what a creation knows (whether the branch
/// is missing on the remote, the target branch) in the same transaction, and
/// returns the row. Each separate write is a commit with its own fsyncs.
#[expect(
  clippy::too_many_arguments,
  reason = "the columns of one insert, as add_workspace takes them"
)]
pub fn add_workspace_targeting(
  repo_path: &str,
  workspace_name: String,
  workspace_path: String,
  branch_name: String,
  description: Option<String>,
  moved_files: Option<Vec<String>>,
  sparse_patterns: Option<Vec<String>>,
  not_on_remote: bool,
  target_branch: &str,
) -> Result<Workspace, String> {
  let conn = get_connection(repo_path)?;
  let tx = conn
    .unchecked_transaction()
    .map_err(|e| format!("Failed to start workspace insert: {}", e))?;
  let id = insert_workspace(
    &tx,
    &workspace_name,
    &workspace_path,
    &branch_name,
    description,
    moved_files,
    sparse_patterns,
  )?;
  tx.execute(
    "UPDATE workspaces SET not_on_remote = ?1, target_branch = ?2 WHERE id = ?3",
    params![not_on_remote as i64, target_branch, id],
  )
  .map_err(|e| format!("Failed to set workspace target branch: {}", e))?;
  let workspace = workspace_by_id(&tx, repo_path, id)?
    .ok_or_else(|| format!("Workspace not found in database after creation: {id}"))?;
  tx.commit()
    .map_err(|e| format!("Failed to commit workspace insert: {}", e))?;
  Ok(workspace)
}

/// Prepares a freshly created workspace to show: sets its `title` when given,
/// and clears the last rebased target commit, which makes its first view
/// rebase. One transaction, and the row back.
pub fn init_new_workspace_state(
  repo_path: &str,
  id: i64,
  title: Option<&str>,
) -> Result<Option<Workspace>, String> {
  let conn = get_connection(repo_path)?;
  let tx = conn
    .unchecked_transaction()
    .map_err(|e| format!("Failed to start workspace update: {}", e))?;
  if let Some(title) = title {
    tx.execute(
      "UPDATE workspaces SET title = ?1 WHERE id = ?2",
      params![title, id],
    )
    .map_err(|e| format!("Failed to update workspace title: {}", e))?;
  }
  set_last_rebased_commit(&tx, id, "")?;
  let workspace = workspace_by_id(&tx, repo_path, id)?;
  tx.commit()
    .map_err(|e| format!("Failed to commit workspace update: {}", e))?;
  Ok(workspace)
}

fn insert_workspace(
  conn: &Connection,
  workspace_name: &str,
  workspace_path: &str,
  branch_name: &str,
  description: Option<String>,
  moved_files: Option<Vec<String>>,
  sparse_patterns: Option<Vec<String>>,
) -> Result<i64, String> {
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

  release_archived_workspace_path(conn, workspace_path)?;
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

fn pr_sync_record(row: &Row) -> rusqlite::Result<(i64, PrSyncRecord)> {
  Ok((
    row.get(0)?,
    PrSyncRecord {
      pr_number: row.get::<_, i64>(1)? as u64,
      title: row.get(2)?,
      description: row.get(3)?,
    },
  ))
}

pub fn get_workspace_pr_sync(
  repo_path: &str,
  workspace_id: i64,
) -> Result<Option<PrSyncRecord>, String> {
  let conn = get_connection(repo_path)?;
  conn
    .query_row(
      "SELECT workspace_id, pr_number, title, description FROM workspace_pr_sync WHERE workspace_id = ?1",
      [workspace_id],
      pr_sync_record,
    )
    .optional()
    .map(|row| row.map(|(_, record)| record))
    .map_err(|e| format!("Failed to read workspace PR sync: {}", e))
}

pub fn list_workspace_pr_syncs(
  repo_path: &str,
) -> Result<std::collections::HashMap<i64, PrSyncRecord>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare("SELECT workspace_id, pr_number, title, description FROM workspace_pr_sync")
    .map_err(|e| format!("Failed to list workspace PR syncs: {}", e))?;
  let rows = stmt
    .query_map([], pr_sync_record)
    .map_err(|e| format!("Failed to list workspace PR syncs: {}", e))?;
  rows
    .collect::<rusqlite::Result<_>>()
    .map_err(|e| format!("Failed to list workspace PR syncs: {}", e))
}

pub fn set_workspace_pr_sync(
  repo_path: &str,
  workspace_id: i64,
  record: &PrSyncRecord,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "INSERT INTO workspace_pr_sync (workspace_id, pr_number, title, description)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(workspace_id) DO UPDATE SET
           pr_number = excluded.pr_number,
           title = excluded.title,
           description = excluded.description",
      params![
        workspace_id,
        record.pr_number as i64,
        record.title,
        record.description
      ],
    )
    .map_err(|e| format!("Failed to save workspace PR sync: {}", e))?;
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

/// The base branch each workspace's pull request had when treq last polled
/// it, by workspace id. Workspaces never seen with an open PR are absent.
pub fn get_workspace_pr_bases(repo_path: &str) -> Result<HashMap<i64, String>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare("SELECT id, pr_base_branch FROM workspaces WHERE pr_base_branch IS NOT NULL")
    .map_err(|e| format!("Failed to prepare PR base query: {}", e))?;
  let rows = stmt
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
    .map_err(|e| format!("Failed to query PR bases: {}", e))?;
  rows
    .collect::<Result<HashMap<_, _>, _>>()
    .map_err(|e| e.to_string())
}

pub fn set_workspace_pr_base(repo_path: &str, id: i64, base_branch: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE workspaces SET pr_base_branch = ?1 WHERE id = ?2",
      params![base_branch, id],
    )
    .map_err(|e| format!("Failed to update workspace PR base: {}", e))?;
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

/// A commit's diff stat and log fields, as the log caches them.
#[derive(Debug, Clone)]
pub struct CommitRow {
  pub commit_id: String,
  pub insertions: u32,
  pub deletions: u32,
  pub change_id: String,
  pub description: String,
  pub author_name: String,
  pub timestamp: String,
}

/// Caches `rows` in one transaction.
pub fn cache_commit_rows(repo_path: &str, rows: &[CommitRow]) -> Result<(), String> {
  if rows.is_empty() {
    return Ok(());
  }
  let mut conn = get_connection(repo_path)?;
  let tx = conn
    .transaction()
    .map_err(|e| format!("Failed to start commit row transaction: {}", e))?;
  {
    let mut stmt = tx
      .prepare(
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
      )
      .map_err(|e| format!("Failed to prepare commit row cache: {}", e))?;
    let updated_at = Utc::now().to_rfc3339();
    for row in rows {
      stmt
        .execute(params![
          row.commit_id,
          row.insertions,
          row.deletions,
          updated_at,
          row.change_id,
          row.description,
          row.author_name,
          row.timestamp,
        ])
        .map_err(|e| format!("Failed to cache commit row: {}", e))?;
    }
  }
  tx.commit()
    .map_err(|e| format!("Failed to commit commit row cache: {}", e))
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

  let tx = conn
    .unchecked_transaction()
    .map_err(|e| format!("Failed to start workspace upsert: {}", e))?;
  release_archived_workspace_path(&tx, workspace_path)?;
  tx.execute(
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

  let id = tx
    .query_row(
      "SELECT id FROM workspaces WHERE workspace_path = ?1",
      [workspace_path],
      |row| row.get(0),
    )
    .map_err(|e| format!("Failed to fetch upserted workspace id: {}", e))?;
  tx.commit()
    .map_err(|e| format!("Failed to commit workspace upsert: {}", e))?;
  Ok(id)
}

/// Upserts discovered workspaces and returns the persisted rows. A row is
/// written only when discovery changed it (or it was never refreshed), so the
/// periodic poll commits nothing in the steady state; `refreshed_at` is the
/// time of the last such change.
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
    // Prepared once per sync rather than once per workspace, which was
    // half of a poll's cost.
    tx.prepare_cached(
            "INSERT INTO workspaces (workspace_name, workspace_path, branch_name, created_at, refreshed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(workspace_path) DO UPDATE SET
                 workspace_name = CASE WHEN ?6 THEN excluded.workspace_name ELSE workspaces.workspace_name END,
                 branch_name = CASE WHEN ?7 THEN excluded.branch_name ELSE workspaces.branch_name END,
                 refreshed_at = excluded.refreshed_at
             WHERE workspaces.refreshed_at IS NULL
                 OR (?6 AND workspaces.workspace_name IS NOT excluded.workspace_name)
                 OR (?7 AND workspaces.branch_name IS NOT excluded.branch_name)",
        )
        .and_then(|mut statement| {
            statement.execute(params![
                workspace.workspace_name,
                workspace.workspace_path,
                workspace.branch_name,
                refreshed_at,
                refreshed_at,
                update_workspace_name,
                update_branch_name,
            ])
        })
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

/// Last rebased target commit recorded in an already-loaded workspace's metadata.
pub fn last_rebased_commit_of(workspace: &Workspace) -> Option<String> {
  let json = serde_json::from_str::<serde_json::Value>(workspace.metadata.as_deref()?).ok()?;
  json
    .get("last_rebased_target_commit")
    .and_then(|v| v.as_str())
    .map(|s| s.to_string())
}

/// Why the workspace's last rebase was held back (`core::rebase_guard`), as
/// stored in its metadata.
pub fn rebase_hold_of(workspace: &Workspace) -> Option<serde_json::Value> {
  let json = serde_json::from_str::<serde_json::Value>(workspace.metadata.as_deref()?).ok()?;
  json.get("rebase_hold").filter(|v| !v.is_null()).cloned()
}

/// Stores the workspace's rebase hold in its metadata; `None` clears it.
pub fn set_workspace_rebase_hold(
  repo_path: &str,
  id: i64,
  hold: Option<serde_json::Value>,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  let current: Option<String> = conn
    .query_row(
      "SELECT metadata FROM workspaces WHERE id = ?1",
      params![id],
      |row| row.get(0),
    )
    .map_err(|e| format!("Failed to read metadata: {}", e))?;
  let mut meta = current
    .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok())
    .filter(|m| m.is_object())
    .unwrap_or(serde_json::json!({}));
  if meta.get("rebase_hold") == hold.as_ref()
    || (hold.is_none() && meta.get("rebase_hold").is_none())
  {
    return Ok(());
  }
  match hold {
    Some(hold) => meta["rebase_hold"] = hold,
    None => {
      if let Some(map) = meta.as_object_mut() {
        map.remove("rebase_hold");
      }
    }
  }
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

/// Get last rebased commit from workspace metadata
pub fn get_workspace_last_rebased_commit(
  repo_path: &str,
  id: i64,
) -> Result<Option<String>, String> {
  let workspaces = get_workspaces(repo_path)?;
  Ok(
    workspaces
      .iter()
      .find(|w| w.id == id)
      .and_then(last_rebased_commit_of),
  )
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
  set_last_rebased_commit(&conn, id, commit_id)
}

/// Records the same rebased target commit for several workspaces in one
/// transaction (one commit instead of one per workspace).
pub fn update_workspaces_last_rebased_commit(
  repo_path: &str,
  ids: &[i64],
  commit_id: &str,
) -> Result<(), String> {
  if ids.is_empty() {
    return Ok(());
  }
  let conn = get_connection(repo_path)?;
  let tx = conn
    .unchecked_transaction()
    .map_err(|e| format!("Failed to start rebase bookkeeping: {}", e))?;
  for id in ids {
    set_last_rebased_commit(&tx, *id, commit_id)?;
  }
  tx.commit()
    .map_err(|e| format!("Failed to commit rebase bookkeeping: {}", e))
}

fn set_last_rebased_commit(conn: &Connection, id: i64, commit_id: &str) -> Result<(), String> {
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
               AND relative_path LIKE ?2 ESCAPE '^'
             ORDER BY
               CASE
                 WHEN LOWER(relative_path) = ?3 OR relative_path LIKE ?4 ESCAPE '^' THEN 0
                 WHEN relative_path LIKE ?5 ESCAPE '^' THEN 1
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

  {
    let mut insert = tx
      .prepare_cached(
        "INSERT INTO workspace_files
             (workspace_id, file_path, relative_path, is_directory, parent_path, cached_at, mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
      )
      .map_err(|e| format!("Failed to prepare file insert: {}", e))?;
    for file in &files {
      insert
        .execute(params![
          workspace_id,
          &file.file_path,
          &file.relative_path,
          if file.is_directory { 1 } else { 0 },
          &file.parent_path,
          &file.cached_at,
          &file.mtime,
        ])
        .map_err(|e| format!("Failed to insert file: {}", e))?;
    }
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
            comment_text, suggested_replacement, status, source, created_at, resolved_at, quoted_text,
            line_hash
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
    quoted_text: row.get(15)?,
    line_hash: row.get(16)?,
  })
}

/// Insert one local review comment. Returns the stored row.
#[expect(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
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
  quoted_text: Option<&str>,
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
    quoted_text: quoted_text.map(|s| s.to_string()),
    line_hash: None,
  };

  conn.execute(
        "INSERT INTO agent_review_comments (id, repo_path, target_type, target_id, file_path, hunk_id, start_line, end_line, side, comment_text, suggested_replacement, status, source, created_at, resolved_at, quoted_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
            comment.quoted_text,
        ],
    )
    .map_err(|e| format!("Failed to insert agent review comment: {}", e))?;

  if is_code_review_target(&comment.target_type) {
    conn
      .execute(
        "INSERT INTO review_history (kind, comment_id, source, target_type, target_id, file_path, start_line, end_line, body, suggestion, quoted_text, outcome, created_at)
         VALUES ('agent_comment', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'open', ?11)",
        params![
          comment.id,
          comment.source,
          comment.target_type,
          comment.target_id,
          comment.file_path,
          comment.start_line,
          comment.end_line,
          comment.comment_text,
          comment.suggested_replacement,
          comment.quoted_text,
          comment.created_at,
        ],
      )
      .map_err(|e| format!("Failed to record review history: {}", e))?;
  }

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
  decide_review_history(&conn, id, "resolved", &["open"])
}

/// Records that the user applied comment `id`'s suggestion.
pub fn mark_review_history_applied(repo_path: &str, id: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  decide_review_history(&conn, id, "applied", &["open", "resolved"])
}

/// Edit a comment's text, or record the hash and text of the lines it
/// covers (PR review drafts); `None` keeps the stored value.
pub fn update_agent_review_comment(
  repo_path: &str,
  id: &str,
  comment_text: Option<&str>,
  line_hash: Option<&str>,
  quoted_text: Option<&str>,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "UPDATE agent_review_comments
         SET comment_text = COALESCE(?2, comment_text), line_hash = COALESCE(?3, line_hash),
             quoted_text = COALESCE(?4, quoted_text)
       WHERE id = ?1",
      params![id, comment_text, line_hash, quoted_text],
    )
    .map_err(|e| format!("Failed to update agent review comment: {e}"))?;
  Ok(())
}

/// Delete one local review comment.
pub fn delete_agent_review_comment(repo_path: &str, id: &str) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute("DELETE FROM agent_review_comments WHERE id = ?1", [id])
    .map_err(|e| format!("Failed to delete agent review comment: {}", e))?;
  decide_review_history(&conn, id, "dismissed", &["open"])
}

// Review History Functions

/// How long review history is kept.
pub const REVIEW_HISTORY_MAX_AGE: chrono::Duration = chrono::Duration::days(30);
/// How often every open repository's review history is pruned.
const REVIEW_HISTORY_PRUNE_EVERY: std::time::Duration =
  std::time::Duration::from_secs(12 * 60 * 60);

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ReviewHistoryEntry {
  pub id: i64,
  /// `agent_comment` or `user_review`.
  pub kind: String,
  pub comment_id: Option<String>,
  pub source: Option<String>,
  pub workspace_id: Option<i64>,
  pub target_type: String,
  pub target_id: Option<String>,
  pub file_path: Option<String>,
  pub start_line: Option<i64>,
  pub end_line: Option<i64>,
  pub body: String,
  pub suggestion: Option<String>,
  pub quoted_text: Option<String>,
  pub mode: Option<String>,
  /// `open`, `resolved`, `applied` or `dismissed` for an agent comment;
  /// `sent` for a user review.
  pub outcome: String,
  pub created_at: String,
  pub decided_at: Option<String>,
}

/// Linear reviews have their own prompt and review prompt edits are not
/// code review; history covers code review only.
fn is_code_review_target(target_type: &str) -> bool {
  !target_type.starts_with("linear") && target_type != "review_prompt"
}

fn decide_review_history(
  conn: &Connection,
  comment_id: &str,
  outcome: &str,
  from: &[&str],
) -> Result<(), String> {
  let from = from
    .iter()
    .map(|o| format!("'{o}'"))
    .collect::<Vec<_>>()
    .join(",");
  conn
    .execute(
      &format!(
        "UPDATE review_history SET outcome = ?2, decided_at = ?3
         WHERE comment_id = ?1 AND outcome IN ({from})"
      ),
      params![comment_id, outcome, Utc::now().to_rfc3339()],
    )
    .map_err(|e| format!("Failed to record review outcome: {}", e))?;
  Ok(())
}

/// Records a review the user sent to an agent: `scope` is `changes` or
/// `files`, `body` the markdown sent.
pub fn add_user_review_history(
  repo_path: &str,
  workspace_id: Option<i64>,
  scope: &str,
  body: &str,
  mode: &str,
) -> Result<(), String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "INSERT INTO review_history (kind, workspace_id, target_type, body, mode, outcome, created_at)
       VALUES ('user_review', ?1, ?2, ?3, ?4, 'sent', ?5)",
      params![workspace_id, scope, body, mode, Utc::now().to_rfc3339()],
    )
    .map_err(|e| format!("Failed to record review history: {}", e))?;
  Ok(())
}

/// The newest `limit` review history rows, newest first.
pub fn list_review_history(
  repo_path: &str,
  limit: usize,
) -> Result<Vec<ReviewHistoryEntry>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare(
      "SELECT id, kind, comment_id, source, workspace_id, target_type, target_id, file_path,
              start_line, end_line, body, suggestion, quoted_text, mode, outcome, created_at, decided_at
       FROM review_history ORDER BY created_at DESC, id DESC LIMIT ?1",
    )
    .map_err(|e| format!("Failed to prepare query: {}", e))?;
  let rows = stmt
    .query_map([limit as i64], |row| {
      Ok(ReviewHistoryEntry {
        id: row.get(0)?,
        kind: row.get(1)?,
        comment_id: row.get(2)?,
        source: row.get(3)?,
        workspace_id: row.get(4)?,
        target_type: row.get(5)?,
        target_id: row.get(6)?,
        file_path: row.get(7)?,
        start_line: row.get(8)?,
        end_line: row.get(9)?,
        body: row.get(10)?,
        suggestion: row.get(11)?,
        quoted_text: row.get(12)?,
        mode: row.get(13)?,
        outcome: row.get(14)?,
        created_at: row.get(15)?,
        decided_at: row.get(16)?,
      })
    })
    .map_err(|e| format!("Failed to list review history: {}", e))?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| format!("Failed to read review history: {}", e))?;
  Ok(rows)
}

/// Deletes review history created before `cutoff`; returns how many rows.
pub fn prune_review_history(
  repo_path: &str,
  cutoff: chrono::DateTime<Utc>,
) -> Result<usize, String> {
  let conn = get_connection(repo_path)?;
  conn
    .execute(
      "DELETE FROM review_history WHERE created_at < ?1",
      [cutoff.to_rfc3339()],
    )
    .map_err(|e| format!("Failed to prune review history: {}", e))
}

fn prune_review_history_now(repo_path: &str) {
  if let Err(err) = prune_review_history(repo_path, Utc::now() - REVIEW_HISTORY_MAX_AGE) {
    log::warn!("Failed to prune review history for {repo_path}: {err}");
  }
}

/// Once per process: prunes every repository opened so far, every 12 hours.
fn start_review_history_pruner() {
  static STARTED: OnceLock<()> = OnceLock::new();
  STARTED.get_or_init(|| {
    std::thread::spawn(|| loop {
      std::thread::sleep(REVIEW_HISTORY_PRUNE_EVERY);
      let repos: Vec<String> = INITIALIZED_DBS
        .get()
        .map(|dbs| dbs.lock_or_recover().iter().cloned().collect())
        .unwrap_or_default();
      for repo in repos {
        prune_review_history_now(&repo);
      }
    });
  });
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
  get_prompt_history_page(repo_path, i64::MAX as usize, 0)
}

/// Up to `limit` prompts sent in this repository after the newest `offset`
/// of them, most recent first.
pub fn get_prompt_history_page(
  repo_path: &str,
  limit: usize,
  offset: usize,
) -> Result<Vec<PromptHistoryEntry>, String> {
  let conn = get_connection(repo_path)?;
  let sql = format!(
    "{} ORDER BY ph.created_at DESC, ph.id DESC LIMIT ?1 OFFSET ?2",
    PROMPT_HISTORY_SELECT
  );
  let mut stmt = conn
    .prepare(&sql)
    .map_err(|e| format!("Failed to prepare prompt history query: {}", e))?;

  let entries = stmt
    .query_map(
      params![
        limit.min(i64::MAX as usize) as i64,
        offset.min(i64::MAX as usize) as i64
      ],
      prompt_history_from_row,
    )
    .map_err(|e| format!("Failed to query prompt history: {}", e))?;

  entries
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

/// Persist a newly created stash entry.
#[expect(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
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

  fn agent_comment(repo: &str, target_type: &str) -> AgentReviewComment {
    create_agent_review_comment(
      repo,
      target_type,
      "7",
      "a.rs",
      None,
      2,
      3,
      Some("new"),
      "use ?",
      Some("x?"),
      "local-agent",
      None,
    )
    .unwrap()
  }

  fn outcomes(repo: &str) -> Vec<(String, String)> {
    list_review_history(repo, 100)
      .unwrap()
      .into_iter()
      .map(|e| (e.kind, e.outcome))
      .collect()
  }

  #[test]
  fn review_history_records_each_agent_comment_and_its_outcome() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let applied = agent_comment(repo, "workspace_diff");
    let resolved = agent_comment(repo, "workspace_diff");
    let dismissed = agent_comment(repo, "file");
    agent_comment(repo, "linear_issue");
    resolve_agent_review_comment(repo, &applied.id).unwrap();
    mark_review_history_applied(repo, &applied.id).unwrap();
    resolve_agent_review_comment(repo, &resolved.id).unwrap();
    delete_agent_review_comment(repo, &dismissed.id).unwrap();

    let entries = list_review_history(repo, 100).unwrap();
    assert_eq!(entries.len(), 3, "linear targets are not code review");
    let outcome = |id: &str| {
      entries
        .iter()
        .find(|e| e.comment_id.as_deref() == Some(id))
        .map(|e| e.outcome.clone())
        .unwrap()
    };
    assert_eq!(outcome(&applied.id), "applied");
    assert_eq!(outcome(&resolved.id), "resolved");
    assert_eq!(outcome(&dismissed.id), "dismissed");
    let first = entries
      .iter()
      .find(|e| e.comment_id.as_deref() == Some(&applied.id))
      .unwrap();
    assert_eq!(first.body, "use ?");
    assert_eq!(first.suggestion.as_deref(), Some("x?"));
    assert_eq!((first.start_line, first.end_line), (Some(2), Some(3)));
    assert!(first.decided_at.is_some());
  }

  #[test]
  fn review_history_records_one_row_per_sent_review() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    add_user_review_history(repo, Some(3), "changes", "## a.rs:4\nrename", "plan").unwrap();
    add_user_review_history(repo, None, "files", "## b.rs:1\nwhy", "acceptEdits").unwrap();
    let entries = list_review_history(repo, 100).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].body, "## b.rs:1\nwhy", "newest first");
    assert_eq!(entries[1].target_type, "changes");
    assert_eq!(entries[1].mode.as_deref(), Some("plan"));
    assert_eq!(outcomes(repo)[0], ("user_review".into(), "sent".into()));
  }

  #[test]
  fn review_history_prunes_old_rows_when_the_repository_is_first_opened() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let db_path = init_local_db(repo).unwrap();
    Connection::open(&db_path)
      .unwrap()
      .execute(
        "INSERT INTO review_history (kind, target_type, body, outcome, created_at)
         VALUES ('user_review', 'changes', 'old', 'sent', ?1)",
        [(Utc::now() - chrono::Duration::days(31)).to_rfc3339()],
      )
      .unwrap();
    assert!(list_review_history(repo, 100).unwrap().is_empty());
  }

  #[test]
  fn review_history_prunes_rows_older_than_the_cutoff() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    add_user_review_history(repo, None, "changes", "old", "plan").unwrap();
    get_connection(repo)
      .unwrap()
      .execute(
        "UPDATE review_history SET created_at = ?1",
        [(Utc::now() - chrono::Duration::days(31)).to_rfc3339()],
      )
      .unwrap();
    add_user_review_history(repo, None, "changes", "new", "plan").unwrap();

    let removed = prune_review_history(repo, Utc::now() - REVIEW_HISTORY_MAX_AGE).unwrap();
    assert_eq!(removed, 1);
    let left = list_review_history(repo, 100).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].body, "new");
  }

  #[test]
  fn updates_review_comment_text_and_line_hash() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let comment = create_agent_review_comment(
      repo,
      "github_pr",
      "7",
      "a.rs",
      None,
      2,
      3,
      Some("new"),
      "first",
      None,
      "local-human",
      None,
    )
    .unwrap();
    assert_eq!(comment.line_hash, None);

    update_agent_review_comment(repo, &comment.id, None, Some("h1"), Some("+a\n+b")).unwrap();
    let stored = get_agent_review_comment(repo, &comment.id)
      .unwrap()
      .unwrap();
    assert_eq!(stored.comment_text, "first");
    assert_eq!(stored.line_hash.as_deref(), Some("h1"));
    assert_eq!(stored.quoted_text.as_deref(), Some("+a\n+b"));

    update_agent_review_comment(repo, &comment.id, Some("second"), None, None).unwrap();
    let stored = get_agent_review_comment(repo, &comment.id)
      .unwrap()
      .unwrap();
    assert_eq!(stored.comment_text, "second");
    assert_eq!(stored.line_hash.as_deref(), Some("h1"));
    assert_eq!(stored.quoted_text.as_deref(), Some("+a\n+b"));
    assert_eq!(stored.source, "local-human");
  }

  #[test]
  fn review_drafts_list_resolve_and_delete_by_target() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let draft = create_agent_review_comment(
      repo,
      "github_pr",
      "42",
      "src/search.ts",
      None,
      3,
      4,
      Some("new"),
      "Trim first.",
      None,
      "local-human",
      None,
    )
    .unwrap();
    update_agent_review_comment(
      repo,
      &draft.id,
      Some("Trim before matching."),
      Some("abc123"),
      None,
    )
    .unwrap();

    let listed = list_agent_review_comments(repo, "github_pr", "42").unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].comment_text, "Trim before matching.");
    assert_eq!(listed[0].line_hash.as_deref(), Some("abc123"));
    assert_eq!((listed[0].start_line, listed[0].end_line), (3, 4));
    assert_eq!(listed[0].side.as_deref(), Some("new"));
    assert_eq!(listed[0].status, "open");
    assert!(list_agent_review_comments(repo, "github_pr", "7")
      .unwrap()
      .is_empty());

    resolve_agent_review_comment(repo, &draft.id).unwrap();
    assert_eq!(
      list_agent_review_comments(repo, "github_pr", "42").unwrap()[0].status,
      "resolved"
    );

    delete_agent_review_comment(repo, &draft.id).unwrap();
    assert!(list_agent_review_comments(repo, "github_pr", "42")
      .unwrap()
      .is_empty());
  }

  #[test]
  fn parses_remote_state_scope_prefix() {
    assert_eq!(
      RepoStateScope::parse("/srv/repo"),
      RepoStateScope::Local("/srv/repo")
    );
    assert_eq!(
      RepoStateScope::parse("treq-remote-state:ssh:ep:gen0:/srv/repo"),
      RepoStateScope::Remote("ssh:ep:gen0:/srv/repo")
    );
  }

  #[test]
  fn stores_remote_state_under_app_data_dir() {
    let app_data = TempDir::new().unwrap();
    let path = RepoStateScope::Remote("ssh:ep:gen0:/srv/repo")
      .db_path(Some(app_data.path()))
      .unwrap();
    assert!(path.starts_with(app_data.path().join("remote-state")));
    assert!(!path.to_string_lossy().contains("/srv/repo"));
    let other = RepoStateScope::Remote("ssh:ep:gen1:/srv/repo")
      .db_path(Some(app_data.path()))
      .unwrap();
    assert_ne!(path, other);
  }

  #[test]
  fn remote_scope_review_drafts_round_trip_without_creating_the_remote_path() {
    let app_data = TempDir::new().unwrap();
    let _guard = crate::core::AppDataDirGuard::set(app_data.path());
    let remote_path = app_data.path().join("not-on-this-machine");
    let scope = format!(
      "{REMOTE_STATE_SCOPE_PREFIX}ssh:ep-review-state#0:{}",
      remote_path.display()
    );

    save_pending_review(&scope, 4, "[]", Some("[]"), Some("pending draft"), None).unwrap();
    save_file_browser_review(&scope, 4, "[]", Some("browser draft")).unwrap();

    assert_eq!(
      get_pending_review(&scope, 4)
        .unwrap()
        .unwrap()
        .summary_text
        .as_deref(),
      Some("pending draft")
    );
    assert_eq!(
      get_file_browser_review(&scope, 4)
        .unwrap()
        .unwrap()
        .summary_text
        .as_deref(),
      Some("browser draft")
    );
    assert!(!remote_path.exists());
  }

  #[test]
  fn enforces_foreign_keys_only_for_local_scope() {
    assert!(RepoStateScope::Local("/srv").enforces_foreign_keys());
    assert!(!RepoStateScope::Remote("ssh:ep:gen0:/srv").enforces_foreign_keys());
  }

  #[test]
  fn refuses_remote_state_without_app_data_dir() {
    assert!(RepoStateScope::Remote("ssh:ep:gen0:/srv")
      .db_path(None)
      .is_err());
  }

  /// The schema as the app reads it: per table, its columns (sorted by name,
  /// since `ALTER TABLE ADD COLUMN` appends), foreign keys and indexes.
  fn schema_shape(conn: &Connection) -> String {
    let names = |sql: &str| -> Vec<String> {
      let mut stmt = conn.prepare(sql).expect("prepare");
      stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
    };
    let rows = |sql: &str| -> Vec<String> {
      let mut stmt = conn.prepare(sql).expect("prepare");
      let n = stmt.column_count();
      let mut out: Vec<String> = stmt
        .query_map([], |row| {
          Ok(
            (0..n)
              .map(|i| match row.get_ref(i).expect("cell") {
                rusqlite::types::ValueRef::Null => "NULL".to_string(),
                rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                rusqlite::types::ValueRef::Real(v) => v.to_string(),
                rusqlite::types::ValueRef::Text(v) | rusqlite::types::ValueRef::Blob(v) => {
                  String::from_utf8_lossy(v).into_owned()
                }
              })
              .collect::<Vec<_>>()
              .join("|"),
          )
        })
        .expect("query")
        .map(|r| r.expect("row"))
        .collect();
      out.sort();
      out
    };
    let mut shape = Vec::new();
    for table in names(
      "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    ) {
      shape.push(format!("table {table}"));
      for col in rows(&format!(
        "SELECT name, type, \"notnull\", dflt_value, pk FROM pragma_table_info('{table}')"
      )) {
        shape.push(format!("  column {col}"));
      }
      for fk in rows(&format!(
        "SELECT \"table\", \"from\", \"to\", on_update, on_delete FROM pragma_foreign_key_list('{table}')"
      )) {
        shape.push(format!("  fk {fk}"));
      }
      for index in rows(&format!(
        "SELECT l.name || '|' || l.\"unique\" || '|' || group_concat(i.name, ',')
           FROM pragma_index_list('{table}') l, pragma_index_info(l.name) i
          WHERE l.origin = 'c' GROUP BY l.name"
      )) {
        shape.push(format!("  index {index}"));
      }
    }
    shape.join("\n") + "\n"
  }

  fn fresh_schema_shape() -> String {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = init_local_db(temp_dir.path().to_str().unwrap()).expect("fresh init");
    schema_shape(&Connection::open(db_path).expect("open fresh database"))
  }

  /// An upgraded database must read and write as a fresh install does: every
  /// fresh table, column, foreign key and index is present and identical.
  /// Tables and columns the app no longer uses may stay behind, as long as no
  /// insert can trip on them (nullable, or with a default).
  fn assert_upgrade_matches_fresh(db_path: &Path, label: &str) {
    let parse = |shape: String| {
      let mut tables: std::collections::BTreeMap<String, Vec<String>> = Default::default();
      let mut current = String::new();
      for line in shape.lines() {
        if let Some(name) = line.strip_prefix("table ") {
          current = name.to_string();
          tables.entry(current.clone()).or_default();
        } else {
          tables
            .get_mut(&current)
            .expect("table")
            .push(line.trim().to_string());
        }
      }
      tables
    };
    let upgraded = parse(schema_shape(
      &Connection::open(db_path).expect("open upgraded database"),
    ));
    let fresh = parse(fresh_schema_shape());
    for (table, fresh_lines) in &fresh {
      let Some(upgraded_lines) = upgraded.get(table) else {
        panic!("{label}: table {table} missing after upgrade");
      };
      for line in fresh_lines {
        assert!(
          upgraded_lines.contains(line),
          "{label}: {table} lacks `{line}` after upgrade; has {upgraded_lines:#?}"
        );
      }
      for line in upgraded_lines {
        if fresh_lines.contains(line) {
          continue;
        }
        let blocks_inserts = line.strip_prefix("column ").is_some_and(|col| {
          let parts: Vec<_> = col.split('|').collect();
          parts[2] == "1" && parts[3] == "NULL"
        });
        let stale_constraint = !line.starts_with("column ");
        assert!(
          !blocks_inserts && !stale_constraint,
          "{label}: {table} keeps legacy `{line}` after upgrade"
        );
      }
    }
  }

  /// Every schema a past release created on a fresh install, replayed from
  /// that commit's `local_db.rs` (see `local_db/upgrade_fixtures`).
  #[test]
  fn every_released_schema_upgrades_to_the_current_one() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/local_db/upgrade_fixtures");
    let mut fixtures: Vec<_> = fs::read_dir(&dir)
      .expect("fixtures dir")
      .map(|e| e.expect("entry").path())
      .collect();
    fixtures.sort();
    assert!(fixtures.len() > 30, "found {} fixtures", fixtures.len());
    let failures: Vec<String> = fixtures
      .iter()
      .filter_map(|fixture| {
        let label = fixture.file_name().unwrap().to_string_lossy().into_owned();
        std::panic::catch_unwind(|| upgrade_fixture(fixture, &label))
          .err()
          .map(|e| {
            e.downcast_ref::<String>()
              .cloned()
              .unwrap_or_else(|| label.clone())
          })
      })
      .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
  }

  fn upgrade_fixture(fixture: &Path, label: &str) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let sql = fs::read_to_string(fixture).expect("read fixture");
    let (repo_path, db_path) = open_legacy_db(&temp_dir, &sql);
    init_local_db(&repo_path).unwrap_or_else(|e| panic!("{label}: {e}"));
    assert_upgrade_matches_fresh(&db_path, label);
    // The app's own writes and reads work on the upgraded file.
    let id = add_workspace(
      &repo_path,
      "ws".into(),
      format!("{repo_path}/.treq/workspaces/ws"),
      "feat/ws".into(),
      None,
      None,
      None,
    )
    .unwrap_or_else(|e| panic!("{label}: add_workspace: {e}"));
    let workspaces = get_workspaces(&repo_path).unwrap_or_else(|e| panic!("{label}: {e}"));
    assert!(
      workspaces.iter().any(|w| w.id == id),
      "{label}: workspace not read back"
    );
  }

  #[test]
  fn fresh_schema_matches_the_snapshot() {
    // A change here is a schema change: it needs a migration for existing
    // databases. Update `local_db/schema.snapshot` alongside the migration.
    assert_eq!(
      fresh_schema_shape(),
      include_str!("local_db/schema.snapshot")
    );
  }

  fn open_legacy_db(temp_dir: &TempDir, schema: &str) -> (String, PathBuf) {
    let repo_path = temp_dir.path().to_str().unwrap().to_string();
    let db_path = get_local_db_path(&repo_path);
    fs::create_dir_all(db_path.parent().unwrap()).expect("create .treq directory");
    let conn = Connection::open(&db_path).expect("open legacy database");
    conn.execute_batch(schema).expect("create legacy schema");
    (repo_path, db_path)
  }

  fn table_exists(conn: &Connection, name: &str) -> bool {
    conn
      .query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
      )
      .expect("sqlite_master")
      > 0
  }

  fn all_migrations_applied(db_path: &PathBuf) -> bool {
    let conn = Connection::open(db_path).expect("open database");
    migrations::is_current(&conn, migrations::MIGRATIONS).expect("read migrations")
  }

  fn user_version(db_path: &PathBuf) -> i64 {
    Connection::open(db_path)
      .expect("open database")
      .query_row("PRAGMA user_version", [], |row| row.get(0))
      .expect("user_version")
  }

  // Windows and background tasks open a repo's db at the same time; each
  // migration checks for its column, then adds it.
  #[test]
  fn concurrent_inits_of_an_old_db_all_succeed() {
    for _ in 0..200 {
      let temp_dir = TempDir::new().expect("Failed to create temp dir");
      // Old enough that every init adds the missing workspaces columns.
      let (repo_path, _) = open_legacy_db(
        &temp_dir,
        "CREATE TABLE workspaces (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           workspace_name TEXT NOT NULL,
           workspace_path TEXT NOT NULL UNIQUE,
           branch_name TEXT NOT NULL,
           created_at TEXT NOT NULL
         );",
      );
      let start = std::sync::Arc::new(std::sync::Barrier::new(16));
      let inits: Vec<_> = (0..16)
        .map(|_| {
          let (repo_path, start) = (repo_path.clone(), start.clone());
          std::thread::spawn(move || {
            start.wait();
            init_local_db(&repo_path)
          })
        })
        .collect();
      for init in inits {
        init.join().unwrap().expect("concurrent init");
      }
    }
  }

  #[test]
  fn init_records_schema_version_and_second_init_is_a_noop() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap().to_string();
    let db_path = init_local_db(&repo_path).expect("fresh init");
    assert!(all_migrations_applied(&db_path));

    // Drop a table the init would recreate: a skipped init leaves it missing.
    let conn = Connection::open(&db_path).expect("open database");
    conn.execute("DROP TABLE repo_trust", []).unwrap();
    drop(conn);
    init_local_db(&repo_path).expect("second init");
    let conn = Connection::open(&db_path).expect("reopen database");
    assert!(!table_exists(&conn, "repo_trust"));
  }

  #[test]
  fn init_migrates_legacy_database_at_version_zero() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (repo_path, db_path) = open_legacy_db(
      &temp_dir,
      "CREATE TABLE workspaces (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         workspace_name TEXT NOT NULL,
         workspace_path TEXT NOT NULL UNIQUE,
         branch_name TEXT NOT NULL,
         created_at TEXT NOT NULL,
         metadata TEXT,
         target_branch TEXT
       );",
    );
    assert_eq!(user_version(&db_path), 0);

    init_local_db(&repo_path).expect("migrate legacy database");

    let conn = Connection::open(&db_path).expect("reopen database");
    assert!(table_exists(&conn, "repo_trust"));
    assert!(conn
      .prepare("SELECT refreshed_at, archived FROM workspaces")
      .is_ok());
    assert!(all_migrations_applied(&db_path));
  }

  #[test]
  fn heartbeat_prunes_stale_rows_and_upserts_in_one_call() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap().to_string();
    let instance = |id: &str, beat: u64| RegisteredInstance {
      instance_id: id.to_string(),
      pid: 1,
      started_at: 0,
      last_heartbeat_at: beat,
      endpoint: "127.0.0.1:1".to_string(),
      windows: Vec::new(),
    };
    upsert_instance_registry(&repo_path, instance("stale", 1_000)).unwrap();
    heartbeat_instance_registry(&repo_path, 100_000, 15_000, instance("me", 100_000)).unwrap();
    let ids: Vec<String> = list_instance_registry(&repo_path)
      .unwrap()
      .into_iter()
      .map(|i| i.instance_id)
      .collect();
    assert_eq!(ids, vec!["me".to_string()]);
  }

  fn cost_instance(heartbeat: u64) -> RegisteredInstance {
    RegisteredInstance {
      instance_id: "me".to_string(),
      pid: 1,
      started_at: 0,
      last_heartbeat_at: heartbeat,
      endpoint: "127.0.0.1:1".to_string(),
      windows: Vec::new(),
    }
  }

  /// 200 one-row writes through `get_connection`. Count the fsyncs with
  /// `strace -f -e trace=fsync,fdatasync` on the test binary, e.g.
  /// `strace -f -c -e trace=fdatasync <bin> local_db_write_cost --ignored`.
  #[test]
  #[ignore]
  fn local_db_write_cost() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap().to_string();
    upsert_instance_registry(&repo_path, cost_instance(0)).unwrap();
    for heartbeat in 1..=200 {
      upsert_instance_registry(&repo_path, cost_instance(heartbeat)).unwrap();
    }
  }

  /// 200 one-row reads through `get_connection`, for callgrind:
  /// `valgrind --tool=callgrind --toggle-collect='*local_db_read_cost*' <bin> local_db_read_cost --ignored`.
  #[test]
  #[ignore]
  fn local_db_read_cost() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap().to_string();
    upsert_instance_registry(&repo_path, cost_instance(0)).unwrap();
    for _ in 0..200 {
      assert_eq!(list_instance_registry(&repo_path).unwrap().len(), 1);
    }
  }

  /// `cargo test --lib init_local_db_cost -- --ignored --nocapture`
  #[test]
  #[ignore]
  fn init_local_db_cost() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_str().unwrap().to_string();
    let first = std::time::Instant::now();
    let db_path = init_local_db(&repo_path).unwrap();
    println!("first init: {:?}", first.elapsed());
    let n = 200;
    let started = std::time::Instant::now();
    for _ in 0..n {
      init_local_db(&repo_path).unwrap();
    }
    println!("gated init avg: {:?}", started.elapsed() / n);
    let forget_migrations = || {
      Connection::open(&db_path)
        .unwrap()
        .execute_batch("DELETE FROM schema_migrations; PRAGMA user_version = 0;")
        .unwrap();
    };
    forget_migrations();
    let started = std::time::Instant::now();
    for _ in 0..n {
      forget_migrations();
      init_local_db(&repo_path).unwrap();
    }
    println!("ungated init avg: {:?}", started.elapsed() / n);

    let instance = || RegisteredInstance {
      instance_id: "me".to_string(),
      pid: 1,
      started_at: 0,
      last_heartbeat_at: 1,
      endpoint: "127.0.0.1:1".to_string(),
      windows: Vec::new(),
    };
    let started = std::time::Instant::now();
    for _ in 0..n {
      init_local_db(&repo_path).unwrap();
      prune_stale_instance_registry(&repo_path, 1, 15_000).unwrap();
      upsert_instance_registry(&repo_path, instance()).unwrap();
    }
    println!("old heartbeat tick avg: {:?}", started.elapsed() / n);
    let started = std::time::Instant::now();
    for _ in 0..n {
      heartbeat_instance_registry(&repo_path, 1, 15_000, instance()).unwrap();
    }
    println!("new heartbeat tick avg: {:?}", started.elapsed() / n);
  }

  #[test]
  fn migrates_legacy_sessions_worktree_column() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (repo_path, db_path) = open_legacy_db(
      &temp_dir,
      "CREATE TABLE workspaces (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         workspace_name TEXT NOT NULL,
         workspace_path TEXT NOT NULL UNIQUE,
         branch_name TEXT NOT NULL,
         created_at TEXT NOT NULL,
         metadata TEXT,
         target_branch TEXT
       );
       INSERT INTO workspaces (id, workspace_name, workspace_path, branch_name, created_at)
         VALUES (3, 'legacy', 'legacy', 'feat/legacy', 'c');
       CREATE TABLE sessions (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         worktree_id INTEGER,
         name TEXT NOT NULL,
         created_at TEXT NOT NULL,
         last_accessed TEXT NOT NULL,
         model TEXT
       );
       INSERT INTO sessions VALUES (5, 3, 'legacy', 'c', 'a', 'opus');",
    );

    init_local_db(&repo_path).expect("migrate legacy database");

    let conn = Connection::open(&db_path).expect("reopen migrated database");
    let row: (i64, i64, String) = conn
      .query_row("SELECT id, workspace_id, name FROM sessions", [], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
      })
      .expect("migrated session");
    assert_eq!(row, (5, 3, "legacy".to_string()));
    assert!(!table_exists(&conn, "sessions_new"));
  }

  #[test]
  fn migrates_legacy_prompt_history_to_cascade_deletes() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (repo_path, db_path) = open_legacy_db(
      &temp_dir,
      "CREATE TABLE workspaces (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         workspace_name TEXT NOT NULL,
         workspace_path TEXT NOT NULL UNIQUE,
         branch_name TEXT NOT NULL,
         created_at TEXT NOT NULL,
         metadata TEXT,
         target_branch TEXT
       );
       INSERT INTO workspaces (id, workspace_name, workspace_path, branch_name, created_at)
         VALUES (3, 'legacy', 'legacy', 'feat/legacy', 'c');
       CREATE TABLE sessions (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         workspace_id INTEGER,
         name TEXT NOT NULL,
         created_at TEXT NOT NULL,
         last_accessed TEXT NOT NULL,
         model TEXT
       );
       CREATE TABLE prompt_history (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         workspace_id INTEGER,
         session_id INTEGER,
         prompt_text TEXT NOT NULL,
         agent TEXT,
         created_at TEXT NOT NULL,
         FOREIGN KEY (workspace_id) REFERENCES workspaces(id),
         FOREIGN KEY (session_id) REFERENCES sessions(id)
       );
       INSERT INTO prompt_history VALUES (9, NULL, NULL, 'hello', 'claude', 'c');",
    );

    init_local_db(&repo_path).expect("migrate legacy database");

    let conn = Connection::open(&db_path).expect("reopen migrated database");
    let non_cascade: i64 = conn
      .query_row(
        "SELECT COUNT(*) FROM pragma_foreign_key_list('prompt_history') WHERE on_delete != 'CASCADE'",
        [],
        |row| row.get(0),
      )
      .expect("foreign keys");
    assert_eq!(non_cascade, 0);
    let prompt: String = conn
      .query_row(
        "SELECT prompt_text FROM prompt_history WHERE id = 9",
        [],
        |row| row.get(0),
      )
      .expect("migrated prompt");
    assert_eq!(prompt, "hello");
  }

  #[test]
  fn failed_pending_reviews_migration_leaves_legacy_table_intact() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    // comments_json was nullable; the new `comments` column is NOT NULL.
    let (repo_path, db_path) = open_legacy_db(
      &temp_dir,
      "CREATE TABLE pending_reviews (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         workspace_id INTEGER NOT NULL UNIQUE,
         comments_json TEXT,
         viewed_files_json TEXT,
         overall_comment TEXT,
         created_at TEXT NOT NULL,
         updated_at TEXT NOT NULL
       );
       INSERT INTO pending_reviews VALUES (1, 2, NULL, NULL, 'keep', 'c', 'u');",
    );

    assert!(init_local_db(&repo_path).is_err());

    let conn = Connection::open(&db_path).expect("reopen database");
    assert!(!table_exists(&conn, "pending_reviews_new"));
    let summary: String = conn
      .query_row("SELECT overall_comment FROM pending_reviews", [], |row| {
        row.get(0)
      })
      .expect("legacy review");
    assert_eq!(summary, "keep");
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

  #[test]
  fn searches_workspace_files_case_insensitively_and_ranks_filename_matches_first() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    sync_workspace_files(
      repo_path,
      None,
      vec![
        cached_file("a/b/c/xMAINx.rs"),
        cached_file("src/Main.rs"),
        cached_file("m/a/i/n.rs"),
        cached_file("MAIN"),
      ],
    )
    .expect("index files");

    let paths = search_workspace_files(repo_path, None, "MaIn", 50)
      .expect("search workspace files")
      .into_iter()
      .map(|file| file.relative_path)
      .collect::<Vec<_>>();

    assert_eq!(
      paths,
      ["MAIN", "src/Main.rs", "a/b/c/xMAINx.rs", "m/a/i/n.rs"]
    );
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
  fn add_workspace_targeting_sets_its_flags_with_the_row() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    init_local_db(repo_path).expect("initialize database");

    let workspace = add_workspace_targeting(
      repo_path,
      "feat-x".to_string(),
      "feat-x".to_string(),
      "feat/x".to_string(),
      Some("about x".to_string()),
      None,
      None,
      true,
      "release",
    )
    .expect("register workspace");

    assert!(workspace.not_on_remote);
    assert_eq!(workspace.target_branch.as_deref(), Some("release"));
    assert_eq!(workspace.description.as_deref(), Some("about x"));
    let stored = get_workspace_by_id(repo_path, workspace.id)
      .expect("lookup")
      .expect("stored");
    assert_eq!(stored.target_branch.as_deref(), Some("release"));
    assert!(stored.not_on_remote);
  }

  #[test]
  fn init_new_workspace_state_sets_the_title_and_clears_the_rebase_mark() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    init_local_db(repo_path).expect("initialize database");
    let id = add_workspace(
      repo_path,
      "feat-y".to_string(),
      "feat-y".to_string(),
      "feat/y".to_string(),
      None,
      None,
      None,
    )
    .expect("register workspace");

    let workspace = init_new_workspace_state(repo_path, id, Some("A title"))
      .expect("init state")
      .expect("row");

    assert_eq!(workspace.title, "A title");
    let metadata: serde_json::Value =
      serde_json::from_str(workspace.metadata.as_deref().expect("metadata")).expect("json");
    assert_eq!(metadata["last_rebased_target_commit"], "");
    let untitled = init_new_workspace_state(repo_path, id, None)
      .expect("init state")
      .expect("row");
    assert_eq!(untitled.title, "A title", "no title leaves the title alone");
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
  fn failed_workspace_insert_keeps_the_archived_row_path() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    init_local_db(repo_path).expect("initialize database");
    let archived_id = add_archived_workspace(repo_path, "fix-example", "fix/example");
    get_connection(repo_path)
      .expect("connection")
      .execute_batch(
        "CREATE TRIGGER fail_insert BEFORE INSERT ON workspaces
         BEGIN SELECT RAISE(ABORT, 'insert refused'); END;",
      )
      .expect("trigger");

    let added = add_workspace(
      repo_path,
      "fix-example".to_string(),
      "fix-example".to_string(),
      "fix/example".to_string(),
      None,
      None,
      None,
    );
    let discovered = upsert_workspace_discovery(
      repo_path,
      "fix-example",
      "fix-example",
      "fix/example",
      "now",
    );

    assert!(added.is_err());
    assert!(discovered.is_err());
    let archived = get_workspace_by_id(repo_path, archived_id)
      .expect("lookup archived")
      .expect("archived row kept");
    assert_eq!(archived.workspace_path, "fix-example");
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
      bookmark_conflicted: false,
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
      bookmark_conflicted: false,
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
      bookmark_conflicted: false,
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
      bookmark_conflicted: false,
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
    let original_created_at = original.created_at;

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
    let first_created_at = first_review.created_at;

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

#[expect(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
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

/// Ids of every run of a workflow file, across all workspaces.
pub fn list_workflow_run_ids(repo_path: &str, filename: &str) -> Result<Vec<i64>, String> {
  let conn = get_connection(repo_path)?;
  let mut stmt = conn
    .prepare("SELECT id FROM workflow_runs WHERE filename = ?1")
    .map_err(|e| format!("Failed to prepare workflow run ids query: {}", e))?;
  let rows = stmt
    .query_map(params![filename], |row| row.get(0))
    .map_err(|e| format!("Failed to query workflow run ids: {}", e))?;
  rows
    .collect::<rusqlite::Result<Vec<i64>>>()
    .map_err(|e| format!("Failed to read workflow run ids: {}", e))
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
