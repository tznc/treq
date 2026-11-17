use crate::linear::LinearIssue;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct LinearKickoffResult {
  pub issue_id: String,
  pub workspace_id: i64,
  pub created: bool,
}

/// Opens or creates the workspace for one Linear issue. A sub-issue whose
/// parent has an open workspace stacks on it, so the sidebar nests it there;
/// any other issue starts on the default branch. The core never
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
  let parent_identifier = issue.parent_identifier.clone();
  let issue_key = issue.identifier.clone();
  let issue_url = issue.url.clone();
  let issue_title = issue.title.clone();

  let (workspace, created) = tauri::async_runtime::spawn_blocking(move || {
    let default_branch = crate::core::get_repo_default_branch(&repo_path_owned)
      .map_err(|e| format!("Failed to get repo default branch: {e}"))?;
    let base_branch = match parent_identifier.as_deref() {
      Some(_) => {
        // A parent whose directory is gone is not a base.
        crate::core::sync_workspaces(&repo_path_owned)?;
        crate::core::linear_issue_base_branch(
          &crate::core::list_workspaces(&repo_path_owned).unwrap_or_default(),
          parent_identifier.as_deref(),
          &default_branch,
        )
      }
      None => default_branch,
    };

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
