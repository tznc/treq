use crate::core::devcontainer::{self, DevcontainerStatus};
use crate::core::feature_preview::PreviewFeature;
use crate::lock_ext::LockExt;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn devcontainer_status(
  state: State<'_, AppState>,
  repo_path: String,
) -> Result<DevcontainerStatus, String> {
  crate::commands::feature_preview::require(&state, PreviewFeature::Devcontainers)?;
  tauri::async_runtime::spawn_blocking(move || devcontainer::status(&repo_path))
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn devcontainer_up(
  state: State<'_, AppState>,
  repo_path: String,
) -> Result<DevcontainerStatus, String> {
  crate::commands::feature_preview::require(&state, PreviewFeature::Devcontainers)?;
  let claude_code = state
    .db
    .lock_or_recover()
    .get_repo_setting(&repo_path, devcontainer::CLAUDE_CODE_SETTING)
    .ok()
    .flatten()
    .is_none_or(|value| value != "false");
  tauri::async_runtime::spawn_blocking(move || devcontainer::up(&repo_path, claude_code))
    .await
    .map_err(|error| error.to_string())?
}
