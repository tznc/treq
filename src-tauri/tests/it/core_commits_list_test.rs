use crate::common::TestRepo;

#[test]
fn test_list_commits_invalid_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(99999), false, None, None);
  assert!(
    result.is_err(),
    "Should return error for non-existent workspace"
  );
}

#[test]
fn test_list_commits_working_copy_diff_stats() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/wc-diff-stats",
    Some("working copy diff stats test".to_string()),
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

  // Write a file but don't commit — it stays in the working copy
  TestRepo::write_workspace_file(
    workspace_path_str,
    "new_file.txt",
    "line 1\nline 2\nline 3\nline 4\nline 5\n",
  )
  .expect("Failed to write file");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");

  let wc_commit = result.commits.iter().find(|c| c.is_working_copy);
  assert!(
    wc_commit.is_none(),
    "Should exclude working copy commits in the results"
  );
}

#[test]
fn test_list_commits_home_repo() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Create a file in the home repo to ensure there's a change
  repo
    .create_file("home_file.txt", "home content\n")
    .expect("Failed to write file");

  let result = treq_lib::core::list_commits(&repo.repo_path, None, false, None, None)
    .expect("Failed to list commits for home repo");

  // Should have at least 1 commit (the working copy or initial commits)
  assert!(
    !result.commits.is_empty(),
    "Should have at least 1 commit for home repo"
  );
}

#[test]
fn test_list_commits_home_repo_with_committed_changes() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Make a git commit on the home repo
  repo
    .commit_file(
      "committed_file.txt",
      "committed content\n",
      "Home repo commit",
    )
    .expect("Failed to create commit");

  let result = treq_lib::core::list_commits(&repo.repo_path, None, false, None, None)
    .expect("Failed to list commits for home repo");

  // Should include the committed change
  let committed: Vec<_> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy)
    .collect();
  assert!(
    !committed.is_empty(),
    "Should have at least 1 committed change in home repo"
  );

  let descriptions: Vec<&str> = committed.iter().map(|c| c.description.as_str()).collect();
  assert!(
    descriptions.contains(&"Home repo commit"),
    "Should contain 'Home repo commit', got: {:?}",
    descriptions
  );
}

#[test]
fn test_list_commits_workspace_after_home_repo_jj_commits() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  for idx in 0..13 {
    repo
      .commit_file(
        &format!("home_{}.txt", idx),
        &format!("home content {}\n", idx),
        &format!("Home commit {}", idx),
      )
      .expect("Failed to create home repo commit");
  }

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/home-regression",
    Some("home regression".to_string()),
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
  TestRepo::write_workspace_file(workspace_path_str, "workspace.txt", "workspace content\n")
    .expect("Failed to write workspace file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Workspace commit")
    .expect("Failed to commit workspace change");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, None, None)
    .expect("Workspace list_commits should succeed after home repo jj commits");

  let descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy)
    .map(|c| c.description.as_str())
    .collect();
  assert!(
    descriptions.contains(&"Workspace commit"),
    "Should include workspace commit, got: {:?}",
    descriptions
  );

  let expanded_result =
    treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, Some(20), None)
      .expect("Expanded target branch history should succeed");
  let target_descriptions: Vec<&str> = expanded_result
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();
  assert!(
    target_descriptions.contains(&"Home commit 0"),
    "Expanded target branch history should include oldest home commit, got: {:?}",
    target_descriptions,
  );
}

#[test]
fn list_commits_includes_history_when_workspace_is_cloned_from_remote_branch() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  repo
    .remote_commit_on_branch(
      "feature-remote",
      "remote-only.txt",
      "new content from the remote\n",
      "Remote-only feature commit",
    )
    .expect("Failed to advance remote branch");
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("Failed to fetch remote branch update");
  let workspace = repo
    .create_workspace_simple("feature-remote")
    .expect("Failed to clone remote branch into workspace");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list cloned remote branch commits");
  let descriptions: Vec<_> = result
    .commits
    .iter()
    .map(|commit| commit.description.as_str())
    .collect();

  assert!(
    descriptions.contains(&"Add feature.txt")
      && descriptions.contains(&"Remote-only feature commit"),
    "Remote branch history should belong to the workspace branch, got: {:?}",
    descriptions
  );

  let status = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("Failed to load cloned remote workspace status");
  assert!(
    status
      .commits_ahead_of_target
      .iter()
      .any(|commit| commit.message == "Add feature.txt")
      && status
        .commits_ahead_of_target
        .iter()
        .any(|commit| commit.message == "Remote-only feature commit"),
    "Linear history should include the cloned remote commit, got: {:?}",
    status.commits_ahead_of_target
  );
}

#[test]
fn list_commits_excludes_unsquashed_parent_after_stacked_base_is_squash_merged() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo");
  let base = repo
    .create_workspace_with_commit("feat/stack-base", "base-one.txt", "base one\n", None)
    .expect("Failed to create stack base");
  repo
    .commit_workspace_file(&base, "base-two.txt", "base two\n", "Base commit two")
    .expect("Failed to create second base commit");
  treq_lib::jj::jj_push(&repo.workspace_full_path(&base)).expect("Failed to push stack base");

  let child = repo
    .create_workspace_with_commit(
      "feat/stack-child",
      "child.txt",
      "child only\n",
      Some("feat/stack-base"),
    )
    .expect("Failed to create stacked child");

  let base_tree = TestRepo::run_git(&repo.repo_path, &["rev-parse", "feat/stack-base^{tree}"])
    .expect("Failed to resolve base tree");
  let parent = TestRepo::run_git(&repo.repo_path, &["rev-parse", repo.default_branch()])
    .expect("Failed to resolve default branch");
  let squash = TestRepo::run_git(
    &repo.repo_path,
    &[
      "commit-tree",
      base_tree.trim(),
      "-p",
      parent.trim(),
      "-m",
      "Squashed stack base",
    ],
  )
  .expect("Failed to create squash commit");
  TestRepo::run_git(
    &repo.repo_path,
    &[
      "update-ref",
      &format!("refs/heads/{}", repo.default_branch()),
      squash.trim(),
    ],
  )
  .expect("Failed to advance default branch");
  treq_lib::jj::jj_util_import_git_refs(&repo.repo_path).expect("Failed to import squash");
  treq_lib::core::delete_workspace(&repo.repo_path, &base.id)
    .expect("Failed to remove merged stack base");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(child.id), true, None, None)
    .expect("Failed to list stacked child commits");
  let workspace_descriptions: Vec<_> = result
    .commits
    .iter()
    .filter(|commit| !commit.on_target_only)
    .map(|commit| commit.description.as_str())
    .collect();

  assert_eq!(workspace_descriptions, vec!["Add child.txt"]);
  assert_eq!(
    result
      .commits
      .iter()
      .filter(|commit| commit.description == "Squashed stack base")
      .count(),
    1
  );
}

#[test]
fn list_commits_resolves_squash_merges_at_every_stack_level() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo");
  let level_one = repo
    .create_workspace_with_commit("feat/level-one", "one.txt", "one\n", None)
    .expect("Failed to create level one");
  let level_two = repo
    .create_workspace_with_commit("feat/level-two", "two.txt", "two\n", Some("feat/level-one"))
    .expect("Failed to create level two");
  let level_three = repo
    .create_workspace_with_commit(
      "feat/level-three",
      "three.txt",
      "three\n",
      Some("feat/level-two"),
    )
    .expect("Failed to create level three");

  for (level, message) in [
    (&level_one, "Squashed level one"),
    (&level_two, "Squashed level two"),
  ] {
    let level_tree = TestRepo::run_git(
      &repo.repo_path,
      &["rev-parse", &format!("{}^{{tree}}", level.branch_name)],
    )
    .expect("Failed to resolve level tree");
    let parent = TestRepo::run_git(&repo.repo_path, &["rev-parse", repo.default_branch()])
      .expect("Failed to resolve squash parent");
    let squash = TestRepo::run_git(
      &repo.repo_path,
      &[
        "commit-tree",
        level_tree.trim(),
        "-p",
        parent.trim(),
        "-m",
        message,
      ],
    )
    .expect("Failed to create squash commit");
    TestRepo::run_git(
      &repo.repo_path,
      &[
        "update-ref",
        &format!("refs/heads/{}", repo.default_branch()),
        squash.trim(),
      ],
    )
    .expect("Failed to advance default branch");
    treq_lib::jj::jj_util_import_git_refs(&repo.repo_path).expect("Failed to import squash");
    treq_lib::core::delete_workspace(&repo.repo_path, &level.id)
      .expect("Failed to remove merged stack level");
  }

  let result =
    treq_lib::core::list_commits(&repo.repo_path, Some(level_three.id), true, None, None)
      .expect("Failed to list level three commits");
  let workspace_descriptions: Vec<_> = result
    .commits
    .iter()
    .filter(|commit| !commit.on_target_only)
    .map(|commit| commit.description.as_str())
    .collect();

  assert_eq!(workspace_descriptions, vec!["Add three.txt"]);
  for message in ["Squashed level one", "Squashed level two"] {
    assert_eq!(
      result
        .commits
        .iter()
        .filter(|commit| commit.description == message)
        .count(),
      1,
      "{message} should appear exactly once"
    );
  }
}

#[test]
fn test_list_commits_with_target_branch_history() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Add commits to main (the base branch) BEFORE creating the workspace.
  repo
    .commit_file("base_file_1.txt", "base content 1\n", "Base commit 1")
    .expect("Failed to create base commit 1");
  repo
    .commit_file("base_file_2.txt", "base content 2\n", "Base commit 2")
    .expect("Failed to create base commit 2");

  // Create a workspace branching off current main
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/target-history",
    Some("target history test".to_string()),
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

  // Make a commit on the workspace branch
  TestRepo::write_workspace_file(workspace_path_str, "branch_file.txt", "branch content\n")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Branch commit")
    .expect("Failed to commit");

  // Call with include_target_branch_history=true
  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, None, None)
    .expect("Failed to list commits");

  let workspace_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy && !c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();
  let target_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();

  assert!(workspace_descriptions.contains(&"Branch commit"));
  assert!(
    !workspace_descriptions.contains(&"Base commit 1"),
    "workspace commits should not include target history, got: {:?}",
    workspace_descriptions
  );
  assert!(
    !workspace_descriptions.contains(&"Base commit 2"),
    "workspace commits should not include target history, got: {:?}",
    workspace_descriptions
  );

  assert!(
    !target_descriptions.is_empty(),
    "target-only commits should not be empty"
  );
  assert!(
    target_descriptions.contains(&"Base commit 1"),
    "target-only commits should contain 'Base commit 1', got: {:?}",
    target_descriptions
  );
  assert!(
    target_descriptions.contains(&"Base commit 2"),
    "target-only commits should contain 'Base commit 2', got: {:?}",
    target_descriptions
  );
}

#[test]
fn test_list_commits_keeps_workspace_and_target_histories_disjoint() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  repo
    .commit_file("base.txt", "base\n", "Base commit")
    .expect("Failed to create base commit");
  repo
    .commit_file("target-only.txt", "target only\n", "Target only commit")
    .expect("Failed to create target-only commit");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/disjoint-history",
    Some("disjoint history test".to_string()),
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
  TestRepo::write_workspace_file(workspace_path_str, "workspace-only.txt", "workspace only\n")
    .expect("Failed to write workspace file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Workspace only commit")
    .expect("Failed to commit workspace change");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, None, None)
    .expect("Failed to list commits");

  let workspace_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy && !c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();
  let target_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();

  assert!(
    workspace_descriptions.contains(&"Workspace only commit"),
    "workspace commits should include the workspace-only commit, got: {:?}",
    workspace_descriptions
  );
  assert!(
    !workspace_descriptions.contains(&"Target only commit"),
    "workspace commits should not include target-only commits, got: {:?}",
    workspace_descriptions
  );
  assert!(
    target_descriptions.contains(&"Target only commit"),
    "target-only commits should include the target-only commit, got: {:?}",
    target_descriptions
  );

  let workspace_commit_ids: std::collections::HashSet<&str> = result
    .commits
    .iter()
    .filter(|c| !c.on_target_only)
    .map(|c| c.commit_id.as_str())
    .collect();
  let target_commit_ids: std::collections::HashSet<&str> = result
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .map(|c| c.commit_id.as_str())
    .collect();
  let overlap: Vec<_> = workspace_commit_ids
    .intersection(&target_commit_ids)
    .copied()
    .collect();
  assert!(
    overlap.is_empty(),
    "workspace and target-only commits should be disjoint, overlap: {:?}",
    overlap
  );
}

#[test]
fn test_list_commits_target_branch_history_limits_to_10() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Create >10 commits on main before workspace
  for i in 1..=15 {
    repo
      .commit_file(
        &format!("file_{}.txt", i),
        &format!("content {}\n", i),
        &format!("Main commit {}", i),
      )
      .unwrap_or_else(|e| panic!("Failed to create commit {i}: {e}"));
  }

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/limit-test",
    Some("limit test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, None, None)
    .expect("Failed to list commits");

  let target_only_count = result.commits.iter().filter(|c| c.on_target_only).count();
  assert!(
    target_only_count <= 10,
    "target-only commits should be limited to 10, got {}",
    target_only_count
  );
}

#[test]
fn test_list_commits_without_target_branch_history() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  repo
    .commit_file("base.txt", "base\n", "Base commit")
    .expect("Failed to create base commit");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/no-history",
    Some("no history test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  repo
    .commit_workspace_file(
      &workspace,
      "workspace.txt",
      "workspace content\n",
      "Workspace commit",
    )
    .expect("Failed to commit workspace change");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");

  let workspace_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy && !c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();
  let target_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();

  assert!(
    workspace_descriptions.contains(&"Workspace commit"),
    "workspace commits should stay on the workspace side, got: {:?}",
    workspace_descriptions
  );
  assert!(
    target_descriptions.is_empty(),
    "target-only commits should be omitted when include_target_branch_history is false, got: {:?}",
    target_descriptions
  );
}

#[test]
fn test_list_commits_target_branch_history_uses_local_bookmark_only() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");

  // Create local-only advancement on main.
  repo
    .commit_file(
      "local_main_only.txt",
      "local-only main content\n",
      "Local main only commit",
    )
    .expect("Failed to create local main commit");

  // Create remote-only advancement on main so local `main` becomes conflicted after fetch.
  repo
    .remote_commit_file(
      "remote_main_only.txt",
      "remote-only main content\n",
      "Remote main only commit",
    )
    .expect("Failed to create remote main commit");
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("Failed to fetch");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/local-target-main",
    Some("local target history".to_string()),
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
    "workspace_only.txt",
    "workspace-only content\n",
  )
  .expect("Failed to write workspace file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Workspace local commit")
    .expect("Failed to commit workspace change");

  let result =
    treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, Some(30), None)
      .expect("list_commits should succeed");

  let target_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();

  assert!(
    target_descriptions.contains(&"Local main only commit"),
    "target-only history should include local main commit, got: {:?}",
    target_descriptions
  );
  assert!(
    !target_descriptions.contains(&"Remote main only commit"),
    "target branch history should not fall back to remote main@git, got: {:?}",
    target_descriptions
  );
}

#[test]
fn test_default_branch_workspace_returns_full_history() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  repo
    .commit_file("base_1.txt", "base 1\n", "Base commit 1")
    .expect("Failed to create base commit 1");
  repo
    .commit_file("base_2.txt", "base 2\n", "Base commit 2")
    .expect("Failed to create base commit 2");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    default_branch,
    Some("root main workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create main workspace");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, None, None)
    .expect("Failed to list commits");

  let committed_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy)
    .map(|c| c.description.as_str())
    .collect();

  assert!(
    committed_descriptions.contains(&"Base commit 1"),
    "default-branch workspace should include base history, got: {:?}",
    committed_descriptions
  );
  assert!(
    committed_descriptions.contains(&"Base commit 2"),
    "default-branch workspace should include base history, got: {:?}",
    committed_descriptions
  );
}

#[test]
fn test_non_default_workspace_includes_target_history() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  repo
    .commit_file("base_1.txt", "base 1\n", "Base commit 1")
    .expect("Failed to create base commit 1");
  repo
    .commit_file("base_2.txt", "base 2\n", "Base commit 2")
    .expect("Failed to create base commit 2");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/ahead-only",
    Some("ahead only workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  repo
    .commit_workspace_file(
      &workspace,
      "workspace_only.txt",
      "workspace\n",
      "Workspace commit",
    )
    .expect("Failed to commit workspace change");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, None, None)
    .expect("Failed to list commits");

  let workspace_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy && !c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();
  let target_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();

  assert!(
    workspace_descriptions.contains(&"Workspace commit"),
    "workspace history should include its own commit, got: {:?}",
    workspace_descriptions
  );
  assert!(
    !workspace_descriptions.contains(&"Base commit 1"),
    "workspace history should not include target branch history, got: {:?}",
    workspace_descriptions
  );
  assert!(
    !workspace_descriptions.contains(&"Base commit 2"),
    "workspace history should not include target branch history, got: {:?}",
    workspace_descriptions
  );
  assert!(
    target_descriptions.contains(&"Base commit 1"),
    "target-only commits should include base history, got: {:?}",
    target_descriptions
  );
  assert!(
    target_descriptions.contains(&"Base commit 2"),
    "target-only commits should include base history, got: {:?}",
    target_descriptions
  );
}

#[test]
fn test_non_default_workspace_empty_ahead_includes_target_history() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  repo
    .commit_file("base_1.txt", "base 1\n", "Base commit 1")
    .expect("Failed to create base commit 1");
  repo
    .commit_file("base_2.txt", "base 2\n", "Base commit 2")
    .expect("Failed to create base commit 2");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/no-ahead",
    Some("no ahead workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, None, None)
    .expect("Failed to list commits");

  let workspace_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy && !c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();
  let target_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();

  assert!(
    workspace_descriptions.is_empty() || workspace_descriptions.contains(&"Workspace commit"),
    "workspace commits should stay on the workspace side, got: {:?}",
    workspace_descriptions
  );
  assert!(
    target_descriptions.contains(&"Base commit 1"),
    "target-only commits should include base history, got: {:?}",
    target_descriptions
  );
  assert!(
    target_descriptions.contains(&"Base commit 2"),
    "target-only commits should include base history, got: {:?}",
    target_descriptions
  );
}

#[test]
fn test_non_default_workspace_combined_history_has_no_duplicate_commit_ids() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  repo
    .commit_file("base_1.txt", "base 1\n", "Base commit 1")
    .expect("Failed to create base commit 1");
  repo
    .commit_file("base_2.txt", "base 2\n", "Base commit 2")
    .expect("Failed to create base commit 2");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/dedupe",
    Some("dedupe workspace".to_string()),
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
  TestRepo::write_workspace_file(workspace_path_str, "workspace_only.txt", "workspace\n")
    .expect("Failed to write workspace file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Workspace commit")
    .expect("Failed to commit workspace change");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, None, None)
    .expect("Failed to list commits");

  let all_commit_ids: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy)
    .map(|c| c.commit_id.as_str())
    .collect();
  let unique_commit_ids: std::collections::HashSet<&str> = all_commit_ids.iter().copied().collect();

  assert_eq!(
    unique_commit_ids.len(),
    all_commit_ids.len(),
    "combined commits should have unique commit ids"
  );
}

#[test]
fn test_no_main_assumption_for_non_main_default() {
  let repo = TestRepo::new_without_init().expect("Failed to create test repo");

  TestRepo::run_git(&repo.repo_path, &["branch", "-m", "trunk"])
    .expect("Failed to rename default branch to trunk");
  TestRepo::run_git(&repo.repo_path, &["checkout", "trunk"])
    .expect("Failed to checkout trunk branch");

  let remote_path = repo.temp_dir.path().join("remote.git");
  TestRepo::ensure_dir(&remote_path).expect("Failed to create remote directory");
  TestRepo::run_git(&remote_path.to_string_lossy(), &["init", "--bare"])
    .expect("Failed to initialize bare remote");
  TestRepo::run_git(
    &repo.repo_path,
    &[
      "remote",
      "add",
      "origin",
      remote_path.to_str().expect("remote path should be utf-8"),
    ],
  )
  .expect("Failed to add origin remote");
  TestRepo::run_git(&repo.repo_path, &["push", "-u", "origin", "trunk"])
    .expect("Failed to push trunk to remote");
  TestRepo::run_git(&repo.repo_path, &["remote", "set-head", "origin", "trunk"])
    .expect("Failed to set origin HEAD to trunk");

  treq_lib::core::init(&repo.repo_path).expect("Failed to initialize treq");

  repo
    .commit_file("trunk_1.txt", "trunk 1\n", "Trunk commit 1")
    .expect("Failed to create trunk commit 1");
  repo
    .commit_file("trunk_2.txt", "trunk 2\n", "Trunk commit 2")
    .expect("Failed to create trunk commit 2");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "trunk",
    Some("root trunk workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create trunk workspace");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("Failed to list commits");

  let committed_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy)
    .map(|c| c.description.as_str())
    .collect();

  assert!(
    committed_descriptions.contains(&"Trunk commit 1"),
    "trunk default-branch workspace should include trunk history, got: {:?}",
    committed_descriptions
  );
  assert!(
    committed_descriptions.contains(&"Trunk commit 2"),
    "trunk default-branch workspace should include trunk history, got: {:?}",
    committed_descriptions
  );
}

#[test]
fn test_non_default_workspace_uses_non_main_default_target_history() {
  let repo = TestRepo::new_without_init().expect("Failed to create test repo");

  TestRepo::run_git(&repo.repo_path, &["branch", "-m", "trunk"])
    .expect("Failed to rename default branch to trunk");
  TestRepo::run_git(&repo.repo_path, &["checkout", "trunk"])
    .expect("Failed to checkout trunk branch");

  let remote_path = repo.temp_dir.path().join("remote.git");
  TestRepo::ensure_dir(&remote_path).expect("Failed to create remote directory");
  TestRepo::run_git(&remote_path.to_string_lossy(), &["init", "--bare"])
    .expect("Failed to initialize bare remote");
  TestRepo::run_git(
    &repo.repo_path,
    &[
      "remote",
      "add",
      "origin",
      remote_path.to_str().expect("remote path should be utf-8"),
    ],
  )
  .expect("Failed to add origin remote");
  TestRepo::run_git(&repo.repo_path, &["push", "-u", "origin", "trunk"])
    .expect("Failed to push trunk to remote");
  TestRepo::run_git(&repo.repo_path, &["remote", "set-head", "origin", "trunk"])
    .expect("Failed to set origin HEAD to trunk");

  treq_lib::core::init(&repo.repo_path).expect("Failed to initialize treq");

  repo
    .commit_file("trunk_1.txt", "trunk 1\n", "Trunk commit 1")
    .expect("Failed to create trunk commit 1");
  repo
    .commit_file("trunk_2.txt", "trunk 2\n", "Trunk commit 2")
    .expect("Failed to create trunk commit 2");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/trunk-target",
    Some("feature on trunk default".to_string()),
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
  TestRepo::write_workspace_file(workspace_path_str, "workspace_only.txt", "workspace\n")
    .expect("Failed to write workspace file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Workspace trunk commit")
    .expect("Failed to commit workspace change");

  let result = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), true, None, None)
    .expect("Failed to list commits");

  let workspace_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy && !c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();
  let target_descriptions: Vec<&str> = result
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .map(|c| c.description.as_str())
    .collect();

  assert!(
    workspace_descriptions.contains(&"Workspace trunk commit"),
    "workspace history should include its own commit, got: {:?}",
    workspace_descriptions
  );
  assert!(
    !workspace_descriptions.contains(&"Trunk commit 1"),
    "workspace history should not include trunk target history, got: {:?}",
    workspace_descriptions
  );
  assert!(
    !workspace_descriptions.contains(&"Trunk commit 2"),
    "workspace history should not include trunk target history, got: {:?}",
    workspace_descriptions
  );
  assert!(
    target_descriptions.contains(&"Trunk commit 1"),
    "target branch commits should include trunk history, got: {:?}",
    target_descriptions
  );
  assert!(
    target_descriptions.contains(&"Trunk commit 2"),
    "target branch commits should include trunk history, got: {:?}",
    target_descriptions
  );
}

#[test]
fn test_unpushed_commits_on_default_branch_are_mutable() {
  // Pushed commits should be immutable; local-only commits on the default branch
  // should be mutable so users can edit/abandon them.
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let default_branch = repo.default_branch();

  // Push a commit to establish the remote tip.
  repo
    .commit_file("pushed.txt", "pushed\n", "Pushed commit")
    .expect("Failed to create pushed commit");
  TestRepo::run_git(&repo.repo_path, &["push", "origin", default_branch])
    .expect("Failed to push commit");

  // Add local commits that are NOT pushed.
  repo
    .commit_file("local_1.txt", "local 1\n", "Local commit 1")
    .expect("Failed to create local commit 1");
  repo
    .commit_file("local_2.txt", "local 2\n", "Local commit 2")
    .expect("Failed to create local commit 2");

  // Fetch to ensure jj knows about the remote state.
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("Failed to fetch");

  let result = treq_lib::jj::jj_get_log(&repo.repo_path, default_branch, Some(true), None)
    .expect("Failed to list commits");

  let pushed_commit = result
    .commits
    .iter()
    .find(|c| c.description == "Pushed commit");
  let local_commit_1 = result
    .commits
    .iter()
    .find(|c| c.description == "Local commit 1");
  let local_commit_2 = result
    .commits
    .iter()
    .find(|c| c.description == "Local commit 2");

  assert!(
    pushed_commit.is_some(),
    "pushed commit should appear in log"
  );
  assert!(
    local_commit_1.is_some(),
    "local commit 1 should appear in log"
  );
  assert!(
    local_commit_2.is_some(),
    "local commit 2 should appear in log"
  );

  assert!(
    pushed_commit.unwrap().is_immutable,
    "pushed commit should be immutable (already shared)"
  );
  assert!(
    !local_commit_1.unwrap().is_immutable,
    "unpushed local commit 1 should be mutable"
  );
  assert!(
    !local_commit_2.unwrap().is_immutable,
    "unpushed local commit 2 should be mutable"
  );
}

#[test]
fn test_default_branch_without_remote_commits_stay_mutable() {
  // Nothing is published without a remote, so nothing is immutable.
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  repo
    .commit_file("commit_1.txt", "content\n", "Commit 1")
    .expect("Failed to create commit 1");
  repo
    .commit_file("commit_2.txt", "content\n", "Commit 2")
    .expect("Failed to create commit 2");

  let result = treq_lib::jj::jj_get_log(&repo.repo_path, default_branch, Some(true), None)
    .expect("Failed to list commits");

  let non_wc_commits: Vec<_> = result
    .commits
    .iter()
    .filter(|c| !c.is_working_copy)
    .collect();
  assert!(!non_wc_commits.is_empty(), "should have commits");

  for commit in &non_wc_commits {
    assert!(
      !commit.is_immutable,
      "without remote, commit '{}' should stay mutable",
      commit.description
    );
  }
}

/// A workspace commit rebased onto a `main` that edits the same file, so the
/// workspace commit's own tree is conflicted on `conflict.txt`.
fn setup_conflicted_workspace_commit(repo: &TestRepo) -> (treq_lib::local_db::Workspace, String) {
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/conflict-resolved-later",
    Some("conflict resolved later".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("create workspace");
  let workspace_path = repo
    .workspaces_dir()
    .join(&workspace.workspace_path)
    .to_str()
    .expect("workspace path should be utf-8")
    .to_string();

  TestRepo::write_workspace_file(&workspace_path, "conflict.txt", "workspace version\n")
    .expect("write workspace conflict.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "conflicting commit")
    .expect("commit workspace");

  repo
    .create_file("conflict.txt", "main version\n")
    .expect("write main conflict.txt");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("commit main");
  let rebase = treq_lib::jj::jj_rebase_workspace_bookmark_onto(
    &workspace_path,
    &workspace.branch_name,
    repo.default_branch(),
  )
  .expect("rebase");
  assert!(rebase.success, "rebase failed: {}", rebase.message);

  (workspace, workspace_path)
}

fn conflict_flags(repo: &TestRepo, workspace_id: i64) -> Vec<(String, bool)> {
  treq_lib::core::list_commits(&repo.repo_path, Some(workspace_id), false, None, None)
    .expect("list commits")
    .commits
    .into_iter()
    .filter(|c| !c.on_target_only)
    .map(|c| (c.description, c.has_conflicts))
    .collect()
}

#[test]
fn list_commits_clears_conflict_resolved_by_a_later_commit() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let (workspace, workspace_path) = setup_conflicted_workspace_commit(&repo);
  assert_eq!(
    conflict_flags(&repo, workspace.id),
    vec![("conflicting commit".to_string(), true)]
  );

  TestRepo::write_workspace_file(&workspace_path, "conflict.txt", "resolved\n")
    .expect("resolve conflict.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "resolve conflict")
    .expect("commit resolution");

  assert_eq!(
    conflict_flags(&repo, workspace.id),
    vec![
      ("resolve conflict".to_string(), false),
      ("conflicting commit".to_string(), false),
    ]
  );
}

#[test]
fn list_commits_keeps_conflict_a_later_commit_leaves_unresolved() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let (workspace, workspace_path) = setup_conflicted_workspace_commit(&repo);

  TestRepo::write_workspace_file(&workspace_path, "other.txt", "unrelated\n")
    .expect("write other.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "unrelated commit")
    .expect("commit unrelated change");

  assert_eq!(
    conflict_flags(&repo, workspace.id),
    vec![
      ("unrelated commit".to_string(), true),
      ("conflicting commit".to_string(), true),
    ]
  );
}

/// The Commits badge (`workspace_status`) and the Commits list (`list_commits`)
/// agree with each other and with the true range after history is rewritten
/// outside treq: an abandon, then a rebase onto an advanced target.
#[test]
fn badge_count_matches_list_after_history_is_rewritten_outside_treq() {
  let repo = TestRepo::new().expect("repo");
  let ws = repo.create_workspace_simple("feat/rewrite").expect("ws");
  let path = repo.workspace_full_path(&ws);
  for idx in 0..4 {
    TestRepo::write_workspace_file(&path, &format!("f{idx}.txt"), &format!("{idx}\n"))
      .expect("write");
    treq_lib::core::commit_workspace(&repo.repo_path, ws.id, &format!("stack {idx}"))
      .expect("commit");
  }
  let counts = || {
    let badge = treq_lib::core::workspace_status(&repo.repo_path, Some(ws.id))
      .expect("status")
      .commits_ahead_of_target
      .len();
    let list = treq_lib::core::list_commits(&repo.repo_path, Some(ws.id), true, None, None)
      .expect("list")
      .commits
      .iter()
      .filter(|c| !c.on_target_only)
      .count();
    (badge, list)
  };
  assert_eq!(counts(), (4, 4));

  // An agent abandons one commit of the stack.
  let doomed = TestRepo::jj_change_id(&path, "@--").expect("change id");
  treq_lib::jj::jj_abandon(&path, &doomed).expect("abandon");
  assert_eq!(counts(), (3, 3));

  // The target advances and the stack is rebased onto it.
  repo
    .commit_file("main.txt", "main\n", "main moves on")
    .expect("main commit");
  treq_lib::jj::jj_rebase_workspace_bookmark_onto(&path, "feat/rewrite", repo.default_branch())
    .expect("rebase");
  assert_eq!(counts(), (3, 3));
}
