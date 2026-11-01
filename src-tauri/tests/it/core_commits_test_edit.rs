use crate::common::TestRepo;

#[test]
fn test_describe_commit_updates_description() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/describe-test",
    Some("describe test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "describe-me.txt", "content")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Original message")
    .expect("Failed to commit");

  let commits_ahead = treq_lib::jj::jj_get_commits_ahead(workspace_path_str, default_branch)
    .expect("Failed to get commits ahead");
  assert!(
    !commits_ahead.commits.is_empty(),
    "Should have at least 1 commit"
  );
  let change_id = commits_ahead.commits.last().unwrap().change_id.clone();

  treq_lib::core::describe_commit(&repo.repo_path, workspace.id, &change_id, "Updated message")
    .expect("Failed to describe commit");

  let updated_description =
    treq_lib::core::get_commit_description(&repo.repo_path, workspace.id, &change_id)
      .expect("Failed to get commit description");
  assert_eq!(
    updated_description, "Updated message",
    "Description should reflect the new message"
  );
}

#[test]
fn test_get_commit_description_returns_full_multiline_description() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/multiline-describe",
    Some("multiline describe test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "multiline.txt", "content")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Summary line")
    .expect("Failed to commit");

  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");
  let committed = log
    .commits
    .iter()
    .find(|c| !c.is_working_copy && c.description == "Summary line")
    .expect("Should find committed change");
  let change_id = committed.change_id.clone();

  let multiline_message =
    "Summary line\n\nBody paragraph explaining the change.\nSecond body line.";
  treq_lib::core::describe_commit(&repo.repo_path, workspace.id, &change_id, multiline_message)
    .expect("Failed to describe commit with multiline message");

  let full_description =
    treq_lib::core::get_commit_description(&repo.repo_path, workspace.id, &change_id)
      .expect("Failed to get full commit description");
  assert_eq!(
    full_description, multiline_message,
    "get_commit_description should return the full, unmodified multiline description"
  );

  // list_commits only surfaces the first line of the description in its
  // `description` field, unlike get_commit_description.
  let log_after =
    treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
      .expect("Failed to list commits after describe");
  let updated_row = log_after
    .commits
    .iter()
    .find(|c| c.change_id == change_id)
    .expect("Should still find commit by change_id after describe");
  assert_eq!(
    updated_row.description, "Summary line",
    "list_commits should only expose the first line of the description"
  );
}

#[test]
fn test_describe_commit_preserves_descendants() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/describe-descendants",
    Some("describe descendants test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "parent.txt", "parent content")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Parent commit")
    .expect("Failed to commit");

  TestRepo::write_workspace_file(workspace_path_str, "child.txt", "child content")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Child commit")
    .expect("Failed to commit");

  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");
  let parent = log
    .commits
    .iter()
    .find(|c| !c.is_working_copy && c.description == "Parent commit")
    .expect("Should find parent commit");
  let change_id = parent.change_id.clone();

  treq_lib::core::describe_commit(
    &repo.repo_path,
    workspace.id,
    &change_id,
    "Renamed parent commit",
  )
  .expect("Failed to describe parent commit");

  // Both files should still exist in the workspace after the rewrite +
  // descendant rebase, proving the child commit survived intact.
  assert!(
    workspace_path.join("parent.txt").exists(),
    "parent.txt should still exist after describing the parent commit"
  );
  assert!(
    workspace_path.join("child.txt").exists(),
    "child.txt should still exist after describing the parent commit"
  );

  let log_after =
    treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
      .expect("Failed to list commits after describe");
  assert!(
    log_after
      .commits
      .iter()
      .any(|c| c.description == "Renamed parent commit"),
    "Renamed parent commit should appear in the log"
  );
  assert!(
    log_after
      .commits
      .iter()
      .any(|c| c.description == "Child commit"),
    "Child commit should still exist after parent was described"
  );
}

#[test]
fn test_describe_commit_invalid_change_id() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/describe-invalid",
    Some("describe invalid test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  // Try with a change_id starting with '-' (injection attempt)
  let result =
    treq_lib::core::describe_commit(&repo.repo_path, workspace.id, "-r malicious", "New message");
  assert!(result.is_err(), "Should reject change_id starting with '-'");

  // Try with empty change_id
  let result = treq_lib::core::get_commit_description(&repo.repo_path, workspace.id, "");
  assert!(result.is_err(), "Should reject empty change_id");
}

#[test]
fn test_describe_and_revert_commit_in_the_home_repo() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  TestRepo::write_workspace_file(&repo.repo_path, "home.txt", "content").expect("write file");
  treq_lib::jj::jj_commit(&repo.repo_path, "Home message").expect("commit in home");

  // No workspace id: jj runs in the repository itself.
  treq_lib::core::describe_commit(&repo.repo_path, None, "@-", "Home renamed")
    .expect("describe in home");
  assert_eq!(
    treq_lib::core::get_commit_description(&repo.repo_path, None, "@-").expect("read"),
    "Home renamed"
  );
  treq_lib::core::revert_commit(&repo.repo_path, None, "@-").expect("revert in home");
}

/// A home-repo commit already on `main@origin` is listed as immutable;
/// deleting or rewording it would rewrite published history.
#[test]
fn refuses_home_rewrite_of_pushed_commit() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let default_branch = repo.default_branch().to_string();
  repo
    .commit_file("pushed.txt", "pushed\n", "Pushed commit")
    .expect("commit");
  TestRepo::run_git(&repo.repo_path, &["push", "origin", &default_branch]).expect("push");
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("fetch");
  let pushed = TestRepo::jj_change_id(&repo.repo_path, "description(substring:\"Pushed commit\")")
    .expect("pushed change id");

  let describe = treq_lib::core::describe_commit(&repo.repo_path, None, &pushed, "rewritten");
  assert!(
    describe.is_err(),
    "describe should be refused: {describe:?}"
  );
  let abandon = treq_lib::core::abandon_commit(&repo.repo_path, None, &pushed);
  assert!(abandon.is_err(), "abandon should be refused: {abandon:?}");

  // An unpushed commit on top stays editable.
  TestRepo::write_workspace_file(&repo.repo_path, "local.txt", "local\n").expect("write file");
  treq_lib::jj::jj_commit(&repo.repo_path, "Local commit").expect("commit in home");
  treq_lib::core::describe_commit(&repo.repo_path, None, "@-", "Local renamed")
    .expect("unpushed commit stays editable");
}
