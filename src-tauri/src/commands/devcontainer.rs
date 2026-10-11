use crate::core::devcontainer::{self, DevcontainerStatus};
use crate::core::feature_preview::PreviewFeature;
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
  tauri::async_runtime::spawn_blocking(move || devcontainer::up(&repo_path))
    .await
    .map_err(|error| error.to_string())?
}
