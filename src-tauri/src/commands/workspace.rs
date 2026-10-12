use crate::jj::{self, JjRebaseResult};
use crate::local_db::Workspace;
use crate::lock_ext::LockExt;
use crate::AppState;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use tauri::State;

// Track which workspaces have been indexed this session
static INDEXED_WORKSPACES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// Fires the repo's `.treq/config.yaml` `setup_script` (if any) for a newly
/// created workspace, in the background so workspace creation itself isn't
/// slowed down. Checks are blocked for the workspace until this finishes —
/// see `core::checks::ensure_setup_script_complete`.
fn spawn_setup_script_if_configured(workspace: &Workspace) {
  let script = match crate::core::configured_setup_script(&workspace.repo_path) {
    Ok(Some(script)) => script,
    Ok(None) => return,
    Err(e) => {
      log::warn!("Failed to read .treq/config.yaml for setup_script: {}", e);
      return;
    }
  };

  let repo_path = workspace.repo_path.clone();
  let workspace_id = workspace.id;
  let workspace_path = crate::auto_rebase::get_full_workspace_path(workspace);
  std::thread::spawn(move || {
    if let Err(e) =
      crate::core::run_setup_script_sync(&repo_path, workspace_id, &workspace_path, &script)
    {
      log::warn!(
        "Setup script failed for workspace {} in {}: {}",
        workspace_id,
        repo_path,
        e
      );
    }
  });
}

#[tauri::command]
pub async fn get_repo_current_branch(repo_path: String) -> Result<crate::core::RepoBranch, String> {
  tauri::async_runtime::spawn_blocking(move || crate::core::get_repo_current_branch(&repo_path))
    .await
    .map_err(|e| format!("Failed to join get_repo_current_branch task: {}", e))?
}

#[tauri::command]
pub async fn get_repo_default_branch(repo_path: String) -> Result<String, String> {
  tauri::async_runtime::spawn_blocking(move || crate::core::get_repo_default_branch(&repo_path))
    .await
    .map_err(|e| format!("Failed to join get_repo_default_branch task: {}", e))?
}

#[tauri::command]
pub async fn get_workspace_changed_files(
  repo_path: String,
  workspace_id: Option<i64>,
) -> Result<Vec<crate::jj::JjFileChange>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::core::list_changed_files(&repo_path, workspace_id)
  })
  .await
  .map_err(|e| format!("Failed to join get_workspace_changed_files task: {}", e))?
}

#[tauri::command]
pub async fn set_git_submodule_synced(
  repo_path: String,
  path: String,
  enabled: bool,
) -> Result<Vec<crate::core::GitSubmodule>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::core::set_submodule_synced(&repo_path, &path, enabled)
  })
  .await
  .map_err(|e| format!("Failed to join set_git_submodule_synced task: {}", e))?
}

#[tauri::command]
pub async fn get_workspaces(repo_path: String) -> Result<Vec<Workspace>, String> {
  tauri::async_runtime::spawn_blocking(move || crate::core::list_workspaces(&repo_path))
    .await
    .map_err(|e| format!("Failed to join get_workspaces task: {}", e))?
}

/// Combined command: creates jj workspace + adds to database atomically
/// Delegates to core::create_workspace() for all workspace creation logic
#[tauri::command]
pub async fn create_workspace(
  state: State<'_, AppState>,
  repo_path: String,
  branch_name: String,
  source_branch: Option<String>,
  metadata: Option<String>,
) -> Result<i64, String> {
  let started_at = Instant::now();
  let parsed_metadata = crate::core::parse_workspace_metadata(metadata.as_deref());

  // Read included_copy_files setting from DB (small files to copy into every workspace)
  let included_copy_files = {
    let db = state.db.lock_or_recover();
    let raw = db
      .get_repo_setting(&repo_path, "included_copy_files")
      .ok()
      .flatten();
    crate::core::parse_included_copy_files(raw.as_deref())
  };

  let repo_path_for_task = repo_path.clone();
  let branch_name_for_task = branch_name.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    let workspace = crate::core::create_new_workspace(
      &repo_path_for_task,
      &branch_name_for_task,
      source_branch.as_deref(),
      parsed_metadata,
      included_copy_files,
    )?;

    spawn_setup_script_if_configured(&workspace);

    Ok(workspace.id)
  })
  .await
  .map_err(|e| format!("Failed to join create_workspace task: {}", e))?;
  log::debug!(
    "create_workspace(repo_path={}, branch_name={}) completed in {:?}",
    repo_path,
    branch_name,
    started_at.elapsed()
  );
  result
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOrCreateWorkspaceFromPrResult {
  pub workspace_id: i64,
  pub created: bool,
}

/// Open an existing workspace for a PR head branch, or create one from the
/// remote tip with the PR base as the rebase target.
#[tauri::command]
pub async fn open_or_create_workspace_from_pr(
  repo_path: String,
  head_branch: String,
  base_branch: String,
  title: Option<String>,
  description: Option<String>,
) -> Result<OpenOrCreateWorkspaceFromPrResult, String> {
  let started_at = Instant::now();
  let result = tauri::async_runtime::spawn_blocking(move || {
    let (workspace, created) = crate::core::open_or_create_workspace_from_pr(
      &repo_path,
      &head_branch,
      &base_branch,
      title.as_deref(),
      description.as_deref(),
    )?;
    if created {
      spawn_setup_script_if_configured(&workspace);
    }
    Ok(OpenOrCreateWorkspaceFromPrResult {
      workspace_id: workspace.id,
      created,
    })
  })
  .await
  .map_err(|e| format!("Failed to join open_or_create_workspace_from_pr task: {e}"))?;
  log::debug!(
    "open_or_create_workspace_from_pr completed in {:?}",
    started_at.elapsed()
  );
  result
}

/// Unified delete workspace command that handles both filesystem and DB cleanup
/// Delegates to core::delete_workspace which correctly constructs the full workspace path
#[tauri::command]
pub async fn delete_workspace(repo_path: String, id: i64) -> Result<(), String> {
  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    crate::core::delete_workspace(&repo_path_for_task, &id).map(|_| ())
  })
  .await
  .map_err(|e| format!("Failed to join delete_workspace task: {}", e))?;
  log::debug!(
    "delete_workspace(repo_path={}, id={}) completed in {:?}",
    repo_path,
    id,
    started_at.elapsed()
  );
  result
}

/// Archive a workspace: forget jj and keep the DB record. Directory removal
/// runs in the background so the UI can drop the row immediately.
#[tauri::command]
pub async fn archive_workspace(repo_path: String, id: i64) -> Result<(), String> {
  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let leftover = tauri::async_runtime::spawn_blocking(move || {
    crate::core::archive_workspace_leaving_directory(&repo_path_for_task, &id)
  })
  .await
  .map_err(|e| format!("Failed to join archive_workspace task: {}", e))??;
  if let Some(path) = leftover {
    tauri::async_runtime::spawn_blocking(move || {
      if let Err(e) = crate::jj::remove_workspace_directory_only(&path) {
        tracing::warn!("Background workspace directory removal failed: {}", e);
      }
    });
  }
  log::debug!(
    "archive_workspace(repo_path={}, id={}) archived in {:?}",
    repo_path,
    id,
    started_at.elapsed()
  );
  Ok(())
}

/// Push workspace to remote and update not_on_remote flag
#[tauri::command]
pub async fn push_workspace_to_remote(
  repo_path: String,
  workspace_id: Option<i64>,
) -> Result<String, String> {
  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    crate::core::push_workspace_to_remote(&repo_path_for_task, workspace_id)
  })
  .await
  .map_err(|e| format!("Failed to join push_workspace_to_remote task: {}", e))?;
  log::debug!(
    "push_workspace_to_remote(repo_path={}, workspace_id={:?}) completed in {:?}",
    repo_path,
    workspace_id,
    started_at.elapsed()
  );
  result
}

/// Merge a workspace into its target branch with a specified merge strategy
#[tauri::command]
pub async fn merge_workspace(
  repo_path: String,
  workspace_id: i64,
  message: String,
  merge_strategy: String,
) -> Result<(), String> {
  use crate::core::MergeCommit;

  // Convert string to enum
  let strategy = match merge_strategy.as_str() {
    "merge" => MergeCommit::Merge,
    "squash" => MergeCommit::SquashAndMerge,
    "rebase" => MergeCommit::RebaseAndMerge,
    _ => return Err(format!("Invalid merge strategy: {}", merge_strategy)),
  };

  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let message_for_task = message.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    crate::core::merge_workspace(
      &repo_path_for_task,
      workspace_id,
      &message_for_task,
      strategy,
    )
  })
  .await
  .map_err(|e| format!("Failed to join merge_workspace task: {}", e))?;
  log::debug!(
    "merge_workspace(repo_path={}, workspace_id={}) completed in {:?}",
    repo_path,
    workspace_id,
    started_at.elapsed()
  );
  result
}

#[tauri::command]
pub async fn get_workspace_status(
  repo_path: String,
  workspace_id: Option<i64>,
) -> Result<crate::core::WorkspaceStatus, String> {
  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    crate::core::workspace_status(&repo_path_for_task, workspace_id)
  })
  .await
  .map_err(|e| format!("Failed to join get_workspace_status task: {}", e))?;
  log::debug!(
    "get_workspace_status(repo_path={}, workspace_id={:?}) completed in {:?}",
    repo_path,
    workspace_id,
    started_at.elapsed()
  );
  result
}

#[tauri::command]
pub async fn list_workspace_statuses(
  repo_path: String,
) -> Result<Vec<crate::core::WorkspaceSidebarStatus>, String> {
  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    crate::core::list_workspace_statuses(&repo_path_for_task)
  })
  .await
  .map_err(|e| format!("Failed to join list_workspace_statuses task: {}", e))?;
  log::debug!(
    "list_workspace_statuses(repo_path={}) completed in {:?}",
    repo_path,
    started_at.elapsed()
  );
  result
}

#[tauri::command]
pub fn ensure_workspace_indexed(
  repo_path: String,
  workspace_id: Option<i64>,
  workspace_path: String,
) -> Result<bool, String> {
  let indexed = INDEXED_WORKSPACES.get_or_init(|| Mutex::new(HashSet::new()));
  let mut guard = indexed.lock_or_recover();

  // Use workspace_path as the key
  if guard.contains(&workspace_path) {
    // Already indexed this session
    return Ok(false);
  }

  // Mark as indexed
  guard.insert(workspace_path.clone());
  drop(guard);

  // Trigger indexing
  crate::file_indexer::index_workspace_files(&repo_path, workspace_id, &workspace_path)?;

  Ok(true)
}

#[tauri::command]
pub async fn update_workspace(
  repo_path: String,
  workspace_id: i64,
  target_branch: Option<String>,
  title: Option<String>,
  description: Option<String>,
) -> Result<Workspace, String> {
  use crate::core::MaybeEmptyParam;

  let tb = match target_branch {
    Some(s) if s.is_empty() => MaybeEmptyParam::EmptyValue,
    Some(s) => MaybeEmptyParam::Some(s),
    None => MaybeEmptyParam::Omitted,
  };
  let t = match title {
    Some(s) if s.is_empty() => MaybeEmptyParam::EmptyValue,
    Some(s) => MaybeEmptyParam::Some(s),
    None => MaybeEmptyParam::Omitted,
  };
  let d = match description {
    Some(s) if s.is_empty() => MaybeEmptyParam::EmptyValue,
    Some(s) => MaybeEmptyParam::Some(s),
    None => MaybeEmptyParam::Omitted,
  };
  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    crate::core::update_workspace_with_title(&repo_path_for_task, workspace_id, tb, t, d)
  })
  .await
  .map_err(|e| format!("Failed to join update_workspace task: {}", e))?;
  log::debug!(
    "update_workspace(repo_path={}, workspace_id={}) completed in {:?}",
    repo_path,
    workspace_id,
    started_at.elapsed()
  );
  result
}

#[tauri::command]
pub async fn set_workspace_target_branch(
  _state: State<'_, AppState>,
  repo_path: String,
  workspace_path: String,
  id: i64,
  target_branch: String,
) -> Result<JjRebaseResult, String> {
  tracing::debug!(
    "[set_workspace_target_branch] repo_path={}, workspace_path={}, id={}, target_branch={}",
    repo_path,
    workspace_path,
    id,
    target_branch
  );

  // Validate workspace path exists
  if !std::path::Path::new(&workspace_path).exists() {
    return Err(format!(
      "Workspace directory is missing on disk and could not be recovered from JJ state: {}",
      workspace_path
    ));
  }

  tauri::async_runtime::spawn_blocking(move || {
    log::debug!(
      "set_workspace_target_branch retarget start: repo_path={}, workspace_id={}, target_branch={}",
      repo_path,
      id,
      target_branch
    );
    crate::core::update_workspace(
      &repo_path,
      id,
      crate::core::MaybeEmptyParam::Some(target_branch.clone()),
      crate::core::MaybeEmptyParam::Omitted,
    )?;
    log::debug!(
      "set_workspace_target_branch retarget end: repo_path={}, workspace_id={}",
      repo_path,
      id
    );
    Ok(JjRebaseResult {
      success: true,
      message: format!("Retargeted workspace onto {}", target_branch),
    })
  })
  .await
  .map_err(|e| format!("Failed to join set_workspace_target_branch task: {}", e))?
}

#[tauri::command]
pub async fn check_and_rebase_workspaces(
  state: State<'_, AppState>,
  repo_path: String,
  workspace_id: Option<i64>,
  default_branch: Option<String>,
  force: Option<bool>,
) -> Result<crate::core::SingleRebaseResult, String> {
  let conflict_style = crate::core::resolve_conflict_marker_style(&state.db);
  tauri::async_runtime::spawn_blocking(move || {
        log::debug!(
            "check_and_rebase_workspaces start: repo_path={}, workspace_id={:?}, default_branch={:?}, force={:?}",
            repo_path,
            workspace_id,
            default_branch,
            force
        );
        let result = crate::core::check_and_rebase_workspaces(
            &repo_path,
            workspace_id,
            default_branch,
            force,
            &conflict_style,
        );
        log::debug!(
            "check_and_rebase_workspaces end: repo_path={}, workspace_id={:?}, success={}",
            repo_path,
            workspace_id,
            result.is_ok()
        );
        result
    })
    .await
    .map_err(|e| format!("Failed to join check_and_rebase_workspaces task: {}", e))?
}

/// Pull workspace from remote, automatically resolving divergence
#[tauri::command]
pub async fn pull_workspace_from_remote(
  state: State<'_, AppState>,
  repo_path: String,
  workspace_id: Option<i64>,
) -> Result<crate::core::PullWorkspaceResult, String> {
  let started_at = Instant::now();
  let conflict_style = crate::core::resolve_conflict_marker_style(&state.db);
  let repo_path_for_task = repo_path.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    crate::core::pull_workspace_from_remote(&repo_path_for_task, workspace_id, &conflict_style)
  })
  .await
  .map_err(|e| format!("Failed to join pull_workspace_from_remote task: {}", e))?;
  log::debug!(
    "pull_workspace_from_remote(repo_path={}, workspace_id={:?}) completed in {:?}",
    repo_path,
    workspace_id,
    started_at.elapsed()
  );
  result
}

/// Resolve a conflicted bookmark while preserving every local-only change.
#[tauri::command]
pub async fn resolve_workspace_bookmark_conflict(
  repo_path: String,
  workspace_id: i64,
  workspace_path: String,
  branch_name: String,
) -> Result<jj::BookmarkConflictResolutionResult, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::core::resolve_workspace_bookmark_conflict(
      &repo_path,
      workspace_id,
      &workspace_path,
      &branch_name,
    )
  })
  .await
  .map_err(|e| {
    format!(
      "Failed to join resolve_workspace_bookmark_conflict task: {}",
      e
    )
  })?
}

/// Rename a workspace's branch/bookmark.
/// Supports dry_run mode for validation without performing the rename.
#[tauri::command]
pub async fn rename_workspace(
  repo_path: String,
  workspace_id: i64,
  new_branch_name: String,
  dry_run: bool,
) -> Result<crate::core::RenameWorkspaceResult, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::core::rename_workspace(&repo_path, workspace_id, &new_branch_name, dry_run)
  })
  .await
  .map_err(|e| format!("Failed to join rename_workspace task: {}", e))?
}

/// Move changes between two existing workspaces (files, hunks, or commits).
/// Delegates to core::move_workspace_changes() for all logic.
#[tauri::command]
pub fn move_workspace_changes(
  repo_path: String,
  source_branch: String,
  destination_branch: String,
  request: crate::core::WorkspaceMoveRequest,
) -> Result<crate::core::WorkspaceMoveResult, String> {
  crate::core::move_workspace_changes(&repo_path, &source_branch, &destination_branch, request)
}

#[tauri::command]
pub fn move_commit_to_existing_workspace(
  repo_path: String,
  source_workspace_id: i64,
  commit_change_id: String,
  target_workspace_id: i64,
) -> Result<(), String> {
  crate::core::move_commit_to_existing_workspace(
    &repo_path,
    source_workspace_id,
    &commit_change_id,
    target_workspace_id,
  )
}

#[tauri::command]
pub async fn abandon_commit(
  repo_path: String,
  workspace_id: i64,
  commit_change_id: String,
) -> Result<String, String> {
  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let commit_change_id_for_task = commit_change_id.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    crate::core::abandon_commit(
      &repo_path_for_task,
      workspace_id,
      &commit_change_id_for_task,
    )
  })
  .await
  .map_err(|e| format!("Failed to join abandon_commit task: {}", e))?;
  log::debug!(
    "abandon_commit(repo_path={}, workspace_id={}, commit_change_id={}) completed in {:?}",
    repo_path,
    workspace_id,
    commit_change_id,
    started_at.elapsed()
  );
  result
}

/// Undo the latest commit in a workspace's own lineage (not the working copy,
/// not a commit on the target branch). Must be undone sequentially from the tip.
#[tauri::command]
pub async fn undo_commit(
  repo_path: String,
  workspace_id: i64,
  commit_change_id: String,
) -> Result<(), String> {
  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let commit_change_id_for_task = commit_change_id.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    crate::core::undo_commit(
      &repo_path_for_task,
      workspace_id,
      &commit_change_id_for_task,
    )
  })
  .await
  .map_err(|e| format!("Failed to join undo_commit task: {}", e))?;
  log::debug!(
    "undo_commit(repo_path={}, workspace_id={}, commit_change_id={}) completed in {:?}",
    repo_path,
    workspace_id,
    commit_change_id,
    started_at.elapsed()
  );
  result
}

/// Revert a commit by creating a new commit that reverses its changes on top
/// of the workspace's current tip. Can target any commit except the working copy.
#[tauri::command]
pub async fn revert_commit(
  repo_path: String,
  workspace_id: i64,
  commit_change_id: String,
) -> Result<(), String> {
  let started_at = Instant::now();
  let repo_path_for_task = repo_path.clone();
  let commit_change_id_for_task = commit_change_id.clone();
  let result = tauri::async_runtime::spawn_blocking(move || {
    crate::core::revert_commit(
      &repo_path_for_task,
      workspace_id,
      &commit_change_id_for_task,
    )
  })
  .await
  .map_err(|e| format!("Failed to join revert_commit task: {}", e))?;
  log::debug!(
    "revert_commit(repo_path={}, workspace_id={}, commit_change_id={}) completed in {:?}",
    repo_path,
    workspace_id,
    commit_change_id,
    started_at.elapsed()
  );
  result
}

#[tauri::command]
pub async fn rebase_home_repo_branch(
  repo_path: String,
  current_branch: String,
  target_branch: String,
) -> Result<crate::jj::JjRebaseResult, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::jj::jj_rebase_home_repo_branch(&repo_path, &current_branch, &target_branch)
      .map_err(|e| e.to_string())
  })
  .await
  .map_err(|e| format!("Failed to join rebase_home_repo_branch task: {}", e))?
}

#[tauri::command]
pub async fn dry_run_home_repo_rebase(
  repo_path: String,
  current_branch: String,
  target_branch: String,
) -> Result<crate::jj::HomeRebaseDryRunResult, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::jj::jj_dry_run_home_repo_rebase(&repo_path, &current_branch, &target_branch)
      .map_err(|e| e.to_string())
  })
  .await
  .map_err(|e| format!("Failed to join dry_run_home_repo_rebase task: {}", e))?
}

#[cfg(test)]
mod tests {
  use super::*;
  // TODO: Add unit tests when a mockable workspace DB abstraction exists.
  use std::fs;
  use tempfile::TempDir;

  // Unit tests cover DB cleanup only; full jj+directory cleanup is e2e-tested.
  #[test]
  fn test_delete_workspace_cleans_up_db_entry() {
    use crate::local_db;

    // Setup: Create a temp directory with a fake workspace under .treq/workspaces/
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();
    let workspaces_dir = temp_dir.path().join(".treq").join("workspaces");
    let workspace_dir = workspaces_dir.join("test_workspace");
    fs::create_dir_all(&workspace_dir).unwrap();

    // Add workspace to DB with just the directory name (matching production behavior)
    local_db::add_workspace(
      repo_path,
      "test".to_string(),
      "test_workspace".to_string(),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .unwrap();

    // Get the workspace ID
    let workspaces = local_db::get_workspaces(repo_path).unwrap();
    assert_eq!(workspaces.len(), 1);
    let workspace_id = workspaces[0].id;

    // Act: delete workspace; jj forget is expected best-effort without a real jj repo.
    let result =
      tauri::async_runtime::block_on(delete_workspace(repo_path.to_string(), workspace_id));

    // Assert: Should succeed (jj errors are non-fatal)
    assert!(
      result.is_ok(),
      "delete_workspace should succeed: {:?}",
      result
    );

    // Assert: DB entry should be removed
    let workspaces_after = local_db::get_workspaces(repo_path).unwrap();
    assert_eq!(
      workspaces_after.len(),
      0,
      "Workspace should be removed from database"
    );
  }

  #[test]
  fn test_delete_workspace_removes_db_even_if_directory_missing() {
    use crate::local_db;

    // Setup: Create a temp directory for the repo
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();

    // Create .treq/workspaces/ but NOT the workspace directory itself
    let workspaces_dir = temp_dir.path().join(".treq").join("workspaces");
    fs::create_dir_all(&workspaces_dir).unwrap();

    // Add workspace to DB with just the directory name (orphaned entry - directory doesn't exist)
    local_db::add_workspace(
      repo_path,
      "test".to_string(),
      "nonexistent_workspace".to_string(),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .unwrap();

    let workspaces = local_db::get_workspaces(repo_path).unwrap();
    assert_eq!(workspaces.len(), 1);
    let workspace_id = workspaces[0].id;

    // Act: Delete the workspace (directory doesn't exist)
    let result =
      tauri::async_runtime::block_on(delete_workspace(repo_path.to_string(), workspace_id));

    // Assert: Should still succeed (core::delete_workspace handles missing directories)
    assert!(
      result.is_ok(),
      "delete_workspace should succeed even when directory missing: {:?}",
      result
    );

    // Assert: DB entry should be removed
    let workspaces_after = local_db::get_workspaces(repo_path).unwrap();
    assert_eq!(
      workspaces_after.len(),
      0,
      "Workspace should be removed from database even if directory was missing"
    );
  }

  #[test]
  fn test_archive_workspace_keeps_db_entry() {
    use crate::local_db;

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();
    let workspaces_dir = temp_dir.path().join(".treq").join("workspaces");
    let workspace_dir = workspaces_dir.join("test_workspace");
    fs::create_dir_all(&workspace_dir).unwrap();

    local_db::add_workspace(
      repo_path,
      "test".to_string(),
      "test_workspace".to_string(),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .unwrap();

    let workspaces = local_db::get_workspaces(repo_path).unwrap();
    assert_eq!(workspaces.len(), 1);
    let workspace_id = workspaces[0].id;

    let result =
      tauri::async_runtime::block_on(archive_workspace(repo_path.to_string(), workspace_id));
    assert!(
      result.is_ok(),
      "archive_workspace should succeed: {:?}",
      result
    );

    assert!(
      local_db::get_workspaces(repo_path).unwrap().is_empty(),
      "Archived workspace should be hidden from get_workspaces"
    );
    let stored = local_db::get_workspace_by_id(repo_path, workspace_id)
      .unwrap()
      .expect("record should remain");
    assert!(stored.archived);
  }
}
