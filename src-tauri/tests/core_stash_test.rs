mod common;

use common::TestRepo;

fn snapshot_disk_into_wc(workspace_path: &str) -> Vec<treq_lib::jj::JjFileChange> {
  treq_lib::jj::jj_get_changed_files(workspace_path).expect("jj_get_changed_files should not panic")
}

#[test]
fn stash_workspace_changes_moves_wc_changes_into_immutable_stash() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stash-basic",
    Some("stash basic".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  TestRepo::write_workspace_file(ws_dir_str, "README.md", "# stashed content\n")
    .expect("Failed to overwrite README.md");
  TestRepo::write_workspace_file(ws_dir_str, "extra.txt", "extra\n")
    .expect("Failed to create extra.txt");
  assert_eq!(
    snapshot_disk_into_wc(ws_dir_str).len(),
    2,
    "expected two working-copy changes before stash"
  );

  let entry = treq_lib::core::stash_workspace_changes(&repo.repo_path, Some(workspace.id))
    .expect("stash_workspace_changes should succeed");

  assert!(!entry.commit_id.is_empty());
  assert!(!entry.change_id.is_empty());
  assert!(!entry.short_commit_id.is_empty());
  assert!(entry.additions > 0 || entry.deletions > 0);
  assert!(entry.files_changed.len() >= 2);
  assert!(
    entry.workspace_label.contains("stash")
      || entry.workspace_label.contains("feat")
      || !entry.workspace_label.is_empty()
  );
  assert!(
    snapshot_disk_into_wc(ws_dir_str).is_empty(),
    "working copy should be clean after stash (changes moved out)"
  );

  let listed = treq_lib::core::list_stashes(&repo.repo_path).expect("list_stashes");
  assert_eq!(listed.len(), 1);
  assert_eq!(listed[0].id, entry.id);
  assert_eq!(listed[0].commit_id, entry.commit_id);
}

#[test]
fn apply_stash_copies_onto_target_and_leaves_stash_intact() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let source = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stash-source",
    Some("source".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create source workspace");
  let target = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stash-target",
    Some("target".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create target workspace");

  let source_dir = repo.workspace_full_path(&source);
  TestRepo::write_workspace_file(&source_dir, "README.md", "# from stash\n")
    .expect("Failed to write README");
  assert!(!snapshot_disk_into_wc(&source_dir).is_empty());

  let entry = treq_lib::core::stash_workspace_changes(&repo.repo_path, Some(source.id))
    .expect("stash should succeed");
  assert!(snapshot_disk_into_wc(&source_dir).is_empty());

  treq_lib::core::apply_stash(&repo.repo_path, entry.id, &target.branch_name)
    .expect("apply_stash should succeed");

  let target_dir = repo.workspace_full_path(&target);
  let applied = std::fs::read_to_string(std::path::Path::new(&target_dir).join("README.md"))
    .expect("README should exist on target after apply");
  assert_eq!(applied, "# from stash\n");
  assert!(
    !snapshot_disk_into_wc(&target_dir).is_empty(),
    "target should show applied changes"
  );

  // Stash remains — can apply again to source (copy semantics).
  let still_listed = treq_lib::core::list_stashes(&repo.repo_path).expect("list");
  assert_eq!(still_listed.len(), 1);

  treq_lib::core::apply_stash(&repo.repo_path, entry.id, &source.branch_name)
    .expect("re-apply to source should succeed");
  let reapplied = std::fs::read_to_string(std::path::Path::new(&source_dir).join("README.md"))
    .expect("README should exist on source after re-apply");
  assert_eq!(reapplied, "# from stash\n");
}

#[test]
fn delete_stash_removes_entry_and_bookmark() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stash-delete",
    Some("delete".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&ws_dir, "README.md", "# delete me\n").expect("Failed to write");
  assert!(!snapshot_disk_into_wc(&ws_dir).is_empty());

  let entry = treq_lib::core::stash_workspace_changes(&repo.repo_path, Some(workspace.id))
    .expect("stash should succeed");

  treq_lib::core::delete_stash(&repo.repo_path, entry.id).expect("delete_stash should succeed");

  let listed = treq_lib::core::list_stashes(&repo.repo_path).expect("list");
  assert!(listed.is_empty());
}

#[test]
fn get_stash_diff_returns_isolated_diff_in_original_context() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stash-diff",
    Some("diff".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&ws_dir, "README.md", "# isolated stash diff\n")
    .expect("Failed to write");
  assert!(!snapshot_disk_into_wc(&ws_dir).is_empty());

  let entry = treq_lib::core::stash_workspace_changes(&repo.repo_path, Some(workspace.id))
    .expect("stash should succeed");

  let diff = treq_lib::core::get_stash_diff(&repo.repo_path, entry.id).expect("get_stash_diff");
  assert!(
    !diff.committed_files.is_empty() || !diff.hunks_by_file.is_empty(),
    "stash diff should include file changes"
  );
  assert!(
    diff.committed_files.iter().any(|f| f.path == "README.md")
      || diff.hunks_by_file.iter().any(|f| f.path == "README.md"),
    "README.md should appear in stash diff"
  );
}

#[test]
fn export_stash_git_patch_returns_unified_diff() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stash-patch",
    Some("patch".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&ws_dir, "README.md", "# patch content\n")
    .expect("Failed to write");
  assert!(!snapshot_disk_into_wc(&ws_dir).is_empty());

  let entry = treq_lib::core::stash_workspace_changes(&repo.repo_path, Some(workspace.id))
    .expect("stash should succeed");

  let patch =
    treq_lib::core::export_stash_git_patch(&repo.repo_path, entry.id).expect("export patch");
  assert!(
    patch.contains("diff --git") || patch.contains("---") || patch.contains("+++"),
    "expected a git-style patch, got: {patch}"
  );
  assert!(
    patch.contains("README.md") || patch.contains("patch content"),
    "patch should mention the changed file or content"
  );
}

#[test]
fn stash_rejects_clean_working_copy() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stash-clean",
    Some("clean".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let err = treq_lib::core::stash_workspace_changes(&repo.repo_path, Some(workspace.id))
    .expect_err("stash on clean WC should fail");
  assert!(
    err.to_lowercase().contains("nothing") || err.to_lowercase().contains("no change"),
    "unexpected error: {err}"
  );
}

#[test]
fn stash_commit_removes_from_branch_and_is_applicable() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let source = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stash-commit-src",
    Some("stash commit src".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create source workspace");
  let target = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stash-commit-dst",
    Some("stash commit dst".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create target workspace");

  let source_path = repo.workspace_full_path(&source);
  TestRepo::write_workspace_file(&source_path, "parked.txt", "park me\n")
    .expect("Failed to write parked.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, source.id, "commit to stash")
    .expect("should create commit");

  let log = treq_lib::core::list_commits(&repo.repo_path, Some(source.id), false, None, None)
    .expect("list commits");
  let parked = log
    .commits
    .iter()
    .find(|c| c.description.contains("commit to stash"))
    .expect("parked commit should exist");
  let change_id = parked.change_id.clone();

  let entry = treq_lib::core::stash_commit(&repo.repo_path, Some(source.id), &change_id)
    .expect("stash_commit should succeed");

  assert!(!entry.commit_id.is_empty());
  assert!(entry.files_changed.iter().any(|f| f == "parked.txt"));
  assert!(entry.additions > 0 || entry.deletions > 0);

  let after = treq_lib::core::list_commits(&repo.repo_path, Some(source.id), false, None, None)
    .expect("list after stash");
  assert!(
    after
      .commits
      .iter()
      .all(|c| !c.description.contains("commit to stash")),
    "stashed commit should no longer appear on the source branch"
  );

  let listed = treq_lib::core::list_stashes(&repo.repo_path).expect("list stashes");
  assert_eq!(listed.len(), 1);
  assert_eq!(listed[0].id, entry.id);

  treq_lib::core::apply_stash(&repo.repo_path, entry.id, &target.branch_name)
    .expect("apply stashed commit onto target");
  let target_path = repo.workspace_full_path(&target);
  let applied = std::fs::read_to_string(std::path::Path::new(&target_path).join("parked.txt"))
    .expect("parked.txt should exist on target after apply");
  assert_eq!(applied, "park me\n");

  // Stash remains (copy apply); can still be applied elsewhere / deleted.
  assert_eq!(
    treq_lib::core::list_stashes(&repo.repo_path)
      .expect("list")
      .len(),
    1
  );
}

#[test]
fn stash_commit_rejects_unknown_change_id() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stash-commit-missing",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let err = treq_lib::core::stash_commit(&repo.repo_path, Some(workspace.id), "zzzzzzzzzzzz")
    .expect_err("unknown change id should fail");
  assert!(
    !err.is_empty(),
    "expected a non-empty error for unknown change id"
  );
}
