use crate::binary_paths;
use crate::db::Database;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

#[derive(Debug, Serialize, Deserialize)]
pub struct EditorAppsResponse {
  pub cursor: bool,
  pub vscode: bool,
  pub zed: bool,
}

/// Load cached binary paths from database on startup
pub fn load_cached_binary_paths(db: &Database) -> HashMap<String, String> {
  let binaries = vec!["git", "jj", "claude", "codex", "cursor-agent", "copilot"];
  let mut paths = HashMap::new();

  for binary in binaries {
    let key = format!("binary_path_{}", binary);
    if let Ok(Some(path)) = db.get_setting(&key) {
      log::info!("Loaded cached {} path: {}", binary, path);
      paths.insert(binary.to_string(), path);
    } else {
      // If not cached, try to detect
      if let Some(detected_path) = binary_paths::detect_binary(binary) {
        log::info!("Detected {} at: {}", binary, detected_path);
        paths.insert(binary.to_string(), detected_path.clone());

        // Cache for next time
        if let Err(e) = db.set_setting(&key, &detected_path) {
          log::warn!("Failed to cache {} path: {}", binary, e);
        }
      }
    }
  }

  paths
}

/// Detect and cache editor applications (Cursor, VSCode, Zed)
#[tauri::command]
pub fn detect_editor_apps(state: State<'_, AppState>) -> Result<EditorAppsResponse, String> {
  let apps = match binary_paths::get_editor_apps_cache() {
    Some(cached) => cached,
    None => {
      let db = state.db.lock().map_err(|e| e.to_string())?;
      crate::core::editor_apps::detect(&db)
    }
  };
  Ok(EditorAppsResponse {
    cursor: *apps.get("cursor").unwrap_or(&false),
    vscode: *apps.get("vscode").unwrap_or(&false),
    zed: *apps.get("zed").unwrap_or(&false),
  })
}

/// Installed state of Git, the agent CLIs and `gh`, for the first-run checklist.
#[tauri::command]
pub async fn check_prerequisites() -> Result<Vec<crate::core::PrerequisiteStatus>, String> {
  tauri::async_runtime::spawn_blocking(crate::core::check_prerequisites)
    .await
    .map_err(|e| format!("Failed to join check_prerequisites task: {e}"))
}

/// Return the directory containing the running treq binary
#[tauri::command]
pub fn get_treq_bin_dir() -> Result<String, String> {
  // Test builds run inside Node, whose bin directory can hold a real agent
  // CLI. Agent commands put this directory first on PATH, so tests point it
  // at their fake agents instead.
  #[cfg(feature = "tauri-test")]
  if let Ok(dir) = std::env::var("TREQ_TEST_BIN_DIR") {
    return Ok(dir);
  }
  binary_paths::get_exe_dir().ok_or_else(|| "Failed to determine treq binary directory".to_string())
}

/// Load cached editor apps from database on startup
pub fn load_cached_editor_apps(db: &Database) -> HashMap<String, bool> {
  let editors = vec![
    ("cursor", "Cursor"),
    ("vscode", "Visual Studio Code"),
    ("zed", "Zed"),
  ];
  let mut apps = HashMap::new();

  for (key, app_name) in editors {
    let db_key = format!("editor_app_{}", key);
    if let Ok(Some(value)) = db.get_setting(&db_key) {
      let is_installed = value == "true";
      log::info!("Loaded cached editor app {}: {}", key, is_installed);
      apps.insert(key.to_string(), is_installed);
    } else {
      // If not cached, detect now
      let is_installed = binary_paths::detect_editor_app(app_name);
      log::info!("Detected editor app {}: {}", app_name, is_installed);
      apps.insert(key.to_string(), is_installed);

      // Cache for next time
      let value = if is_installed { "true" } else { "false" };
      if let Err(e) = db.set_setting(&db_key, value) {
        log::warn!("Failed to cache {} app: {}", key, e);
      }
    }
  }

  apps
}
