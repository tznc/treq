use crate::common::{JjVerifier, TestRepo};
use std::path::Path;
use treq_lib::core::{commit_repo, MergeCommit};
use treq_lib::local_db::Workspace;

// TODO: rolling merge for stacked workspaces

// TODO: merge individual workspace into another workspace

#[test]
fn test_can_rebase_and_merge_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feature-rebase",
    Some("rebasing feature".to_string()),
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

  // Record this change on `main` at the home repo (not `commit_workspace`), so the home
  // log includes it as a main-branch commit before the workspace feature work.
  repo
    .create_file("home_file.txt", "home content")
    .expect("Failed to write home file");
  commit_repo(&repo.repo_path, "Add home file").expect("Failed to commit home file on main");
  let commits = treq_lib::core::list_commits(&repo.repo_path, None, false, None, None)
    .expect("Failed to list commits");

  assert_eq!(
    commits.commits.len(),
    2,
    "should have 2 commits, the initial commit and the new commit"
  );

  // write a commit to workspace
  TestRepo::write_workspace_file(
    workspace_path_str,
    "rebase-feature.txt",
    "rebase feature content",
  )
  .expect("Failed to write feature file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add rebase feature")
    .expect("Failed to commit");

  // merge the workspace into the home repo
  treq_lib::core::merge_workspace(
    &repo.repo_path,
    workspace.id,
    "Rebase feature-rebase onto main",
    MergeCommit::RebaseAndMerge,
  )
  .expect("Failed to rebase merge workspace");

  let main_feature_file = Path::new(&repo.repo_path).join("rebase-feature.txt");
  assert!(
    main_feature_file.exists(),
    "Feature file should exist in main repo after rebase merge"
  );

  // verify that workspace is in git log
  let git_log_str = TestRepo::run_git(&repo.repo_path, &["log", "-1", "--pretty=%s"])
    .expect("Failed to run git log");
  assert_eq!(
    git_log_str.trim(),
    "Rebase feature-rebase onto main",
    "Git log head should be the rebase-merge commit message, got: {}",
    git_log_str
  );

  // verify that workspace directory is deleted
  assert!(
    !workspace_path.exists(),
    "Workspace directory should be deleted after rebase merge"
  );
  let workspaces =
    treq_lib::core::list_workspaces(&repo.repo_path).expect("Failed to list workspaces");
  assert!(
    !workspaces.iter().any(|w| w.id == workspace.id),
    "Rebased workspace should not appear in list_workspaces"
  );

  // verify that commit list order is respected (newest first; no working-copy row).
  let commits = treq_lib::core::list_commits(&repo.repo_path, None, false, None, None)
    .expect("Failed to list commits");
  let descriptions: Vec<&str> = commits
    .commits
    .iter()
    .map(|c| c.description.as_str())
    .collect();
  assert_eq!(
    descriptions.len(),
    4,
    "Commit list should contain four commits, got {:?}",
    descriptions
  );
  assert!(
    !commits.commits.iter().any(|c| c.is_working_copy),
    "Home-repo commit list should not include a working-copy row, got: {:?}",
    commits
      .commits
      .iter()
      .map(|c| (c.description.as_str(), c.is_working_copy))
      .collect::<Vec<_>>()
  );

  assert_eq!(
    commits.commits[0].description, "Rebase feature-rebase onto main",
    "Tip should be the rebase-merge commit"
  );
  assert_eq!(
    commits.commits[1].description, "Add rebase feature",
    "Then the rebased feature commit"
  );
  assert_eq!(
    commits.commits[2].description, "Add home file",
    "Then the home-repo commit from before the feature work"
  );
  assert_eq!(
    commits.commits[3].description, "Initial commit",
    "Root should be the initial commit"
  );
}

#[test]
fn test_can_merge_workspace_into_home_repo_with_merge_commit_strategy() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Create workspace
  let workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feature-merge",
    Some("merging feature".to_string()),
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

  // Add a file to the workspace and commit
  TestRepo::write_workspace_file(
    workspace_path_str,
    "merge-feature.txt",
    "merge feature content",
  )
  .expect("Failed to write feature file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add merge feature")
    .expect("Failed to commit");

  // Get workspace status - should show commits ahead of target
  let status = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("Failed to get workspace status");
  assert_eq!(
    status.commits_ahead_of_target.len(),
    1,
    "Status should show 1 commit ahead of target"
  );
  assert!(
    !status.commits_ahead_of_target[0].timestamp.is_empty(),
    "Commit timestamp should be a non-empty string"
  );
  assert_ne!(
    status.commits_ahead_of_target[0].timestamp, "NaN",
    "Commit timestamp should not be NaN"
  );

  assert!(
    !status.commits_ahead_of_target[0].hash.is_empty(),
    "Commit hash should be a non-empty string"
  );
  assert!(
    !status.commits_ahead_of_target[0].message.is_empty(),
    "Commit message should be a non-empty string"
  );

  // Get the current branch before merge (should be on main)
  let initial_branch_str =
    TestRepo::run_git(&repo.repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
      .expect("Failed to get current branch")
      .trim()
      .to_string();

  // Perform the merge
  treq_lib::core::merge_workspace(
    &repo.repo_path,
    workspace.id,
    "Merge feature-merge into main",
    MergeCommit::Merge,
  )
  .expect("Failed to merge workspace");

  // Verify the file is now present in the main repo
  let main_feature_file = Path::new(&repo.repo_path).join("merge-feature.txt");
  assert!(
    main_feature_file.exists(),
    "Feature file should exist in main repo after merge"
  );

  // Verify jj picks up the merge commit
  let log = JjVerifier::get_log(&repo.repo_path, 5).expect("Failed to get jj log");
  assert!(
    log
      .iter()
      .any(|(desc, _)| desc.contains("Merge feature-merge into main") || desc.contains("merge")),
    "JJ log should contain merge commit, got: {:?}",
    log
  );

  // Verify git picks up the merge commit
  let git_log_str =
    TestRepo::run_git(&repo.repo_path, &["log", "--oneline", "-5"]).expect("Failed to run git log");
  assert!(
    git_log_str.contains("Merge") || git_log_str.contains("merge"),
    "Git log should contain merge commit, got: {}",
    git_log_str
  );

  // Verify workspace directory is deleted
  assert!(
    !workspace_path.exists(),
    "Workspace directory should be deleted after merge"
  );

  // Verify workspace is removed from database (not in list_workspaces)
  let workspaces =
    treq_lib::core::list_workspaces(&repo.repo_path).expect("Failed to list workspaces");
  assert!(
    !workspaces.iter().any(|w| w.id == workspace.id),
    "Merged workspace should not appear in list_workspaces"
  );

  // Verify workspace is not in jj workspace list
  let jj_workspaces =
    JjVerifier::list_workspaces(&repo.repo_path).expect("Failed to list jj workspaces");
  assert!(
    !jj_workspaces.contains(&workspace.workspace_name),
    "Merged workspace should not appear in jj workspace list, got: {:?}",
    jj_workspaces
  );

  // Verify home repo is correctly checked out to the branch it was at prior to the merge
  // and is not in detached HEAD state
  let final_branch_str = TestRepo::run_git(&repo.repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
    .expect("Failed to get current branch after merge")
    .trim()
    .to_string();

  assert_eq!(
    final_branch_str, initial_branch_str,
    "Home repo should remain on the same branch after merge, was on '{}', now on '{}'",
    initial_branch_str, final_branch_str
  );

  assert!(
    final_branch_str != "HEAD",
    "Home repo should not be in detached HEAD state after merge"
  );
}

#[test]
fn test_can_squash_and_merge_workspace_into_home_repo() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feature-squash",
    Some("squashing feature".to_string()),
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

  TestRepo::write_workspace_file(
    workspace_path_str,
    "squash-feature.txt",
    "squash feature content",
  )
  .expect("Failed to write feature file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add squash feature")
    .expect("Failed to commit");

  // edit the squash-feature.txt file

  TestRepo::write_workspace_file(
    workspace_path_str,
    "squash-feature.txt",
    "squash feature content 2",
  )
  .expect("Failed to write feature file");

  // make another commit to the workspace
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add squash feature 2")
    .expect("Failed to commit");

  treq_lib::core::merge_workspace(
    &repo.repo_path,
    workspace.id,
    "Squash feature-squash into main",
    MergeCommit::SquashAndMerge,
  )
  .expect("Failed to squash and merge workspace");

  let main_feature_file = Path::new(&repo.repo_path).join("squash-feature.txt");
  assert!(
    main_feature_file.exists(),
    "Feature file should exist in main repo after squash merge"
  );
  let main_feature_content =
    std::fs::read_to_string(&main_feature_file).expect("Failed to read squashed feature file");
  assert_eq!(
    main_feature_content, "squash feature content 2",
    "Squash merge should preserve the final workspace file contents"
  );

  let git_log_str = TestRepo::run_git(&repo.repo_path, &["log", "-1", "--pretty=%s"])
    .expect("Failed to run git log");
  assert_eq!(
    git_log_str.trim(),
    "Squash feature-squash into main",
    "Git log should contain the squash commit message, got: {}",
    git_log_str
  );

  assert!(
    !workspace_path.exists(),
    "Workspace directory should be deleted after squash merge"
  );

  let workspaces =
    treq_lib::core::list_workspaces(&repo.repo_path).expect("Failed to list workspaces");
  assert!(
    !workspaces.iter().any(|w| w.id == workspace.id),
    "Squashed workspace should not appear in list_workspaces"
  );

  // GitHub-style squash merge adds one new commit on top of the existing main history.
  let commits = treq_lib::core::list_commits(&repo.repo_path, None, false, None, None)
    .expect("Failed to list commits");
  assert_eq!(
    commits.commits.len(),
    2,
    "Main repo should contain the initial commit plus one squashed merge commit"
  );
}

#[test]
fn merge_into_main_succeeds_when_main_plus_empty() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feature-main-plus-empty",
    Some("merge when main+ is empty".to_string()),
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

  TestRepo::write_workspace_file(workspace_path_str, "main-plus-empty.txt", "feature content")
    .expect("Failed to write feature file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add feature for main+ empty")
    .expect("Failed to commit workspace change");
  let main_before = treq_lib::jj::jj_get_commit_id(&repo.repo_path, default_branch)
    .expect("main should resolve before merge");

  treq_lib::jj::jj_create_merge_commit(
    workspace_path_str,
    &workspace.branch_name,
    default_branch,
    "Merge workspace when main+ is empty",
    "git",
  )
  .expect("Merge should succeed");

  let main_after =
    treq_lib::jj::jj_get_commit_id(&repo.repo_path, default_branch).expect("main should resolve");
  assert!(
    main_before != main_after,
    "main should move to the new merge commit"
  );
}

#[test]
fn merge_returns_clear_error_when_target_branch_missing() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feature-missing-target",
    Some("missing target branch".to_string()),
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
  TestRepo::write_workspace_file(workspace_path_str, "missing-target.txt", "content")
    .expect("Failed to write feature file");
  treq_lib::core::commit_workspace(
    &repo.repo_path,
    workspace.id,
    "Add feature for missing target test",
  )
  .expect("Failed to commit workspace change");

  let err = treq_lib::jj::jj_create_merge_commit(
    workspace_path_str,
    &workspace.branch_name,
    "definitely-missing-target-branch",
    "Merge with missing target branch",
    "git",
  )
  .expect_err("Merge should fail when target branch is missing");

  assert!(
    err
      .to_string()
      .contains("Target branch 'definitely-missing-target-branch' could not be resolved"),
    "Error should clearly mention unresolved target branch, got: {}",
    err
  );
}

#[test]
fn test_merge_removing_directory_later_moves_home_after_returning() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/deferred-home",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  TestRepo::write_workspace_file(
    workspace_path.to_str().unwrap(),
    "deferred.txt",
    "deferred content",
  )
  .expect("Failed to write feature file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Add deferred")
    .expect("Failed to commit");

  treq_lib::core::merge_workspace_removing_directory_later(
    &repo.repo_path,
    workspace.id,
    "Merge feat/deferred-home",
    MergeCommit::SquashAndMerge,
  )
  .expect("Failed to merge workspace");

  // The sidebar poll takes the repo's commit lock, so it waits for the home
  // checkout to move: once it returns, the merged file is there.
  treq_lib::core::list_workspace_statuses(&repo.repo_path).expect("poll after merge");
  assert!(
    Path::new(&repo.repo_path).join("deferred.txt").exists(),
    "home repo should be on the target branch once the poll returns"
  );
}
