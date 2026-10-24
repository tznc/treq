use crate::local_db::Workspace;
use crate::lock_ext::LockExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// Track which workspaces have been indexed this session. Each workspace has its
// own lock, held while it indexes, so a search waits for an index another
// thread (the warm-up when a workspace shows) is still building.
static INDEXED_WORKSPACES: OnceLock<Mutex<HashMap<String, Arc<Mutex<bool>>>>> = OnceLock::new();

/// Fires the repo's `.treq/config.yaml` `setup_script` (if any) for a newly
/// created workspace, in the background so workspace creation itself isn't
/// slowed down. Checks are blocked for the workspace until this finishes —
/// see `core::checks::ensure_setup_script_complete`.
pub(crate) fn spawn_setup_script_if_configured(workspace: &Workspace) {
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

pub fn ensure_workspace_indexed(
  repo_path: String,
  workspace_id: Option<i64>,
  workspace_path: String,
) -> Result<bool, String> {
  let slot = INDEXED_WORKSPACES
    .get_or_init(|| Mutex::new(HashMap::new()))
    .lock_or_recover()
    .entry(workspace_path.clone())
    .or_default()
    .clone();
  let mut indexed = slot.lock_or_recover();
  if *indexed {
    return Ok(false);
  }
  crate::file_indexer::index_workspace_files(&repo_path, workspace_id, &workspace_path)?;
  *indexed = true;
  Ok(true)
}

#[cfg(test)]
mod tests {
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
    let result = crate::core::delete_workspace(repo_path, &workspace_id);

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
    let result = crate::core::delete_workspace(repo_path, &workspace_id);

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

    let result = crate::core::archive_workspace_removing_directory_later(repo_path, workspace_id);
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
