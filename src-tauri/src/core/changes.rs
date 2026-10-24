use crate::{jj, local_db};
use serde::Serialize;
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileHunksBatchFile {
  pub path: String,
  pub hunks: Vec<jj::JjDiffHunk>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileHunksBatch {
  /// Names the working-copy diff the files were read from, and the marker
  /// style; empty when unknown. Equal tokens mean equal hunks.
  pub snapshot_token: String,
  pub files: Vec<WorkspaceFileHunksBatchFile>,
}

/// [`list_file_hunks`] for several files, read from one snapshot.
pub fn list_file_hunks_batch(
  repo_path: &str,
  workspace_id: Option<i64>,
  file_paths: Vec<String>,
  conflict_marker_style: &str,
) -> Result<WorkspaceFileHunksBatch, String> {
  if repo_path.trim().is_empty() {
    return Err("Repository path cannot be empty".to_string());
  }
  let path = resolve_workspace_dir(repo_path, workspace_id)?;
  let paths = deduplicate_file_paths(file_paths);
  let error = |e: jj::JjError| format!("Failed to get file hunks: {}", e);
  let (snapshot_token, hunks): (String, Vec<Result<_, String>>) =
    match jj::jj_get_file_hunks_batch(&path, &paths, conflict_marker_style) {
      Ok((token, hunks)) => (
        format!("{token}:{conflict_marker_style}"),
        hunks.into_iter().map(|h| h.map_err(error)).collect(),
      ),
      Err(e) => {
        let message = error(e);
        (
          String::new(),
          paths.iter().map(|_| Err(message.clone())).collect(),
        )
      }
    };
  let files = paths
    .into_iter()
    .zip(hunks)
    .map(|(path, hunks)| match hunks {
      Ok(hunks) => WorkspaceFileHunksBatchFile {
        path,
        hunks,
        error: None,
      },
      Err(error) => WorkspaceFileHunksBatchFile {
        path,
        hunks: Vec::new(),
        error: Some(error),
      },
    })
    .collect();
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
      let _ = crate::auto_rebase::rebase_after_commit(&repo_path, &branch);
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
}
