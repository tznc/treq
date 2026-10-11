use crate::local_db;
use ignore::WalkBuilder;

pub type DirectoryEntry = crate::core::WorkspaceEntry;

#[derive(serde::Serialize)]
pub struct CachedDirectoryEntry {
  pub name: String,
  pub path: String,
  pub is_directory: bool,
  pub relative_path: String,
  pub modified_at: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryBatchResult {
  pub path: String,
  pub entries: Vec<DirectoryEntry>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<String>,
}

fn modified_at_rfc3339(path: &str) -> Option<String> {
  std::fs::metadata(path)
    .ok()
    .and_then(|metadata| metadata.modified().ok())
    .map(|modified| chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339())
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
pub fn cleanup_agent_cli_files(paths: Vec<String>) -> Result<(), String> {
  crate::core::cleanup_agent_cli_files(&paths)
}

#[tauri::command]
pub fn get_file_modified_at(path: String) -> Result<Option<String>, String> {
  if !std::path::Path::new(&path).exists() {
    return Ok(None);
  }

  Ok(modified_at_rfc3339(&path))
}

#[tauri::command]
pub fn list_directory(path: String) -> Result<Vec<DirectoryEntry>, String> {
  use std::path::Path;

  let base_path = Path::new(&path);
  if !base_path.is_dir() {
    return Err(format!("Directory does not exist: {}", path));
  }
  let mut files = Vec::new();

  // Use ignore::WalkBuilder to respect .gitignore patterns
  let walker = WalkBuilder::new(&path)
    .max_depth(Some(1)) // Only immediate children
    .hidden(false) // Show hidden files (except those in .gitignore)
    .git_ignore(true) // Respect .gitignore patterns
    .git_global(true) // Respect global gitignore
    .git_exclude(true) // Respect .git/info/exclude
    .parents(true) // Check parent directories for ignore files
    .build();

  for entry in walker.flatten() {
    let entry_path = entry.path();

    // Skip the base directory itself
    if entry_path == base_path {
      continue;
    }

    if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
      let is_dir = entry_path.is_dir();
      files.push(DirectoryEntry {
        name: name.to_string(),
        path: entry_path.to_string_lossy().to_string(),
        is_directory: is_dir,
        modified_at: modified_at_rfc3339(&entry_path.to_string_lossy()),
        submodule_pin: None,
        submodule_synced: None,
        status: None,
      });
    }
  }

  // Sort: directories first, then files
  files.sort_by(|a, b| match (a.is_directory, b.is_directory) {
    (true, false) => std::cmp::Ordering::Less,
    (false, true) => std::cmp::Ordering::Greater,
    _ => a.name.cmp(&b.name),
  });

  Ok(files)
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

#[tauri::command]
pub fn list_directory_cached(
  repo_path: String,
  workspace_id: Option<i64>,
  parent_path: String,
) -> Result<Vec<CachedDirectoryEntry>, String> {
  use std::path::Path;

  // Try cache first
  if let Ok(cached) = local_db::get_cached_directory_listing(&repo_path, workspace_id, &parent_path)
  {
    if !cached.is_empty() {
      // Convert to CachedDirectoryEntry format
      let entries: Vec<CachedDirectoryEntry> = cached
        .into_iter()
        .map(|file| {
          let name = Path::new(&file.file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&file.relative_path)
            .to_string();
          let modified_at = modified_at_rfc3339(&file.file_path);
          CachedDirectoryEntry {
            name,
            path: file.file_path,
            is_directory: file.is_directory,
            relative_path: file.relative_path,
            modified_at,
          }
        })
        .collect();
      return Ok(entries);
    }
  }

  // Cache miss: fall back to live filesystem
  let live_entries = list_directory(parent_path.clone())?;

  // Convert live entries to cached format
  let entries: Vec<CachedDirectoryEntry> = live_entries
    .into_iter()
    .map(|entry| {
      // Compute relative path
      let base = Path::new(&parent_path);
      let full_path = Path::new(&entry.path);
      let relative = full_path
        .strip_prefix(base)
        .ok()
        .and_then(|p| p.to_str())
        .unwrap_or(&entry.name)
        .to_string();

      CachedDirectoryEntry {
        name: entry.name,
        path: entry.path,
        is_directory: entry.is_directory,
        relative_path: relative,
        modified_at: entry.modified_at,
      }
    })
    .collect();

  Ok(entries)
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
