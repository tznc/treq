use crate::local_db;

pub type DirectoryEntry = crate::core::WorkspaceEntry;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryBatchResult {
  pub path: String,
  pub entries: Vec<DirectoryEntry>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<String>,
}

#[cfg(test)]
mod batch_tests {
  use super::*;
  use tempfile::TempDir;

  #[test]
  fn lists_directories_in_requested_order_and_deduplicates() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();

    let result = list_directories_batch(vec![
      first.to_string_lossy().to_string(),
      second.to_string_lossy().to_string(),
      first.to_string_lossy().to_string(),
    ]);

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].path, first.to_string_lossy());
    assert_eq!(result[1].path, second.to_string_lossy());
  }

  #[test]
  fn returns_directory_errors_without_failing_siblings() {
    let temp = TempDir::new().unwrap();
    let valid = temp.path().join("valid");
    std::fs::create_dir_all(&valid).unwrap();
    std::fs::write(valid.join("visible.txt"), "ok").unwrap();
    let missing = temp.path().join("missing");

    let result = list_directories_batch(vec![
      missing.to_string_lossy().to_string(),
      valid.to_string_lossy().to_string(),
    ]);

    assert!(result[0].error.is_some());
    assert_eq!(result[1].entries[0].name, "visible.txt");
  }
}

#[tauri::command]
pub fn read_file(path: String) -> Result<String, String> {
  std::fs::read_to_string(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_send_review_image(
  repo_path: String,
  suggested_name: String,
  contents_base64: String,
) -> Result<String, String> {
  crate::send_dispatch::write_send_review_image(&repo_path, &suggested_name, &contents_base64)
    .map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn write_agent_cli_files(
  prompt: String,
  settings_json: Option<String>,
  cwd: Option<String>,
) -> Result<crate::core::AgentCliFiles, String> {
  crate::core::write_agent_cli_files(&prompt, settings_json.as_deref(), cwd.as_deref())
}

#[tauri::command]
pub fn get_file_modified_at(path: String) -> Result<Option<String>, String> {
  Ok(crate::core::file_modified_at(&path))
}

#[tauri::command]
pub fn list_directory(path: String) -> Result<Vec<DirectoryEntry>, String> {
  crate::core::list_directory(&path)
}

#[tauri::command]
pub fn list_directories_batch(paths: Vec<String>) -> Vec<DirectoryBatchResult> {
  let mut seen = std::collections::HashSet::new();
  paths
    .into_iter()
    .filter(|path| seen.insert(path.clone()))
    .map(|path| match list_directory(path.clone()) {
      Ok(entries) => DirectoryBatchResult {
        path,
        entries,
        error: None,
      },
      Err(error) => DirectoryBatchResult {
        path,
        entries: Vec::new(),
        error: Some(error),
      },
    })
    .collect()
}

#[tauri::command]
pub fn ls_workspace_with_status(
  repo_path: String,
  workspace_id: Option<i64>,
) -> Result<Vec<DirectoryEntry>, String> {
  crate::core::ls_workspace_with_status(&repo_path, workspace_id)
}

/// Suggests gitignored root paths for workspace create overlays (symlinks/copies).
#[tauri::command]
pub fn list_gitignored_path_suggestions(repo_path: String) -> Result<Vec<String>, String> {
  crate::core::list_gitignored_path_suggestions(&repo_path)
}

#[tauri::command]
pub fn get_workspace_readme(
  repo_path: String,
  workspace_id: Option<i64>,
) -> Result<Option<String>, String> {
  crate::core::get_workspace_readme(&repo_path, workspace_id)
}

#[derive(serde::Serialize)]
pub struct FileSearchResult {
  pub file_path: String,
  pub relative_path: String,
}

#[tauri::command]
pub fn search_workspace_files(
  repo_path: String,
  workspace_id: Option<i64>,
  query: String,
  limit: Option<usize>,
) -> Result<Vec<FileSearchResult>, String> {
  let max_results = limit.unwrap_or(50);

  let files = local_db::search_workspace_files(&repo_path, workspace_id, &query, max_results)?;

  Ok(
    files
      .into_iter()
      .map(|f| FileSearchResult {
        file_path: f.file_path,
        relative_path: f.relative_path,
      })
      .collect(),
  )
}
