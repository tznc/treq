use crate::result_ext::ResultExt;
use crate::{jj, local_db};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;

pub(crate) fn resolve_workspace_dir(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<String, String> {
  match workspace_id {
    Some(id) => {
      let workspace = local_db::get_workspace_by_id(repo_path, id)
        .map_err(|e| format!("Failed to get workspace: {}", e))?
        .ok_or_else(|| format!("Workspace not found: {}", id))?;
      Ok(
        Path::new(repo_path)
          .join(".treq")
          .join("workspaces")
          .join(&workspace.workspace_path)
          .to_str()
          .ok_or("Invalid workspace path")?
          .to_string(),
      )
    }
    None => Ok(repo_path.to_string()),
  }
}

/// List changed files for a workspace (or the home repo when workspace_id is None).
pub fn list_changed_files(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<Vec<jj::JjFileChange>, String> {
  let path = resolve_workspace_dir(repo_path, workspace_id)?;
  jj::jj_get_changed_files(&path).map_err(|e| format!("Failed to list changed files: {}", e))
}

/// Get diff hunks for a single file in a workspace.
pub fn list_file_hunks(
  repo_path: &str,
  workspace_id: Option<i64>,
  file_path: &str,
  conflict_marker_style: &str,
) -> Result<Vec<jj::JjDiffHunk>, String> {
  let path = resolve_workspace_dir(repo_path, workspace_id)?;
  jj::jj_get_file_hunks(&path, file_path, conflict_marker_style)
    .map_err(|e| format!("Failed to get file hunks: {}", e))
}

pub(crate) fn deduplicate_file_paths(file_paths: Vec<String>) -> Vec<String> {
  let mut seen = HashSet::new();
  file_paths
    .into_iter()
    .filter(|path| seen.insert(path.clone()))
    .collect()
}

pub(crate) fn content_hash<T: Serialize>(value: &T) -> String {
  Sha256::digest(serde_json::to_vec(value).unwrap_or_default())
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileHunksBatchFile {
  pub path: String,
  pub content_hash: String,
  pub hunks: Vec<jj::JjDiffHunk>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileHunksBatch {
  pub snapshot_token: String,
  pub files: Vec<WorkspaceFileHunksBatchFile>,
}

pub fn list_file_hunks_batch(
  repo_path: &str,
  workspace_id: Option<i64>,
  file_paths: Vec<String>,
  conflict_marker_style: &str,
) -> Result<WorkspaceFileHunksBatch, String> {
  if repo_path.trim().is_empty() {
    return Err("Repository path cannot be empty".to_string());
  }
  resolve_workspace_dir(repo_path, workspace_id)?;
  let files = deduplicate_file_paths(file_paths)
    .into_iter()
    .map(
      |path| match list_file_hunks(repo_path, workspace_id, &path, conflict_marker_style) {
        Ok(hunks) => WorkspaceFileHunksBatchFile {
          content_hash: content_hash(&hunks),
          path,
          hunks,
          error: None,
        },
        Err(error) => WorkspaceFileHunksBatchFile {
          content_hash: content_hash(&error),
          path,
          hunks: Vec::new(),
          error: Some(error),
        },
      },
    )
    .collect();
  let snapshot_token = content_hash(&list_changed_files(repo_path, workspace_id)?);
  Ok(WorkspaceFileHunksBatch {
    snapshot_token,
    files,
  })
}

/// Get lines from a file in a workspace.
pub fn get_file_lines(
  repo_path: &str,
  workspace_id: Option<i64>,
  file_path: &str,
  from_parent: bool,
  start_line: usize,
  end_line: usize,
) -> Result<jj::JjFileLines, String> {
  let path = resolve_workspace_dir(repo_path, workspace_id)?;
  jj::jj_get_file_lines(&path, file_path, from_parent, start_line, end_line)
    .map_err(|e| format!("Failed to get file lines: {}", e))
}

/// Discard all uncommitted working-copy changes in a workspace directory.
pub fn discard_all_changes(workspace_path: &str) -> Result<String, String> {
  jj::jj_restore_all(workspace_path).map_err(|e| format!("Failed to discard changes: {}", e))
}

/// Discard uncommitted changes for a single file in a workspace directory.
pub fn discard_file_changes(workspace_path: &str, file_path: &str) -> Result<String, String> {
  jj::jj_restore_file(workspace_path, file_path)
    .map_err(|e| format!("Failed to discard file changes: {}", e))
}

/// Commits only `file_paths` of the working copy (`jj split`), then rebases
/// the branch's stack onto it so jj is settled when this returns.
pub fn split_working_copy(
  workspace_path: &str,
  message: &str,
  file_paths: Vec<String>,
) -> Result<String, String> {
  let result = jj::jj_split(workspace_path, message, file_paths).map_err(|e| e.to_string())?;
  if let Some(repo_path) = jj::derive_repo_path_from_workspace(workspace_path) {
    if let Ok(branch) = jj::get_workspace_branch(workspace_path) {
      log::debug!("jj_split auto_rebase: repo_path={repo_path}, branch={branch}");
      crate::auto_rebase::rebase_after_commit(&repo_path, &branch)
        .warn_on_err("rebase after commit");
    }
  }
  Ok(result)
}

#[cfg(test)]
mod batch_tests {
  use super::*;

  #[test]
  fn batch_paths_preserve_order_and_deduplicate() {
    assert_eq!(
      deduplicate_file_paths(vec!["b.rs".into(), "a.rs".into(), "b.rs".into()]),
      vec!["b.rs", "a.rs"]
    );
  }

  #[test]
  fn content_hash_is_deterministic_and_changes_with_hunks() {
    let empty: Vec<jj::JjDiffHunk> = Vec::new();
    assert_eq!(content_hash(&empty), content_hash(&empty));
    assert_ne!(content_hash(&empty), content_hash(&vec![test_hunk()]));
  }

  fn test_hunk() -> jj::JjDiffHunk {
    serde_json::from_value(serde_json::json!({
      "id": "h", "header": "@@", "lines": ["+x"], "patch": "+x"
    }))
    .unwrap()
  }
}
