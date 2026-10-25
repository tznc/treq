use crate::db::Database;
use crate::local_db::{self, PromptHistoryEntry, Session};

/// Returns the model a new agent session should start with: the repo's
/// `default_model` setting, or the app-level one when the repo has none.
pub fn resolve_default_session_model(db: &Database, repo_path: &str) -> Option<String> {
  let non_empty = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
  non_empty(
    db.get_repo_setting(repo_path, "default_model")
      .ok()
      .flatten(),
  )
  .or_else(|| non_empty(db.get_setting("default_model").ok().flatten()))
}

/// Creates an agent session row and applies the configured default model.
pub fn create_session(
  db: &Database,
  repo_path: &str,
  workspace_id: Option<i64>,
  name: String,
) -> Result<i64, String> {
  let id = local_db::add_session(repo_path, workspace_id, name)?;
  if let Some(model) = resolve_default_session_model(db, repo_path) {
    local_db::set_session_model(repo_path, id, Some(model))?;
  }
  Ok(id)
}

/// The repo's agent sessions, oldest first.
pub fn list_sessions(repo_path: &str) -> Result<Vec<Session>, String> {
  local_db::get_sessions(repo_path)
}

/// Gives the session a new name.
pub fn rename_session(repo_path: &str, session_id: i64, name: &str) -> Result<(), String> {
  local_db::update_session_name(repo_path, session_id, name.to_string())
}

/// The model the session runs, when one was chosen.
pub fn session_model(repo_path: &str, session_id: i64) -> Result<Option<String>, String> {
  local_db::get_session_model(repo_path, session_id)
}

/// Records a prompt sent to an agent, so it can be recalled later.
pub fn add_prompt_history(
  repo_path: &str,
  workspace_id: Option<i64>,
  session_id: i64,
  prompt: &str,
  agent: Option<&str>,
) -> Result<(), String> {
  local_db::add_prompt_history(repo_path, workspace_id, Some(session_id), prompt, agent).map(|_| ())
}

/// Every prompt sent in the repo, newest first.
pub fn prompt_history(repo_path: &str) -> Result<Vec<PromptHistoryEntry>, String> {
  local_db::get_prompt_history(repo_path)
}

/// Up to `limit` prompts sent in the repo after the newest `offset`.
pub fn prompt_history_page(
  repo_path: &str,
  limit: usize,
  offset: usize,
) -> Result<Vec<PromptHistoryEntry>, String> {
  local_db::get_prompt_history_page(repo_path, limit, offset)
}

/// The prompt a workspace was started with, when it has one.
pub fn workspace_starting_prompt(
  repo_path: &str,
  workspace_id: i64,
) -> Result<Option<PromptHistoryEntry>, String> {
  local_db::get_workspace_starting_prompt(repo_path, workspace_id)
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn setup() -> (TempDir, Database, String) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db = Database::new(":memory:".into()).expect("Failed to open db");
    db.init().expect("Failed to init db");
    let repo_path = temp_dir.path().join("repo").to_string_lossy().to_string();
    (temp_dir, db, repo_path)
  }

  #[test]
  fn creates_session_without_model_when_no_default_set() {
    let (_temp_dir, db, repo_path) = setup();

    let id = create_session(&db, &repo_path, None, "Claude 1".to_string()).unwrap();

    assert_eq!(local_db::get_session_model(&repo_path, id).unwrap(), None);
  }

  #[test]
  fn creates_session_with_app_default_model() {
    let (_temp_dir, db, repo_path) = setup();
    db.set_setting("default_model", "sonnet").unwrap();

    let id = create_session(&db, &repo_path, None, "Claude 1".to_string()).unwrap();

    assert_eq!(
      local_db::get_session_model(&repo_path, id)
        .unwrap()
        .as_deref(),
      Some("sonnet")
    );
  }

  #[test]
  fn creates_session_with_repo_default_model_over_app_default() {
    let (_temp_dir, db, repo_path) = setup();
    db.set_setting("default_model", "sonnet").unwrap();
    db.set_repo_setting(&repo_path, "default_model", "opus")
      .unwrap();

    let id = create_session(&db, &repo_path, None, "Claude 1".to_string()).unwrap();

    assert_eq!(
      local_db::get_session_model(&repo_path, id)
        .unwrap()
        .as_deref(),
      Some("opus")
    );
  }

  #[test]
  fn ignores_empty_repo_default_model() {
    let (_temp_dir, db, repo_path) = setup();
    db.set_setting("default_model", "sonnet").unwrap();
    db.set_repo_setting(&repo_path, "default_model", "")
      .unwrap();

    assert_eq!(
      resolve_default_session_model(&db, &repo_path).as_deref(),
      Some("sonnet")
    );
  }
}
