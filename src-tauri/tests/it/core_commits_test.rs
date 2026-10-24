use crate::common::TestRepo;

#[test]
fn test_jj_get_log_diff_stats_with_multiline_output() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Create a workspace with a committed change that has insertions and deletions
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/diff-stats",
    Some("diff stats test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  // Create multiple files to trigger multiline diff.stat() output
  // (diff.stat() shows per-file stats + a summary line)
  TestRepo::write_workspace_file(workspace_path_str, "file_a.txt", "line 1\nline 2\nline 3\n")
    .expect("Failed to write file_a");
  TestRepo::write_workspace_file(
    workspace_path_str,
    "file_b.txt",
    "alpha\nbeta\ngamma\ndelta\nepsilon\n",
  )
  .expect("Failed to write file_b");

  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add two files")
    .expect("Failed to commit");

  // Now call list_commits — this should correctly parse the multiline diff.stat() output
  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");

  // Filter out working copy commit to get only the committed change
  let commit = result
    .commits
    .iter()
    .find(|c| !c.is_working_copy && c.description == "Add two files")
    .expect("Should include 'Add two files' commit");
  assert_eq!(commit.description, "Add two files");

  // file_a has 3 lines, file_b has 5 lines => 8 total insertions
  assert_eq!(
    commit.insertions, 8,
    "Should have 8 insertions (3 from file_a + 5 from file_b), got {}",
    commit.insertions
  );
  assert_eq!(
    commit.deletions, 0,
    "Should have 0 deletions, got {}",
    commit.deletions
  );
}

#[test]
fn test_jj_get_log_diff_stats_with_modifications() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/diff-mods",
    Some("diff modifications test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  // First commit: create files
  TestRepo::write_workspace_file(
    workspace_path_str,
    "modify_me.txt",
    "original line 1\noriginal line 2\noriginal line 3\n",
  )
  .expect("Failed to write file");
  TestRepo::write_workspace_file(workspace_path_str, "another.txt", "content\n")
    .expect("Failed to write another file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Initial files")
    .expect("Failed to commit");

  // Second commit: modify and delete lines across multiple files
  TestRepo::write_workspace_file(
    workspace_path_str,
    "modify_me.txt",
    "changed line 1\noriginal line 2\nnew line 3\nnew line 4\n",
  )
  .expect("Failed to modify file");
  TestRepo::remove_file_path(workspace_path.join("another.txt")).expect("Failed to delete file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Modify and delete")
    .expect("Failed to commit");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");

  let committed: Vec<_> = result
    .commits
    .iter()
    .filter(|c| {
      !c.is_working_copy
        && (c.description == "Initial files" || c.description == "Modify and delete")
    })
    .collect();

  // Sum up total insertions and deletions across all commits
  let total_insertions: u32 = committed.iter().map(|c| c.insertions).sum();
  let total_deletions: u32 = committed.iter().map(|c| c.deletions).sum();

  assert!(
    total_insertions > 0,
    "Total insertions should be > 0, got {}",
    total_insertions
  );
  assert!(
    total_deletions > 0,
    "Total deletions should be > 0 (modified lines + deleted file), got {}",
    total_deletions
  );
}

#[test]
fn test_list_commits_includes_tentative_working_copy_for_dirty_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/tentative-working-copy",
    Some("tentative working copy test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  let clean_result =
    treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
      .expect("Failed to list clean workspace commits");

  assert!(
    clean_result.tentative_working_copy.is_none(),
    "Clean workspace should not include a tentative working copy entry"
  );
  assert!(
    clean_result.commits.iter().all(|c| !c.is_working_copy),
    "Clean workspace history should not include a working copy commit"
  );

  TestRepo::write_workspace_file(
    workspace_path_str,
    "tentative.txt",
    "dirty working copy content",
  )
  .expect("Failed to dirty workspace");

  let dirty_result =
    treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
      .expect("Failed to list dirty workspace commits");

  let tentative = dirty_result
    .tentative_working_copy
    .as_ref()
    .expect("Dirty workspace should include a tentative working copy entry");

  assert_eq!(
    tentative.workspace_label, workspace.branch_name,
    "Tentative entry should use the workspace branch name as its label"
  );
  assert!(
    tentative.commit.is_working_copy,
    "Tentative entry should wrap the working copy commit"
  );
  assert!(
        dirty_result.commits.iter().all(|c| !c.is_working_copy),
        "Committed history should not include the working copy commit when a tentative entry is present"
    );
}

#[test]
fn test_list_commits_includes_stacked_working_copies_for_dirty_chain() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let root = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/root",
    Some("root workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create root workspace");
  let child = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/child",
    Some("child workspace".to_string()),
    None,
    Some(&root.branch_name),
    None,
    None,
  )
  .expect("Failed to create child workspace");
  let grandchild = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/grandchild",
    Some("grandchild workspace".to_string()),
    None,
    Some(&child.branch_name),
    None,
    None,
  )
  .expect("Failed to create grandchild workspace");

  let root_path = repo.workspaces_dir().join(&root.workspace_path);
  let child_path = repo.workspaces_dir().join(&child.workspace_path);
  let grandchild_path = repo.workspaces_dir().join(&grandchild.workspace_path);

  TestRepo::write_workspace_file(
    root_path.to_str().expect("root path should be utf-8"),
    "root.txt",
    "root dirty",
  )
  .expect("Failed to dirty root workspace");
  TestRepo::write_workspace_file(
    child_path.to_str().expect("child path should be utf-8"),
    "child.txt",
    "child dirty",
  )
  .expect("Failed to dirty child workspace");
  TestRepo::write_workspace_file(
    grandchild_path
      .to_str()
      .expect("grandchild path should be utf-8"),
    "grandchild.txt",
    "grandchild dirty",
  )
  .expect("Failed to dirty grandchild workspace");

  let result =
    treq_lib::core::list_commits(&repo.repo_path, Some(grandchild.id), false, None, None)
      .expect("Failed to list stacked workspace commits");

  assert_eq!(
    result
      .tentative_working_copy
      .as_ref()
      .expect("Grandchild should have a tentative working copy")
      .workspace_label,
    grandchild.branch_name
  );

  let stacked_labels: Vec<String> = std::iter::once(
    result
      .tentative_working_copy
      .as_ref()
      .expect("Grandchild should have a tentative working copy")
      .workspace_label
      .clone(),
  )
  .chain(
    result
      .commits
      .iter()
      .filter(|c| c.is_working_copy)
      .map(|c| {
        c.workspace_label
          .clone()
          .expect("stacked working-copy rows should be labeled")
      }),
  )
  .collect();

  assert_eq!(
    stacked_labels,
    vec![grandchild.branch_name, child.branch_name, root.branch_name],
    "Stacked working copies should be ordered from terminal workspace upward"
  );
}

#[test]
fn test_list_commits_skips_clean_branches_in_stacked_chain() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let root = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/root-clean",
    Some("root clean workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create root workspace");
  let child = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/child-clean",
    Some("child clean workspace".to_string()),
    None,
    Some(&root.branch_name),
    None,
    None,
  )
  .expect("Failed to create child workspace");
  let grandchild = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/grandchild-clean",
    Some("grandchild clean workspace".to_string()),
    None,
    Some(&child.branch_name),
    None,
    None,
  )
  .expect("Failed to create grandchild workspace");

  let root_path = repo.workspaces_dir().join(&root.workspace_path);
  let grandchild_path = repo.workspaces_dir().join(&grandchild.workspace_path);

  TestRepo::write_workspace_file(
    root_path.to_str().expect("root path should be utf-8"),
    "root.txt",
    "root dirty",
  )
  .expect("Failed to dirty root workspace");
  TestRepo::write_workspace_file(
    grandchild_path
      .to_str()
      .expect("grandchild path should be utf-8"),
    "grandchild.txt",
    "grandchild dirty",
  )
  .expect("Failed to dirty grandchild workspace");

  let result =
    treq_lib::core::list_commits(&repo.repo_path, Some(grandchild.id), false, None, None)
      .expect("Failed to list stacked workspace commits");

  let stacked_labels: Vec<String> = std::iter::once(
    result
      .tentative_working_copy
      .as_ref()
      .expect("Grandchild should have a tentative working copy")
      .workspace_label
      .clone(),
  )
  .chain(
    result
      .commits
      .iter()
      .filter(|c| c.is_working_copy)
      .map(|c| {
        c.workspace_label
          .clone()
          .expect("stacked working-copy rows should be labeled")
      }),
  )
  .collect();

  assert_eq!(
    stacked_labels,
    vec![grandchild.branch_name, root.branch_name],
    "Clean intermediate branches should be skipped without breaking ancestry traversal"
  );
}

#[test]
fn test_list_commits_non_stacked_workspace_keeps_single_working_copy() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/solo-working-copy",
    Some("solo workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  TestRepo::write_workspace_file(
    workspace_path
      .to_str()
      .expect("workspace path should be utf-8"),
    "solo.txt",
    "dirty",
  )
  .expect("Failed to dirty workspace");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list workspace commits");

  assert_eq!(
    result
      .tentative_working_copy
      .as_ref()
      .expect("Single workspace should still expose a tentative working copy")
      .workspace_label,
    workspace.branch_name
  );
  assert!(
    result.commits.iter().all(|c| !c.is_working_copy),
    "Non-stacked workspace should not surface additional working-copy rows"
  );
}

#[test]
fn test_move_commit_to_existing_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  // Create source workspace
  let source = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/source",
    Some("source workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create source workspace");

  let source_path = repo.workspaces_dir().join(&source.workspace_path);
  let source_path_str = source_path.to_str().unwrap();

  // Add a file to the source workspace and commit it
  TestRepo::write_workspace_file(source_path_str, "moved.txt", "moved content")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, source.id, "Commit to move")
    .expect("Failed to commit");

  // Get the change_id of the committed change
  let commits_ahead = treq_lib::jj::jj_get_commits_ahead(source_path_str, default_branch)
    .expect("Failed to get commits ahead");
  assert!(
    !commits_ahead.commits.is_empty(),
    "Should have at least 1 commit ahead of main"
  );
  let change_id = commits_ahead.commits.last().unwrap().change_id.clone();

  // Create target workspace
  let target = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/target",
    Some("target workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create target workspace");

  let target_path = repo.workspaces_dir().join(&target.workspace_path);

  // Move commit to existing workspace
  treq_lib::core::move_commit_to_existing_workspace(
    &repo.repo_path,
    source.id,
    &change_id,
    target.id,
  )
  .expect("Failed to move commit to existing workspace");

  // Verify target workspace has the file from the moved commit
  assert!(
    target_path.join("moved.txt").exists(),
    "moved.txt should exist in target workspace after commit was moved"
  );
}

#[test]
fn test_abandon_commit() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  // Create workspace
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/abandon-test",
    Some("abandon test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  // Add a file and commit
  TestRepo::write_workspace_file(workspace_path_str, "abandon-me.txt", "content")
    .expect("Failed to write");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Commit to abandon")
    .expect("Failed to commit");

  // Get the change_id
  let commits_ahead = treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch)
    .expect("Failed to get commits ahead");
  assert!(
    !commits_ahead.commits.is_empty(),
    "Should have at least 1 commit"
  );
  let change_id = commits_ahead.commits.last().unwrap().change_id.clone();

  // Abandon the commit
  treq_lib::core::abandon_commit(&repo.repo_path, workspace.id, &change_id)
    .expect("Failed to abandon commit");

  // Verify commit is gone
  let commits_after = treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch)
    .expect("Failed to get commits after abandon");
  assert!(
    commits_after.commits.is_empty(),
    "Commit should be abandoned, but found {} commits",
    commits_after.commits.len()
  );

  // Verify file from abandoned commit is no longer in workspace
  assert!(
    !workspace_path.join("abandon-me.txt").exists(),
    "abandon-me.txt should not exist after commit was abandoned"
  );
}

#[test]
fn test_undo_abandon_commit_restores_commit_and_files() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/undo-abandon",
    Some("undo abandon".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "undo-abandon.txt", "keep me")
    .expect("Failed to write");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Commit to undo-abandon")
    .expect("Failed to commit");

  let commits_ahead = treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch)
    .expect("Failed to get commits ahead");
  let change_id = commits_ahead.commits.last().unwrap().change_id.clone();

  let op_id = treq_lib::core::abandon_commit(&repo.repo_path, workspace.id, &change_id)
    .expect("Failed to abandon commit");
  assert!(
    !op_id.is_empty(),
    "abandon_commit should return the operation id"
  );
  assert!(
    !workspace_path.join("undo-abandon.txt").exists(),
    "file should be gone after abandon"
  );

  treq_lib::core::undo_repo_operation(&repo.repo_path, Some(workspace.id), &op_id)
    .expect("undo should restore the abandoned commit");

  let commits_restored = treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch)
    .expect("Failed to get commits after undo");
  assert_eq!(
    commits_restored.commits.len(),
    1,
    "abandoned commit should be restored"
  );
  assert!(
    workspace_path.join("undo-abandon.txt").exists(),
    "undo-abandon.txt should exist after undo"
  );
}

#[test]
fn test_undo_repo_operation_rejects_stale_operation() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stale-undo",
    Some("stale undo".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();
  TestRepo::write_workspace_file(workspace_path_str, "stale.txt", "one").expect("Failed to write");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "first")
    .expect("Failed to commit");

  let default_branch = repo.default_branch();
  let commits =
    treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch).expect("commits");
  let change_id = commits.commits.last().unwrap().change_id.clone();
  let op_id =
    treq_lib::core::abandon_commit(&repo.repo_path, workspace.id, &change_id).expect("abandon");

  TestRepo::write_workspace_file(workspace_path_str, "later.txt", "two")
    .expect("Failed to write later file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "after abandon")
    .expect("Failed to commit after abandon");

  let err = treq_lib::core::undo_repo_operation(&repo.repo_path, Some(workspace.id), &op_id)
    .expect_err("undo of a stale operation should fail");
  assert!(
    err.to_lowercase().contains("changed") || err.to_lowercase().contains("since"),
    "unexpected error: {err}"
  );
}

#[test]
fn test_undo_commit_only_allows_latest_commit_sequentially() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/undo-test",
    Some("undo test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "first.txt", "first")
    .expect("Failed to write first.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "First commit")
    .expect("Failed to commit first change");

  TestRepo::write_workspace_file(workspace_path_str, "second.txt", "second")
    .expect("Failed to write second.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Second commit")
    .expect("Failed to commit second change");

  let commits_ahead = treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch)
    .expect("Failed to get commits ahead");
  assert_eq!(
    commits_ahead.commits.len(),
    2,
    "Should have 2 commits ahead"
  );
  let newest_change_id = commits_ahead.commits[0].change_id.clone();
  let oldest_change_id = commits_ahead.commits[1].change_id.clone();

  // Cannot undo an older commit while a newer commit still sits on top of it.
  let err = treq_lib::core::undo_commit(&repo.repo_path, workspace.id, &oldest_change_id)
    .expect_err("Should not allow undoing a non-tip commit");
  assert!(
    err.contains("Only the latest commit"),
    "unexpected error: {}",
    err
  );

  // Undoing the tip commit succeeds.
  treq_lib::core::undo_commit(&repo.repo_path, workspace.id, &newest_change_id)
    .expect("Failed to undo tip commit");

  let commits_after = treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch)
    .expect("Failed to get commits after undo");
  assert_eq!(commits_after.commits.len(), 1);
  assert_eq!(commits_after.commits[0].change_id, oldest_change_id);
  assert!(!workspace_path.join("second.txt").exists());

  // The now-tip commit (previously the older one) can be undone in turn.
  treq_lib::core::undo_commit(&repo.repo_path, workspace.id, &oldest_change_id)
    .expect("Failed to undo the now-latest commit");

  let commits_final = treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch)
    .expect("Failed to get commits after second undo");
  assert!(commits_final.commits.is_empty());
  assert!(!workspace_path.join("first.txt").exists());
}

#[test]
fn test_undo_commit_rejects_commit_on_target_branch() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/undo-target-test",
    Some("undo target test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  // No new commits made in this workspace — its tip is the target branch tip itself.
  let err = treq_lib::core::undo_commit(&repo.repo_path, workspace.id, default_branch)
    .expect_err("Should not allow undoing a commit that belongs to the target branch");
  assert!(err.contains("default branch"), "unexpected error: {}", err);
}

#[test]
fn test_revert_commit_creates_backout_commit() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/revert-test",
    Some("revert test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "revert-me.txt", "hello")
    .expect("Failed to write revert-me.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add revert-me")
    .expect("Failed to commit");

  let commits_ahead = treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch)
    .expect("Failed to get commits ahead");
  let change_id = commits_ahead.commits[0].change_id.clone();

  treq_lib::core::revert_commit(&repo.repo_path, workspace.id, &change_id)
    .expect("Failed to revert commit");

  let commits_after = treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch)
    .expect("Failed to get commits after revert");
  assert_eq!(
    commits_after.commits.len(),
    2,
    "Should have the original commit plus the new revert commit"
  );
  assert!(
    commits_after.commits[0]
      .description
      .starts_with("Revert \"Add revert-me\""),
    "unexpected description: {}",
    commits_after.commits[0].description
  );

  assert!(
    !workspace_path.join("revert-me.txt").exists(),
    "revert-me.txt should be removed by the revert commit"
  );
}

#[test]
fn test_revert_commit_rejects_working_copy() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/revert-wc-test",
    Some("revert wc test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let err = treq_lib::core::revert_commit(&repo.repo_path, workspace.id, "@")
    .expect_err("Should not allow reverting the working copy");
  assert!(
    err.contains("Cannot revert the working copy"),
    "unexpected error: {}",
    err
  );
}

#[test]
fn test_commit_diff_added_files() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/commit-diff-add",
    Some("commit diff add test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  // Create files and commit
  TestRepo::write_workspace_file(workspace_path_str, "hello.txt", "hello world\n")
    .expect("Failed to write file");
  TestRepo::write_workspace_file(workspace_path_str, "foo.txt", "foo\nbar\nbaz\n")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add two files")
    .expect("Failed to commit");

  // Get the commit's change_id from the log
  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");
  let committed = log
    .commits
    .iter()
    .find(|c| !c.is_working_copy && c.description == "Add two files")
    .expect("Should include commit 'Add two files'");
  let change_id = &committed.change_id;

  // Call get_commit_diff
  let diff = treq_lib::core::get_commit_diff_with_conflict_style(
    &repo.repo_path,
    Some(workspace.id),
    change_id,
    "git",
  )
  .expect("Failed to get commit diff");

  // Should have 2 files in the summary
  assert_eq!(
    diff.committed_files.len(),
    2,
    "Should have 2 changed files, got {}",
    diff.committed_files.len()
  );

  // All files should be Added
  for file in &diff.committed_files {
    assert_eq!(
      file.status, "A",
      "File {} should have status 'A', got '{}'",
      file.path, file.status
    );
  }

  // Should have hunks for both files
  assert_eq!(
    diff.hunks_by_file.len(),
    2,
    "Should have hunks for 2 files, got {}",
    diff.hunks_by_file.len()
  );

  // Each file's hunks should have content
  for file_diff in &diff.hunks_by_file {
    assert!(
      !file_diff.hunks.is_empty(),
      "File {} should have at least one hunk",
      file_diff.path
    );
  }
}

#[test]
fn test_commit_diff_modified_files() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/commit-diff-mod",
    Some("commit diff mod test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  // First commit: create a file
  TestRepo::write_workspace_file(workspace_path_str, "data.txt", "line 1\nline 2\nline 3\n")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Create data file")
    .expect("Failed to commit");

  // Second commit: modify the file
  TestRepo::write_workspace_file(
    workspace_path_str,
    "data.txt",
    "line 1\nchanged line 2\nline 3\nnew line 4\n",
  )
  .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Modify data file")
    .expect("Failed to commit");

  // Get commits
  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");
  // Find the modification commit
  let mod_commit = log
    .commits
    .iter()
    .find(|c| c.description == "Modify data file")
    .expect("Should find modification commit");

  let diff = treq_lib::core::get_commit_diff_with_conflict_style(
    &repo.repo_path,
    Some(workspace.id),
    &mod_commit.change_id,
    "git",
  )
  .expect("Failed to get commit diff");

  // Should have 1 modified file
  assert_eq!(diff.committed_files.len(), 1);
  assert_eq!(diff.committed_files[0].status, "M");
  assert_eq!(diff.committed_files[0].path, "data.txt");

  // Should have hunks
  assert_eq!(diff.hunks_by_file.len(), 1);
  assert!(!diff.hunks_by_file[0].hunks.is_empty());
}

#[test]
fn test_commit_diff_deleted_files() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/commit-diff-del",
    Some("commit diff del test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  // First commit: create a file
  TestRepo::write_workspace_file(workspace_path_str, "temp.txt", "temporary content\n")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add temp file")
    .expect("Failed to commit");

  // Second commit: delete the file
  TestRepo::remove_file_path(workspace_path.join("temp.txt")).expect("Failed to delete file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Delete temp file")
    .expect("Failed to commit");

  // Get commits
  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");
  let committed: Vec<_> = log.commits.iter().filter(|c| !c.is_working_copy).collect();

  let del_commit = committed
    .iter()
    .find(|c| c.description == "Delete temp file")
    .expect("Should find deletion commit");

  let diff = treq_lib::core::get_commit_diff_with_conflict_style(
    &repo.repo_path,
    Some(workspace.id),
    &del_commit.change_id,
    "git",
  )
  .expect("Failed to get commit diff");

  // Should have 1 deleted file
  assert_eq!(diff.committed_files.len(), 1);
  assert_eq!(diff.committed_files[0].status, "D");
  assert_eq!(diff.committed_files[0].path, "temp.txt");

  // Should have hunks showing deletion
  assert_eq!(diff.hunks_by_file.len(), 1);
  assert!(!diff.hunks_by_file[0].hunks.is_empty());
}

#[test]
fn test_commit_diff_defers_large_file_diffs() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/commit-diff-large",
    Some("commit diff large test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  let large_diff = (1..=501)
    .map(|idx| format!("large diff line {}", idx))
    .collect::<Vec<_>>()
    .join("\n")
    + "\n";
  TestRepo::write_workspace_file(workspace_path_str, "large.txt", &large_diff)
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add large diff")
    .expect("Failed to commit");

  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");
  let large_commit = log
    .commits
    .iter()
    .find(|commit| commit.description == "Add large diff")
    .expect("Should find large diff commit");

  let diff = treq_lib::core::get_commit_diff_with_conflict_style(
    &repo.repo_path,
    Some(workspace.id),
    &large_commit.change_id,
    "git",
  )
  .expect("Failed to get commit diff");

  assert_eq!(diff.committed_files.len(), 1);
  assert_eq!(diff.committed_files[0].path, "large.txt");
  assert!(diff.committed_files[0].diff_deferred);
  assert_eq!(diff.committed_files[0].changed_line_count, 501);
  assert!(
    diff
      .hunks_by_file
      .iter()
      .all(|file_diff| file_diff.path != "large.txt"),
    "Large deferred file diff should not be returned in hunks_by_file"
  );
}

#[test]
fn test_commit_diff_blocks_rendering_when_commit_is_too_large() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/commit-diff-too-large",
    Some("commit diff too large test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  let huge_diff = (1..=10_001)
    .map(|idx| format!("huge diff line {}", idx))
    .collect::<Vec<_>>()
    .join("\n")
    + "\n";
  TestRepo::write_workspace_file(workspace_path_str, "huge.txt", &huge_diff)
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add huge diff")
    .expect("Failed to commit");

  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");
  let huge_commit = log
    .commits
    .iter()
    .find(|commit| commit.description == "Add huge diff")
    .expect("Should find huge diff commit");

  let diff = treq_lib::core::get_commit_diff_with_conflict_style(
    &repo.repo_path,
    Some(workspace.id),
    &huge_commit.change_id,
    "git",
  )
  .expect("Failed to get commit diff");

  assert!(diff.too_large_to_render);
  assert_eq!(diff.committed_files.len(), 0);
  assert_eq!(diff.hunks_by_file.len(), 0);
  assert_eq!(
    diff.render_block_reason.as_deref(),
    Some("This commit changes more than 10,000 lines and is too large to render.")
  );
}

#[test]
fn test_commit_file_diff_fetches_deferred_file_on_demand() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/commit-file-diff",
    Some("commit file diff test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  let large_diff = (1..=501)
    .map(|idx| format!("large diff line {}", idx))
    .collect::<Vec<_>>()
    .join("\n")
    + "\n";
  TestRepo::write_workspace_file(workspace_path_str, "large.txt", &large_diff)
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add deferred diff")
    .expect("Failed to commit");

  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");
  let deferred_commit = log
    .commits
    .iter()
    .find(|commit| commit.description == "Add deferred diff")
    .expect("Should find deferred diff commit");

  let file_diff = treq_lib::core::get_commit_file_diff_with_conflict_style(
    &repo.repo_path,
    Some(workspace.id),
    &deferred_commit.change_id,
    "large.txt",
    "git",
  )
  .expect("Failed to fetch deferred file diff");

  assert_eq!(file_diff.path, "large.txt");
  assert!(!file_diff.hunks.is_empty());
  assert!(
    file_diff
      .hunks
      .iter()
      .flat_map(|hunk| hunk.lines.iter())
      .any(|line| line.contains("large diff line 501")),
    "Deferred file diff should include the lazily fetched file contents"
  );
}

#[test]
fn test_commit_diff_invalid_change_id() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/commit-diff-invalid",
    Some("commit diff invalid test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  // Try with a change_id starting with '-' (injection attempt)
  let result = treq_lib::core::get_commit_diff_with_conflict_style(
    &repo.repo_path,
    Some(workspace.id),
    "-r malicious",
    "git",
  );
  assert!(result.is_err(), "Should reject change_id starting with '-'");

  // Try with empty change_id
  let result = treq_lib::core::get_commit_diff_with_conflict_style(
    &repo.repo_path,
    Some(workspace.id),
    "",
    "git",
  );
  assert!(result.is_err(), "Should reject empty change_id");
}

#[test]
fn test_list_commits() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/list-commits",
    Some("list commits test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path
    .to_str()
    .expect("workspace path should be utf-8");

  // First commit
  TestRepo::write_workspace_file(workspace_path_str, "hello.txt", "hello\n")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add hello")
    .expect("Failed to commit");

  // Second commit
  TestRepo::write_workspace_file(workspace_path_str, "world.txt", "world\n")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add world")
    .expect("Failed to commit");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");

  // Should have both committed workspace changes
  let committed: Vec<_> = result
    .commits
    .iter()
    .filter(|c| {
      !c.is_working_copy && (c.description == "Add hello" || c.description == "Add world")
    })
    .collect();
  assert_eq!(
    committed.len(),
    2,
    "Should have 2 non-working-copy commits, got {}",
    committed.len()
  );

  // Verify commit descriptions are present
  let descriptions: Vec<&str> = committed.iter().map(|c| c.description.as_str()).collect();
  assert!(
    descriptions.contains(&"Add hello"),
    "Should contain 'Add hello' commit"
  );
  assert!(
    descriptions.contains(&"Add world"),
    "Should contain 'Add world' commit"
  );

  // Should exclude working copy commits
  assert_eq!(
    result.commits.iter().filter(|c| c.is_working_copy).count(),
    0,
    "Should have no working copy commits"
  );
}

#[test]
fn test_stacked_working_copy_row_tracks_edits_and_caches_in_the_repo() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let root = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stats-root",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create root workspace");
  let child = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stats-child",
    None,
    None,
    Some(&root.branch_name),
    None,
    None,
  )
  .expect("Failed to create child workspace");
  let root_path = repo.workspaces_dir().join(&root.workspace_path);
  let root_path_str = root_path.to_str().expect("root path should be utf-8");
  let stacked_row = || {
    treq_lib::core::list_commits(&repo.repo_path, Some(child.id), false, None, None)
      .expect("Failed to list stacked workspace commits")
      .commits
      .into_iter()
      .find(|c| c.workspace_label.as_deref() == Some(root.branch_name.as_str()))
  };

  assert!(
    stacked_row().is_none(),
    "a clean lower workspace has no row"
  );

  TestRepo::write_workspace_file(root_path_str, "root.txt", "one\n").expect("write");
  let first = stacked_row().expect("a dirty lower workspace has a row");
  assert_eq!((first.insertions, first.deletions), (1, 0));
  assert_eq!(
    stacked_row().map(|c| c.insertions),
    Some(1),
    "reads again the same"
  );

  TestRepo::write_workspace_file(root_path_str, "root.txt", "one\ntwo\nthree\n").expect("write");
  let edited = stacked_row().expect("still dirty");
  assert_eq!((edited.insertions, edited.deletions), (3, 0));
  assert_ne!(edited.commit_id, first.commit_id);

  assert!(
    !root_path.join(".treq").join("local.db").exists(),
    "the diff stat cache is the repository's, not one inside the workspace"
  );
}
