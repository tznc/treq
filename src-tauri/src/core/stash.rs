use crate::jj::{self, JjRevisionDiff};
use crate::local_db::{self, StashEntry};
use crate::result_ext::ResultExt;
use std::path::Path;
use uuid::Uuid;

use super::workspaces::{resolve_workspace_root, HOME_MOVE_ENDPOINT};

fn workspace_label_for(repo_path: &str, workspace_id: Option<i64>) -> Result<String, String> {
  match workspace_id {
    Some(id) => {
      let workspace = local_db::get_workspace_by_id(repo_path, id)
        .map_err(|e| format!("Failed to get workspace: {}", e))?
        .ok_or_else(|| format!("Workspace not found: {}", id))?;
      let label = if !workspace.title.trim().is_empty() {
        workspace.title
      } else {
        workspace.branch_name
      };
      Ok(label)
    }
    None => Ok("Home repo".to_string()),
  }
}

fn resolve_apply_target_path(repo_path: &str, target_branch: &str) -> Result<String, String> {
  if target_branch == HOME_MOVE_ENDPOINT {
    return Ok(repo_path.to_string());
  }
  let workspace = local_db::get_workspace_by_branch(repo_path, target_branch)
    .map_err(|e| format!("Failed to look up target workspace: {}", e))?
    .ok_or_else(|| format!("Target workspace '{}' not found", target_branch))?;
  let full_path = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  full_path
    .to_str()
    .map(|s| s.to_string())
    .ok_or_else(|| "Failed to convert target workspace path".to_string())
}

/// Move working-copy changes into an immutable stash entry (local gist storage).
pub fn stash_workspace_changes(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<StashEntry, String> {
  let workspace_path = resolve_workspace_root(repo_path, workspace_id)?;
  let workspace_label = workspace_label_for(repo_path, workspace_id)?;
  let bookmark_name = format!("{}{}", jj::STASH_BOOKMARK_PREFIX, Uuid::new_v4());

  let stash_commit =
    jj::jj_stash_working_copy(&workspace_path, &bookmark_name).map_err(|e| e.to_string())?;

  local_db::add_stash(
    repo_path,
    workspace_id,
    &workspace_label,
    &stash_commit.commit_id,
    &stash_commit.change_id,
    &stash_commit.short_commit_id,
    &stash_commit.bookmark_name,
    stash_commit.additions,
    stash_commit.deletions,
    &stash_commit.files_changed,
  )
}

/// Park an existing commit into an immutable stash entry, removing it from the branch.
pub fn stash_commit(
  repo_path: &str,
  workspace_id: Option<i64>,
  change_id: &str,
) -> Result<StashEntry, String> {
  let workspace_path = resolve_workspace_root(repo_path, workspace_id)?;
  let workspace_label = workspace_label_for(repo_path, workspace_id)?;
  let bookmark_name = format!("{}{}", jj::STASH_BOOKMARK_PREFIX, Uuid::new_v4());
  if let Some(id) = workspace_id {
    let workspace = local_db::get_workspace_by_id(repo_path, id)?
      .ok_or_else(|| format!("Workspace not found: {}", id))?;
    super::commits::ensure_rewritable(repo_path, &workspace, change_id)?;
  }

  let stash_commit =
    jj::jj_stash_commit(&workspace_path, change_id, &bookmark_name).map_err(|e| e.to_string())?;

  local_db::add_stash(
    repo_path,
    workspace_id,
    &workspace_label,
    &stash_commit.commit_id,
    &stash_commit.change_id,
    &stash_commit.short_commit_id,
    &stash_commit.bookmark_name,
    stash_commit.additions,
    stash_commit.deletions,
    &stash_commit.files_changed,
  )
}

/// List all stashes for a repository, most recent first.
pub fn list_stashes(repo_path: &str) -> Result<Vec<StashEntry>, String> {
  local_db::get_stashes(repo_path)
}

/// Delete a stash entry and its reachability bookmark.
pub fn delete_stash(repo_path: &str, stash_id: i64) -> Result<(), String> {
  let entry = local_db::get_stash_by_id(repo_path, stash_id)?
    .ok_or_else(|| format!("Stash not found: {}", stash_id))?;

  // Prefer deleting via the home/repo path; bookmark lives in the shared store.
  jj::jj_delete_stash_bookmark(repo_path, &entry.bookmark_name)
    .warn_on_err("jj delete stash bookmark");
  if let Some(workspace_id) = entry.workspace_id {
    if let Ok(ws_path) = resolve_workspace_root(repo_path, Some(workspace_id)) {
      jj::jj_delete_stash_bookmark(&ws_path, &entry.bookmark_name)
        .warn_on_err("jj delete stash bookmark");
    }
  }

  local_db::delete_stash(repo_path, stash_id)
}

/// Copy a stash onto a target workspace branch (stash remains intact).
pub fn apply_stash(repo_path: &str, stash_id: i64, target_branch: &str) -> Result<(), String> {
  let entry = local_db::get_stash_by_id(repo_path, stash_id)?
    .ok_or_else(|| format!("Stash not found: {}", stash_id))?;
  let target_path = resolve_apply_target_path(repo_path, target_branch)?;
  jj::jj_apply_stash_commit(&target_path, &entry.commit_id).map_err(|e| e.to_string())
}

/// Diff of a stash commit in isolation (original stash context, not vs a target branch).
pub fn get_stash_diff(repo_path: &str, stash_id: i64) -> Result<JjRevisionDiff, String> {
  let entry = local_db::get_stash_by_id(repo_path, stash_id)?
    .ok_or_else(|| format!("Stash not found: {}", stash_id))?;
  let conflict_style = super::DEFAULT_CONFLICT_MARKER_STYLE;
  jj::jj_get_commit_diff(repo_path, &entry.commit_id, conflict_style).map_err(|e| e.to_string())
}

/// Export a stash as a unified git patch string.
pub fn export_stash_git_patch(repo_path: &str, stash_id: i64) -> Result<String, String> {
  let entry = local_db::get_stash_by_id(repo_path, stash_id)?
    .ok_or_else(|| format!("Stash not found: {}", stash_id))?;
  jj::jj_export_commit_git_patch(repo_path, &entry.commit_id).map_err(|e| e.to_string())
}
