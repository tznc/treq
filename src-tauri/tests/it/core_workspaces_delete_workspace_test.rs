use crate::common::{JjVerifier, TestRepo};
use treq_lib::local_db::Workspace;

#[test]
fn test_can_delete_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/delete",
    Some("delete feature".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_name = workspace.workspace_name.clone();

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);

  let result = treq_lib::core::delete_workspace(&repo.repo_path, &workspace.id)
    .expect("Failed to delete workspace");
  assert!(result, "Workspace should be deleted");

  assert!(
    !workspace_path.exists(),
    "Workspace directory should be removed"
  );

  let bookmarks = JjVerifier::list_bookmarks(&repo.repo_path).expect("Failed to list bookmarks");
  assert!(
    bookmarks.iter().any(|b| b == &workspace.branch_name),
    "Bookmark '{}' should exist in workspace, got: {:?}",
    workspace.branch_name,
    bookmarks
  );

  let jj_workspaces_after =
    JjVerifier::list_workspaces(&repo.repo_path).expect("Failed to list jj workspaces");
  assert!(
    !jj_workspaces_after.contains(&workspace_name),
    "Workspace should NOT be in jj list after deletion, got: {:?}",
    jj_workspaces_after
  );

  let workspaces =
    treq_lib::local_db::get_workspaces(&repo.repo_path).expect("Failed to get workspaces");
  assert!(
    !workspaces.iter().any(|w| w.id == workspace.id),
    "Workspace should be removed from database"
  );
}

#[test]
fn delete_removing_directory_later_frees_the_slot_at_once() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let sibling = repo
    .create_workspace_simple("feat/sibling")
    .expect("Failed to create sibling");
  let workspace = repo
    .create_workspace_simple("feat/delete-later")
    .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  std::fs::write(workspace_path.join("big.txt"), "x").expect("write file");

  treq_lib::core::delete_workspace_removing_directory_later(&repo.repo_path, workspace.id)
    .expect("Failed to delete workspace");

  assert!(
    !workspace_path.exists(),
    "the slot is free before the directory is gone"
  );
  assert!(
    JjVerifier::list_workspaces(&repo.repo_path)
      .expect("Failed to list jj workspaces")
      .contains(&sibling.workspace_name),
    "the sibling stays registered"
  );
  assert!(
    treq_lib::local_db::get_workspace_by_id(&repo.repo_path, workspace.id)
      .expect("Failed to read workspace")
      .is_none()
  );
  // The slot can be taken again straight away.
  repo
    .create_workspace_simple("feat/delete-later")
    .expect("Failed to recreate workspace");
}

#[test]
fn deletes_workspace_with_stash_and_preserves_applicable_stash() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/delete-stashed",
    Some("Stashed workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  TestRepo::write_workspace_file(
    workspace_path.to_str().expect("utf-8 workspace path"),
    "README.md",
    "# saved before deletion\n",
  )
  .expect("write workspace change");
  let stash = treq_lib::core::stash_workspace_changes(&repo.repo_path, Some(workspace.id))
    .expect("stash workspace changes");

  treq_lib::core::delete_workspace(&repo.repo_path, &workspace.id)
    .expect("delete workspace with stash");

  assert!(
    !workspace_path.exists(),
    "workspace directory should be removed"
  );
  assert!(
    treq_lib::local_db::get_workspace_by_id(&repo.repo_path, workspace.id)
      .expect("lookup workspace")
      .is_none()
  );
  let preserved = treq_lib::core::list_stashes(&repo.repo_path).expect("list preserved stashes");
  assert_eq!(preserved.len(), 1);
  assert_eq!(preserved[0].id, stash.id);
  assert_eq!(preserved[0].workspace_id, None);

  let target = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/apply-preserved",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("create target workspace");
  treq_lib::core::apply_stash(&repo.repo_path, stash.id, &target.branch_name)
    .expect("preserved stash should remain applicable");
}

#[test]
fn test_archive_workspace_leaving_directory_keeps_db_record() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/archive-defer-dir",
    Some("archive feature".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_name = workspace.workspace_name.clone();
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  assert!(workspace_path.exists(), "workspace directory should exist");

  let leftover_path =
    treq_lib::core::archive_workspace_leaving_directory(&repo.repo_path, &workspace.id)
      .expect("archive_workspace_leaving_directory should succeed")
      .expect("should return the leftover directory path");

  // The directory leaves the workspace slot at once (so a recreate under the
  // same name cannot race the removal); deleting it is deferred.
  let leftover = std::path::Path::new(&leftover_path);
  assert!(
    leftover.starts_with(repo.workspaces_dir().parent().unwrap().join("trash")),
    "leftover should be moved into .treq/trash, got: {}",
    leftover_path
  );
  assert!(
    !workspace_path.exists(),
    "workspace slot should be free right after archive"
  );
  assert!(
    leftover.exists(),
    "directory deletion is deferred to a background job"
  );

  let jj_workspaces_after =
    JjVerifier::list_workspaces(&repo.repo_path).expect("Failed to list jj workspaces");
  assert!(
    !jj_workspaces_after.contains(&workspace_name),
    "Workspace should NOT be in jj list after archive, got: {:?}",
    jj_workspaces_after
  );

  let listed =
    treq_lib::local_db::get_workspaces(&repo.repo_path).expect("Failed to get workspaces");
  assert!(
    !listed.iter().any(|w| w.id == workspace.id),
    "Archived workspace should not appear in get_workspaces"
  );

  let stored = treq_lib::local_db::get_workspace_by_id(&repo.repo_path, workspace.id)
    .expect("Failed to get workspace by id")
    .expect("Workspace record should still exist");
  assert!(
    stored.archived,
    "Workspace record should be marked archived"
  );
}

#[test]
fn test_delete_workspace_retargets_children_to_default_branch() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let parent: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/parent",
    Some("parent feature".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create parent workspace");

  let child1: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/child1",
    None,
    None,
    Some(&parent.branch_name),
    None,
    None,
  )
  .expect("Failed to create child1 workspace");

  let child2: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/child2",
    None,
    None,
    Some(&parent.branch_name),
    None,
    None,
  )
  .expect("Failed to create child2 workspace");

  let grandchild: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/grandchild",
    None,
    None,
    Some(&child1.branch_name),
    None,
    None,
  )
  .expect("Failed to create grandchild workspace");

  assert_eq!(child1.target_branch, Some("feat/parent".to_string()));
  assert_eq!(child2.target_branch, Some("feat/parent".to_string()));
  assert_eq!(grandchild.target_branch, Some("feat/child1".to_string()));

  let result = treq_lib::core::delete_workspace(&repo.repo_path, &parent.id)
    .expect("Failed to delete parent workspace");
  assert!(result, "delete_workspace should return true");

  let workspaces =
    treq_lib::local_db::get_workspaces(&repo.repo_path).expect("Failed to get workspaces");

  let updated_child1 = workspaces
    .iter()
    .find(|w| w.id == child1.id)
    .expect("child1 should still exist");
  let updated_child2 = workspaces
    .iter()
    .find(|w| w.id == child2.id)
    .expect("child2 should still exist");
  let updated_grandchild = workspaces
    .iter()
    .find(|w| w.id == grandchild.id)
    .expect("grandchild should still exist");

  assert_eq!(
    updated_child1.target_branch,
    Some(default_branch.to_string())
  );
  assert_eq!(
    updated_child2.target_branch,
    Some(default_branch.to_string())
  );
  assert_eq!(
    updated_grandchild.target_branch,
    Some("feat/child1".to_string())
  );
}

#[test]
fn test_delete_middle_workspace_splices_children_onto_its_parent() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let base: Workspace =
    treq_lib::core::create_workspace(&repo.repo_path, "feat/base", None, None, None, None, None)
      .expect("Failed to create base workspace");
  let middle: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/middle",
    None,
    None,
    Some(&base.branch_name),
    None,
    None,
  )
  .expect("Failed to create middle workspace");
  let top: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/top",
    None,
    None,
    Some(&middle.branch_name),
    None,
    None,
  )
  .expect("Failed to create top workspace");

  treq_lib::core::archive_workspace(&repo.repo_path, &middle.id)
    .expect("Failed to archive middle workspace");

  let workspaces =
    treq_lib::local_db::get_workspaces(&repo.repo_path).expect("Failed to get workspaces");
  let updated_top = workspaces
    .iter()
    .find(|w| w.id == top.id)
    .expect("top should still exist");
  assert_eq!(updated_top.target_branch, Some("feat/base".to_string()));
}

#[test]
fn test_delete_squash_merged_parent_rebases_child_off_its_commits() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch().to_string();
  let parent = repo
    .create_workspace_with_commit("feat/merged-parent", "parent.txt", "parent\n", None)
    .expect("Failed to create parent workspace");
  let child = repo
    .create_workspace_with_commit(
      "feat/spliced-child",
      "child.txt",
      "child\n",
      Some("feat/merged-parent"),
    )
    .expect("Failed to create child workspace");

  // Squash-merge the parent: same tree as its tip, new commit on the default branch.
  let parent_tree = TestRepo::run_git(
    &repo.repo_path,
    &["rev-parse", &format!("{}^{{tree}}", parent.branch_name)],
  )
  .expect("Failed to resolve parent tree");
  let base = TestRepo::run_git(&repo.repo_path, &["rev-parse", &default_branch])
    .expect("Failed to resolve default branch");
  let squash = TestRepo::run_git(
    &repo.repo_path,
    &[
      "commit-tree",
      parent_tree.trim(),
      "-p",
      base.trim(),
      "-m",
      "Squashed parent",
    ],
  )
  .expect("Failed to create squash commit");
  TestRepo::run_git(
    &repo.repo_path,
    &[
      "update-ref",
      &format!("refs/heads/{default_branch}"),
      squash.trim(),
    ],
  )
  .expect("Failed to advance default branch");
  treq_lib::jj::jj_util_import_git_refs(&repo.repo_path).expect("Failed to import squash");

  treq_lib::core::delete_workspace(&repo.repo_path, &parent.id)
    .expect("Failed to delete merged parent");

  let child_path = repo.workspace_full_path(&child);
  let carried = treq_lib::jj::jj_log_revset_commit_ids(
    &child_path,
    &format!("{default_branch}..{}", child.branch_name),
  )
  .expect("Failed to inspect child topology");
  assert_eq!(
    carried.len(),
    1,
    "child should carry only its own commit once the merged parent is removed"
  );
}
