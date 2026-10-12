use crate::common::TestRepo;
use treq_lib::local_db::Workspace;

#[test]
fn archive_workspace_removes_directory_and_keeps_db_record() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/archive",
    Some("archive feature".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_name = workspace.workspace_name.clone();
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);

  let result = treq_lib::core::archive_workspace(&repo.repo_path, &workspace.id)
    .expect("Failed to archive workspace");
  assert!(result, "archive_workspace should return true");

  assert!(
    !workspace_path.exists(),
    "Workspace directory should be removed"
  );

  let jj_workspaces_after =
    treq_lib::jj::list_jj_workspaces(&repo.repo_path).expect("Failed to list jj workspaces");
  assert!(
    !jj_workspaces_after.contains(&workspace_name),
    "Workspace should NOT be in jj list after archive, got: {:?}",
    jj_workspaces_after
  );

  let listed = treq_lib::core::list_workspaces(&repo.repo_path).expect("Failed to list workspaces");
  assert!(
    !listed.iter().any(|w| w.id == workspace.id),
    "Archived workspace should not appear in list_workspaces"
  );

  let stored = treq_lib::local_db::get_workspace_by_id(&repo.repo_path, workspace.id)
    .expect("Failed to get workspace by id")
    .expect("Workspace record should still exist");
  assert!(
    stored.archived,
    "Workspace record should be marked archived"
  );

  treq_lib::core::sync_workspaces(&repo.repo_path).expect("Failed to sync workspaces");
  let stored_after_sync = treq_lib::local_db::get_workspace_by_id(&repo.repo_path, workspace.id)
    .expect("Failed to get workspace by id after sync")
    .expect("Archived workspace record should survive sync_workspaces");
  assert!(stored_after_sync.archived);
}
