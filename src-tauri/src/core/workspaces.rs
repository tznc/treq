use crate::lock_ext::LockExt;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

use crate::auto_rebase::{self, WorkspaceBookmarkConflict};
use crate::auto_review::{self, ReviewTrigger};
use crate::core::repo::{commit_lock_for_repo, repo_status};
use crate::jj;
use crate::local_db;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceCommit {
  pub hash: String,
  pub timestamp: String,
  pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct WorkspaceEntry {
  pub name: String,
  pub path: String,
  pub is_directory: bool,
  pub modified_at: Option<String>,
  /// Superproject gitlink SHA when this row is a Git submodule.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub submodule_pin: Option<String>,
  /// Repo-level preference: populate this submodule in every workspace. Default off.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub submodule_synced: Option<bool>,
  /// File status for highlighting in the Code tab file list: "conflict",
  /// "workingCopy", or "committed". Absent means untouched. Only populated by
  /// `core::files::list_workspace_files`; plain directory listings leave it unset.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub status: Option<String>,
}

/// Defines how a workspace is merged into its target branch.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum MergeCommit {
  Merge,
  SquashAndMerge,
  RebaseAndMerge,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PullWorkspaceResult {
  pub success: bool,
  pub message: String,
  pub was_diverged: bool,
  pub commits_rebased: usize,
  /// True when the pull/rebase left unresolved file conflicts in the workspace.
  /// Callers (e.g. Sync) should surface these for local resolution instead of pushing.
  #[serde(default)]
  pub has_conflicts: bool,
}

pub enum MaybeEmptyParam<T> {
  EmptyValue,
  Omitted,
  Some(T),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncSource {
  HomeToWorkspace,
  WorkspaceToHome,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct WorkspaceMoveRequest {
  pub files: Vec<String>,
  pub hunks: Vec<HunkSpec>,
  pub commits: Vec<String>,
}

impl WorkspaceMoveRequest {
  pub fn has_selectors(&self) -> bool {
    !self.files.is_empty() || !self.hunks.is_empty() || !self.commits.is_empty()
  }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct WorkspaceMoveResult {
  pub commits_moved: usize,
  pub files_moved: usize,
  pub hunks_applied: usize,
  pub hunks_skipped: usize,
  pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct HunkSpec {
  pub file_path: String,
  pub start_line: usize,
  pub end_line: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RenameWorkspaceResult {
  pub success: bool,
  pub message: String,
  pub workspace: Option<local_db::Workspace>,
  pub updated_children_ids: Vec<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum RemoteSyncStatus {
  NotOnRemote,
  InSync,
  Ahead { count: usize },
  Behind { count: usize },
  Diverged { ahead: usize, behind: usize },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspacePartialStatus {
  pub current: local_db::Workspace,
  pub has_conflicts: bool,
  pub has_changes: bool,
  pub commits_ahead: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceSidebarStatus {
  pub current: local_db::Workspace,
  pub has_conflicts: bool,
  /// Working-copy tip author timestamp (RFC3339) for sibling recency ordering.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub last_activity_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceStatus {
  #[serde(flatten)]
  pub partial: WorkspacePartialStatus,
  pub conflicted_files: Vec<String>,
  pub remote_sync: RemoteSyncStatus,
  pub target: Option<local_db::Workspace>,
  pub children: Vec<local_db::Workspace>,
  pub dag_nodes: Vec<WorkspaceNode>,
  pub conflicted_workspace_ids: Vec<i64>,
  pub commits_ahead_of_target: Vec<WorkspaceCommit>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceNode {
  pub status: WorkspacePartialStatus,
  pub parent_id: Option<i64>,
  pub child_ids: Vec<i64>,
  pub depth: usize,
}

/// Metadata for workspace creation, supporting both simple description and complex metadata with files.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct WorkspaceMetadata {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub description: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub moved_files: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub sparse_patterns: Option<Vec<String>>,
  /// Paths relative to the repo root to symlink from the home working copy
  /// into the new workspace (e.g. `node_modules`). Applied at create time.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub symlinked_dirs: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub linear_issue_key: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub linear_issue_url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub linear_issue_title: Option<String>,
  /// Issue-tracker item this workspace was kicked off from (e.g. `github`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub tracker_provider: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub tracker_item_key: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub tracker_item_url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub tracker_item_title: Option<String>,
}

/// Parse the creation metadata JSON callers send.
/// Unknown fields are ignored; empty arrays are normalized to `None`.
/// Invalid or missing JSON yields an all-`None` metadata.
pub fn parse_workspace_metadata(metadata: Option<&str>) -> WorkspaceMetadata {
  let parsed: Option<WorkspaceMetadata> = metadata.and_then(|m| serde_json::from_str(m).ok());
  let mut parsed = parsed.unwrap_or_default();
  parsed.moved_files = parsed.moved_files.filter(|v| !v.is_empty());
  parsed.sparse_patterns = parsed.sparse_patterns.filter(|v| !v.is_empty());
  parsed.symlinked_dirs = parsed.symlinked_dirs.filter(|v| !v.is_empty());
  parsed
}

pub(crate) fn resolve_workspace_root(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<String, String> {
  match workspace_id {
    Some(id) => {
      let workspace = local_db::get_workspace_by_id(repo_path, id)
        .map_err(|e| format!("Failed to get workspace: {}", e))?
        .ok_or_else(|| format!("Workspace not found: {}", id))?;
      let workspace_path = Path::new(repo_path)
        .join(".treq")
        .join("workspaces")
        .join(&workspace.workspace_path);
      Ok(
        workspace_path
          .to_str()
          .ok_or("Failed to convert workspace path to string")
          .map(|path| path.to_string())?,
      )
    }
    None => Ok(repo_path.to_string()),
  }
}

pub fn ls_workspace(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<Vec<WorkspaceEntry>, String> {
  let workspace_root = resolve_workspace_root(repo_path, workspace_id)?;
  let base_path = Path::new(&workspace_root);
  let mut entries = Vec::new();

  let walker = WalkBuilder::new(&workspace_root)
    .max_depth(Some(1))
    .hidden(false)
    .git_ignore(true)
    .git_global(true)
    .git_exclude(true)
    .parents(true)
    // Version control metadata is not workspace content.
    .filter_entry(|entry| !matches!(entry.file_name().to_str(), Some(".git" | ".jj")))
    .build();

  for entry in walker {
    let entry = entry.map_err(|e| e.to_string())?;
    let entry_path = entry.path();

    if entry_path == base_path {
      continue;
    }

    if let Some(name) = entry_path.file_name().and_then(|name| name.to_str()) {
      let modified_at = std::fs::metadata(entry_path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .map(|modified| chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339());
      entries.push(WorkspaceEntry {
        name: name.to_string(),
        path: entry_path.to_string_lossy().to_string(),
        is_directory: entry_path.is_dir(),
        modified_at,
        submodule_pin: None,
        submodule_synced: None,
        status: None,
      });
    }
  }

  crate::core::submodules::merge_submodules_into_entries(
    repo_path,
    workspace_id,
    &workspace_root,
    &mut entries,
  );

  entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
    (true, false) => std::cmp::Ordering::Less,
    (false, true) => std::cmp::Ordering::Greater,
    _ => a.name.cmp(&b.name),
  });

  Ok(entries)
}

/// Like `ls_workspace`, but also annotates each entry with its jj file
/// status (conflict/committed/working-copy) for the Code tab's file list.
/// This is meaningfully more expensive (three extra jj computations), so it
/// is kept separate from the plain `ls_workspace` used by callers (e.g.
/// `get_workspace_readme`) that don't need status.
pub fn ls_workspace_with_status(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<Vec<WorkspaceEntry>, String> {
  let workspace_root = resolve_workspace_root(repo_path, workspace_id)?;
  let mut entries = ls_workspace(repo_path, workspace_id)?;

  let target_branch = match workspace_id {
    Some(id) => local_db::get_workspace_by_id(repo_path, id)
      .ok()
      .flatten()
      .and_then(|ws| ws.target_branch),
    None => None,
  };
  crate::core::files::annotate_entry_statuses(
    &workspace_root,
    &mut entries,
    target_branch.as_deref(),
  );

  Ok(entries)
}

pub fn get_workspace_readme(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<Option<String>, String> {
  let readme = ls_workspace(repo_path, workspace_id)?
    .into_iter()
    .find(|entry| !entry.is_directory && entry.name.eq_ignore_ascii_case("README.md"));

  match readme {
    Some(entry) => std::fs::read_to_string(&entry.path)
      .map(Some)
      .map_err(|e| format!("Failed to read workspace README '{}': {}", entry.path, e)),
    None => Ok(None),
  }
}

impl WorkspaceMetadata {
  /// Serialize metadata to JSON string for storage
  pub fn to_json(&self) -> String {
    serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
  }
}

pub fn merge_linear_issue_metadata_json(
  existing: Option<&str>,
  issue_key: &str,
  issue_url: &str,
  issue_title: &str,
) -> Result<String, String> {
  let mut metadata = parse_workspace_metadata(existing);
  metadata.linear_issue_key = Some(issue_key.to_string());
  metadata.linear_issue_url = Some(issue_url.to_string());
  metadata.linear_issue_title = Some(issue_title.to_string());
  serde_json::to_string(&metadata).map_err(|e| format!("Failed to serialize metadata: {e}"))
}

pub fn merge_linear_issue_metadata(
  repo_path: &str,
  workspace_id: i64,
  issue_key: &str,
  issue_url: &str,
  issue_title: &str,
) -> Result<local_db::Workspace, String> {
  let existing_workspace = local_db::get_workspace_by_id(repo_path, workspace_id)?
    .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?;

  let new_json = merge_linear_issue_metadata_json(
    existing_workspace.metadata.as_deref(),
    issue_key,
    issue_url,
    issue_title,
  )?;

  local_db::set_workspace_metadata(repo_path, workspace_id, &new_json)?;

  local_db::get_workspace_by_id(repo_path, workspace_id)?
    .ok_or_else(|| format!("Workspace not found after update: {workspace_id}"))
}

pub fn merge_tracker_item_metadata_json(
  existing: Option<&str>,
  provider: &str,
  item_key: &str,
  item_url: &str,
  item_title: &str,
) -> Result<String, String> {
  let mut metadata = parse_workspace_metadata(existing);
  metadata.tracker_provider = Some(provider.to_string());
  metadata.tracker_item_key = Some(item_key.to_string());
  metadata.tracker_item_url = Some(item_url.to_string());
  metadata.tracker_item_title = Some(item_title.to_string());
  serde_json::to_string(&metadata).map_err(|e| format!("Failed to serialize metadata: {e}"))
}

pub fn merge_tracker_item_metadata(
  repo_path: &str,
  workspace_id: i64,
  provider: &str,
  item_key: &str,
  item_url: &str,
  item_title: &str,
) -> Result<local_db::Workspace, String> {
  let existing_workspace = local_db::get_workspace_by_id(repo_path, workspace_id)?
    .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?;

  let new_json = merge_tracker_item_metadata_json(
    existing_workspace.metadata.as_deref(),
    provider,
    item_key,
    item_url,
    item_title,
  )?;

  local_db::set_workspace_metadata(repo_path, workspace_id, &new_json)?;

  local_db::get_workspace_by_id(repo_path, workspace_id)?
    .ok_or_else(|| format!("Workspace not found after update: {workspace_id}"))
}

pub fn parse_hunk_spec(raw: &str) -> Result<HunkSpec, String> {
  let (file_path, range) = raw
    .rsplit_once(':')
    .ok_or_else(|| format!("Invalid hunk spec '{}': expected file:start-end", raw))?;
  if file_path.is_empty() {
    return Err(format!(
      "Invalid hunk spec '{}': file path cannot be empty",
      raw
    ));
  }
  let (start, end) = range
    .split_once('-')
    .ok_or_else(|| format!("Invalid hunk spec '{}': expected start-end", raw))?;
  let start_line = start
    .parse::<usize>()
    .map_err(|_| format!("Invalid hunk spec '{}': start line is not a number", raw))?;
  let end_line = end
    .parse::<usize>()
    .map_err(|_| format!("Invalid hunk spec '{}': end line is not a number", raw))?;
  if start_line == 0 || end_line == 0 {
    return Err(format!(
      "Invalid hunk spec '{}': line numbers must be >= 1",
      raw
    ));
  }
  if end_line < start_line {
    return Err(format!(
      "Invalid hunk spec '{}': end line must be >= start line",
      raw
    ));
  }
  Ok(HunkSpec {
    file_path: file_path.to_string(),
    start_line,
    end_line,
  })
}

/// Creates a new workspace in the repository.
///
/// # Arguments
/// * `repo_path` - Path to the repository root
/// * `branch_name` - Name of the branch to create
/// * `description` - Description for the workspace
/// * `source_branch` - Source branch to create the workspace from
///
/// # Returns
/// Returns the workspace if successful, otherwise an error message.
pub fn create_workspace(
  repo_path: &str,
  branch_name: &str,
  description: Option<String>,
  moved_files: Option<Vec<String>>,
  source_branch: Option<&str>,
  included_copy_files: Option<Vec<String>>,
  sparse_patterns: Option<Vec<String>>,
) -> Result<local_db::Workspace, String> {
  create_workspace_with_symlinked_dirs(
    repo_path,
    branch_name,
    description,
    moved_files,
    source_branch,
    included_copy_files,
    sparse_patterns,
    None,
  )
}

/// Parses the `included_copy_files` repo setting: one repo-relative path per
/// line, blank lines ignored.
pub fn parse_included_copy_files(raw: Option<&str>) -> Option<Vec<String>> {
  raw.map(|s| {
    s.lines()
      .map(str::trim)
      .filter(|l| !l.is_empty())
      .map(str::to_string)
      .collect()
  })
}

/// Creates a workspace the way the app and the CLI both expect: jj workspace,
/// DB row, title, and the flag that triggers a rebase on first view.
pub fn create_new_workspace(
  repo_path: &str,
  branch_name: &str,
  source_branch: Option<&str>,
  metadata: WorkspaceMetadata,
  included_copy_files: Option<Vec<String>>,
) -> Result<local_db::Workspace, String> {
  let workspace = create_workspace_with_symlinked_dirs(
    repo_path,
    branch_name,
    metadata.description,
    metadata.moved_files,
    source_branch,
    included_copy_files,
    metadata.sparse_patterns,
    metadata.symlinked_dirs,
  )?;
  if let Some(title) = metadata
    .title
    .as_deref()
    .map(str::trim)
    .filter(|t| !t.is_empty())
  {
    local_db::update_workspace_title(repo_path, workspace.id, title)?;
  }
  local_db::update_workspace_last_rebased_commit(repo_path, workspace.id, "")?;
  local_db::get_workspace_by_id(repo_path, workspace.id)?
    .ok_or_else(|| format!("Workspace not found after creation: {}", workspace.id))
}

/// Open an existing workspace for a pull-request head branch, or create one.
///
/// Fetches remotes first so the PR head tip is visible, then creates a workspace
/// from that remote tip (when present) and sets `base_branch` as the rebase target.
/// If a workspace for `head_branch` already exists, returns it unchanged.
///
/// Returns `(workspace, created)` where `created` is false when an existing
/// workspace was reused.
pub fn open_or_create_workspace_from_pr(
  repo_path: &str,
  head_branch: &str,
  base_branch: &str,
  title: Option<&str>,
  description: Option<&str>,
) -> Result<(local_db::Workspace, bool), String> {
  let request = BranchWorkspaceRequest {
    head_branch: head_branch.to_string(),
    base_branch: base_branch.to_string(),
    title: title.map(str::to_string),
    description: description.map(str::to_string),
  };
  request.validate()?;

  // Prune rows whose directory or jj registration is gone first; otherwise a
  // stale row is "opened" while the sidebar, which lists jj workspaces, shows nothing.
  sync_workspaces(repo_path)?;

  if let Some(existing) = existing_branch_workspace(repo_path, request.head_branch.trim())? {
    return Ok((existing, false));
  }

  // Ensure remote bookmarks (including the PR head) are imported before create.
  // Best effort: offline, the workspace still opens on the local bookmarks.
  if let Err(error) = jj::jj_git_fetch(repo_path) {
    tracing::warn!(%error, repo_path, "fetch before opening a workspace failed; using local bookmarks");
  }
  open_or_create_synced(repo_path, &request)
}

/// A branch to open a workspace for, as [`open_or_create_workspaces`] takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchWorkspaceRequest {
  pub head_branch: String,
  /// The workspace's rebase target.
  pub base_branch: String,
  pub title: Option<String>,
  pub description: Option<String>,
}

impl BranchWorkspaceRequest {
  fn validate(&self) -> Result<(), String> {
    if self.head_branch.trim().is_empty() {
      return Err("PR head branch is required".to_string());
    }
    if self.base_branch.trim().is_empty() {
      return Err("PR base branch is required".to_string());
    }
    Ok(())
  }
}

/// [`open_or_create_workspace_from_pr`] for several branches of one
/// repository, syncing and fetching once. Each request gets its own result,
/// so one bad branch does not stop the others.
pub fn open_or_create_workspaces(
  repo_path: &str,
  requests: &[BranchWorkspaceRequest],
) -> Vec<Result<(local_db::Workspace, bool), String>> {
  if let Err(err) = sync_workspaces(repo_path) {
    return requests.iter().map(|_| Err(err.clone())).collect();
  }
  if let Err(error) = jj::jj_git_fetch(repo_path) {
    tracing::warn!(%error, repo_path, "fetch before opening workspaces failed; using local bookmarks");
  }
  requests
    .iter()
    .map(|request| {
      request.validate()?;
      open_or_create_synced(repo_path, request)
    })
    .collect()
}

fn existing_branch_workspace(
  repo_path: &str,
  branch: &str,
) -> Result<Option<local_db::Workspace>, String> {
  local_db::get_workspace_by_branch(repo_path, branch)
    .map_err(|e| format!("Failed to check existing workspace: {e}"))
}

/// Opens or creates `request`'s workspace once the repository is synced and
/// fetched.
pub(crate) fn open_or_create_synced(
  repo_path: &str,
  request: &BranchWorkspaceRequest,
) -> Result<(local_db::Workspace, bool), String> {
  let head_branch = request.head_branch.trim();
  let base_branch = request.base_branch.trim();
  if let Some(existing) = existing_branch_workspace(repo_path, head_branch)? {
    return Ok((existing, false));
  }

  let remote_ref = format!("{head_branch}@origin");
  let branch_status = jj::check_branch_exists(repo_path, head_branch)
    .map_err(|e| format!("Failed to check PR head branch '{head_branch}': {e}"))?;
  if !branch_status.local_exists && !branch_status.remote_exists {
    return Err(format!(
      "PR head branch '{head_branch}' was not found locally or on the remote"
    ));
  }

  let source_branch = if branch_status.remote_exists {
    Some(remote_ref.as_str())
  } else {
    None
  };

  let workspace = create_workspace(
    repo_path,
    head_branch,
    request.description.clone(),
    None,
    source_branch,
    None,
    None,
  )?;

  // create_workspace may default the target to the repo default branch; retarget
  // onto the PR base so stacked rebases follow the PR's merge base.
  apply_workspace_target_branch(repo_path, workspace.id, base_branch)?;

  if let Some(title) = request
    .title
    .as_deref()
    .map(str::trim)
    .filter(|s| !s.is_empty())
  {
    local_db::update_workspace_title(repo_path, workspace.id, title)
      .map_err(|e| format!("Failed to set workspace title: {e}"))?;
  }

  let updated = local_db::get_workspace_by_id(repo_path, workspace.id)
    .map_err(|e| format!("Failed to reload workspace after PR create: {e}"))?
    .ok_or_else(|| {
      format!(
        "Workspace not found in database after PR create: {}",
        workspace.id
      )
    })?;

  Ok((updated, true))
}

pub fn open_or_create_workspace_from_issue(
  repo_path: &str,
  branch_name: &str,
  base_branch: &str,
  title: &str,
  description: Option<&str>,
) -> Result<(local_db::Workspace, bool), String> {
  sync_workspaces(repo_path)?;
  if let Some(existing) = local_db::get_workspace_by_branch(repo_path, branch_name)
    .map_err(|e| format!("Failed to check existing workspace: {e}"))?
  {
    return Ok((existing, false));
  }
  let workspace = create_workspace(
    repo_path,
    branch_name,
    description.map(str::to_string),
    None,
    None,
    None,
    None,
  )?;
  apply_workspace_target_branch(repo_path, workspace.id, base_branch)?;
  if !title.trim().is_empty() {
    local_db::update_workspace_title(repo_path, workspace.id, title.trim())
      .map_err(|e| format!("Failed to set workspace title: {e}"))?;
  }
  let updated = local_db::get_workspace_by_id(repo_path, workspace.id)
    .map_err(|e| format!("Failed to reload workspace: {e}"))?
    .ok_or_else(|| format!("Workspace not found after create: {}", workspace.id))?;
  Ok((updated, true))
}

/// Rejects names git cannot store as a branch ref (the `git check-ref-format`
/// rules) plus a leading `-`, so a bad name fails before any jj or DB change
/// instead of at push time.
pub fn validate_branch_name(name: &str) -> Result<(), String> {
  let invalid = |reason: &str| Err(format!("Invalid branch name '{name}': {reason}"));
  if name.is_empty() {
    return invalid("name is empty");
  }
  if name == "@" {
    return invalid("'@' is reserved");
  }
  if name.starts_with('-') {
    return invalid("must not start with '-'");
  }
  if name.starts_with('/') || name.ends_with('/') || name.contains("//") {
    return invalid("empty path component");
  }
  if name.ends_with('.') {
    return invalid("must not end with '.'");
  }
  if name.contains("..") || name.contains("@{") {
    return invalid("contains '..' or '@{'");
  }
  if let Some(c) = name
    .chars()
    .find(|c| c.is_ascii_control() || " ~^:?*[\\".contains(*c))
  {
    return invalid(&format!("contains {c:?}"));
  }
  if name
    .split('/')
    .any(|part| part.starts_with('.') || part.ends_with(".lock"))
  {
    return invalid("a component starts with '.' or ends with '.lock'");
  }
  Ok(())
}

/// Creates a workspace and optionally symlinks heavy directories from the home repo.
///
/// `symlinked_dirs` are paths relative to the repo root (e.g. `node_modules`). Each
/// existing path is linked into the new workspace instead of being copied, so large
/// dependency trees are shared with the home working copy.
#[allow(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
pub fn create_workspace_with_symlinked_dirs(
  repo_path: &str,
  branch_name: &str,
  description: Option<String>,
  moved_files: Option<Vec<String>>,
  source_branch: Option<&str>,
  included_copy_files: Option<Vec<String>>,
  sparse_patterns: Option<Vec<String>>,
  symlinked_dirs: Option<Vec<String>>,
) -> Result<local_db::Workspace, String> {
  validate_branch_name(branch_name)?;
  // None and an empty list both mean a full checkout.
  let sparse_patterns = sparse_patterns.filter(|patterns| !patterns.is_empty());

  // Workspace creation mutates the same JJ operation graph as commits and
  // auto-rebases. Keep the guard through JJ creation and DB registration so
  // an in-process retry cannot observe a half-registered workspace.
  let repo_mutation_lock = commit_lock_for_repo(repo_path);
  let _repo_mutation_guard = repo_mutation_lock.lock_or_recover();

  if local_db::get_workspace_by_branch(repo_path, branch_name)
    .map_err(|e| format!("Failed to check existing workspace: {e}"))?
    .is_some()
  {
    return Err(format!(
      "Workspace '{}' is already registered; refusing to recreate it",
      branch_name
    ));
  }

  // Moved files outside the sparse cone would land in the commit but be invisible on disk.
  if let (Some(patterns), Some(files)) = (&sparse_patterns, &moved_files) {
    for file in files {
      let file = file.trim_end_matches('/');
      let visible = patterns.iter().any(|pattern| {
        let pattern = pattern.trim_end_matches('/');
        file == pattern
          || file.starts_with(&format!("{}/", pattern))
          || pattern.starts_with(&format!("{}/", file))
      });
      if !visible {
        return Err(format!(
                    "Moved file '{}' is outside the sparse patterns {:?}; add a covering pattern or deselect the file",
                    file, patterns
                ));
      }
    }
  }

  let stacked_source_workspace = source_branch
    .map(|src_branch| {
      local_db::get_workspace_by_branch(repo_path, src_branch)
        .map_err(|e| format!("Failed to get source workspace: {}", e))
    })
    .transpose()?
    .flatten();

  if let Some(source_workspace) = &stacked_source_workspace {
    let source_workspace_path = Path::new(repo_path)
      .join(".treq")
      .join("workspaces")
      .join(&source_workspace.workspace_path);
    let _ = jj::jj_get_changed_files(&source_workspace_path.to_string_lossy());
  }

  let branches =
    jj::get_branches(repo_path).map_err(|e| format!("Failed to get branches: {}", e))?;

  let branch_exists: bool = branches.iter().any(|b| b.name == branch_name);

  // Always check if branch exists on remote
  let remote_ref = format!("{}@origin", branch_name);
  let branch_exists_on_remote = jj::check_remote_branch_exists(repo_path, &remote_ref)
    .map_err(|e| format!("Failed to check remote branch: {}", e))?;

  let resolved_source_branch = if source_branch.is_none() {
    // A local tracking branch can lag its remote after fetching. In that case,
    // cloning the branch must start at the remote tip rather than whichever
    // side of the imported bookmark conflict happens to sort first.
    if branch_exists_on_remote
      && (!branch_exists || jj::is_local_branch_behind_remote(repo_path, branch_name))
    {
      Some(remote_ref.clone())
    } else {
      None
    }
  } else {
    source_branch.map(|s| s.to_string())
  };

  // Effective target branch for parent resolution: stacked → source branch, plain → default (usually "main").
  let effective_target_branch: String = if let Some(source_ws) = &stacked_source_workspace {
    source_ws.branch_name.clone()
  } else {
    jj::get_default_branch(repo_path).unwrap_or_else(|_| "main".to_string())
  };

  let new_branch: bool = !branch_exists || resolved_source_branch.as_deref() == Some(&remote_ref);
  let prior_bookmark = branch_exists.then(|| jj::jj_get_commit_id(repo_path, branch_name));
  let workspace_name = jj::create_workspace(
    repo_path,
    branch_name,
    branch_name,
    new_branch,
    resolved_source_branch.as_deref(),
    Some(&effective_target_branch),
    sparse_patterns.as_deref(),
  )
  .map_err(|e| format!("Failed to create workspace: {}", e))?;

  // jj has registered the workspace. Any failure from here on must undo that,
  // or the half-made workspace blocks a retry and later shows up in the
  // sidebar through discovery without its metadata.
  let mut registered_id: Option<i64> = None;
  let result = (|| -> Result<local_db::Workspace, String> {
    // Extract just the sanitized workspace name from the full path
    let workspace_path = Path::new(&workspace_name)
      .file_name()
      .and_then(|name| name.to_str())
      .ok_or("Failed to extract workspace name from path")?
      .to_string();

    let ws_full = Path::new(repo_path)
      .join(".treq")
      .join("workspaces")
      .join(&workspace_path);

    // Copy included files/directories from repo to new workspace
    if let Some(ref patterns) = included_copy_files {
      if !patterns.is_empty() {
        copy_included_files(repo_path, ws_full.to_str().unwrap_or_default(), patterns)?;
      }
    }

    // Symlink heavy directories (e.g. node_modules) to the home repo instead of copying.
    if let Some(ref patterns) = symlinked_dirs {
      if !patterns.is_empty() {
        symlink_included_dirs(repo_path, ws_full.to_str().unwrap_or_default(), patterns)?;
      }
    }

    // Copy .claude/settings.local.json so workspaces inherit local permissions/hooks.
    let claude_src = Path::new(repo_path)
      .join(".claude")
      .join("settings.local.json");
    if claude_src.exists() {
      let claude_dst_dir = ws_full.join(".claude");
      std::fs::create_dir_all(&claude_dst_dir)
        .map_err(|e| format!("Failed to create .claude dir in workspace: {}", e))?;
      std::fs::copy(&claude_src, claude_dst_dir.join("settings.local.json"))
        .map_err(|e| format!("Failed to copy .claude/settings.local.json: {}", e))?;
    }

    if crate::core::feature_preview::is_enabled_in_app_db(
      crate::core::feature_preview::PreviewFeature::SkillsInstallation,
    ) {
      crate::core::skills::materialize_installed_skills(
        repo_path,
        ws_full.to_str().unwrap_or_default(),
      )
      .map_err(|e| format!("Failed to install library skills into workspace: {e}"))?;
    }

    let workspace_id = local_db::add_workspace(
      repo_path,
      workspace_path.clone(),
      workspace_path,
      branch_name.to_string(),
      description,
      moved_files.clone(),
      sparse_patterns.clone(),
    )
    .map_err(|e| format!("Failed to add workspace to db: {}", e))?;
    registered_id = Some(workspace_id);

    // Set not_on_remote flag if branch doesn't exist on remote
    if !branch_exists_on_remote {
      local_db::update_workspace_not_on_remote(repo_path, workspace_id, true)?;
    }

    local_db::update_workspace_target_branch(repo_path, workspace_id, &effective_target_branch)
      .map_err(|e| format!("Failed to set target branch: {}", e))?;

    let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
      .map_err(|e| format!("Failed to get workspace from db: {}", e))?;
    let Some(workspace) = workspace else {
      return Err(format!(
        "Workspace not found in database after creation: {}",
        workspace_id
      ));
    };

    // If moved_files are specified, perform the squash operation
    if let Some(files) = moved_files.clone() {
      if !files.is_empty() {
        let source_workspace_path = if let Some(src_branch) = source_branch {
          // For stacked workspaces, squash from the source workspace
          match stacked_source_workspace.as_ref() {
            Some(ws) => {
              let workspace_dir = Path::new(repo_path)
                .join(".treq")
                .join("workspaces")
                .join(&ws.workspace_path);
              workspace_dir.to_string_lossy().to_string()
            }
            None => {
              let source_ws = local_db::get_workspace_by_branch(repo_path, src_branch)
                .map_err(|e| format!("Failed to get source workspace: {}", e))?;
              match source_ws {
                Some(ws) => {
                  let workspace_dir = Path::new(repo_path)
                    .join(".treq")
                    .join("workspaces")
                    .join(&ws.workspace_path);
                  workspace_dir.to_string_lossy().to_string()
                }
                None => repo_path.to_string(),
              }
            }
          }
        } else {
          // For regular workspaces, squash from the repo root
          repo_path.to_string()
        };

        // Perform the squash operation
        jj::squash_to_workspace(
          &source_workspace_path,
          &workspace.workspace_name,
          Some(files),
        )
        .map_err(|e| format!("Failed to squash files to workspace: {}", e))?;

        let new_workspace_path = Path::new(repo_path)
          .join(".treq")
          .join("workspaces")
          .join(&workspace.workspace_path)
          .to_string_lossy()
          .to_string();
        for path in [&new_workspace_path, &source_workspace_path] {
          if let Err(error) = jj::update_stale_workspace(path) {
            tracing::warn!(%error, workspace_path = %path, "could not update a stale working copy");
          }
        }
      }
    }

    if let Err(error) =
      crate::core::submodules::populate_synced_submodules(repo_path, Some(workspace.id))
    {
      tracing::warn!(%error, repo_path, workspace_id = workspace.id, "could not populate synced submodules");
    }

    Ok(workspace)
  })();

  if result.is_err() {
    // jj::create_workspace pointed the bookmark at the new working copy; put it back.
    let restored = match prior_bookmark {
      None => jj::jj_delete_bookmark(repo_path, branch_name),
      Some(Ok(commit)) => jj::jj_set_bookmark(repo_path, branch_name, &commit),
      Some(Err(e)) => Err(e),
    };
    if let Err(e) = restored {
      tracing::warn!(
        "Rollback: failed to restore bookmark {}: {}",
        branch_name,
        e
      );
    }
    let workspace_dir = Path::new(repo_path)
      .join(".treq")
      .join("workspaces")
      .join(&workspace_name);
    rollback_created_workspace(repo_path, &workspace_dir.to_string_lossy(), registered_id);
  }
  result
}

/// Undo a workspace creation that failed after jj registered the workspace:
/// forget it in jj, remove its directory, and drop its DB row if one was written.
fn rollback_created_workspace(
  repo_path: &str,
  workspace_full_path: &str,
  workspace_id: Option<i64>,
) {
  if let Err(e) = jj::forget_workspace(repo_path, workspace_full_path) {
    tracing::warn!(
      "Rollback: failed to forget workspace {}: {}",
      workspace_full_path,
      e
    );
  }
  if let Err(e) = jj::remove_workspace_directory_only(workspace_full_path) {
    tracing::warn!(
      "Rollback: failed to remove workspace dir {}: {}",
      workspace_full_path,
      e
    );
  }
  if let Some(id) = workspace_id {
    if let Err(e) = local_db::delete_workspace(repo_path, id) {
      tracing::warn!("Rollback: failed to delete workspace row {}: {}", id, e);
    }
  }
}

/// Deletes a workspace from the repository.
pub fn delete_workspace(repo_path: &str, workspace_id: &i64) -> Result<bool, String> {
  let leftover = forget_workspace_record(
    repo_path,
    workspace_id,
    RemoveWorkspaceDiskMode::JjForgetThenRemoveDir,
    WorkspaceRecordMode::Delete,
  )?;
  remove_leftover_directory(leftover);
  Ok(true)
}

/// Forget the jj workspace and remove `.treq/workspaces/…`, but keep the DB row
/// marked `archived`.
pub fn archive_workspace(repo_path: &str, workspace_id: &i64) -> Result<bool, String> {
  let leftover = forget_workspace_record(
    repo_path,
    workspace_id,
    RemoveWorkspaceDiskMode::JjForgetThenRemoveDir,
    WorkspaceRecordMode::Archive,
  )?;
  remove_leftover_directory(leftover);
  Ok(true)
}

/// Forget jj and mark the DB row archived; return the leftover directory so the
/// Tauri command can delete it in the background.
pub fn archive_workspace_leaving_directory(
  repo_path: &str,
  workspace_id: &i64,
) -> Result<Option<String>, String> {
  forget_workspace_record(
    repo_path,
    workspace_id,
    RemoveWorkspaceDiskMode::JjForgetThenRemoveDir,
    WorkspaceRecordMode::Archive,
  )
}

/// Archives a workspace and removes its directory in the background, so the
/// row can go at once (`archive_workspace`).
pub fn archive_workspace_removing_directory_later(
  repo_path: &str,
  workspace_id: i64,
) -> Result<(), String> {
  remove_directory_later(archive_workspace_leaving_directory(
    repo_path,
    &workspace_id,
  )?);
  Ok(())
}

/// [`delete_workspace`] that removes the directory in the background: the
/// checkout has already moved out of `.treq/workspaces/`, and deleting a
/// `node_modules` takes seconds the caller need not wait for.
pub fn delete_workspace_removing_directory_later(
  repo_path: &str,
  workspace_id: i64,
) -> Result<(), String> {
  remove_directory_later(forget_workspace_record(
    repo_path,
    &workspace_id,
    RemoveWorkspaceDiskMode::JjForgetThenRemoveDir,
    WorkspaceRecordMode::Delete,
  )?);
  Ok(())
}

fn remove_directory_later(leftover: Option<String>) {
  if let Some(path) = leftover {
    std::thread::spawn(move || {
      if let Err(e) = jj::remove_workspace_directory_only(&path) {
        tracing::warn!("Background workspace directory removal failed: {}", e);
      }
    });
  }
}

#[derive(Clone, Copy)]
enum RemoveWorkspaceDiskMode {
  /// Forget the jj working copy, leave the directory for a later rm.
  JjForgetThenRemoveDir,
  /// Do not touch jj — use when jj already dropped the workspace (e.g. forgotten elsewhere).
  DirectoryOnly,
}

#[derive(Clone, Copy)]
enum WorkspaceRecordMode {
  Delete,
  Archive,
}

fn remove_leftover_directory(leftover: Option<String>) {
  if let Some(path) = leftover {
    if let Err(e) = jj::remove_workspace_directory_only(&path) {
      tracing::warn!("Warning: Failed to remove workspace directory: {}", e);
    }
  }
}

fn forget_workspace_record(
  repo_path: &str,
  workspace_id: &i64,
  disk: RemoveWorkspaceDiskMode,
  record: WorkspaceRecordMode,
) -> Result<Option<String>, String> {
  let workspace = local_db::get_workspace_by_id(repo_path, *workspace_id)
    .map_err(|e| format!("Failed to get workspace from db: {}", e))?;

  match workspace {
    Some(workspace) => {
      let workspace_path = Path::new(repo_path)
        .join(".treq")
        .join("workspaces")
        .join(&workspace.workspace_path);
      let workspace_path_str = workspace_path
        .to_str()
        .ok_or_else(|| "Workspace path is not valid UTF-8".to_string())?
        .to_string();

      // Splice direct children onto this workspace's own target so a stack
      // stays connected when a middle workspace goes away. Children keep
      // their commits: work from the removed branch now counts as theirs.
      let children = local_db::get_workspaces_by_target_branch(repo_path, &workspace.branch_name)
        .map_err(|e| format!("Failed to get child workspaces: {}", e))?;
      if !children.is_empty() {
        let new_target = match workspace.target_branch.as_deref() {
          Some(target) if !target.is_empty() && target != workspace.branch_name => {
            target.to_string()
          }
          _ => jj::get_default_branch(repo_path).unwrap_or_else(|_| "main".to_string()),
        };
        for child in &children {
          local_db::update_workspace_target_branch(repo_path, child.id, &new_target)
            .map_err(|e| format!("Failed to update child target branch: {}", e))?;
        }
      }

      // Best effort only: DB cleanup must proceed even if jj forget fails.
      let forget_result = match disk {
        RemoveWorkspaceDiskMode::JjForgetThenRemoveDir => {
          jj::forget_workspace(repo_path, &workspace_path_str)
        }
        RemoveWorkspaceDiskMode::DirectoryOnly => Ok(()),
      };
      if let Err(e) = forget_result {
        tracing::warn!("Warning: Failed to forget workspace: {}", e);
      }
      match record {
        WorkspaceRecordMode::Delete => {
          local_db::delete_workspace(repo_path, *workspace_id)
            .map_err(|e| format!("Failed to delete workspace from db: {}", e))?;
        }
        WorkspaceRecordMode::Archive => {
          local_db::archive_workspace(repo_path, *workspace_id)
            .map_err(|e| format!("Failed to archive workspace in db: {}", e))?;
        }
      }
      // Rebase spliced children now so a merged parent's commits drop out of
      // their PRs immediately. Best effort: the next auto-rebase retries.
      if !children.is_empty() {
        let style = resolve_workspace_diff_conflict_marker_style(repo_path)
          .unwrap_or_else(|_| crate::core::DEFAULT_CONFLICT_MARKER_STYLE.to_string());
        for child in &children {
          if let Err(e) = check_and_rebase_workspaces(repo_path, Some(child.id), None, None, &style)
          {
            tracing::warn!("Failed to rebase child workspace {}: {}", child.id, e);
          }
        }
      }
      Ok(Some(move_workspace_dir_to_trash(
        repo_path,
        &workspace_path,
        *workspace_id,
      )))
    }
    _ => Err(format!("Workspace not found in database: {}", workspace_id)),
  }
}

/// Move a forgotten workspace directory out of `.treq/workspaces/` so its slot
/// is free at once. Callers may delete the returned path in the background; a
/// workspace recreated under the same name will not share that path. Falls back
/// to the original path when the rename fails (e.g. the directory is gone).
fn move_workspace_dir_to_trash(repo_path: &str, workspace_dir: &Path, workspace_id: i64) -> String {
  let original = workspace_dir.to_string_lossy().to_string();
  if !workspace_dir.exists() {
    return original;
  }
  let Some(name) = workspace_dir.file_name().and_then(|n| n.to_str()) else {
    return original;
  };
  let trash_dir = Path::new(repo_path).join(".treq").join("trash");
  if let Err(e) = std::fs::create_dir_all(&trash_dir) {
    tracing::warn!("Failed to create workspace trash dir: {}", e);
    return original;
  }
  let stamp = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or_default();
  let target = trash_dir.join(format!("{name}-{workspace_id}-{stamp}"));
  match std::fs::rename(workspace_dir, &target) {
    Ok(()) => target.to_string_lossy().to_string(),
    Err(e) => {
      tracing::warn!("Failed to move workspace dir to trash: {}", e);
      original
    }
  }
}

/// Push workspace to remote and update not_on_remote flag if successful.
/// # Arguments
/// * `repo_path` - Path to the repository root
/// * `workspace_id` - ID of the workspace (None to push home repo)
/// * `force` - Whether to force push
///
/// # Returns
/// Returns the push result message if successful, otherwise an error message.
pub fn push_workspace_to_remote(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<String, String> {
  // Determine the push path and target branch based on workspace_id
  let (push_path, branch_name, target_branch) = if let Some(id) = workspace_id {
    // For workspace, look up the path from database
    let workspace = local_db::get_workspace_by_id(repo_path, id)
      .map_err(|e| format!("Failed to get workspace: {}", e))?
      .ok_or_else(|| format!("Workspace not found: {}", id))?;
    let workspace_dir = Path::new(repo_path)
      .join(".treq")
      .join("workspaces")
      .join(&workspace.workspace_path);
    let push_path = workspace_dir
      .to_str()
      .ok_or("Failed to convert workspace path to string")?
      .to_string();
    let target_branch = workspace
      .target_branch
      .clone()
      .unwrap_or_else(|| "main".to_string());
    (push_path, workspace.branch_name, target_branch)
  } else {
    // For home repo, use repo_path directly
    let branch_name = jj::resolve_home_repo_branch(repo_path)
      .map_err(|e| format!("Failed to resolve home repo branch: {}", e))?;
    let target_branch = jj::get_default_branch(repo_path).unwrap_or_else(|_| "main".to_string());
    (repo_path.to_string(), branch_name, target_branch)
  };

  // An ancestry-only merge cannot be abandoned cleanly while its bookmark still
  // points at it: jj preserves both parents and turns the bookmark into a conflict.
  let _ = jj::jj_collapse_empty_merge_tip(&push_path, &branch_name);
  // Ensure no empty commits reach the remote; best-effort, never blocks the push.
  let _ = jj::jj_abandon_empty_commits(&push_path, &target_branch);

  // Perform the push
  let result = jj::jj_push(&push_path).map_err(|e| format!("Push failed: {}", e))?;

  // Clear the not_on_remote flag after successful push (only for workspaces)
  if let Some(id) = workspace_id {
    local_db::update_workspace_not_on_remote(repo_path, id, false)?;
  }

  Ok(result)
}

/// Capture a token for the working copy's current content, so it can later
/// be restored with `restore_working_copy_snapshot` — used to undo a discard
/// (`jj_restore_file`/`jj_restore_all`).
pub fn snapshot_working_copy(workspace_path: &str) -> Result<String, String> {
  jj::jj_snapshot_working_copy(workspace_path).map_err(|e| e.to_string())
}

/// Restore the working copy to a snapshot previously captured with
/// `snapshot_working_copy`.
pub fn restore_working_copy_snapshot(
  workspace_path: &str,
  snapshot_id: &str,
) -> Result<String, String> {
  jj::jj_restore_snapshot(workspace_path, snapshot_id).map_err(|e| e.to_string())
}

pub fn list_workspaces(repo_path: &str) -> Result<Vec<local_db::Workspace>, String> {
  let workspaces =
    local_db::get_workspaces(repo_path).map_err(|e| format!("Failed to get workspaces: {}", e))?;
  Ok(
    workspaces
      .into_iter()
      .filter(|w| !crate::core::resolve::is_resolve_workspace(w))
      .collect(),
  )
}

/// Every active workspace of the repo, including the ones that resolve a
/// conflict; `list_workspaces` leaves those out.
pub fn list_all_workspaces(repo_path: &str) -> Result<Vec<local_db::Workspace>, String> {
  local_db::get_workspaces(repo_path)
}

/// The workspace with `workspace_id`, when the repo has one.
pub fn find_workspace(
  repo_path: &str,
  workspace_id: i64,
) -> Result<Option<local_db::Workspace>, String> {
  local_db::get_workspace_by_id(repo_path, workspace_id)
}

/// Prunes workspaces whose `.treq/workspaces/…` directories are missing (e.g. deleted outside treq),
/// or whose jj registration was dropped while the directory still exists (e.g. `jj workspace forget`).
pub fn sync_workspaces(repo_path: &str) -> Result<(), String> {
  let workspaces = local_db::get_workspaces(repo_path)?;
  let jj_registered: Option<HashSet<String>> = jj::list_jj_workspaces(repo_path)
    .ok()
    .map(|names| names.into_iter().collect());

  for ws in workspaces {
    let full_path = Path::new(repo_path)
      .join(".treq")
      .join("workspaces")
      .join(&ws.workspace_path);
    if !full_path.exists() {
      delete_workspace(repo_path, &ws.id)?;
      continue;
    }

    if let Some(ref jj_names) = jj_registered {
      if !jj_names.contains(&ws.workspace_name) {
        let leftover = forget_workspace_record(
          repo_path,
          &ws.id,
          RemoveWorkspaceDiskMode::DirectoryOnly,
          WorkspaceRecordMode::Delete,
        )?;
        remove_leftover_directory(leftover);
      }
    }
  }
  Ok(())
}

/// Lists the minimal workspace status needed by the sidebar.
///
/// Discovers jj workspaces, heals conflicted bookmarks losslessly onto a linear
/// tip (rebase local-only commits onto `@origin`), then persists identity fields
/// for the sidebar.
pub fn list_workspace_statuses(repo_path: &str) -> Result<Vec<WorkspaceSidebarStatus>, String> {
  // Workspace creation registers the jj workspace before it writes the DB row.
  // Discovery must not see that window, or it inserts a bare row that makes
  // the create fail on the UNIQUE path. The bookmark healing below also
  // rewrites history, so it belongs under the same lock as commits.
  let repo_mutation_lock = commit_lock_for_repo(repo_path);
  let repo_mutation_guard = repo_mutation_lock.lock_or_recover();
  let discovered = jj::discover_workspaces_with_conflicts(repo_path)
    .map_err(|e| format!("Failed to discover workspaces from jj: {}", e))?;

  // Conflicted bookmarks break tip/PR revsets (`Revision '…' doesn't exist` /
  // ambiguous). Resolve them automatically with linear, lossless history before
  // the sidebar reads identity or conflict state.
  let mut resolved_any = false;
  for workspace in discovered.iter().filter(|w| w.bookmark_conflicted) {
    let workspace_dir = Path::new(repo_path)
      .join(".treq")
      .join("workspaces")
      .join(&workspace.workspace_path);
    let Some(workspace_dir_str) = workspace_dir.to_str() else {
      continue;
    };
    resolved_any = true;
    let remote_ref = format!("{}@origin", workspace.branch_name);
    match jj::jj_resolve_bookmark_conflict_losslessly(
      workspace_dir_str,
      &workspace.branch_name,
      &remote_ref,
    ) {
      Ok(resolution) => {
        log::info!(
          "Auto-resolved conflicted bookmark '{}' for workspace '{}': preserved {} local change(s)",
          workspace.branch_name,
          workspace.workspace_name,
          resolution.preserved_change_ids.len()
        );
      }
      Err(err) => {
        log::warn!(
          "Failed to auto-resolve conflicted bookmark '{}' for workspace '{}': {}",
          workspace.branch_name,
          workspace.workspace_name,
          err
        );
      }
    }
  }

  // Discovery loads the repository once for every workspace; only a
  // resolution above changes what it saw.
  let discovered = if resolved_any {
    jj::discover_workspaces_with_conflicts(repo_path).map_err(|e| {
      format!(
        "Failed to rediscover workspaces after bookmark resolve: {}",
        e
      )
    })?
  } else {
    discovered
  };
  let refreshed_at = chrono::Utc::now().to_rfc3339();
  let conflict_by_path: HashMap<String, bool> = discovered
    .iter()
    .map(|workspace| (workspace.workspace_path.clone(), workspace.has_conflicts))
    .collect();
  let activity_by_path: HashMap<String, Option<String>> = discovered
    .iter()
    .map(|workspace| {
      (
        workspace.workspace_path.clone(),
        workspace.last_activity_at.clone(),
      )
    })
    .collect();
  let persisted = local_db::sync_discovered_workspaces(repo_path, &discovered, &refreshed_at)?;
  drop(repo_mutation_guard);

  persisted
    .into_iter()
    .map(|current| {
      let discovered_has_conflicts = conflict_by_path
        .get(&current.workspace_path)
        .copied()
        .ok_or_else(|| {
          format!(
            "Discovered workspace missing conflict state after sync: {}",
            current.workspace_path
          )
        })?;
      let last_activity_at = activity_by_path
        .get(&current.workspace_path)
        .cloned()
        .flatten();
      // Discovery's flag already covers the working-copy tree's conflicts
      // (`workspace_has_unresolved_conflicts`), from the same load.
      let has_conflicts = discovered_has_conflicts;
      Ok(WorkspaceSidebarStatus {
        current,
        has_conflicts,
        last_activity_at,
      })
    })
    .collect()
}

/// One row of a repository's workspace list on a phone: the workspace and
/// the status the desktop sidebar shows as dots and counts, in one read.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceSummary {
  pub workspace: local_db::Workspace,
  pub has_conflicts: bool,
  /// Uncommitted changes in the working copy.
  pub has_changes: bool,
  /// Commits ahead of the target branch (or the default branch).
  pub commits_ahead: usize,
  /// Persistent terminals (shells and agents) running in the workspace.
  pub terminals: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub last_activity_at: Option<String>,
}

/// [`list_workspace_statuses`] plus each workspace's uncommitted changes,
/// commits ahead and running terminals.
pub fn workspace_summaries(repo_path: &str) -> Result<Vec<WorkspaceSummary>, String> {
  let statuses = list_workspace_statuses(repo_path)?;
  let default_branch = jj::get_default_branch(repo_path).unwrap_or_default();
  let sessions =
    crate::core::pty_remote_supervisor::list_sessions(repo_path, None).unwrap_or_default();
  Ok(
    statuses
      .into_iter()
      .filter(|s| !crate::core::resolve::is_resolve_workspace(&s.current))
      .map(|status| {
        let workspace = status.current;
        let dir = Path::new(repo_path)
          .join(".treq")
          .join("workspaces")
          .join(&workspace.workspace_path);
        let dir = dir.to_string_lossy();
        let has_changes = jj::jj_get_changed_files(&dir)
          .map(|files| !files.is_empty())
          .unwrap_or(false);
        let target = workspace
          .target_branch
          .clone()
          .unwrap_or_else(|| default_branch.clone());
        let commits_ahead = if target.is_empty() {
          0
        } else {
          jj::jj_get_commits_ahead(&dir, &target)
            .map(|ahead| ahead.commits.len())
            .unwrap_or(0)
        };
        let id = workspace.id.to_string();
        let terminals = sessions
          .iter()
          .filter(|s| s.running && (s.workspace == id || s.workspace == workspace.workspace_name))
          .count();
        WorkspaceSummary {
          workspace,
          has_conflicts: status.has_conflicts,
          has_changes,
          commits_ahead,
          terminals,
          last_activity_at: status.last_activity_at,
        }
      })
      .collect(),
  )
}

/// Gets the status of a workspace, including parent, children, and full DAG hierarchy.
///
/// # Arguments
/// * `workspace_path` - Full path to the workspace directory
///
/// # Returns
/// Returns a WorkspaceStatus containing the current workspace, parent, children, DAG nodes, and conflicted workspace IDs.
/// [`workspace_status`]'s `has_changes` alone, without the ahead, conflict
/// and remote reads the full status makes.
pub fn workspace_has_changes(repo_path: &str, workspace_id: i64) -> Result<bool, String> {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get workspace: {}", e))?
    .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
  let workspace_path = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let workspace_path_str = workspace_path
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;
  Ok(
    jj::jj_get_changed_files(workspace_path_str)
      .map(|files| !files.is_empty())
      .unwrap_or(false),
  )
}

pub fn workspace_status(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<WorkspaceStatus, String> {
  let resolve_status_default_branch = || {
    jj::get_default_branch(repo_path)
      .map_err(|e| format!("Failed to resolve default branch: {}", e))
      .unwrap_or_default()
  };

  // Home repo case: no workspace, build a minimal status
  let Some(workspace_id) = workspace_id else {
    let rs = repo_status(repo_path)?;
    if let Some(err) = &rs.fetch_error {
      log::warn!("Home repo fetch: {}", err);
    }
    let default_branch = resolve_status_default_branch();
    let home_branch = jj::resolve_home_repo_branch(repo_path)
      .ok()
      .filter(|branch| !branch.is_empty() && branch != "HEAD")
      .unwrap_or_else(|| default_branch.clone());

    // Synthesize a Workspace-like entry for the home repo
    let home_workspace = local_db::Workspace {
      id: 0,
      repo_path: repo_path.to_string(),
      workspace_name: "home".to_string(),
      workspace_path: repo_path.to_string(),
      branch_name: home_branch.clone(),
      created_at: String::new(),
      refreshed_at: None,
      metadata: None,
      target_branch: None,
      title: home_branch.clone(),
      description: None,
      moved_files: None,
      not_on_remote: matches!(rs.remote_sync, RemoteSyncStatus::NotOnRemote),
      sparse_patterns: None,
      archived: false,
    };

    let conflicted_files =
      jj::get_conflicted_files(repo_path, Some(&home_branch)).unwrap_or_default();

    return Ok(WorkspaceStatus {
      partial: WorkspacePartialStatus {
        current: home_workspace,
        has_conflicts: rs.has_conflicts || !conflicted_files.is_empty(),
        has_changes: rs.has_changes,
        commits_ahead: 0,
      },
      conflicted_files,
      remote_sync: rs.remote_sync,
      target: None,
      children: Vec::new(),
      dag_nodes: Vec::new(),
      conflicted_workspace_ids: Vec::new(),
      commits_ahead_of_target: Vec::new(),
    });
  };

  // Workspace case: look up from DB
  let all_workspaces =
    local_db::get_workspaces(repo_path).map_err(|e| format!("Failed to get workspaces: {}", e))?;

  let current_workspace = all_workspaces
    .iter()
    .find(|w| w.id == workspace_id)
    .cloned()
    .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;

  let workspace_path = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&current_workspace.workspace_path);
  let workspace_path_str = workspace_path
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  // Build branch_name → Workspace lookup map
  let branch_map: HashMap<String, &local_db::Workspace> = all_workspaces
    .iter()
    .map(|w| (w.branch_name.clone(), w))
    .collect();

  // Find direct parent (workspace whose branch_name matches current.target_branch)
  let target = current_workspace
    .target_branch
    .as_ref()
    .and_then(|target| branch_map.get(target).map(|w| (*w).clone()));

  // Find direct children (workspaces where target_branch matches current.branch_name)
  let children: Vec<local_db::Workspace> =
    local_db::get_workspaces_by_target_branch(repo_path, &current_workspace.branch_name)
      .unwrap_or_default();

  let dag_nodes = Vec::new();
  let conflicted_workspace_ids = Vec::new();

  // Calculate commits ahead of target
  let commits_ahead_of_target = if let Some(target_workspace) = &target {
    match jj::jj_get_commits_ahead(workspace_path_str, &target_workspace.branch_name) {
      Ok(commits_ahead) => commits_ahead
        .commits
        .iter()
        .map(|c| WorkspaceCommit {
          hash: c.commit_id.clone(),
          timestamp: c.timestamp.clone(),
          message: c.description.clone(),
        })
        .collect(),
      Err(_) => Vec::new(),
    }
  } else {
    // No target branch, commits are ahead of the repository default branch.
    let default_branch = resolve_status_default_branch();
    if default_branch.is_empty() {
      Vec::new()
    } else {
      match jj::jj_get_commits_ahead(workspace_path_str, &default_branch) {
        Ok(commits_ahead) => commits_ahead
          .commits
          .iter()
          .map(|c| WorkspaceCommit {
            hash: c.commit_id.clone(),
            timestamp: c.timestamp.clone(),
            message: c.description.clone(),
          })
          .collect(),
        Err(_) => Vec::new(),
      }
    }
  };

  let has_changes = jj::jj_get_changed_files(workspace_path_str)
    .map(|files| !files.is_empty())
    .unwrap_or(false);
  let conflict_target = current_workspace
    .target_branch
    .clone()
    .unwrap_or_else(resolve_status_default_branch);
  let conflicted_files = if conflict_target.is_empty() {
    jj::get_conflicted_files(workspace_path_str, None).unwrap_or_default()
  } else {
    jj::get_conflicted_files(workspace_path_str, Some(&conflict_target)).unwrap_or_default()
  };
  // Tree-id resolution and path discovery can diverge (e.g. conflicted committed
  // tip with an empty WC child). Surface a conflict whenever either signal fires.
  let has_conflicts = jj::workspace_has_unresolved_conflicts(workspace_path_str).unwrap_or(false)
    || !conflicted_files.is_empty();

  let commits_ahead_count = commits_ahead_of_target.len();

  let remote_sync = if current_workspace.not_on_remote {
    RemoteSyncStatus::NotOnRemote
  } else if jj::jj_is_bookmark_conflicted(workspace_path_str, &current_workspace.branch_name) {
    match jj::jj_get_diverged_sync_counts(workspace_path_str, &current_workspace.branch_name) {
      Ok((ahead, behind)) => RemoteSyncStatus::Diverged { ahead, behind },
      Err(_) => RemoteSyncStatus::Diverged {
        ahead: 0,
        behind: 0,
      },
    }
  } else {
    match jj::jj_get_sync_status(workspace_path_str, &current_workspace.branch_name, false) {
      Ok((ahead, behind)) => match (ahead, behind) {
        (0, 0) => RemoteSyncStatus::InSync,
        (a, 0) => RemoteSyncStatus::Ahead { count: a },
        (0, b) => RemoteSyncStatus::Behind { count: b },
        (a, b) => RemoteSyncStatus::Diverged {
          ahead: a,
          behind: b,
        },
      },
      Err(_) => RemoteSyncStatus::NotOnRemote,
    }
  };

  Ok(WorkspaceStatus {
    partial: WorkspacePartialStatus {
      current: current_workspace,
      has_conflicts,
      has_changes,
      commits_ahead: commits_ahead_count,
    },
    conflicted_files,
    remote_sync,
    target,
    children,
    dag_nodes,
    conflicted_workspace_ids,
    commits_ahead_of_target,
  })
}

#[cfg(test)]
mod tests {
  use super::{
    merge_linear_issue_metadata_json, merge_tracker_item_metadata_json, parse_hunk_spec,
    parse_workspace_metadata, plan_workspace_target_move,
    resolve_workspace_diff_base_revision_from_last_rebased,
    resolve_workspace_diff_conflict_marker_style,
    resolve_workspace_diff_tip_revision_from_workspace_state,
    resolve_workspace_diff_workspace_revision, HunkSpec, WorkspaceMetadata, WorkspaceMoveRequest,
    WorkspaceTargetMoveStep,
  };
  use crate::jj;
  use crate::local_db::Workspace;
  use rusqlite::Connection;
  use std::fs;
  use std::process::Command;
  use tempfile::TempDir;

  #[test]
  fn ls_workspace_leaves_out_version_control_directories() {
    let temp = TempDir::new().expect("tempdir");
    for dir in [".git", ".jj", ".github", "src"] {
      fs::create_dir(temp.path().join(dir)).expect("create dir");
    }
    fs::write(temp.path().join(".env"), "").expect("write file");
    let repo_path = temp.path().to_str().expect("utf8 path");

    let entries = super::ls_workspace(repo_path, None).expect("list workspace");

    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, [".github", "src", ".env"]);
  }

  fn test_workspace(id: i64, branch: &str, target: Option<&str>) -> Workspace {
    Workspace {
      id,
      repo_path: "/tmp/repo".to_string(),
      workspace_name: branch.to_string(),
      workspace_path: format!("ws/{}", branch),
      branch_name: branch.to_string(),
      created_at: "2026-01-01T00:00:00.000Z".to_string(),
      refreshed_at: None,
      metadata: None,
      target_branch: target.map(str::to_string),
      title: branch.to_string(),
      description: None,
      moved_files: None,
      not_on_remote: false,
      sparse_patterns: None,
      archived: false,
    }
  }

  fn init_workspace_creation_repo(temp: &TempDir) -> String {
    let status = Command::new("jj")
      .current_dir(temp.path())
      .args(["git", "init", "--colocate", "."])
      .status()
      .expect("jj init");
    assert!(status.success());
    // get_default_branch() falls back to git's init.defaultBranch config once no
    // "main"/"master" branch ref exists yet (true here: no commit has been made,
    // so the colocated repo has no real git branch ref at all). That config
    // depends on the runner's git version/settings (observed "master" on the
    // Windows CI image); pin it to "main" so resolution is deterministic.
    let status = Command::new("git")
      .current_dir(temp.path())
      .args(["config", "init.defaultBranch", "main"])
      .status()
      .expect("git config init.defaultBranch");
    assert!(status.success());
    let repo_path = temp.path().to_str().expect("utf8 path").to_string();
    crate::jj::jj_set_bookmark(&repo_path, "main", "@").expect("set main bookmark");
    repo_path
  }

  #[test]
  fn create_workspace_rejects_registered_retry_without_changing_existing_work() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let created = super::create_workspace(&repo_path, "fix/example", None, None, None, None, None)
      .expect("create workspace");
    let marker = temp
      .path()
      .join(".treq/workspaces")
      .join(&created.workspace_path)
      .join("keep-me.txt");
    fs::write(&marker, "existing work").expect("write marker");
    let before = crate::jj::jj_get_commit_id(&repo_path, "fix/example").expect("bookmark target");

    let err = super::create_workspace(&repo_path, "fix/example", None, None, None, None, None)
      .expect_err("retry must fail");

    assert!(
      err.contains("already registered"),
      "unexpected error: {err}"
    );
    assert_eq!(
      fs::read_to_string(marker).expect("marker retained"),
      "existing work"
    );
    assert_eq!(
      crate::jj::jj_get_commit_id(&repo_path, "fix/example").expect("bookmark target unchanged"),
      before
    );
  }

  #[test]
  fn list_workspace_statuses_waits_for_in_flight_create() {
    use crate::lock_ext::LockExt;

    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let lock = crate::core::repo::commit_lock_for_repo(&repo_path);
    let guard = lock.lock_or_recover();

    // Reproduce create_workspace's window: jj has registered the workspace
    // but the DB row is not written yet.
    let full_path = crate::jj::create_workspace(
      &repo_path,
      "fix/race",
      "fix/race",
      true,
      None,
      Some("main"),
      None,
    )
    .expect("register jj workspace");
    let name = std::path::Path::new(&full_path)
      .file_name()
      .and_then(|n| n.to_str())
      .expect("workspace dir name")
      .to_string();
    let status_repo = repo_path.clone();
    let statuses = std::thread::spawn(move || super::list_workspace_statuses(&status_repo));
    std::thread::sleep(std::time::Duration::from_millis(500));

    let id = crate::local_db::add_workspace(
      &repo_path,
      name.clone(),
      name,
      "fix/race".to_string(),
      Some("created by create_workspace".to_string()),
      None,
      None,
    )
    .expect("create's DB insert must not collide with sidebar discovery");
    drop(guard);

    let statuses = statuses.join().expect("status thread").expect("statuses");
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].current.id, id);
    assert_eq!(
      statuses[0].current.description.as_deref(),
      Some("created by create_workspace")
    );
  }

  #[test]
  fn create_workspace_rolls_back_when_setup_step_fails() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    fs::write(temp.path().join("tracked.txt"), "tracked").expect("write tracked file");
    for args in [
      vec!["commit", "-m", "add tracked file"],
      vec!["bookmark", "set", "main", "-r", "@-"],
    ] {
      let status = Command::new("jj")
        .current_dir(temp.path())
        .args(&args)
        .status()
        .expect("jj");
      assert!(status.success(), "jj {:?}", args);
    }

    // Symlinking a tracked path fails after jj has created the workspace.
    let err = super::create_workspace_with_symlinked_dirs(
      &repo_path,
      "fix/rollback",
      None,
      None,
      None,
      None,
      None,
      Some(vec!["tracked.txt".to_string()]),
    )
    .expect_err("symlink over a tracked file must fail");
    assert!(err.contains("already exists"), "unexpected error: {err}");

    assert!(!crate::jj::list_jj_workspaces(&repo_path)
      .expect("list jj workspaces")
      .iter()
      .any(|name| name == "fix-rollback"));
    assert!(!temp.path().join(".treq/workspaces/fix-rollback").exists());
    assert!(super::list_workspaces(&repo_path)
      .expect("list workspaces")
      .is_empty());
    assert!(crate::jj::jj_get_commit_id(&repo_path, "fix/rollback").is_err());
    assert!(gix::open(&repo_path)
      .expect("open git")
      .try_find_reference("refs/heads/fix/rollback")
      .expect("find ref")
      .is_none());

    let retried = super::create_workspace(&repo_path, "fix/rollback", None, None, None, None, None)
      .expect("retry after rollback");
    assert_eq!(retried.branch_name, "fix/rollback");
  }

  #[test]
  fn create_workspace_rollback_restores_an_existing_bookmark() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    fs::write(temp.path().join("tracked.txt"), "tracked").expect("write tracked file");
    for args in [
      vec!["commit", "-m", "add tracked file"],
      vec!["bookmark", "set", "main", "fix/existing", "-r", "@-"],
    ] {
      let status = Command::new("jj")
        .current_dir(temp.path())
        .args(&args)
        .status()
        .expect("jj");
      assert!(status.success(), "jj {:?}", args);
    }
    let before = crate::jj::jj_get_commit_id(&repo_path, "fix/existing").expect("bookmark");

    super::create_workspace_with_symlinked_dirs(
      &repo_path,
      "fix/existing",
      None,
      None,
      None,
      None,
      None,
      Some(vec!["tracked.txt".to_string()]),
    )
    .expect_err("symlink over a tracked file must fail");

    assert_eq!(
      crate::jj::jj_get_commit_id(&repo_path, "fix/existing").expect("bookmark kept"),
      before
    );
  }

  #[test]
  fn create_workspace_rollback_warns_when_conflicted_bookmark_cannot_be_restored() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    fs::write(temp.path().join("tracked.txt"), "tracked").expect("write tracked file");
    for args in [
      vec!["commit", "-m", "add tracked file"],
      vec!["bookmark", "set", "fix/conflicted", "-r", "@-"],
      vec!["bookmark", "set", "fix/conflicted", "-r", "@"],
      vec!["--at-op", "@-", "bookmark", "forget", "fix/conflicted"],
    ] {
      let status = Command::new("jj")
        .current_dir(temp.path())
        .args(&args)
        .status();
      assert!(status.expect("jj").success(), "jj {:?}", args);
    }
    let log = std::sync::Arc::new(fs::File::create(temp.path().join("log")).expect("log"));
    let _g = tracing::subscriber::set_default(tracing_subscriber::fmt().with_writer(log).finish());
    super::create_workspace_with_symlinked_dirs(
      &repo_path,
      "fix/conflicted",
      None,
      None,
      None,
      None,
      None,
      Some(vec!["tracked.txt".to_string()]),
    )
    .expect_err("symlink over a tracked file must fail");
    let log = fs::read_to_string(temp.path().join("log")).expect("read log");
    assert!(log.contains("restore bookmark fix/conflicted"), "{log}");
  }

  #[test]
  fn validate_branch_name_rejects_invalid_git_ref_names() {
    for bad in [
      "",
      " ",
      "foo bar",
      " leading",
      "trailing ",
      "tab\tname",
      "a..b",
      "feat/",
      "/feat",
      "a//b",
      "-x",
      "x.lock",
      "a/.b",
      ".a",
      "x.",
      "a~b",
      "a^b",
      "a:b",
      "a?b",
      "a*b",
      "a[b",
      "a\\b",
      "@",
      "a@{b",
    ] {
      assert!(
        super::validate_branch_name(bad).is_err(),
        "{bad:?} should be rejected"
      );
    }
    for good in [
      "feat/x",
      "fix-1",
      "ty/eng-1_x",
      "release/1.2",
      "café",
      "a@b",
    ] {
      assert!(
        super::validate_branch_name(good).is_ok(),
        "{good:?} should be accepted"
      );
    }
  }

  #[test]
  fn create_workspace_rejects_invalid_branch_name() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);

    let err = super::create_workspace(&repo_path, "foo bar", None, None, None, None, None)
      .expect_err("invalid name must be rejected");

    assert!(
      err.contains("Invalid branch name"),
      "unexpected error: {err}"
    );
    assert!(super::list_workspaces(&repo_path)
      .expect("list workspaces")
      .is_empty());
  }

  #[test]
  fn rename_workspace_dry_run_rejects_invalid_branch_name() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let ws = super::create_workspace(&repo_path, "feat/old", None, None, None, None, None)
      .expect("create workspace");

    for dry_run in [true, false] {
      let result =
        super::rename_workspace(&repo_path, ws.id, "foo bar", dry_run).expect("rename result");
      assert!(!result.success, "dry_run={dry_run}");
      assert!(
        result.message.contains("Invalid branch name"),
        "unexpected message: {}",
        result.message
      );
    }
    assert!(crate::jj::jj_get_commit_id(&repo_path, "feat/old").is_ok());
  }

  #[test]
  fn rename_workspace_moves_bookmark_and_retargets_children() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let parent = super::create_workspace(&repo_path, "feat/old", None, None, None, None, None)
      .expect("create parent");
    let child = super::create_workspace(
      &repo_path,
      "feat/child",
      None,
      None,
      Some("feat/old"),
      None,
      None,
    )
    .expect("create child");
    let before = crate::jj::jj_get_commit_id(&repo_path, "feat/old").expect("old target");

    let result = super::rename_workspace(&repo_path, parent.id, "feat/new", false).expect("rename");

    assert!(result.success, "{}", result.message);
    assert_eq!(result.updated_children_ids, vec![child.id]);
    assert_eq!(
      crate::jj::jj_get_commit_id(&repo_path, "feat/new").expect("new target"),
      before
    );
    assert!(crate::jj::jj_get_commit_id(&repo_path, "feat/old").is_err());
    let child = crate::local_db::get_workspace_by_id(&repo_path, child.id)
      .expect("lookup")
      .expect("child");
    assert_eq!(child.target_branch.as_deref(), Some("feat/new"));

    // The sidebar's discovery pass must not undo the rename.
    super::list_workspace_statuses(&repo_path).expect("statuses");
    let parent = crate::local_db::get_workspace_by_id(&repo_path, parent.id)
      .expect("lookup")
      .expect("parent");
    assert_eq!(parent.branch_name, "feat/new");
  }

  #[test]
  fn retarget_rolls_back_bridge_lift_when_the_move_fails() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let a = super::create_workspace(&repo_path, "feat/a", None, None, None, None, None)
      .expect("create a");
    let b = super::create_workspace(&repo_path, "feat/b", None, None, Some("feat/a"), None, None)
      .expect("create b");
    super::create_workspace(&repo_path, "feat/c", None, None, Some("feat/b"), None, None)
      .expect("create c");
    // Break the final step: a's workspace directory is gone, so its rebase fails.
    fs::remove_dir_all(temp.path().join(".treq/workspaces").join(&a.workspace_path))
      .expect("remove a's workspace dir");

    // Moving a onto its grandchild c first lifts b onto main, then fails.
    super::retarget_workspace(&repo_path, a.id, "feat/c", "main")
      .expect_err("moving a workspace with no directory must fail");

    let b = crate::local_db::get_workspace_by_id(&repo_path, b.id)
      .expect("lookup")
      .expect("b");
    assert_eq!(
      b.target_branch.as_deref(),
      Some("feat/a"),
      "the bridge lift must be undone when a later step fails"
    );
  }

  #[test]
  fn update_workspace_with_title_clears_title_and_description() {
    use super::MaybeEmptyParam;

    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let ws = super::create_workspace(
      &repo_path,
      "feat/clear",
      Some("old description".to_string()),
      None,
      None,
      None,
      None,
    )
    .expect("create workspace");
    crate::local_db::update_workspace_title(&repo_path, ws.id, "Old title").expect("set title");

    let updated = super::update_workspace_with_title(
      &repo_path,
      ws.id,
      MaybeEmptyParam::Omitted,
      MaybeEmptyParam::EmptyValue,
      MaybeEmptyParam::EmptyValue,
    )
    .expect("update workspace");

    assert_eq!(updated.title, "feat/clear", "title falls back to branch");
    assert_eq!(updated.description, None);
  }

  #[test]
  fn update_workspace_with_empty_target_uses_repo_default_branch() {
    use super::MaybeEmptyParam;

    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    // A repo whose trunk is not called "main".
    crate::jj::jj_set_bookmark(&repo_path, "trunk", "main").expect("set trunk");
    crate::jj::jj_delete_bookmark(&repo_path, "main").expect("drop main");
    let status = Command::new("git")
      .current_dir(temp.path())
      .args(["config", "init.defaultBranch", "trunk"])
      .status()
      .expect("git config");
    assert!(status.success());
    let base = super::create_workspace(&repo_path, "feat/base", None, None, None, None, None)
      .expect("create base");
    let ws = super::create_workspace(
      &repo_path,
      "feat/child",
      None,
      None,
      Some("feat/base"),
      None,
      None,
    )
    .expect("create child");
    assert_eq!(ws.target_branch.as_deref(), Some(base.branch_name.as_str()));

    let updated = super::update_workspace_with_title(
      &repo_path,
      ws.id,
      MaybeEmptyParam::EmptyValue,
      MaybeEmptyParam::Omitted,
      MaybeEmptyParam::Omitted,
    )
    .expect("clear target");

    assert_eq!(updated.target_branch.as_deref(), Some("trunk"));
  }

  #[test]
  fn create_new_workspace_applies_title_and_marks_first_rebase() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);

    let ws = super::create_new_workspace(
      &repo_path,
      "feat/cli",
      None,
      WorkspaceMetadata {
        title: Some("From the CLI".to_string()),
        description: Some("desc".to_string()),
        ..Default::default()
      },
      None,
    )
    .expect("create workspace");

    assert_eq!(ws.title, "From the CLI");
    assert_eq!(ws.description.as_deref(), Some("desc"));
    assert_eq!(
      crate::local_db::get_workspace_last_rebased_commit(&repo_path, ws.id).expect("rebase flag"),
      Some(String::new())
    );
  }

  #[test]
  fn parse_included_copy_files_trims_and_skips_blank_lines() {
    assert_eq!(
      super::parse_included_copy_files(Some(" .env \n\n  config/local.yml\n")),
      Some(vec![".env".to_string(), "config/local.yml".to_string()])
    );
    assert_eq!(super::parse_included_copy_files(None), None);
  }

  #[test]
  fn create_workspace_reuses_branch_name_after_archive() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let first = super::create_workspace(&repo_path, "fix/reuse", None, None, None, None, None)
      .expect("create workspace");
    super::archive_workspace(&repo_path, &first.id).expect("archive workspace");

    let second = super::create_workspace(&repo_path, "fix/reuse", None, None, None, None, None)
      .expect("recreate archived branch name");

    assert_ne!(second.id, first.id);
    assert!(temp
      .path()
      .join(".treq/workspaces")
      .join(&second.workspace_path)
      .exists());
    let live = super::list_workspaces(&repo_path).expect("list workspaces");
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].id, second.id);
  }

  #[test]
  fn archive_moves_directory_out_before_background_removal() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let first = super::create_workspace(&repo_path, "fix/race", None, None, None, None, None)
      .expect("create workspace");
    let workspace_dir = temp
      .path()
      .join(".treq/workspaces")
      .join(&first.workspace_path);

    let leftover = super::archive_workspace_leaving_directory(&repo_path, &first.id)
      .expect("archive workspace")
      .expect("leftover directory");

    // The directory is out of the workspace slot before the caller deletes it,
    // so an immediate recreate cannot race the background removal.
    assert!(!workspace_dir.exists());
    let second = super::create_workspace(&repo_path, "fix/race", None, None, None, None, None)
      .expect("recreate while removal is pending");
    let marker = workspace_dir.join("keep-me.txt");
    fs::write(&marker, "new work").expect("write marker");

    crate::jj::remove_workspace_directory_only(&leftover).expect("background removal");

    assert_eq!(second.workspace_path, first.workspace_path);
    assert_eq!(fs::read_to_string(marker).expect("marker"), "new work");
  }

  #[test]
  fn open_or_create_workspace_from_pr_ignores_archived_workspace() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let first = super::create_workspace(&repo_path, "fix/pr", None, None, None, None, None)
      .expect("create workspace");
    super::archive_workspace(&repo_path, &first.id).expect("archive workspace");

    let (workspace, created) =
      super::open_or_create_workspace_from_pr(&repo_path, "fix/pr", "main", None, None)
        .expect("open PR workspace");

    assert!(created);
    assert_ne!(workspace.id, first.id);
    assert!(!workspace.archived);
  }

  #[test]
  fn open_or_create_workspace_from_pr_recreates_workspace_with_missing_directory() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let first = super::create_workspace(&repo_path, "fix/pr", None, None, None, None, None)
      .expect("create workspace");
    let workspace_dir = temp
      .path()
      .join(".treq/workspaces")
      .join(&first.workspace_path);
    fs::remove_dir_all(&workspace_dir).expect("remove workspace directory");

    let (workspace, created) =
      super::open_or_create_workspace_from_pr(&repo_path, "fix/pr", "main", None, None)
        .expect("open PR workspace");

    assert!(created);
    assert!(workspace_dir.exists());
    let listed = super::list_workspace_statuses(&repo_path).expect("list statuses");
    assert!(listed.iter().any(|s| s.current.id == workspace.id));
  }

  #[test]
  fn open_or_create_workspaces_reports_each_branch() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let existing = super::create_workspace(&repo_path, "fix/open", None, None, None, None, None)
      .expect("create workspace");
    crate::jj::jj_set_bookmark(&repo_path, "feat/new", "@").expect("set bookmark");
    // Branch checks read git refs.
    let status = Command::new("jj")
      .current_dir(temp.path())
      .args(["git", "export"])
      .status()
      .expect("jj git export");
    assert!(status.success());
    let request = |head: &str, title: Option<&str>| super::BranchWorkspaceRequest {
      head_branch: head.into(),
      base_branch: "main".into(),
      title: title.map(str::to_string),
      description: None,
    };

    let results = super::open_or_create_workspaces(
      &repo_path,
      &[
        request("fix/open", None),
        request("feat/new", Some("New thing")),
        request("feat/missing", None),
      ],
    );

    let (reused, created) = results[0].as_ref().expect("existing workspace");
    assert_eq!((reused.id, *created), (existing.id, false));
    let (made, created) = results[1].as_ref().expect("new workspace");
    assert!(*created);
    assert_eq!(made.branch_name, "feat/new");
    assert_eq!(made.title, "New thing");
    assert_eq!(made.target_branch.as_deref(), Some("main"));
    assert!(results[2].as_ref().unwrap_err().contains("feat/missing"));
  }

  #[test]
  fn open_or_create_workspace_from_issue_ignores_archived_workspace() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let (first, _) =
      super::open_or_create_workspace_from_issue(&repo_path, "ty/eng-1", "main", "Issue", None)
        .expect("create issue workspace");
    super::archive_workspace(&repo_path, &first.id).expect("archive workspace");

    let (workspace, created) =
      super::open_or_create_workspace_from_issue(&repo_path, "ty/eng-1", "main", "Issue", None)
        .expect("reopen issue workspace");

    assert!(created);
    assert_ne!(workspace.id, first.id);
  }

  #[test]
  fn create_workspace_recovers_orphaned_partial_directory() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let partial = temp.path().join(".treq/workspaces/fix-partial");
    fs::create_dir_all(partial.join(".jj")).expect("create partial");
    fs::write(partial.join("partial"), "incomplete").expect("write marker");

    let created = super::create_workspace(&repo_path, "fix/partial", None, None, None, None, None)
      .expect("recover partial workspace");

    assert_eq!(created.branch_name, "fix/partial");
    assert!(!partial.join("partial").exists());
  }

  #[test]
  fn create_workspace_recovers_workspace_registered_by_interrupted_create() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    // A create killed after jj registered the workspace but before it moved
    // the working copy off the root commit and set the bookmark.
    fs::create_dir_all(temp.path().join(".treq/workspaces")).expect("workspaces dir");
    let status = Command::new("jj")
      .current_dir(temp.path())
      .args(["workspace", "add", "--name", "fix-killed", "-r", "root()"])
      .arg(".treq/workspaces/fix-killed")
      .status()
      .expect("jj workspace add");
    assert!(status.success());

    let created = super::create_workspace(&repo_path, "fix/killed", None, None, None, None, None)
      .expect("retry recovers the interrupted create");

    assert_eq!(created.workspace_path, "fix-killed");
    assert!(crate::jj::jj_get_commit_id(&repo_path, "fix/killed").is_ok());
  }

  #[test]
  fn create_workspace_leaves_no_directory_when_source_is_missing() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);

    super::create_workspace(
      &repo_path,
      "fix/bad-source",
      None,
      None,
      Some("nonexistent"),
      None,
      None,
    )
    .expect_err("missing source must fail");

    assert!(!temp.path().join(".treq/workspaces/fix-bad-source").exists());
  }

  #[test]
  fn opens_or_creates_linear_issue_workspace_without_existing_branch() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);

    let (created, was_created) = super::open_or_create_workspace_from_issue(
      &repo_path,
      "ty/treq-281-linear-integration",
      "main",
      "Linear integration should CRUD issues",
      Some("Implement the issue workflow"),
    )
    .expect("create Linear issue workspace");

    assert!(was_created);
    assert_eq!(created.branch_name, "ty/treq-281-linear-integration");
    assert_eq!(created.target_branch.as_deref(), Some("main"));
    assert_eq!(created.title, "Linear integration should CRUD issues");
    assert_eq!(
      created.description.as_deref(),
      Some("Implement the issue workflow")
    );

    let (reopened, was_created) = super::open_or_create_workspace_from_issue(
      &repo_path,
      "ty/treq-281-linear-integration",
      "main",
      "Ignored replacement title",
      None,
    )
    .expect("reuse Linear issue workspace");

    assert!(!was_created);
    assert_eq!(reopened.id, created.id);
    assert_eq!(reopened.title, "Linear integration should CRUD issues");
  }

  #[test]
  fn plan_move_onto_non_descendant_is_single_step() {
    let parent = test_workspace(1, "feat/parent", Some("main"));
    let other = test_workspace(2, "feat/other", Some("main"));
    let child = test_workspace(3, "feat/child", Some("feat/parent"));

    let steps = plan_workspace_target_move(
      &[parent, other, child.clone()],
      "feat/child",
      "feat/other",
      "main",
    )
    .expect("plan should succeed");

    assert_eq!(
      steps,
      vec![WorkspaceTargetMoveStep {
        workspace_id: child.id,
        branch_name: "feat/child".to_string(),
        new_target_branch: "feat/other".to_string(),
      }]
    );
  }

  #[test]
  fn plan_parent_below_child_lifts_child_first() {
    let parent = test_workspace(1, "feat/parent", Some("main"));
    let child = test_workspace(2, "feat/child", Some("feat/parent"));

    let steps = plan_workspace_target_move(
      &[parent.clone(), child.clone()],
      "feat/parent",
      "feat/child",
      "main",
    )
    .expect("plan should succeed");

    assert_eq!(
      steps,
      vec![
        WorkspaceTargetMoveStep {
          workspace_id: child.id,
          branch_name: "feat/child".to_string(),
          new_target_branch: "main".to_string(),
        },
        WorkspaceTargetMoveStep {
          workspace_id: parent.id,
          branch_name: "feat/parent".to_string(),
          new_target_branch: "feat/child".to_string(),
        },
      ]
    );
  }

  #[test]
  fn plan_three_level_root_below_tip_lifts_bridge() {
    let a = test_workspace(1, "feat/a", Some("main"));
    let b = test_workspace(2, "feat/b", Some("feat/a"));
    let c = test_workspace(3, "feat/c", Some("feat/b"));

    let steps = plan_workspace_target_move(&[a.clone(), b.clone(), c], "feat/a", "feat/c", "main")
      .expect("plan should succeed");

    assert_eq!(
      steps,
      vec![
        WorkspaceTargetMoveStep {
          workspace_id: b.id,
          branch_name: "feat/b".to_string(),
          new_target_branch: "main".to_string(),
        },
        WorkspaceTargetMoveStep {
          workspace_id: a.id,
          branch_name: "feat/a".to_string(),
          new_target_branch: "feat/c".to_string(),
        },
      ]
    );
  }

  #[test]
  fn plan_middle_below_child_in_three_level_stack() {
    let a = test_workspace(1, "feat/a", Some("main"));
    let b = test_workspace(2, "feat/b", Some("feat/a"));
    let c = test_workspace(3, "feat/c", Some("feat/b"));

    let steps = plan_workspace_target_move(&[a, b.clone(), c.clone()], "feat/b", "feat/c", "main")
      .expect("plan should succeed");

    assert_eq!(
      steps,
      vec![
        WorkspaceTargetMoveStep {
          workspace_id: c.id,
          branch_name: "feat/c".to_string(),
          new_target_branch: "feat/a".to_string(),
        },
        WorkspaceTargetMoveStep {
          workspace_id: b.id,
          branch_name: "feat/b".to_string(),
          new_target_branch: "feat/c".to_string(),
        },
      ]
    );
  }

  #[test]
  fn plan_rejects_self_target() {
    let parent = test_workspace(1, "feat/parent", Some("main"));
    let err = plan_workspace_target_move(&[parent], "feat/parent", "feat/parent", "main")
      .expect_err("self target should fail");
    assert!(err.to_lowercase().contains("cycle"));
  }

  #[test]
  fn plan_uses_default_branch_when_lifting_off_null_target_root() {
    let parent = test_workspace(1, "feat/parent", None);
    let child = test_workspace(2, "feat/child", Some("feat/parent"));

    let steps = plan_workspace_target_move(
      &[parent.clone(), child.clone()],
      "feat/parent",
      "feat/child",
      "develop",
    )
    .expect("plan should succeed");

    assert_eq!(
      steps,
      vec![
        WorkspaceTargetMoveStep {
          workspace_id: child.id,
          branch_name: "feat/child".to_string(),
          new_target_branch: "develop".to_string(),
        },
        WorkspaceTargetMoveStep {
          workspace_id: parent.id,
          branch_name: "feat/parent".to_string(),
          new_target_branch: "feat/child".to_string(),
        },
      ]
    );
  }

  #[test]
  fn parse_workspace_metadata_full_json() {
    let parsed = parse_workspace_metadata(Some(
      r#"{"title":"T","description":"D","moved_files":["a.rs"],"sparse_patterns":["src","docs"],"symlinked_dirs":["node_modules"]}"#,
    ));
    assert_eq!(parsed.title.as_deref(), Some("T"));
    assert_eq!(parsed.description.as_deref(), Some("D"));
    assert_eq!(parsed.moved_files, Some(vec!["a.rs".to_string()]));
    assert_eq!(
      parsed.sparse_patterns,
      Some(vec!["src".to_string(), "docs".to_string()])
    );
    assert_eq!(
      parsed.symlinked_dirs,
      Some(vec!["node_modules".to_string()])
    );
  }

  #[test]
  fn parse_workspace_metadata_missing_fields_and_empty_arrays() {
    let parsed = parse_workspace_metadata(Some(
      r#"{"title":"T","moved_files":[],"sparse_patterns":[],"symlinked_dirs":[]}"#,
    ));
    assert_eq!(parsed.title.as_deref(), Some("T"));
    assert_eq!(parsed.description, None);
    assert_eq!(parsed.moved_files, None, "empty array should become None");
    assert_eq!(
      parsed.sparse_patterns, None,
      "empty array should become None"
    );
    assert_eq!(
      parsed.symlinked_dirs, None,
      "empty array should become None"
    );

    let absent = parse_workspace_metadata(None);
    assert_eq!(absent.title, None);
    assert_eq!(absent.sparse_patterns, None);
    assert_eq!(absent.symlinked_dirs, None);

    let invalid = parse_workspace_metadata(Some("not json"));
    assert_eq!(invalid.title, None);
    assert_eq!(invalid.sparse_patterns, None);
    assert_eq!(invalid.symlinked_dirs, None);
  }

  #[test]
  fn resolve_workspace_diff_conflict_marker_style_defaults_when_settings_table_missing() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let db_path = temp_dir.path().join("treq.db");
    Connection::open(&db_path)
      .expect("db should be openable")
      .execute("CREATE TABLE unrelated (id INTEGER PRIMARY KEY)", [])
      .expect("setup table should succeed");

    let _app_data_dir = crate::core::AppDataDirGuard::set(temp_dir.path());
    let style = resolve_workspace_diff_conflict_marker_style("/unused/repo/path");

    assert_eq!(
      style.expect("should resolve style"),
      crate::core::DEFAULT_CONFLICT_MARKER_STYLE
    );
  }

  #[test]
  fn resolve_workspace_diff_base_revision_prefers_last_rebased_commit() {
    let base = resolve_workspace_diff_base_revision_from_last_rebased(
      Some("  abc123  ".to_string()),
      "main",
    );
    assert_eq!(base, "abc123");
  }

  #[test]
  fn revision_diff_is_empty_when_workspace_dir_is_gone() {
    let temp_dir = TempDir::new().expect("temp dir");
    let missing = temp_dir.path().join("already-removed");
    let diff =
      jj::revision_diff_if_workspace_missing(&missing).expect("missing dir should skip jj load");
    assert!(diff.uncommitted_files.is_empty());
    assert!(diff.committed_files.is_empty());
    assert!(!diff.too_large_to_render);
    assert!(jj::revision_diff_if_workspace_missing(temp_dir.path()).is_none());
  }

  #[test]
  fn resolve_workspace_diff_base_revision_falls_back_to_target_branch() {
    assert_eq!(
      resolve_workspace_diff_base_revision_from_last_rebased(None, "main"),
      "main"
    );
    assert_eq!(
      resolve_workspace_diff_base_revision_from_last_rebased(Some("   ".to_string()), "dev"),
      "dev"
    );
  }

  #[test]
  fn resolve_workspace_diff_tip_revision_prefers_workspace_branch_over_parent_pointer() {
    let tip = resolve_workspace_diff_tip_revision_from_workspace_state("feature", "main", 3);
    assert_eq!(tip, "feature");
  }

  #[test]
  fn resolve_workspace_diff_tip_revision_uses_parent_pointer_only_for_empty_target_workspace() {
    let tip = resolve_workspace_diff_tip_revision_from_workspace_state("main", "main", 0);
    assert_eq!(tip, "@-");
  }

  #[test]
  fn resolve_workspace_diff_uses_working_copy_for_conflicted_bookmark() {
    assert_eq!(
      resolve_workspace_diff_workspace_revision("feature", true),
      "@"
    );
    assert_eq!(
      resolve_workspace_diff_workspace_revision("feature", false),
      "feature"
    );
  }

  #[test]
  fn parse_hunk_spec_accepts_file_range_format() {
    let spec = parse_hunk_spec("src/main.rs:10-20").expect("valid hunk spec");
    assert_eq!(
      spec,
      HunkSpec {
        file_path: "src/main.rs".to_string(),
        start_line: 10,
        end_line: 20,
      }
    );
  }

  #[test]
  fn parse_hunk_spec_rejects_malformed_values() {
    assert!(parse_hunk_spec("src/main.rs").is_err());
    assert!(parse_hunk_spec("src/main.rs:10").is_err());
    assert!(parse_hunk_spec("src/main.rs:abc-10").is_err());
    assert!(parse_hunk_spec("src/main.rs:20-10").is_err());
  }

  #[test]
  fn workspace_move_request_requires_at_least_one_selector() {
    let request = WorkspaceMoveRequest::default();
    assert!(!request.has_selectors());
  }

  #[test]
  fn merge_tracker_item_metadata_preserves_linear_fields() {
    let existing = merge_linear_issue_metadata_json(None, "ENG-1", "https://l/ENG-1", "L").unwrap();
    let result = merge_tracker_item_metadata_json(
      Some(&existing),
      "github",
      "#7",
      "https://github.com/acme/app/issues/7",
      "Rotate keys",
    )
    .unwrap();
    let parsed: WorkspaceMetadata = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed.linear_issue_key.as_deref(), Some("ENG-1"));
    assert_eq!(parsed.tracker_provider.as_deref(), Some("github"));
    assert_eq!(parsed.tracker_item_key.as_deref(), Some("#7"));
    assert_eq!(
      parsed.tracker_item_url.as_deref(),
      Some("https://github.com/acme/app/issues/7")
    );
    assert_eq!(parsed.tracker_item_title.as_deref(), Some("Rotate keys"));
  }

  #[test]
  fn merge_linear_issue_metadata_empty_existing() {
    let result = merge_linear_issue_metadata_json(
      None,
      "ENG-123",
      "https://linear.app/issue/ENG-123",
      "Test Issue",
    );
    assert!(result.is_ok());
    let json = result.unwrap();
    let parsed: WorkspaceMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.linear_issue_key, Some("ENG-123".to_string()));
    assert_eq!(
      parsed.linear_issue_url,
      Some("https://linear.app/issue/ENG-123".to_string())
    );
    assert_eq!(parsed.linear_issue_title, Some("Test Issue".to_string()));
  }

  #[test]
  fn merge_linear_issue_metadata_preserves_existing_fields() {
    let existing = r#"{"title":"My Workspace","description":"A description"}"#;
    let result = merge_linear_issue_metadata_json(
      Some(existing),
      "ENG-456",
      "https://linear.app/issue/ENG-456",
      "Another Issue",
    );
    assert!(result.is_ok());
    let json = result.unwrap();
    let parsed: WorkspaceMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.title, Some("My Workspace".to_string()));
    assert_eq!(parsed.description, Some("A description".to_string()));
    assert_eq!(parsed.linear_issue_key, Some("ENG-456".to_string()));
    assert_eq!(
      parsed.linear_issue_url,
      Some("https://linear.app/issue/ENG-456".to_string())
    );
    assert_eq!(parsed.linear_issue_title, Some("Another Issue".to_string()));
  }

  #[test]
  fn merge_linear_issue_metadata_persists_to_real_workspace() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = init_workspace_creation_repo(&temp);
    let created = super::create_workspace(
      &repo_path,
      "eng-142-fix-login-bug",
      None,
      None,
      None,
      None,
      None,
    )
    .expect("create workspace");

    let updated = super::merge_linear_issue_metadata(
      &repo_path,
      created.id,
      "ENG-142",
      "https://linear.app/treq/issue/ENG-142",
      "Fix login bug",
    )
    .expect("merge linear metadata");

    let parsed = parse_workspace_metadata(updated.metadata.as_deref());
    assert_eq!(parsed.linear_issue_key, Some("ENG-142".to_string()));
    assert_eq!(
      parsed.linear_issue_url,
      Some("https://linear.app/treq/issue/ENG-142".to_string())
    );
    assert_eq!(parsed.linear_issue_title, Some("Fix login bug".to_string()));

    let reloaded = crate::local_db::get_workspace_by_id(&repo_path, created.id)
      .expect("get workspace")
      .expect("workspace exists");
    let reparsed = parse_workspace_metadata(reloaded.metadata.as_deref());
    assert_eq!(reparsed.linear_issue_key, Some("ENG-142".to_string()));
  }
}

/// Merges a workspace's commits into the home repository and cleans up the workspace.
///
/// # Arguments
/// * `repo_path` - Path to the repository root
/// * `workspace_id` - ID of the workspace to merge
/// * `message` - Commit message for the merge
///
/// # Returns
/// Returns Ok(()) on success, or an error message on failure.
pub fn merge_workspace(
  repo_path: &str,
  workspace_id: i64,
  message: &str,
  merge_strategy: MergeCommit,
) -> Result<(), String> {
  merge_workspace_with(repo_path, workspace_id, message, merge_strategy, false)
}

/// [`merge_workspace`] that removes the merged checkout in the background
/// (see [`delete_workspace_removing_directory_later`]).
pub fn merge_workspace_removing_directory_later(
  repo_path: &str,
  workspace_id: i64,
  message: &str,
  merge_strategy: MergeCommit,
) -> Result<(), String> {
  merge_workspace_with(repo_path, workspace_id, message, merge_strategy, true)
}

fn merge_workspace_with(
  repo_path: &str,
  workspace_id: i64,
  message: &str,
  merge_strategy: MergeCommit,
  defer_removal: bool,
) -> Result<(), String> {
  // Get the workspace from the database
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get workspace from db: {}", e))?
    .ok_or("Workspace not found in database")?;

  let workspace_path = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);

  let workspace_path_str = workspace_path
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  // Get target branch for comparison
  let target_branch = workspace.target_branch.as_deref().unwrap_or("main");

  // Abandon empty commits before merge to keep history clean
  let _ = jj::jj_abandon_empty_commits(workspace_path_str, target_branch);

  // Get commits ahead of target
  let commits_ahead = jj::jj_get_commits_ahead(workspace_path_str, target_branch)
    .map_err(|e| format!("Failed to get commits: {}", e))?;

  if commits_ahead.commits.is_empty() {
    return Err("No commits to merge".to_string());
  }

  match merge_strategy {
    MergeCommit::Merge => {
      jj::jj_create_merge_commit(
        workspace_path_str,
        &workspace.branch_name,
        target_branch,
        message,
        "diff",
      )
      .map_err(|e| format!("Failed to create merge commit: {}", e))?;
    }
    MergeCommit::SquashAndMerge => {
      jj::jj_squash_merge_commit(
        workspace_path_str,
        &workspace.branch_name,
        target_branch,
        message,
      )
      .map_err(|e| format!("Failed to squash merge workspace: {}", e))?;
    }
    MergeCommit::RebaseAndMerge => {
      let rebase_result = jj::jj_rebase_merge_commit(
        workspace_path_str,
        &workspace.branch_name,
        target_branch,
        message,
      )
      .map_err(|e| format!("Failed to rebase merge workspace: {}", e))?;

      if !rebase_result.success {
        return Err(format!("Rebase merge failed: {}", rebase_result.message));
      }
    }
  }

  jj::jj_edit_bookmark(repo_path, target_branch)
    .map_err(|e| format!("Failed to update home repo to target branch: {}", e))?;
  if workspace.branch_name == target_branch {
    sync_home_and_workspace_for_branch(repo_path, target_branch, SyncSource::WorkspaceToHome)?;
  }

  // Remove the workspace from jj and its directory from disk.
  jj::forget_workspace(repo_path, workspace_path_str)
    .map_err(|e| format!("Failed to remove workspace from jj: {}", e))?;
  if defer_removal {
    remove_directory_later(Some(move_workspace_dir_to_trash(
      repo_path,
      &workspace_path,
      workspace_id,
    )));
  } else {
    jj::remove_workspace_directory_only(workspace_path_str)
      .map_err(|e| format!("Failed to remove workspace from jj: {}", e))?;
  }

  // Remove from database
  local_db::delete_workspace(repo_path, workspace_id)
    .map_err(|e| format!("Failed to delete workspace from db: {}", e))?;

  Ok(())
}

fn sync_home_and_workspace_for_branch(
  repo_path: &str,
  branch: &str,
  source: SyncSource,
) -> Result<(), String> {
  if branch.is_empty() || branch == "HEAD" {
    return Ok(());
  }

  let home_branch = jj::resolve_home_repo_branch(repo_path)
    .map_err(|e| format!("Failed to resolve home repo branch: {}", e))?;
  if home_branch.is_empty() || home_branch == "HEAD" || home_branch != branch {
    return Ok(());
  }

  let Some(workspace) = local_db::get_workspace_by_branch(repo_path, branch)
    .map_err(|e| format!("Failed to get workspace by branch: {}", e))?
  else {
    return Ok(());
  };
  let workspace_path = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let workspace_path_str = workspace_path
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  let source_path = match source {
    SyncSource::HomeToWorkspace => repo_path,
    SyncSource::WorkspaceToHome => workspace_path_str,
  };
  let _ = jj::jj_workspace_update_stale(source_path);
  // Use `@-` when `@` is the empty working copy, so no empty commit gets pushed.
  let source_tip = jj::jj_resolve_bookmark_tip(source_path)
    .map_err(|e| format!("Failed to resolve source tip: {}", e))?;

  let destination_path = match source {
    SyncSource::HomeToWorkspace => workspace_path_str,
    SyncSource::WorkspaceToHome => repo_path,
  };
  let _ = jj::jj_workspace_update_stale(destination_path);
  jj::jj_set_bookmark(destination_path, branch, &source_tip).map_err(|e| {
    format!(
      "Failed to force-sync destination bookmark '{}' to source tip: {}",
      branch, e
    )
  })?;
  match source {
    SyncSource::WorkspaceToHome => {
      jj::jj_edit_bookmark(destination_path, branch).map_err(|e| {
        format!(
          "Failed to sync destination working copy to branch '{}': {}",
          branch, e
        )
      })?;
      let _ = jj::jj_workspace_update_stale(destination_path);
    }
    SyncSource::HomeToWorkspace => {
      let _ = jj::jj_workspace_update_stale(destination_path);
      // Move @ onto the new bookmark tip so the workspace working copy stays in sync (avoids infinite auto-sync loop).
      if let Err(e) = jj::jj_sync_working_copy_if_safe(destination_path, branch) {
        log::warn!(
          "sync_home_and_workspace_for_branch: could not sync workspace @ to '{}': {}",
          branch,
          e
        );
      }
    }
  }
  Ok(())
}

/// One target_branch update in a cycle-safe retarget plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceTargetMoveStep {
  pub workspace_id: i64,
  pub branch_name: String,
  pub new_target_branch: String,
}

fn is_descendant_of(workspaces: &[local_db::Workspace], candidate: &str, ancestor: &str) -> bool {
  if candidate == ancestor {
    return false;
  }

  let by_branch: HashMap<&str, &local_db::Workspace> = workspaces
    .iter()
    .map(|ws| (ws.branch_name.as_str(), ws))
    .collect();

  let mut current = candidate;
  let mut visited = HashSet::new();

  loop {
    if !visited.insert(current) {
      return false;
    }
    let Some(ws) = by_branch.get(current) else {
      return false;
    };
    let Some(target) = ws.target_branch.as_deref() else {
      return false;
    };
    if target == ancestor {
      return true;
    }
    current = target;
  }
}

/// Plan `target_branch` updates so `moving_branch` can target `new_target_branch`
/// without creating a cycle.
///
/// When the new target is a descendant (parent dragged below a child), the
/// immediate child of `moving_branch` on the path to the new target is lifted
/// onto `moving_branch`'s current parent first, then the move is applied.
pub fn plan_workspace_target_move(
  workspaces: &[local_db::Workspace],
  moving_branch: &str,
  new_target_branch: &str,
  default_branch: &str,
) -> Result<Vec<WorkspaceTargetMoveStep>, String> {
  let by_branch: HashMap<&str, &local_db::Workspace> = workspaces
    .iter()
    .map(|ws| (ws.branch_name.as_str(), ws))
    .collect();

  let Some(moving) = by_branch.get(moving_branch) else {
    return Ok(vec![]);
  };

  if moving_branch == new_target_branch {
    return Err("Cannot target self: would create a cycle".to_string());
  }

  let mut steps = Vec::new();

  if is_descendant_of(workspaces, new_target_branch, moving_branch) {
    let mut current = new_target_branch;
    let mut visited = HashSet::new();
    let mut bridge: Option<&local_db::Workspace> = None;

    loop {
      if !visited.insert(current) {
        break;
      }
      let Some(ws) = by_branch.get(current) else {
        break;
      };
      let Some(target) = ws.target_branch.as_deref() else {
        break;
      };
      if target == moving_branch {
        bridge = Some(ws);
        break;
      }
      current = target;
    }

    if let Some(bridge) = bridge {
      let lift_target = moving.target_branch.as_deref().unwrap_or(default_branch);
      if lift_target != bridge.branch_name {
        steps.push(WorkspaceTargetMoveStep {
          workspace_id: bridge.id,
          branch_name: bridge.branch_name.clone(),
          new_target_branch: lift_target.to_string(),
        });
      }
    }
  }

  steps.push(WorkspaceTargetMoveStep {
    workspace_id: moving.id,
    branch_name: moving.branch_name.clone(),
    new_target_branch: new_target_branch.to_string(),
  });

  Ok(steps)
}

/// Rebase a single workspace onto `target_branch` and persist the target.
pub fn apply_workspace_target_branch(
  repo_path: &str,
  workspace_id: i64,
  target_branch: &str,
) -> Result<local_db::Workspace, String> {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get workspace from db: {}", e))?
    .ok_or("Workspace not found in database")?;

  let workspace_path = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let workspace_path_str = workspace_path
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  let _ = jj::jj_git_fetch(repo_path);

  // Lifting off a parent workspace must leave that parent's commits in place.
  let parent_workspace = workspace.target_branch.as_deref().filter(|parent| {
    matches!(
      local_db::get_workspace_by_branch(repo_path, parent),
      Ok(Some(_))
    )
  });
  let rebase_result = jj::jj_retarget_workspace_bookmark(
    workspace_path_str,
    &workspace.branch_name,
    target_branch,
    parent_workspace,
  )
  .map_err(|e| format!("Failed to rebase workspace: {}", e))?;

  if !rebase_result.success {
    return Err(format!("Rebase failed: {}", rebase_result.message));
  }

  local_db::update_workspace_target_branch(repo_path, workspace_id, target_branch)
    .map_err(|e| format!("Failed to update target branch: {}", e))?;

  local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get updated workspace: {}", e))?
    .ok_or_else(|| "Workspace not found after update".to_string())
}

/// Retarget a workspace onto `new_target_branch`, lifting bridge children first
/// when needed so parent/child stack reorders stay acyclic.
///
/// Shared by the UI (`set_workspace_target_branch` / `update_workspace`) and CLI
/// (`treq set -t`).
pub fn retarget_workspace(
  repo_path: &str,
  workspace_id: i64,
  new_target_branch: &str,
  default_branch: &str,
) -> Result<local_db::Workspace, String> {
  let workspaces =
    local_db::get_workspaces(repo_path).map_err(|e| format!("Failed to list workspaces: {}", e))?;
  let moving = workspaces
    .iter()
    .find(|ws| ws.id == workspace_id)
    .ok_or("Workspace not found in database")?;

  let steps = plan_workspace_target_move(
    &workspaces,
    &moving.branch_name,
    new_target_branch,
    default_branch,
  )?;

  let mut updated = None;
  // (workspace_id, original target) of each applied step, for rollback.
  let mut applied: Vec<(i64, String)> = Vec::new();
  for step in steps {
    match apply_workspace_target_branch(repo_path, step.workspace_id, &step.new_target_branch) {
      Ok(ws) => {
        let original = workspaces
          .iter()
          .find(|w| w.id == step.workspace_id)
          .and_then(|w| w.target_branch.clone())
          .unwrap_or_else(|| default_branch.to_string());
        applied.push((step.workspace_id, original));
        updated = Some(ws);
      }
      Err(err) => {
        // Undo earlier steps so a failed reorder doesn't leave the stack half-moved.
        let rollback_errors: Vec<String> = applied
          .iter()
          .rev()
          .filter_map(|(id, original)| {
            apply_workspace_target_branch(repo_path, *id, original).err()
          })
          .collect();
        if rollback_errors.is_empty() {
          return Err(err);
        }
        return Err(format!(
          "{err} (rollback of earlier steps also failed: {})",
          rollback_errors.join("; ")
        ));
      }
    }
  }

  updated.ok_or_else(|| "No retarget steps produced".to_string())
}

/// Updates a workspace's target branch and/or description.
/// Rebases the workspace to the target branch and updates metadata.
/// The workspace's branch name remains unchanged.
pub fn update_workspace(
  repo_path: &str,
  workspace_id: i64,
  target_branch: MaybeEmptyParam<String>,
  description: MaybeEmptyParam<String>,
) -> Result<local_db::Workspace, String> {
  update_workspace_with_title(
    repo_path,
    workspace_id,
    target_branch,
    MaybeEmptyParam::Omitted,
    description,
  )
}

pub fn update_workspace_with_title(
  repo_path: &str,
  workspace_id: i64,
  target_branch: MaybeEmptyParam<String>,
  title: MaybeEmptyParam<String>,
  description: MaybeEmptyParam<String>,
) -> Result<local_db::Workspace, String> {
  let _workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get workspace from db: {}", e))?
    .ok_or("Workspace not found in database")?;

  if !matches!(target_branch, MaybeEmptyParam::Omitted) {
    let default_branch = jj::get_default_branch(repo_path).unwrap_or_else(|_| "main".to_string());
    // An empty target means "back onto the repo's default branch".
    let new_target = match target_branch {
      MaybeEmptyParam::Some(branch) => branch,
      _ => default_branch.clone(),
    };
    retarget_workspace(repo_path, workspace_id, &new_target, &default_branch)?;
  }

  match title {
    MaybeEmptyParam::Some(title_str) => {
      local_db::update_workspace_title(repo_path, workspace_id, &title_str)
        .map_err(|e| format!("Failed to update title: {}", e))?
    }
    MaybeEmptyParam::EmptyValue => local_db::clear_workspace_title(repo_path, workspace_id)
      .map_err(|e| format!("Failed to clear title: {}", e))?,
    MaybeEmptyParam::Omitted => {}
  }

  match description {
    MaybeEmptyParam::Some(description_str) => {
      local_db::update_workspace_description(repo_path, workspace_id, &description_str)
        .map_err(|e| format!("Failed to update description: {}", e))?
    }
    MaybeEmptyParam::EmptyValue => local_db::clear_workspace_description(repo_path, workspace_id)
      .map_err(|e| format!("Failed to clear description: {}", e))?,
    MaybeEmptyParam::Omitted => {}
  }

  local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get updated workspace: {}", e))?
    .ok_or_else(|| "Workspace not found after update".to_string())
}

/// Renames a workspace's jj bookmark/branch.
///
/// In dry_run mode, validates the new name (checks for clashes with existing local/remote
/// branches) without performing the rename.
///
/// # Arguments
/// * `repo_path` - Path to the repository root
/// * `workspace_id` - ID of the workspace to rename
/// * `new_branch_name` - The new branch name
/// * `dry_run` - If true, only validate without performing the rename
pub fn rename_workspace(
  repo_path: &str,
  workspace_id: i64,
  new_branch_name: &str,
  dry_run: bool,
) -> Result<RenameWorkspaceResult, String> {
  // 1. Look up workspace by ID
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get workspace: {}", e))?
    .ok_or("Workspace not found")?;

  let old_branch_name = &workspace.branch_name;

  // 2. Check same-name
  if old_branch_name == new_branch_name {
    return Ok(RenameWorkspaceResult {
      success: false,
      message: "New name is the same as the current name".to_string(),
      workspace: None,
      updated_children_ids: vec![],
    });
  }

  if let Err(message) = validate_branch_name(new_branch_name) {
    return Ok(RenameWorkspaceResult {
      success: false,
      message,
      workspace: None,
      updated_children_ids: vec![],
    });
  }

  if local_db::get_workspace_by_branch(repo_path, new_branch_name)
    .map_err(|e| format!("Failed to check existing workspace: {}", e))?
    .is_some()
  {
    return Ok(RenameWorkspaceResult {
      success: false,
      message: format!(
        "Branch '{}' already exists for another workspace",
        new_branch_name
      ),
      workspace: None,
      updated_children_ids: vec![],
    });
  }

  // 3. Check local branch clash
  let branches =
    jj::get_branches(repo_path).map_err(|e| format!("Failed to get branches: {}", e))?;
  if branches.iter().any(|b| b.name == new_branch_name) {
    return Ok(RenameWorkspaceResult {
      success: false,
      message: format!("Branch '{}' already exists locally", new_branch_name),
      workspace: None,
      updated_children_ids: vec![],
    });
  }

  // 4. Check remote branch clash
  let remote_ref = format!("{}@origin", new_branch_name);
  let remote_exists = jj::check_remote_branch_exists(repo_path, &remote_ref)
    .map_err(|e| format!("Failed to check remote branch: {}", e))?;
  if remote_exists {
    return Ok(RenameWorkspaceResult {
      success: false,
      message: format!("Branch '{}' already exists on remote", new_branch_name),
      workspace: None,
      updated_children_ids: vec![],
    });
  }

  // 5. If dry_run, return success without performing the rename
  if dry_run {
    return Ok(RenameWorkspaceResult {
      success: true,
      message: format!("'{}' is available", new_branch_name),
      workspace: None,
      updated_children_ids: vec![],
    });
  }

  // Bookmark edits share the jj operation graph with commits and rebases.
  let repo_mutation_lock = commit_lock_for_repo(repo_path);
  let _repo_mutation_guard = repo_mutation_lock.lock_or_recover();

  // 6. Construct full workspace path
  let workspace_path = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let workspace_path_str = workspace_path
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  // 7. Rename the bookmark in one jj transaction so a failure can't leave both names or neither.
  jj::jj_rename_bookmark(workspace_path_str, old_branch_name, new_branch_name)
    .map_err(|e| format!("Failed to rename bookmark: {}", e))?;

  // 8. Update branch name in DB
  local_db::update_workspace_branch_name(repo_path, workspace_id, new_branch_name)
    .map_err(|e| format!("Failed to update branch name in DB: {}", e))?;

  // 9. Mark as not_on_remote (new name hasn't been pushed)
  local_db::update_workspace_not_on_remote(repo_path, workspace_id, true)
    .map_err(|e| format!("Failed to update not_on_remote: {}", e))?;

  // 10. Update children targeting the old branch name
  let children = local_db::get_workspaces_by_target_branch(repo_path, old_branch_name)
    .map_err(|e| format!("Failed to get child workspaces: {}", e))?;

  let mut updated_children_ids = Vec::new();
  for child in &children {
    local_db::update_workspace_target_branch(repo_path, child.id, new_branch_name)
      .map_err(|e| format!("Failed to update child target branch: {}", e))?;
    updated_children_ids.push(child.id);
  }

  // 11. Return updated workspace
  let updated_workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get updated workspace: {}", e))?;

  Ok(RenameWorkspaceResult {
    success: true,
    message: format!("Renamed '{}' to '{}'", old_branch_name, new_branch_name),
    workspace: updated_workspace,
    updated_children_ids,
  })
}
/// Reserved source/destination name for `move_workspace_changes` that refers to the
/// home repo (the repo root outside of any `.treq` workspace) instead of a registered
/// workspace. Mirrors the shell convention for "here" (the home repo root).
pub const HOME_MOVE_ENDPOINT: &str = ".";

fn is_home_move_endpoint(branch: &str) -> bool {
  branch == HOME_MOVE_ENDPOINT
}

struct MoveEndpoint {
  /// `None` for the home repo, `Some(id)` for a registered workspace.
  workspace_id: Option<i64>,
  full_path: String,
}

fn resolve_move_endpoint(
  repo_path: &str,
  branch: &str,
  label: &str,
) -> Result<MoveEndpoint, String> {
  if is_home_move_endpoint(branch) {
    return Ok(MoveEndpoint {
      workspace_id: None,
      full_path: repo_path.to_string(),
    });
  }

  let workspace = local_db::get_workspace_by_branch(repo_path, branch)
    .map_err(|e| format!("Failed to look up {} workspace: {}", label, e))?
    .ok_or_else(|| format!("{} workspace '{}' not found", label, branch))?;
  let full_path = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let full_path_str = full_path
    .to_str()
    .ok_or_else(|| format!("Failed to convert {} workspace path to string", label))?
    .to_string();

  Ok(MoveEndpoint {
    workspace_id: Some(workspace.id),
    full_path: full_path_str,
  })
}

pub fn move_workspace_changes(
  repo_path: &str,
  source_branch: &str,
  destination_branch: &str,
  request: WorkspaceMoveRequest,
) -> Result<WorkspaceMoveResult, String> {
  if !request.has_selectors() {
    return Err("Must specify at least one selector: -f, -r, or -c".to_string());
  }
  if source_branch == destination_branch
    || (is_home_move_endpoint(source_branch) && is_home_move_endpoint(destination_branch))
  {
    return Err("Source and destination must be different".to_string());
  }

  let source = resolve_move_endpoint(repo_path, source_branch, "Source")?;
  let destination = resolve_move_endpoint(repo_path, destination_branch, "Destination")?;

  let source_full_path_str = source.full_path;
  let destination_full_path_str = destination.full_path;

  let mut result = WorkspaceMoveResult {
    commits_moved: 0,
    files_moved: 0,
    hunks_applied: 0,
    hunks_skipped: 0,
    warnings: Vec::new(),
  };
  let mut commits_to_abandon_from_source: Vec<String> = Vec::new();

  if !request.commits.is_empty() {
    let source_log =
      crate::core::commits::list_commits(repo_path, source.workspace_id, false, None, None)?;
    let history_ids: HashSet<String> = source_log
      .commits
      .iter()
      .flat_map(|commit| [commit.change_id.clone(), commit.commit_id.clone()])
      .filter(|id| !id.is_empty())
      .collect();
    let source_workspace = match source.workspace_id {
      Some(id) => local_db::get_workspace_by_id(repo_path, id)?,
      None => None,
    };
    for commit_id in &request.commits {
      if !history_ids.contains(commit_id) {
        return Err(format!(
          "Commit '{}' not found in source workspace history",
          commit_id
        ));
      }
      if let Some(workspace) = &source_workspace {
        crate::core::commits::ensure_rewritable(repo_path, workspace, commit_id)?;
      }
    }

    for commit_id in &request.commits {
      let commit_paths = jj::jj_diff_summary(&source_full_path_str, commit_id)
        .map_err(|e| format!("Failed to diff commit '{}': {}", commit_id, e))?;
      jj::move_paths_between_workspace_paths(
        &source_full_path_str,
        &destination_full_path_str,
        Some(commit_paths),
      )
      .map_err(|e| format!("Failed to move commit to destination workspace: {}", e))?;
      jj::update_stale_workspace(&destination_full_path_str)
        .map_err(|e| format!("Failed to update destination workspace working copy: {}", e))?;
      commits_to_abandon_from_source.push(commit_id.clone());
      result.commits_moved += 1;
    }
  }

  // Preserve commit-moved files across abandon-triggered destination checkout.
  let mut commit_moved_file_saves: Vec<(String, Vec<u8>)> = Vec::new();
  for commit_id in &commits_to_abandon_from_source {
    if let Ok(file_paths) = jj::jj_diff_summary(&source_full_path_str, commit_id) {
      for file_path in file_paths {
        let dest_file = std::path::Path::new(&destination_full_path_str).join(&file_path);
        if let Ok(content) = std::fs::read(&dest_file) {
          commit_moved_file_saves.push((file_path, content));
        }
      }
    }
  }

  // Only files changed in the source move; the rest become warnings.
  let mut files = Vec::new();
  if !request.files.is_empty() {
    // Record the source changes before the filesystem transfer so tracked
    // files can be restored to their parent state rather than left deleted.
    let changed = jj::jj_get_changed_files(&source_full_path_str)
      .map_err(|e| format!("Failed to snapshot source files: {}", e))?;
    for file in &request.files {
      let prefix = format!("{}/", file.trim_end_matches('/'));
      let has_changes = changed.iter().any(|change| {
        std::iter::once(&change.path)
          .chain(change.previous_path.as_ref())
          .any(|path| path == file || path.starts_with(&prefix))
      });
      if has_changes {
        files.push(file.clone());
      } else {
        result.warnings.push(format!(
          "'{}' has no changes in '{}'; skipped",
          file, source_branch
        ));
      }
    }
    if files.is_empty() && request.commits.is_empty() && request.hunks.is_empty() {
      return Err(format!(
        "Nothing to move: the selected files have no changes in '{}': {}",
        source_branch,
        request.files.join(", ")
      ));
    }
  }
  if !files.is_empty() {
    jj::move_paths_between_workspace_paths(
      &source_full_path_str,
      &destination_full_path_str,
      Some(files.clone()),
    )
    .map_err(|e| format!("Failed to move files: {}", e))?;
    // Source restoration can reconcile other workspaces and check out the
    // destination before its copied paths have been snapshotted. Preserve
    // those bytes across that rewrite, then snapshot them afterward.
    let destination_file_saves: Vec<(String, Option<Vec<u8>>)> = files
      .iter()
      .map(|file_path| {
        let path = Path::new(&destination_full_path_str).join(file_path);
        (file_path.clone(), std::fs::read(path).ok())
      })
      .collect();
    for file_path in &files {
      jj::jj_restore_file(&source_full_path_str, file_path)
        .map_err(|e| format!("Failed to restore source file '{}': {}", file_path, e))?;
    }
    for (file_path, content) in destination_file_saves {
      let path = Path::new(&destination_full_path_str).join(&file_path);
      if let Some(content) = content {
        if let Some(parent) = path.parent() {
          std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to recreate destination directory: {}", e))?;
        }
        std::fs::write(&path, content)
          .map_err(|e| format!("Failed to restore destination file: {}", e))?;
      } else if path.exists() {
        std::fs::remove_file(&path)
          .map_err(|e| format!("Failed to restore destination deletion: {}", e))?;
      }
    }
    jj::jj_get_changed_files(&destination_full_path_str)
      .map_err(|e| format!("Failed to snapshot destination files: {}", e))?;
    result.files_moved = files.len();
  }

  if !request.hunks.is_empty() {
    let hunk_outcome = jj::move_hunks_between_workspaces(
      &source_full_path_str,
      &destination_full_path_str,
      &request.hunks,
    )
    .map_err(|e| format!("Failed to move hunks: {}", e))?;
    result.hunks_applied = hunk_outcome.applied;
    result.hunks_skipped = hunk_outcome.skipped;
    result.warnings.extend(hunk_outcome.warnings);
  }

  for commit_id in &commits_to_abandon_from_source {
    jj::jj_abandon(&source_full_path_str, commit_id)
      .map_err(|e| format!("Failed to abandon commit: {}", e))?;
    jj::update_stale_workspace(&source_full_path_str)
      .map_err(|e| format!("Failed to update source workspace working copy: {}", e))?;
  }

  // Restore files that destination checkout may have removed.
  for (file_path, content) in &commit_moved_file_saves {
    let dest_file = std::path::Path::new(&destination_full_path_str).join(file_path);
    if let Some(parent) = dest_file.parent() {
      let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&dest_file, content);
  }

  let _ = jj::update_stale_workspace(&source_full_path_str);
  let _ = jj::update_stale_workspace(&destination_full_path_str);

  Ok(result)
}

/// Pull a workspace from remote, automatically resolving divergence by rebasing
/// local mutable commits onto the remote tip.
///
/// When the local bookmark has diverged from its remote counterpart (both have
/// new commits), this function:
/// 1. Fetches remote changes
/// 2. Captures local-only commit IDs (before resolving the bookmark)
/// 3. Resolves the bookmark conflict by pointing to the remote tip
/// 4. Rebases local mutable commits onto the new bookmark tip
/// 5. Refreshes the working copy
pub fn pull_workspace_from_remote(
  repo_path: &str,
  workspace_id: Option<i64>,
  conflict_marker_style: &str,
) -> Result<PullWorkspaceResult, String> {
  // When workspace_id is None, do a simple pull for the home repo
  let Some(workspace_id) = workspace_id else {
    // For home repo, just fetch — do not rebase, which would detach HEAD
    jj::jj_git_fetch(repo_path).map_err(|e| format!("Fetch failed: {}", e))?;
    let home_branch = jj::resolve_home_repo_branch(repo_path)
      .map_err(|e| format!("Failed to resolve home repo branch: {}", e))?;
    sync_home_and_workspace_for_branch(repo_path, &home_branch, SyncSource::HomeToWorkspace)?;
    return Ok(PullWorkspaceResult {
      success: true,
      message: "Fetched home repo".to_string(),
      was_diverged: false,
      commits_rebased: 0,
      has_conflicts: false,
    });
  };

  // Look up workspace from DB
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get workspace: {}", e))?
    .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;

  let full_path = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let full_path_str = full_path
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  let branch_name = &workspace.branch_name;

  // An automatic review is due only when the fetch actually moved the branch
  // this workspace tracks, so read its remote tip on both sides of the fetch.
  let remote_target = format!("{}@origin", branch_name);
  let target_before = jj::jj_get_commit_id(full_path_str, &remote_target).ok();

  // Step 1: Fetch remote changes
  jj::jj_git_fetch(repo_path).map_err(|e| format!("Fetch failed: {}", e))?;

  let target_moved = jj::jj_get_commit_id(full_path_str, &remote_target).ok() != target_before;

  // Step 2: Check if bookmark is conflicted (diverged)
  let is_conflicted = jj::jj_is_bookmark_conflicted(full_path_str, branch_name);

  if !is_conflicted {
    // No divergence — try to sync working copy if safe
    let _ = jj::jj_workspace_update_stale(full_path_str);
    let _ = jj::jj_sync_working_copy_if_safe(full_path_str, branch_name);

    if target_moved {
      auto_review::notify(
        repo_path,
        workspace_id,
        branch_name,
        full_path_str,
        ReviewTrigger::Pull,
      );
    }

    return Ok(PullWorkspaceResult {
      success: true,
      message: "Fetched remote changes (no divergence)".to_string(),
      was_diverged: false,
      commits_rebased: 0,
      has_conflicts: workspace_pull_has_conflicts(full_path_str, branch_name),
    });
  }

  // Step 3: Diverged — capture local-only commit IDs BEFORE resolving bookmark
  let remote_ref = format!("{}@origin", branch_name);
  let local_revset = format!("({}..@-) & mutable()", remote_ref);
  let local_commit_ids = jj::jj_log_revset_commit_ids(full_path_str, &local_revset)
    .map_err(|e| format!("Failed to capture local commits: {}", e))?;

  let commits_rebased = local_commit_ids.len();

  // Step 4: Resolve bookmark conflict — point local bookmark to remote tip
  jj::jj_set_bookmark(full_path_str, branch_name, &remote_ref)
    .map_err(|e| format!("Failed to resolve bookmark conflict: {}", e))?;

  // Step 5: Rebase local commits onto the new bookmark tip (if any)
  if !local_commit_ids.is_empty() {
    let ids_revset = local_commit_ids.join(" | ");
    let roots_revset = format!("roots({})", ids_revset);

    jj::jj_rebase_with_revset(
      full_path_str,
      &roots_revset,
      branch_name,
      branch_name,
      conflict_marker_style,
    )
    .map_err(|e| format!("Failed to rebase local commits: {}", e))?;

    // Step 5b: Advance the bookmark to the rebased local tip.
    // Without this, the bookmark stays on origin and Sync reports InSync while
    // conflicted/rebased work only lives on @ — users can't resolve locally and push.
    advance_bookmark_to_working_copy_tip(full_path_str, branch_name)?;
  }

  // Step 6: refresh stale working copy only; do not sync bookmark tip here.
  let _ = jj::jj_workspace_update_stale(full_path_str);
  sync_home_and_workspace_for_branch(repo_path, branch_name, SyncSource::WorkspaceToHome)?;

  let has_conflicts = workspace_pull_has_conflicts(full_path_str, branch_name);

  // A diverged pull rewrites the workspace's own commits onto the remote tip,
  // so its result is worth reviewing even when the remote tip stood still.
  auto_review::notify(
    repo_path,
    workspace_id,
    branch_name,
    full_path_str,
    ReviewTrigger::Pull,
  );

  Ok(PullWorkspaceResult {
    success: true,
    message: if has_conflicts {
      format!(
        "Rebased {} local commit(s) onto remote tip with conflicts to resolve",
        commits_rebased
      )
    } else {
      format!(
        "Resolved divergence: rebased {} local commit(s) onto remote tip",
        commits_rebased
      )
    },
    was_diverged: true,
    commits_rebased,
    has_conflicts,
  })
}

/// Point `branch_name` at the workspace tip (`@-` when `@` is the empty WC commit).
fn advance_bookmark_to_working_copy_tip(
  workspace_path: &str,
  branch_name: &str,
) -> Result<(), String> {
  let tip = jj::jj_resolve_bookmark_tip(workspace_path)
    .map_err(|e| format!("Failed to resolve rebased tip: {}", e))?;
  jj::jj_set_bookmark(workspace_path, branch_name, &tip).map_err(|e| {
    format!(
      "Failed to advance bookmark '{}' to rebased tip: {}",
      branch_name, e
    )
  })
}

fn workspace_pull_has_conflicts(workspace_path: &str, _branch_name: &str) -> bool {
  if jj::workspace_has_unresolved_conflicts(workspace_path).unwrap_or(false) {
    return true;
  }
  !jj::get_conflicted_files(workspace_path, None)
    .unwrap_or_default()
    .is_empty()
}

/// Copies files/directories listed in `patterns` from `repo_path` into `workspace_dir`.
/// Each pattern is an exact file or directory name relative to the repo root.
/// Missing patterns are silently skipped.
pub fn copy_included_files(
  repo_path: &str,
  workspace_dir: &str,
  patterns: &[String],
) -> Result<(), String> {
  let repo = Path::new(repo_path);
  let workspace = Path::new(workspace_dir);

  for pattern in patterns {
    let source = repo.join(pattern);
    if !source.exists() {
      continue;
    }
    let dest = workspace.join(pattern);
    if source.is_file() {
      if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
          .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
      }
      std::fs::copy(&source, &dest)
        .map_err(|e| format!("Failed to copy file {:?}: {}", source, e))?;
    } else if source.is_dir() {
      copy_dir_recursive(&source, &dest)?;
    }
  }
  Ok(())
}

/// Creates symlinks in `workspace_dir` pointing at paths in `repo_path`.
///
/// Each pattern is an exact file or directory path relative to the repo root
/// (e.g. `node_modules`, `vendor/cache`). Missing patterns are silently skipped.
/// Symlink targets are absolute so the link remains valid from the workspace tree.
pub fn symlink_included_dirs(
  repo_path: &str,
  workspace_dir: &str,
  patterns: &[String],
) -> Result<(), String> {
  let repo = Path::new(repo_path);
  let workspace = Path::new(workspace_dir);

  for pattern in patterns {
    let pattern = pattern.trim().trim_end_matches('/');
    if pattern.is_empty() {
      continue;
    }
    let source = repo.join(pattern);
    if !source.exists() {
      continue;
    }
    let dest = workspace.join(pattern);
    if dest.exists() || dest.symlink_metadata().is_ok() {
      return Err(format!(
        "Cannot symlink '{}': destination already exists at {:?}",
        pattern, dest
      ));
    }
    if let Some(parent) = dest.parent() {
      std::fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
    }
    let abs_source = source
      .canonicalize()
      .map_err(|e| format!("Failed to resolve absolute path for {:?}: {}", source, e))?;
    create_symlink(&abs_source, &dest)?;
  }
  Ok(())
}

fn create_symlink(source: &Path, dest: &Path) -> Result<(), String> {
  #[cfg(unix)]
  {
    std::os::unix::fs::symlink(source, dest)
      .map_err(|e| format!("Failed to create symlink {:?} -> {:?}: {}", dest, source, e))
  }
  #[cfg(windows)]
  {
    let result = if source.is_dir() {
      std::os::windows::fs::symlink_dir(source, dest)
    } else {
      std::os::windows::fs::symlink_file(source, dest)
    };
    result.map_err(|e| format!("Failed to create symlink {:?} -> {:?}: {}", dest, source, e))
  }
}

/// Gets the combined diff of all workspace commits relative to the target branch.
///
/// This shows only the changes introduced by the workspace's own commits,
/// excluding changes already present on the target branch.
///
/// # Arguments
/// * `repo_path`              - Path to the repository root
/// * `workspace_id`           - ID of the workspace
/// * `db`                     - App database for reading settings
///
/// # Returns
/// The parsed revision diff on success, or an error string.
pub fn workspace_diff(repo_path: &str, workspace_id: i64) -> Result<jj::JjRevisionDiff, String> {
  let conflict_marker_style = resolve_workspace_diff_conflict_marker_style(repo_path)?;

  workspace_diff_with_conflict_style(repo_path, workspace_id, &conflict_marker_style)
}

/// Workspace diff for CLI callers that have no app settings database yet.
pub fn workspace_cli_diff(
  repo_path: &str,
  workspace_id: i64,
) -> Result<jj::JjRevisionDiff, String> {
  workspace_diff_with_conflict_style(
    repo_path,
    workspace_id,
    super::DEFAULT_CONFLICT_MARKER_STYLE,
  )
}

fn resolve_workspace_diff_conflict_marker_style(repo_path: &str) -> Result<String, String> {
  let app_db_path = crate::core::resolve_app_db_path(repo_path);
  if !app_db_path.exists() {
    return Err(format!(
      "Failed to access app database at {}: file does not exist",
      app_db_path.display()
    ));
  }
  let db = crate::db::Database::new(app_db_path.clone()).map_err(|e| {
    format!(
      "Failed to access app database at {}: {}",
      app_db_path.display(),
      e
    )
  })?;
  match db.get_setting("conflict_marker_style") {
    Ok(Some(value)) => {
      let trimmed = value.trim();
      if trimmed.is_empty() {
        Ok(crate::core::DEFAULT_CONFLICT_MARKER_STYLE.to_string())
      } else {
        Ok(trimmed.to_string())
      }
    }
    Ok(None) => Ok(crate::core::DEFAULT_CONFLICT_MARKER_STYLE.to_string()),
    // Settings table not yet initialized → default; other db errors propagate.
    Err(e) if e.to_string().contains("no such table") => {
      Ok(crate::core::DEFAULT_CONFLICT_MARKER_STYLE.to_string())
    }
    Err(e) => Err(format!(
      "Failed to read app database setting conflict_marker_style: {}",
      e
    )),
  }
}

fn workspace_diff_with_conflict_style(
  repo_path: &str,
  workspace_id: i64,
  conflict_marker_style: &str,
) -> Result<jj::JjRevisionDiff, String> {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get workspace: {}", e))?
    .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;

  let target_branch = workspace.target_branch.as_deref().unwrap_or("main");
  let workspace_dir = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let workspace_dir_str = workspace_dir
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;
  if let Some(empty) = jj::revision_diff_if_workspace_missing(&workspace_dir) {
    return Ok(empty);
  }
  let repo = jj::WorkspaceRepo::load(workspace_dir_str)
    .map_err(|e| format!("Failed to load workspace repo: {}", e))?;
  let base_revision = resolve_workspace_diff_base_revision(repo_path, &workspace, &repo)?;
  let tip_revision = resolve_workspace_diff_tip_revision(&workspace, &repo)?;
  // Diff from the fork point, not the base tip: when the target moves ahead
  // (e.g. a remote merge is fetched) before the workspace is rebased, a
  // two-dot diff shows the target's new commits as reverted Committed files.
  let base_revision = if base_revision == "@" {
    base_revision
  } else {
    jj::fork_point_revset(&base_revision, &tip_revision)
  };

  let mut diff = repo
    .merge_diff_between_revisions(&base_revision, &tip_revision, conflict_marker_style)
    .map_err(|e| format!("Failed to get workspace diff: {}", e))?;

  let uncommitted_files = jj::jj_get_changed_files(workspace_dir_str)
    .map_err(|e| format!("Failed to get uncommitted workspace changes: {}", e))?;
  let uncommitted_paths: std::collections::HashSet<String> = uncommitted_files
    .iter()
    .map(|file| file.path.clone())
    .collect();
  let conflicted_files =
    jj::get_conflicted_files_after_snapshot(workspace_dir_str, Some(target_branch))
      .map_err(|e| format!("Failed to get conflicted workspace files: {}", e))?;
  let conflicted_paths: std::collections::HashSet<String> =
    conflicted_files.iter().cloned().collect();
  // Drop clean overlap with the working copy so WC-only edits (bookmark at @)
  // are not duplicated under Committed + Changes. Conflicted paths stay in
  // committed_files — rebase conflicts live in committed hunks. Review's Show
  // toggle keeps those dirty/conflicted committed rows visible when off.
  diff.committed_files.retain(|file| {
    !uncommitted_paths.contains(&file.path) || conflicted_paths.contains(&file.path)
  });
  diff.uncommitted_files = uncommitted_files;
  diff.conflicted_files = conflicted_files;

  Ok(diff)
}

fn resolve_workspace_diff_base_revision(
  repo_path: &str,
  workspace: &local_db::Workspace,
  repo: &jj::WorkspaceRepo,
) -> Result<String, String> {
  let target_branch = workspace.target_branch.as_deref().unwrap_or("main");
  let last_rebased_target_commit =
    local_db::get_workspace_last_rebased_commit(repo_path, workspace.id)?;
  let base_revision = resolve_workspace_diff_base_revision_from_last_rebased(
    last_rebased_target_commit,
    target_branch,
  );
  if base_revision != target_branch {
    let workspace_revision = resolve_workspace_diff_workspace_revision(
      &workspace.branch_name,
      repo.is_bookmark_conflicted(&workspace.branch_name),
    );
    let current_tip = repo
      .commit_id(&workspace_revision)
      .map_err(|e| format!("Failed to resolve workspace tip: {}", e))?;
    if current_tip != base_revision {
      return Ok(base_revision);
    }
  }
  if workspace.branch_name == target_branch {
    let committed_ahead = repo
      .commits_ahead(target_branch)
      .map_err(|e| format!("Failed to get workspace commits ahead: {}", e))?;
    if committed_ahead.total_count == 0 {
      return Ok("@".to_string());
    }
  }

  Ok(target_branch.to_string())
}

fn resolve_workspace_diff_tip_revision(
  workspace: &local_db::Workspace,
  repo: &jj::WorkspaceRepo,
) -> Result<String, String> {
  let target_branch = workspace.target_branch.as_deref().unwrap_or("main");
  let committed_ahead = repo
    .commits_ahead(&workspace.branch_name)
    .map_err(|e| format!("Failed to get workspace commits ahead: {}", e))?;
  let tip_revision = resolve_workspace_diff_tip_revision_from_workspace_state(
    &workspace.branch_name,
    target_branch,
    committed_ahead.total_count,
  );
  if tip_revision == workspace.branch_name {
    Ok(resolve_workspace_diff_workspace_revision(
      &workspace.branch_name,
      repo.is_bookmark_conflicted(&workspace.branch_name),
    ))
  } else {
    Ok(tip_revision)
  }
}

/// A conflicted bookmark names multiple revisions, but `@` always identifies
/// the concrete working-copy side whose changes the workspace view should show.
fn resolve_workspace_diff_workspace_revision(branch_name: &str, is_conflicted: bool) -> String {
  if is_conflicted {
    "@".to_string()
  } else {
    branch_name.to_string()
  }
}

fn resolve_workspace_diff_tip_revision_from_workspace_state(
  branch_name: &str,
  target_branch: &str,
  committed_ahead_count: usize,
) -> String {
  if branch_name == target_branch && committed_ahead_count == 0 {
    "@-".to_string()
  } else {
    branch_name.to_string()
  }
}

fn resolve_workspace_diff_base_revision_from_last_rebased(
  last_rebased_target_commit: Option<String>,
  target_branch: &str,
) -> String {
  last_rebased_target_commit
    .map(|commit| commit.trim().to_string())
    .filter(|commit| !commit.is_empty())
    .unwrap_or_else(|| target_branch.to_string())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
  std::fs::create_dir_all(dst)
    .map_err(|e| format!("Failed to create directory {:?}: {}", dst, e))?;
  for entry in
    std::fs::read_dir(src).map_err(|e| format!("Failed to read directory {:?}: {}", src, e))?
  {
    let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
    let src_path = entry.path();
    let dst_path = dst.join(entry.file_name());
    if src_path.is_dir() {
      copy_dir_recursive(&src_path, &dst_path)?;
    } else {
      std::fs::copy(&src_path, &dst_path)
        .map_err(|e| format!("Failed to copy file {:?}: {}", src_path, e))?;
    }
  }
  Ok(())
}

// Rebase

/// Serializable result returned to callers (including the Tauri frontend).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleRebaseResult {
  pub rebased: bool,
  pub success: bool,
  pub message: String,
  pub bookmark_conflicts: Vec<WorkspaceBookmarkConflict>,
}

/// Check and optionally rebase workspaces against their target branch.
///
/// - `workspace_id = Some(id)` — rebase only that workspace (uses `default_branch` as fallback
///   when the workspace has no `target_branch` set, and respects `force`).
/// - `workspace_id = None` — rebase all workspaces in the repo that have a `target_branch`.
pub fn check_and_rebase_workspaces(
  repo_path: &str,
  workspace_id: Option<i64>,
  default_branch: Option<String>,
  force: Option<bool>,
  conflict_style: &str,
) -> Result<SingleRebaseResult, String> {
  // Dashboard/ShowWorkspace refreshes and post-commit auto-rebase can overlap. jj transactions
  // share one operation store across all workspaces, so serialize these history rewrites with
  // commits to prevent stale operations, competing checkouts, and conflicted bookmarks.
  let repo_commit_lock = commit_lock_for_repo(repo_path);
  let _repo_commit_guard = repo_commit_lock.lock_or_recover();

  if let Some(id) = workspace_id {
    let default_branch = default_branch.unwrap_or_else(|| "main".to_string());
    let force = force.unwrap_or(false);
    let result = if force {
      auto_rebase::rebase_root_subtree_from_workspace_force(
        repo_path,
        id,
        &default_branch,
        conflict_style,
      )?
    } else {
      auto_rebase::rebase_single_workspace(repo_path, id, &default_branch, force, conflict_style)?
    };

    match result {
      Some(auto_result) => {
        if auto_result.rebase_result.success {
          notify_auto_review_rebase(repo_path, id);
        }
        Ok(SingleRebaseResult {
          rebased: true,
          success: auto_result.rebase_result.success,
          message: auto_result.rebase_result.message,
          bookmark_conflicts: auto_result.bookmark_conflicts,
        })
      }
      None => Ok(SingleRebaseResult {
        rebased: false,
        success: true,
        message: "No rebase needed".to_string(),
        bookmark_conflicts: Vec::new(),
      }),
    }
  } else {
    let results = auto_rebase::check_and_rebase_all(repo_path, conflict_style)?;

    // Only the workspaces that actually moved get a review, never the whole repo.
    for result in results.iter().filter(|r| r.rebase_result.success) {
      for branch in &result.workspaces_rebased {
        if let Ok(Some(workspace)) = local_db::get_workspace_by_branch(repo_path, branch) {
          notify_auto_review_rebase(repo_path, workspace.id);
        }
      }
    }

    let rebased_count: usize = results.iter().map(|r| r.workspaces_rebased.len()).sum();
    let all_success = results.iter().all(|r| r.rebase_result.success);
    let bookmark_conflicts: Vec<WorkspaceBookmarkConflict> = results
      .iter()
      .flat_map(|r| r.bookmark_conflicts.clone())
      .collect();

    let mut summary = String::new();
    for result in &results {
      summary.push_str(&format!(
        "Target '{}': rebased {} workspace(s) - {}\n",
        result.target_branch,
        result.workspaces_rebased.len(),
        if result.rebase_result.success {
          "success"
        } else {
          "failed"
        }
      ));
    }
    if results.is_empty() {
      summary.push_str("No workspaces with target branches to rebase\n");
    }

    Ok(SingleRebaseResult {
      rebased: rebased_count > 0,
      success: all_success,
      message: summary,
      bookmark_conflicts,
    })
  }
}

/// Signals a finished rebase of one workspace to the automatic review
/// triggers. Best-effort: a workspace row that has gone away is skipped.
fn notify_auto_review_rebase(repo_path: &str, workspace_id: i64) {
  let Ok(Some(workspace)) = local_db::get_workspace_by_id(repo_path, workspace_id) else {
    return;
  };
  let Ok(workspace_root) = resolve_workspace_root(repo_path, Some(workspace_id)) else {
    return;
  };
  auto_review::notify(
    repo_path,
    workspace_id,
    &workspace.branch_name,
    &workspace_root,
    ReviewTrigger::Rebase,
  );
}

/// Reads the repo-level "auto_push" setting from the app database.
/// Defaults to `false` when unset, unparseable, or the database is unavailable.
fn resolve_auto_push(repo_path: &str) -> bool {
  let app_db_path = crate::core::resolve_app_db_path(repo_path);
  if !app_db_path.exists() {
    return false;
  }
  let Ok(db) = crate::db::Database::new(app_db_path) else {
    return false;
  };
  db.get_repo_setting(repo_path, "auto_push")
    .ok()
    .flatten()
    .map(|value| value == "true")
    .unwrap_or(false)
}

/// Commits the workspace (or home repo) and, when the repository's "auto_push"
/// setting is enabled, pushes the result to the remote.
///
/// # Arguments
/// * `repo_path`     - Path to the repository root
/// * `workspace_id`  - ID of the workspace to commit, or `None` for the home repo
/// * `message`       - Commit message
///
/// # Returns
/// The commit result message on success, or an error string. A failed auto-push
/// is returned as an error even though the commit itself already succeeded.
pub fn commit_workspace_with_auto_push<T>(
  repo_path: &str,
  workspace_id: T,
  message: &str,
) -> Result<String, String>
where
  T: Into<Option<i64>>,
{
  commit_with_auto_push(repo_path, workspace_id.into(), message, false)
}

/// `commit_workspace_changes` followed by the auto-push.
pub fn commit_workspace_changes_with_auto_push(
  repo_path: &str,
  workspace_id: Option<i64>,
  message: &str,
) -> Result<String, String> {
  commit_with_auto_push(repo_path, workspace_id, message, true)
}

fn commit_with_auto_push(
  repo_path: &str,
  workspace_id: Option<i64>,
  message: &str,
  require_changes: bool,
) -> Result<String, String> {
  let result = commit_workspace_locked(repo_path, workspace_id, message, require_changes)?;

  if resolve_auto_push(repo_path) {
    push_workspace_to_remote(repo_path, workspace_id)?;
  }

  Ok(result)
}

/// Error from a `require_changes` commit whose working copy has no changes.
pub const NOTHING_TO_COMMIT: &str = "nothing to commit in the working copy";

pub fn commit_workspace<T>(
  repo_path: &str,
  workspace_id: T,
  message: &str,
) -> Result<String, String>
where
  T: Into<Option<i64>>,
{
  commit_workspace_locked(repo_path, workspace_id.into(), message, false)
}

/// Like `commit_workspace`, but fails with `NOTHING_TO_COMMIT` when the working
/// copy is clean.
pub fn commit_workspace_changes(
  repo_path: &str,
  workspace_id: Option<i64>,
  message: &str,
) -> Result<String, String> {
  commit_workspace_locked(repo_path, workspace_id, message, true)
}

fn commit_workspace_locked(
  repo_path: &str,
  workspace_id: Option<i64>,
  message: &str,
  require_changes: bool,
) -> Result<String, String> {
  let repo_commit_lock = commit_lock_for_repo(repo_path);
  let _repo_commit_guard = repo_commit_lock.lock_or_recover();
  let workspace_root = resolve_workspace_root(repo_path, workspace_id)?;
  // The mutex above only covers this process; CLI calls run in separate processes.
  let _workspace_commit_lock = jj_lib::lock::FileLock::lock(
    Path::new(&workspace_root)
      .join(".jj")
      .join("treq-commit.lock"),
  )
  .map_err(|e| format!("Failed to lock workspace for commit: {}", e))?;
  let (committed_branch, target_branch) = if let Some(id) = workspace_id {
    let workspace = local_db::get_workspace_by_id(repo_path, id)
      .map_err(|e| format!("Failed to get workspace: {}", e))?
      .ok_or_else(|| format!("Workspace not found: {}", id))?;
    let target_branch = workspace
      .target_branch
      .clone()
      .unwrap_or_else(|| "main".to_string());
    (workspace.branch_name, target_branch)
  } else {
    let branch = jj::resolve_home_repo_branch(repo_path)
      .map_err(|e| format!("Failed to resolve home repo branch: {}", e))?;
    let target_branch = jj::get_default_branch(repo_path).unwrap_or_else(|_| "main".to_string());
    (branch, target_branch)
  };

  // Checked under the commit lock: a check made before it races with a
  // concurrent commit that empties the working copy first.
  if require_changes
    && jj::jj_get_changed_files(&workspace_root)
      .map_err(|e| format!("Failed to list changed files: {}", e))?
      .is_empty()
  {
    return Err(NOTHING_TO_COMMIT.to_string());
  }
  let result = jj::jj_commit(&workspace_root, message)
    .map_err(|e| format!("Failed to create commit: {}", e))?;
  // Check-pass autosaves are checkpoints. Fold them away once the user commits.
  let _ = jj::jj_drop_autosave_ancestors(&workspace_root);
  // Keep branch history free of empty commits; best-effort, never fails the commit.
  let _ = jj::jj_abandon_empty_commits(&workspace_root, &target_branch);
  if !committed_branch.is_empty() {
    if let Some(id) = workspace_id {
      if let Ok(new_tip) = jj::jj_get_commit_id(&workspace_root, &committed_branch) {
        let _ = local_db::update_workspace_last_rebased_commit(repo_path, id, &new_tip);
      }
    }
  }
  if !committed_branch.is_empty() {
    let _ = auto_rebase::rebase_after_commit(repo_path, &committed_branch);
  }
  match workspace_id {
    Some(_) => {
      sync_home_and_workspace_for_branch(repo_path, &committed_branch, SyncSource::WorkspaceToHome)?
    }
    None => {
      sync_home_and_workspace_for_branch(repo_path, &committed_branch, SyncSource::HomeToWorkspace)?
    }
  };

  if let Some(id) = workspace_id {
    auto_review::notify(
      repo_path,
      id,
      &committed_branch,
      &workspace_root,
      ReviewTrigger::Commit,
    );
  }

  Ok(result)
}
