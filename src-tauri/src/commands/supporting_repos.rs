use crate::core::supporting_repos::{self, SupportingRepo};
use crate::lock_ext::LockExt;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn list_supporting_repos(repo_path: String) -> Result<Vec<SupportingRepo>, String> {
  supporting_repos::list(&repo_path)
}

#[tauri::command]
pub fn add_supporting_repo(repo_path: String, supporting_path: String) -> Result<String, String> {
  supporting_repos::add(&repo_path, &supporting_path)
}

#[tauri::command]
pub fn remove_supporting_repo(repo_path: String, supporting_path: String) -> Result<(), String> {
  supporting_repos::remove(&repo_path, &supporting_path)
}

/// Registers the supporting repositories shown in a window so Treq CLI
/// requests (`treq agent`, `treq send`) from those repositories reach it.
#[tauri::command]
pub fn set_window_supporting_repo_paths(
  state: State<AppState>,
  repo_paths: Vec<String>,
  window_label: Option<String>,
) -> Result<(), String> {
  let label = window_label
    .filter(|label| !label.is_empty())
    .unwrap_or_else(|| "main".to_string());
  let mut map = state.window_supporting_repo_paths.lock_or_recover();
  if repo_paths.is_empty() {
    map.remove(&label);
  } else {
    map.insert(label, repo_paths);
  }
  Ok(())
}
