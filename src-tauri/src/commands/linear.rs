use crate::linear::{
  LinearComment, LinearDocument, LinearIssue, LinearProject, LinearTeam, LinearUser,
};
use crate::lock_ext::LockExt;
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Serialize, Deserialize, Clone)]
pub struct LinearKickoffResult {
  pub issue_id: String,
  pub workspace_id: i64,
  pub created: bool,
}

#[tauri::command]
pub async fn linear_list_teams(
  state: State<'_, AppState>,
  repo_path: String,
) -> Result<Vec<LinearTeam>, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  let client_source = {
    let db = state.db.lock_or_recover();
    crate::linear::resolve_linear_client(&repo_path, &db)?
  };

  crate::linear::linear_list_teams_impl(&client_source).await
}

#[tauri::command]
pub async fn linear_list_issues(
  state: State<'_, AppState>,
  repo_path: String,
  team_filter: Option<String>,
) -> Result<Vec<LinearIssue>, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  let client_source = {
    let db = state.db.lock_or_recover();
    crate::linear::resolve_linear_client(&repo_path, &db)?
  };

  crate::linear::linear_list_issues_impl(&client_source, team_filter.as_deref()).await
}

#[tauri::command]
pub async fn linear_open_or_create_workspace_from_issue(
  state: State<'_, AppState>,
  repo_path: String,
  issue_id: String,
) -> Result<LinearKickoffResult, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  let client_source = {
    let db = state.db.lock_or_recover();
    crate::linear::resolve_linear_client(&repo_path, &db)?
  };

  let issue = crate::linear::linear_get_issue_impl(&client_source, &issue_id).await?;
  open_or_create_workspace_from_linear_issue(&repo_path, &issue).await
}

/// Opens or creates the workspace for one Linear issue. The core never
/// creates more than one workspace per call; the UI opens sub-issues one
/// call at a time.
pub async fn open_or_create_workspace_from_linear_issue(
  repo_path: &str,
  issue: &LinearIssue,
) -> Result<LinearKickoffResult, String> {
  let repo_path_owned = repo_path.to_string();
  let branch_name = issue.branch_name.clone();
  let title = issue.title.clone();
  let description = issue.description.clone();
  let identifier = issue.identifier.clone();
  let issue_id = issue.id.clone();
  let issue_key = issue.identifier.clone();
  let issue_url = issue.url.clone();
  let issue_title = issue.title.clone();

  let (workspace, created) = tauri::async_runtime::spawn_blocking(move || {
    let base_branch = crate::core::get_repo_default_branch(&repo_path_owned)
      .map_err(|e| format!("Failed to get repo default branch: {e}"))?;

    let (ws, ws_created) = crate::core::open_or_create_workspace_from_issue(
      &repo_path_owned,
      &branch_name,
      &base_branch,
      &title,
      description.as_deref(),
    )
    .map_err(|e| {
      format!(
        "Failed to create workspace for Linear issue {}: {e}",
        identifier
      )
    })?;

    if let Err(e) = crate::core::merge_linear_issue_metadata(
      &repo_path_owned,
      ws.id,
      &issue_key,
      &issue_url,
      &issue_title,
    ) {
      log::warn!(
        "Failed to set Linear issue metadata for workspace {}: {}",
        ws.id,
        e
      );
    }

    Ok::<_, String>((ws, ws_created))
  })
  .await
  .map_err(|e| format!("Failed to join workspace creation task: {e}"))?
  .map(|ws| (ws.0, ws.1))?;

  Ok(LinearKickoffResult {
    issue_id,
    workspace_id: workspace.id,
    created,
  })
}

#[tauri::command]
pub async fn linear_get_viewer(
  state: State<'_, AppState>,
  repo_path: String,
) -> Result<LinearUser, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  let client_source = {
    let db = state.db.lock_or_recover();
    crate::linear::resolve_linear_client(&repo_path, &db)?
  };

  crate::linear::linear_get_viewer_impl(&client_source).await
}

#[tauri::command]
pub async fn linear_list_projects(
  state: State<'_, AppState>,
  repo_path: String,
) -> Result<Vec<LinearProject>, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  let client_source = {
    let db = state.db.lock_or_recover();
    crate::linear::resolve_linear_client(&repo_path, &db)?
  };

  crate::linear::linear_list_projects_impl(&client_source).await
}

#[tauri::command]
pub async fn linear_list_project_documents(
  state: State<'_, AppState>,
  repo_path: String,
  project_id: String,
) -> Result<Vec<LinearDocument>, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  let client_source = {
    let db = state.db.lock_or_recover();
    crate::linear::resolve_linear_client(&repo_path, &db)?
  };

  crate::linear::linear_list_project_documents_impl(&client_source, &project_id).await
}

#[tauri::command]
pub async fn linear_list_issue_comments(
  state: State<'_, AppState>,
  repo_path: String,
  issue_id: String,
) -> Result<Vec<LinearComment>, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  let client_source = {
    let db = state.db.lock_or_recover();
    crate::linear::resolve_linear_client(&repo_path, &db)?
  };

  crate::linear::linear_list_issue_comments_impl(&client_source, &issue_id).await
}

#[tauri::command]
pub async fn linear_list_project_comments(
  state: State<'_, AppState>,
  repo_path: String,
  project_id: String,
) -> Result<Vec<LinearComment>, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  let client_source = {
    let db = state.db.lock_or_recover();
    crate::linear::resolve_linear_client(&repo_path, &db)?
  };

  crate::linear::linear_list_project_comments_impl(&client_source, &project_id).await
}

#[tauri::command]
pub async fn linear_list_document_comments(
  state: State<'_, AppState>,
  repo_path: String,
  document_id: String,
) -> Result<Vec<LinearComment>, String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  let client_source = {
    let db = state.db.lock_or_recover();
    crate::linear::resolve_linear_client(&repo_path, &db)?
  };

  crate::linear::linear_list_document_comments_impl(&client_source, &document_id).await
}

/// Gives the Linear client the Supabase session it sends to the
/// `linear-proxy` Edge Function for OAuth-connected users. `None` clears it
/// on sign-out. Used by `src/lib/linear-proxy-auth.ts`.
#[tauri::command]
pub fn linear_set_proxy_session(supabase_url: Option<String>, access_token: Option<String>) {
  crate::linear::set_proxy_session(supabase_url, access_token);
}

#[tauri::command]
pub fn linear_start_auto_kickoff_polling(
  state: State<'_, AppState>,
  repo_path: String,
) -> Result<(), String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  crate::linear::kickoff_poller().watch_repo(&repo_path);
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn kickoff_result_serializes_to_json() {
    let result = LinearKickoffResult {
      issue_id: "ENG-1".to_string(),
      workspace_id: 42,
      created: true,
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("ENG-1"));
    assert!(json.contains("42"));
    assert!(json.contains("true"));
  }

  #[test]
  fn kickoff_result_deserializes_from_json() {
    let json = r#"{"issue_id":"ENG-2","workspace_id":99,"created":false}"#;
    let result: LinearKickoffResult = serde_json::from_str(json).unwrap();
    assert_eq!(result.issue_id, "ENG-2");
    assert_eq!(result.workspace_id, 99);
    assert!(!result.created);
  }
}

/// Writes what a Linear review covers into the snapshot directory the review
/// agent reads and anchors its comments on. Returns that directory.
#[tauri::command]
pub fn linear_prepare_review(
  repo_path: String,
  target_type: String,
  target_id: String,
  snapshot: crate::core::linear_review::LinearReviewSnapshot,
) -> Result<String, String> {
  crate::core::linear_review::write_snapshot(&repo_path, &target_type, &target_id, &snapshot)
    .map(|dir| dir.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn linear_apply_review_suggestion(
  state: State<'_, AppState>,
  repo_path: String,
  comment_id: String,
) -> Result<(), String> {
  crate::commands::feature_preview::require(
    &state,
    crate::core::feature_preview::PreviewFeature::LinearIntegration,
  )?;
  let client_source = {
    let db = state.db.lock_or_recover();
    crate::linear::resolve_linear_client(&repo_path, &db)?
  };

  crate::linear::linear_apply_review_suggestion_impl(&client_source, &repo_path, &comment_id).await
}
