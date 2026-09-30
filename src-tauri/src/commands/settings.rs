use crate::core;
use crate::lock_ext::LockExt;
use crate::AppState;
use std::collections::HashMap;
use std::time::Instant;
use tauri::State;

#[tauri::command]
pub async fn init_repo(repo_path: String) -> Result<bool, String> {
  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    core::init(&repo_path_for_task).map_err(|e| e.to_string())
  })
  .await
  .map_err(|e| format!("Failed to join init_repo task: {}", e))?;
  log::debug!(
    "init_repo(repo_path={}) completed in {:?}",
    repo_path,
    started_at.elapsed()
  );
  result
}

#[tauri::command]
pub fn get_setting(state: State<AppState>, key: String) -> Result<Option<String>, String> {
  let db = state.db.lock_or_recover();
  db.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings_batch(
  state: State<AppState>,
  keys: Vec<String>,
) -> Result<HashMap<String, Option<String>>, String> {
  let db = state.db.lock_or_recover();
  db.get_settings_batch(&keys).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(state: State<AppState>, key: String, value: String) -> Result<(), String> {
  let db = state.db.lock_or_recover();
  db.set_setting(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_repo_setting(
  state: State<AppState>,
  repo_path: String,
  key: String,
) -> Result<Option<String>, String> {
  let db = state.db.lock_or_recover();
  db.get_repo_setting(&repo_path, &key)
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_repo_setting(
  state: State<AppState>,
  repo_path: String,
  key: String,
  value: String,
) -> Result<(), String> {
  let db = state.db.lock_or_recover();
  db.set_repo_setting(&repo_path, &key, &value)
    .map_err(|e| e.to_string())?;
  if key == "ignore_generated_treq_paths" && value == "true" {
    crate::jj::ensure_optional_gitignore_entries(&repo_path).map_err(|e| e.to_string())?;
  }
  Ok(())
}

fn window_map_label(window_label: Option<String>) -> String {
  let label = window_label.unwrap_or_default();
  if label.is_empty() {
    "main".to_string()
  } else {
    label
  }
}

#[tauri::command]
pub fn set_window_repo_path(
  state: State<AppState>,
  repo_path: String,
  window_label: Option<String>,
) -> Result<(), String> {
  let mut map = state.window_repo_paths.lock_or_recover();
  map.insert(window_map_label(window_label), repo_path);
  Ok(())
}

#[tauri::command]
pub fn get_app_setup_script_status(
  state: State<AppState>,
) -> Result<core::app_setup_script::AppSetupScriptStatus, String> {
  let db = state.db.lock_or_recover();
  core::app_setup_script::get_status(&db)
}

#[tauri::command]
pub fn save_app_setup_script(
  state: State<AppState>,
  script: String,
  always_run: bool,
) -> Result<(), String> {
  {
    let db = state.db.lock_or_recover();
    core::app_setup_script::save(&db, &script, always_run)?;
  }
  core::app_setup_script::spawn_run_if_needed(core::app_setup_script::Trigger::SettingsChanged);
  Ok(())
}
