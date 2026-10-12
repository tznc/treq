use crate::core;
use crate::local_db::{self, PromptHistoryEntry, Session};
use crate::lock_ext::LockExt;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn create_session(
  state: State<AppState>,
  repo_path: String,
  workspace_id: Option<i64>,
  name: String,
) -> Result<i64, String> {
  let db = state.db.lock_or_recover();
  core::sessions::create_session(&db, &repo_path, workspace_id, name)
}

#[tauri::command]
pub fn get_sessions(repo_path: String) -> Result<Vec<Session>, String> {
  local_db::get_sessions(&repo_path)
}

#[tauri::command]
pub fn update_session_access(repo_path: String, id: i64) -> Result<(), String> {
  local_db::update_session_access(&repo_path, id)
}

#[tauri::command]
pub fn get_session_model(repo_path: String, id: i64) -> Result<Option<String>, String> {
  local_db::get_session_model(&repo_path, id)
}

#[tauri::command]
pub fn add_prompt_history(
  repo_path: String,
  workspace_id: Option<i64>,
  session_id: Option<i64>,
  prompt_text: String,
  agent: Option<String>,
) -> Result<i64, String> {
  local_db::add_prompt_history(
    &repo_path,
    workspace_id,
    session_id,
    &prompt_text,
    agent.as_deref(),
  )
}

#[tauri::command]
pub fn get_prompt_history(repo_path: String) -> Result<Vec<PromptHistoryEntry>, String> {
  local_db::get_prompt_history(&repo_path)
}

#[tauri::command]
pub fn get_workspace_starting_prompt(
  repo_path: String,
  workspace_id: i64,
) -> Result<Option<PromptHistoryEntry>, String> {
  local_db::get_workspace_starting_prompt(&repo_path, workspace_id)
}
