#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  clippy::string_slice,
  reason = "test code"
)]
mod common;

use common::{JjVerifier, TestRepo};
use std::fs;
use std::path::Path;

use treq_lib::local_db::Workspace;

#[test]
fn test_can_create_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  assert!(
    &repo.workspaces_dir().exists(),
    "Workspaces directory should exist"
  );

  // Create workspace (new branch)
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/test",
    Some("new feature".to_string()),
    None, // moved_files
    None, // source_branch (defaults to current)
    None,
    None,
  )
  .expect("Failed to create workspace");

  // Verify workspace was created with correct fields
  assert!(workspace.id > 0, "Workspace should have valid database ID");
  assert_eq!(
    workspace.branch_name, "feat/test",
    "Branch name should match"
  );
  assert_eq!(
    workspace.repo_path, repo.repo_path,
    "Repo path should match"
  );
  assert_eq!(
    workspace.workspace_path, "feat-test",
    "Workspace path should be generated and sanitised correctly"
  );

  assert!(
    Path::new(&repo.workspaces_dir().join(&workspace.workspace_path)).exists(),
    "Workspace directory should exist"
  );

  let workspace_path = &repo.workspaces_dir().join(&workspace.workspace_path);
  assert!(
    workspace_path.join(".jj").exists(),
    ".jj directory should exist in workspace"
  );
  assert!(
    workspace_path.join("README.md").exists(),
    "README.md should exist in workspace"
  );
  assert!(
    !workspace_path.join(".treq").exists(),
    ".treq directory should not exist in workspace"
  );

  // verify workspace is valid jj workspace
  TestRepo::jj_snapshot(workspace_path.to_str().unwrap())
    .expect("Workspace should be valid jj workspace");

  // JJ VERIFICATION: Verify workspace via jj workspace list (primary source of truth)
  let jj_workspaces =
    JjVerifier::list_workspaces(&repo.repo_path).expect("Failed to list jj workspaces");
  assert!(
    jj_workspaces.contains(&workspace.workspace_name),
    "jj workspace list should contain '{}', got: {:?}",
    workspace.branch_name,
    jj_workspaces
  );

  // JJ VERIFICATION: Verify bookmark was created
  let bookmarks =
    JjVerifier::list_bookmarks(workspace_path.to_str().unwrap()).expect("Failed to list bookmarks");
  assert!(
    bookmarks.iter().any(|b| b == &workspace.branch_name),
    "Bookmark '{}' should exist in workspace, got: {:?}",
    workspace.branch_name,
    bookmarks
  );
}

#[test]
fn workspace_creation_creates_empty_wc_but_bookmark_targets_parent() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/wc-parent-check",
    Some("wc parent check".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path
    .to_str()
    .expect("Workspace path should be valid UTF-8");

  assert!(
    TestRepo::jj_working_copy_is_clean(workspace_path_str),
    "Expected newly created workspace to report an empty working copy"
  );

  let at_commit = JjVerifier::get_commit_id_for_rev(workspace_path_str, "@")
    .expect("Failed to resolve @ commit id")
    .expect("@ commit id should exist");
  let parent_commit = JjVerifier::get_commit_id_for_rev(workspace_path_str, "@-")
    .expect("Failed to resolve @- commit id")
    .expect("@- commit id should exist");
  assert_ne!(
    at_commit, parent_commit,
    "Expected @ to be an empty working-copy commit above @-"
  );

  let bookmark_tip = JjVerifier::get_bookmark_commit_id(&repo.repo_path, &workspace.branch_name)
    .expect("Failed to resolve workspace bookmark tip")
    .expect("Workspace bookmark should resolve");
  assert_eq!(
    bookmark_tip, at_commit,
    "Workspace bookmark should point to @ (working-copy commit)"
  );
  assert_ne!(
    bookmark_tip, parent_commit,
    "Workspace bookmark should not point to @- (parent commit)"
  );

  let recent_ids = TestRepo::jj_commit_ids_in_revset(workspace_path_str, "::@")
    .expect("Failed to get jj log")
    .into_iter()
    .take(8)
    .collect::<Vec<_>>();
  assert!(
    recent_ids.iter().any(|id| id == &at_commit),
    "jj log should include @ commit id (empty working copy): {at_commit}, got: {recent_ids:?}"
  );
  assert!(
    recent_ids.iter().any(|id| id == &parent_commit),
    "jj log should include @- commit id: {parent_commit}, got: {recent_ids:?}"
  );
}

#[test]
fn workspace_creation_list_commits_excludes_all_working_copy_commits() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/list-commits-filter-check",
    Some("list commits filter check".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path
    .to_str()
    .expect("Workspace path should be valid UTF-8");

  assert!(
    TestRepo::jj_working_copy_is_clean(workspace_path_str),
    "Expected newly created workspace to report an empty working copy"
  );

  let at_commit = JjVerifier::get_commit_id_for_rev(workspace_path_str, "@")
    .expect("Failed to resolve @ commit id")
    .expect("@ commit id should exist");

  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("list_commits should succeed");

  assert!(
    log.commits.iter().all(|c| !c.is_working_copy),
    "list_commits should exclude all working-copy commits, got: {:?}",
    log
      .commits
      .iter()
      .map(|c| (
        c.commit_id.clone(),
        c.description.clone(),
        c.is_working_copy
      ))
      .collect::<Vec<_>>()
  );
  assert!(
    !log.commits.iter().any(|c| c.commit_id == at_commit),
    "list_commits should not include current @ commit id (empty working-copy commit): {}",
    at_commit
  );
}

#[test]
fn test_can_create_workspace_with_same_source_branch() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  assert!(
    &repo.workspaces_dir().exists(),
    "Workspaces directory should exist"
  );

  // Create workspace (new branch)
  let _workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/test1",
    Some("new feature".to_string()),
    None,                 // moved_files
    Some(default_branch), // source_branch (defaults to current)
    None,
    None,
  )
  .expect("Failed to create workspace");

  // Create workspace (new branch)
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/test2",
    Some("new feature2".to_string()),
    None,                 // moved_files
    Some(default_branch), // source_branch (defaults to current)
    None,
    None,
  )
  .expect("Failed to create workspace");

  // JJ VERIFICATION: Verify workspace via jj workspace list (should contain the *second* branch)
  let jj_workspaces =
    JjVerifier::list_workspaces(&repo.repo_path).expect("Failed to list jj workspaces");
  assert!(
    jj_workspaces.contains(&workspace.workspace_name),
    "jj workspace list should contain '{}', got: {:?}",
    workspace.branch_name,
    jj_workspaces
  );

  // JJ VERIFICATION: Verify bookmark was created for the second workspace
  let bookmarks = JjVerifier::list_bookmarks(&repo.repo_path).expect("Failed to list bookmarks");
  assert!(
    bookmarks.iter().any(|b| b == &workspace.branch_name),
    "Bookmark '{}' should exist in workspace, got: {:?}",
    workspace.branch_name,
    bookmarks
  );
}
#[test]
fn creates_workspace_stacked_on_default_branch_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch().to_string();

  let parent = treq_lib::core::create_workspace(
    &repo.repo_path,
    &default_branch,
    Some("default branch workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("default branch workspace should be created");

  let child = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stacked-on-default",
    Some("stacked workspace".to_string()),
    None,
    Some(&default_branch),
    None,
    None,
  )
  .expect("workspace should stack on the existing default branch workspace");

  assert_ne!(child.id, parent.id);
  assert_ne!(child.workspace_path, parent.workspace_path);
  assert_eq!(
    child.target_branch.as_deref(),
    Some(default_branch.as_str())
  );
  assert!(
    treq_lib::local_db::get_workspace_by_id(&repo.repo_path, parent.id)
      .expect("parent workspace query should succeed")
      .is_some(),
    "stacking must not replace the default branch workspace"
  );
}
#[test]
fn test_can_create_workspace_from_remote_branch() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");

  // create workspace from a remote branch
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feature-remote",
    Some("feature-remote".to_string()),
    None, // moved_files
    None,
    None,
    None,
  )
  .expect("Failed to create workspace from remote branch");

  let workspace_name = workspace.workspace_name;
  // Verify workspace exists
  let workspace_path = repo.workspaces_dir().join(&workspace_name);
  assert!(
    workspace_path.exists(),
    "Workspace from remote branch should exist"
  );

  // Verify the file from the branch is present
  assert!(
    workspace_path.join("feature.txt").exists(),
    "File from remote branch should exist in workspace"
  );

  // Verify workspace has correctly checked out the remote branch: its bookmarks
  // (what `jj status` displays for @ and @-) should include feature-remote.
  let workspace_path_str = workspace_path.to_str().unwrap();

  let mut bookmarks = TestRepo::jj_bookmarks_on_revision(workspace_path_str, "@")
    .expect("Failed to resolve bookmarks on @");
  bookmarks.extend(
    TestRepo::jj_bookmarks_on_revision(workspace_path_str, "@-")
      .expect("Failed to resolve bookmarks on @-"),
  );

  assert!(
    bookmarks.iter().any(|b| b == "feature-remote"),
    "Workspace should show 'feature-remote' bookmark, got: {:?}",
    bookmarks
  );
}

#[test]
fn test_open_or_create_workspace_from_pr_creates_from_remote_head() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let default_branch = repo.default_branch().to_string();

  let (workspace, created) = treq_lib::core::open_or_create_workspace_from_pr(
    &repo.repo_path,
    "feature-remote",
    &default_branch,
    Some("PR title"),
    Some("From GitHub PR #42"),
  )
  .expect("should create workspace from PR head");

  assert!(created, "first call should create the workspace");
  assert_eq!(workspace.branch_name, "feature-remote");
  assert_eq!(
    workspace.target_branch.as_deref(),
    Some(default_branch.as_str())
  );
  assert_eq!(workspace.title, "PR title");
  assert_eq!(workspace.description.as_deref(), Some("From GitHub PR #42"));

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  assert!(
    workspace_path.join("feature.txt").exists(),
    "workspace should contain files from the PR head branch"
  );
}

#[test]
fn test_open_or_create_workspace_from_pr_returns_existing() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let default_branch = repo.default_branch().to_string();

  let (first, created_first) = treq_lib::core::open_or_create_workspace_from_pr(
    &repo.repo_path,
    "feature-remote",
    &default_branch,
    Some("PR title"),
    None,
  )
  .expect("should create workspace from PR head");
  assert!(created_first);

  let (second, created_second) = treq_lib::core::open_or_create_workspace_from_pr(
    &repo.repo_path,
    "feature-remote",
    "some-other-base",
    Some("ignored"),
    None,
  )
  .expect("should return existing workspace");

  assert!(!created_second, "second call should not recreate");
  assert_eq!(second.id, first.id);
  assert_eq!(
    second.target_branch.as_deref(),
    Some(default_branch.as_str()),
    "existing workspace target should be left unchanged"
  );
  assert_eq!(second.title, "PR title");
}

#[test]
fn test_open_or_create_workspace_from_pr_errors_when_head_missing() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let default_branch = repo.default_branch().to_string();

  let err = treq_lib::core::open_or_create_workspace_from_pr(
    &repo.repo_path,
    "does-not-exist-on-remote",
    &default_branch,
    None,
    None,
  )
  .expect_err("missing PR head should fail");

  assert!(
    err.to_lowercase().contains("does-not-exist-on-remote")
      || err.to_lowercase().contains("not found")
      || err.to_lowercase().contains("missing"),
    "error should mention the missing head branch, got: {err}"
  );
}

// TODO: create a workspace from non-default home repo branch

#[test]
fn test_can_create_stacked_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Create first workspace
  let base: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/base",
    Some("feature-base".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create base workspace");

  let workspace1_path = repo.workspaces_dir().join(&base.workspace_path);

  // make an edit to the base workspace.
  TestRepo::write_workspace_file(
    workspace1_path.to_str().unwrap(),
    "base.txt",
    "base content",
  )
  .expect("Failed to write base file");

  // Create stacked workspace based on the first workspace's branch
  let stacked: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stacked",
    None,
    None,
    Some(&base.branch_name),
    None,
    None,
  )
  .expect("Failed to create stacked workspace");

  // Verify stacked workspace exists
  let stacked_path = repo.workspaces_dir().join(&stacked.workspace_path);
  assert!(stacked_path.exists(), "Stacked workspace should exist");

  // Verify it has the base file from the source branch
  assert!(
    stacked_path.join("base.txt").exists(),
    "Stacked workspace should have file from source branch"
  );

  let workspaces =
    treq_lib::core::list_workspaces(&repo.repo_path).expect("Failed to list workspaces");
  assert_eq!(
    workspaces.len(),
    2,
    "Should have 2 workspaces, got {}",
    workspaces.len()
  );

  // Fetch workspace status for both workspaces
  let base_status = treq_lib::core::workspace_status(&repo.repo_path, Some(base.id))
    .expect("Failed to get base workspace status");
  let stacked_status = treq_lib::core::workspace_status(&repo.repo_path, Some(stacked.id))
    .expect("Failed to get stacked workspace status");

  // workspace_status no longer builds DAG data.
  assert_eq!(
    base_status.dag_nodes.len(),
    stacked_status.dag_nodes.len(),
    "dag_nodes should be consistently empty"
  );
  assert_eq!(
    base_status.dag_nodes.len(),
    0,
    "workspace_status should not include DAG nodes"
  );

  // Base workspace should show stacked as a child
  assert_eq!(
    base_status.children.len(),
    1,
    "Base workspace should have 1 child"
  );
  assert_eq!(
    base_status.children[0].branch_name, "feat/stacked",
    "Child should be the stacked workspace"
  );
  assert!(
    base_status.target.is_none(),
    "Base workspace should have no target"
  );

  // Stacked workspace should show base as target
  assert!(
    stacked_status.target.is_some(),
    "Stacked workspace should have a target"
  );
  assert_eq!(
    stacked_status.target.as_ref().unwrap().branch_name,
    "feat/base",
    "Target should be the base workspace"
  );
  assert_eq!(
    stacked_status.children.len(),
    0,
    "Stacked workspace should have no children"
  );

  assert!(
    base_status.conflicted_workspace_ids.is_empty(),
    "workspace_status should not include DAG-derived conflict ids"
  );
}

#[test]
fn test_create_workspace_from_ahead_source_stacks_history_and_working_copy_and_diff() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let b_workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/source-ahead",
    Some("source ahead workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create B workspace");

  let b_path = repo.workspaces_dir().join(&b_workspace.workspace_path);
  let b_path_str = b_path
    .to_str()
    .expect("B workspace path should be valid UTF-8");

  TestRepo::write_workspace_file(b_path_str, "b-committed.txt", "B committed change marker\n")
    .expect("Failed to write B change");
  treq_lib::core::commit_workspace(&repo.repo_path, b_workspace.id, "B committed change")
    .expect("Failed to commit in B");

  let b_ahead = treq_lib::jj::jj_get_commits_ahead(b_path_str, default_branch)
    .expect("Failed to compute commits ahead for B");
  assert_eq!(
    b_ahead.total_count, 1,
    "B should be exactly 1 commit ahead of main, got {}",
    b_ahead.total_count
  );

  let a_workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stacked-on-ahead",
    Some("stacked on B".to_string()),
    None,
    Some(&b_workspace.branch_name),
    None,
    None,
  )
  .expect("Failed to create A workspace from B");

  let a_path = repo.workspaces_dir().join(&a_workspace.workspace_path);
  let a_path_str = a_path
    .to_str()
    .expect("A workspace path should be valid UTF-8");

  TestRepo::write_workspace_file(a_path_str, "a-committed.txt", "A committed change marker\n")
    .expect("Failed to write A change");
  treq_lib::core::commit_workspace(&repo.repo_path, a_workspace.id, "A committed change")
    .expect("Failed to commit in A");

  for (workspace_label, workspace_path) in [("A", a_path_str), ("B", b_path_str)] {
    assert!(
      TestRepo::jj_working_copy_is_clean(workspace_path),
      "{} working copy should be clean",
      workspace_label
    );
  }

  let b_log = TestRepo::jj_log_entries(b_path_str, "::@", 12).expect("Failed to collect B jj log");
  assert!(
    b_log
      .iter()
      .any(|e| e.description.contains("B committed change")),
    "B jj log should include B committed change, got:\n{b_log:?}"
  );
  assert!(
    b_log.iter().any(|e| e.is_empty),
    "B jj log should include working-copy lineage, got:\n{b_log:?}"
  );

  let a_log = TestRepo::jj_log_entries(a_path_str, "::@", 16).expect("Failed to collect A jj log");
  assert!(
    a_log
      .iter()
      .any(|e| e.description.contains("B committed change")),
    "A jj log should include B committed change, got:\n{a_log:?}"
  );
  assert!(
    a_log
      .iter()
      .any(|e| e.description.contains("A committed change")),
    "A jj log should include A committed change, got:\n{a_log:?}"
  );
  assert!(
    a_log.iter().any(|e| e.is_empty),
    "A jj log should include working-copy lineage, got:\n{a_log:?}"
  );

  let b_commits =
    treq_lib::core::list_commits(&repo.repo_path, Some(b_workspace.id), false, None, None)
      .expect("list_commits should succeed for B");
  assert!(
    b_commits.commits.iter().all(|c| !c.is_working_copy),
    "B list_commits should exclude working-copy commits"
  );
  assert!(
    b_commits
      .commits
      .iter()
      .any(|c| c.description.contains("B committed change")),
    "B list_commits should include B committed change"
  );

  let a_commits =
    treq_lib::core::list_commits(&repo.repo_path, Some(a_workspace.id), false, None, None)
      .expect("list_commits should succeed for A");
  assert!(
    a_commits.commits.iter().all(|c| !c.is_working_copy),
    "A list_commits should exclude working-copy commits"
  );
  assert!(
    !a_commits
      .commits
      .iter()
      .any(|c| c.description.contains("B committed change")),
    "A list_commits should not include B committed change (include_target_branch_history=false)"
  );
  assert!(
    a_commits
      .commits
      .iter()
      .any(|c| c.description.contains("A committed change")),
    "A list_commits should include A committed change"
  );

  let b_status = treq_lib::core::workspace_status(&repo.repo_path, Some(b_workspace.id))
    .expect("workspace_status should succeed for B");
  let a_status = treq_lib::core::workspace_status(&repo.repo_path, Some(a_workspace.id))
    .expect("workspace_status should succeed for A");
  assert!(
    b_status
      .children
      .iter()
      .any(|child| child.id == a_workspace.id),
    "B should list A as child"
  );
  assert_eq!(
    a_status.target.as_ref().map(|ws| ws.id),
    Some(b_workspace.id),
    "A target should be B"
  );

  let app_db_path = std::path::Path::new(&repo.repo_path)
    .join(".treq")
    .join("treq.db");
  let app_db = treq_lib::db::Database::new(app_db_path).expect("test app db should open");
  app_db.init().expect("test app db should initialize");
  app_db
    .set_setting("conflict_marker_style", "git")
    .expect("set conflict marker style should succeed");

  let a_diff = treq_lib::core::workspace_diff(&repo.repo_path, a_workspace.id)
    .expect("workspace_diff should succeed for A");
  let a_diff_text = format!("{:?}", a_diff.hunks_by_file);
  assert!(
    a_diff_text.contains("A committed change marker"),
    "A workspace_diff should include A committed change content, got:\n{}",
    a_diff_text
  );
  assert!(
    a_diff_text.contains("A committed change marker"),
    "A workspace_diff should include A committed change content, got:\n{}",
    a_diff_text
  );

  let b_diff = treq_lib::core::workspace_diff(&repo.repo_path, b_workspace.id)
    .expect("workspace_diff should succeed for B");
  let b_diff_text = format!("{:?}", b_diff.hunks_by_file);
  assert!(
        !b_diff_text.contains("A committed change marker"),
        "B workspace_diff should not include descendant workspace change A committed change content, got:\n{}",
        b_diff_text
    );
  assert!(
    b_diff_text.contains("B committed change marker"),
    "B workspace_diff should include B committed change content, got:\n{}",
    b_diff_text
  );
}

#[test]
fn test_list_workspaces_removes_db_workspace_missing_from_jj_state() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace: Workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stale-db-row",
    Some("stale db row".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  TestRepo::run_git(
    &repo.repo_path,
    &["checkout", workspace.branch_name.as_str()],
  )
  .expect("Failed to checkout workspace branch in home repo");

  let workspace_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  treq_lib::jj::forget_workspace(&repo.repo_path, workspace_dir.to_str().unwrap())
    .expect("jj workspace forget should succeed");

  treq_lib::core::init(&repo.repo_path).expect("Failed to re-run repo init");

  let workspaces =
    treq_lib::core::list_workspaces(&repo.repo_path).expect("Failed to list workspaces");

  assert!(
    workspaces.is_empty(),
    "list_workspaces should omit workspaces that JJ no longer tracks"
  );
  assert!(
    !workspaces.iter().any(|ws| ws.id == workspace.id),
    "Workspace missing from JJ state should be removed from the database"
  );

  let db_workspace = treq_lib::local_db::get_workspace_by_id(&repo.repo_path, workspace.id)
    .expect("Failed to query workspace by id");
  assert!(
    db_workspace.is_none(),
    "Stale database row should be deleted after reconciliation"
  );
}

#[test]
fn test_moved_files_from_main_repo() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Create test files in the main repo
  let file1_path = repo
    .create_file("feature1.rs", "// Feature 1 code")
    .expect("Failed to create file1");
  let file2_path = repo
    .create_file("feature2.rs", "// Feature 2 code")
    .expect("Failed to create file2");

  // Verify files exist in main repo
  assert!(file1_path.exists(), "feature1.rs should exist in main repo");
  assert!(file2_path.exists(), "feature2.rs should exist in main repo");

  // jj auto-tracks files, no need to explicitly add them

  // Create workspace with moved_files metadata
  let moved_files = vec!["feature1.rs".to_string(), "feature2.rs".to_string()];
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/refactor",
    Some("refactor code".to_string()),
    Some(moved_files.clone()),
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  // Verify workspace has moved_files set
  assert_eq!(
    workspace.moved_files,
    Some(moved_files),
    "Workspace should have moved_files set after creation"
  );

  // Verify workspace has description set
  assert_eq!(
    workspace.description,
    Some("refactor code".to_string()),
    "Workspace should have description set"
  );

  // Verify the workspace directory exists and is a valid jj workspace
  let workspace_path = Path::new(&repo.repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);

  // Verify files are now in the workspace (moved by create_workspace when moved_files provided)
  let file1_in_workspace = workspace_path.join("feature1.rs");
  let file2_in_workspace = workspace_path.join("feature2.rs");
  assert!(
    file1_in_workspace.exists(),
    "feature1.rs should exist in workspace after create_workspace"
  );
  assert!(
    file2_in_workspace.exists(),
    "feature2.rs should exist in workspace after create_workspace"
  );

  // Verify files are no longer in main repo
  assert!(
    !file1_path.exists(),
    "feature1.rs should be removed from main repo after squash"
  );
  assert!(
    !file2_path.exists(),
    "feature2.rs should be removed from main repo after squash"
  );
}

#[test]
fn test_moved_files_from_workspace_to_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Create base workspace
  let base_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/base",
    Some("base feature".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create base workspace");

  let base_path = Path::new(&repo.repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&base_workspace.workspace_path);

  // Create test files in the base workspace
  let file1_path = TestRepo::write_workspace_file(
    base_path.to_str().unwrap(),
    "component1.ts",
    "// Component 1",
  )
  .expect("Failed to create component1");
  let file2_path = TestRepo::write_workspace_file(
    base_path.to_str().unwrap(),
    "component2.ts",
    "// Component 2",
  )
  .expect("Failed to create component2");

  // jj auto-tracks files, no need to explicitly add them

  // Create stacked workspace with moved_files from base
  let moved_files = vec!["component1.ts".to_string(), "component2.ts".to_string()];
  let stacked_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/components",
    Some("extract components".to_string()),
    Some(moved_files),
    Some("feat/base"),
    None,
    None,
  )
  .expect("Failed to create stacked workspace");

  // Verify the stacked workspace directory exists and is a valid jj workspace
  let stacked_path = Path::new(&repo.repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&stacked_workspace.workspace_path);

  // Verify files are now in the stacked workspace (moved by create_workspace when moved_files provided)
  let file1_in_stacked = stacked_path.join("component1.ts");
  let file2_in_stacked = stacked_path.join("component2.ts");
  assert!(
    file1_in_stacked.exists(),
    "component1.ts should exist in stacked workspace after create_workspace"
  );
  assert!(
    file2_in_stacked.exists(),
    "component2.ts should exist in stacked workspace after create_workspace"
  );

  // Verify files are no longer in the base workspace
  assert!(
    !file1_path.exists(),
    "component1.ts should be removed from base workspace after create_workspace"
  );
  assert!(
    !file2_path.exists(),
    "component2.ts should be removed from base workspace after create_workspace"
  );
}

#[test]
fn test_create_workspace_copies_included_files() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Create files in the repo root to be copied
  repo
    .create_file("node_modules/pkg/index.js", "module.exports = {}")
    .expect("Failed to create node_modules file");
  repo
    .create_file(".env", "SECRET=abc")
    .expect("Failed to create .env file");

  let patterns = vec!["node_modules".to_string(), ".env".to_string()];
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/copy-test",
    Some("copy test".to_string()),
    None,
    None,
    Some(patterns),
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);

  // Verify files were copied
  assert!(
    ws_dir.join("node_modules/pkg/index.js").exists(),
    "node_modules/pkg/index.js should be copied to workspace"
  );
  assert_eq!(
    fs::read_to_string(ws_dir.join("node_modules/pkg/index.js")).unwrap(),
    "module.exports = {}"
  );
  assert!(
    ws_dir.join(".env").exists(),
    ".env should be copied to workspace"
  );
  assert_eq!(
    fs::read_to_string(ws_dir.join(".env")).unwrap(),
    "SECRET=abc"
  );
}

#[test]
fn test_create_workspace_copies_nested_directories() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  repo
    .create_file("config/sub/file.toml", "[settings]\nkey = true")
    .expect("Failed to create nested config file");

  let patterns = vec!["config".to_string()];
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/nested-copy",
    Some("nested copy test".to_string()),
    None,
    None,
    Some(patterns),
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let copied = ws_dir.join("config/sub/file.toml");

  assert!(
    copied.exists(),
    "config/sub/file.toml should be copied to workspace"
  );
  assert_eq!(
    fs::read_to_string(copied).unwrap(),
    "[settings]\nkey = true"
  );
}

#[test]
fn test_create_workspace_skips_missing_included_files() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let patterns = vec!["nonexistent".to_string()];
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/skip-missing",
    Some("skip missing test".to_string()),
    None,
    None,
    Some(patterns),
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);

  // Workspace should exist but not contain the nonexistent pattern
  assert!(ws_dir.exists(), "Workspace directory should exist");
  assert!(
    !ws_dir.join("nonexistent").exists(),
    "nonexistent should not be in workspace"
  );
}

/// Regression: workspace creation must parent on the target branch's bookmark tip, not on
/// git HEAD (which can lag behind if main advanced while the user was on a different branch).
///
/// Scenario: init the repo (jj imports HEAD = initial commit on main), then make a new git
/// commit on main while git HEAD is detached (so git HEAD no longer equals main's tip).
/// create_workspace should parent on the new main tip, not on the stale git HEAD.
#[test]
fn test_create_workspace_parents_on_target_branch_tip_after_external_commit() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  // Record the commit hash of main's current tip (the initial commit).
  let initial_sha = TestRepo::run_git(&repo.repo_path, &["rev-parse", "HEAD"])
    .expect("Failed to get initial HEAD")
    .trim()
    .to_string();

  // Detach HEAD so that subsequent commits on main won't move git HEAD.
  TestRepo::run_git(&repo.repo_path, &["checkout", "--detach", &initial_sha])
    .expect("Failed to detach HEAD");

  // Advance main bookmark via a raw git commit on the main branch without touching git HEAD
  // (simulate another tool or fetch updating main while we are on a detached HEAD / other branch).
  TestRepo::run_git(
    &repo.repo_path,
    &["branch", "-f", default_branch, &initial_sha],
  )
  .expect("Failed to reset main");
  // Create a temp branch to land the new commit, then move main there.
  TestRepo::run_git(&repo.repo_path, &["checkout", "-b", "tmp-advance"])
    .expect("Failed to create tmp branch");
  repo
    .commit_file("external.txt", "external change", "external commit on main")
    .expect("Failed to make external commit");
  let new_main_sha = TestRepo::run_git(&repo.repo_path, &["rev-parse", "HEAD"])
    .expect("Failed to get new HEAD")
    .trim()
    .to_string();
  // Move main to this new commit, but keep git HEAD on tmp-advance (detached from main).
  TestRepo::run_git(
    &repo.repo_path,
    &["branch", "-f", default_branch, &new_main_sha],
  )
  .expect("Failed to move main to new commit");
  // Detach HEAD so it stays at the NEW commit but is NOT on main.
  TestRepo::run_git(&repo.repo_path, &["checkout", "--detach", &new_main_sha])
    .expect("Failed to detach at new commit");
  // Go back to initial so git HEAD != main tip (simulating stale HEAD scenario).
  TestRepo::run_git(&repo.repo_path, &["checkout", "--detach", &initial_sha])
    .expect("Failed to set git HEAD to initial");

  // At this point: git HEAD = initial commit, main bookmark = new_main_sha with external.txt.
  // create_workspace should import git refs and parent on main's tip, not on git HEAD.
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/after-external",
    Some("after external commit".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);

  assert!(
    workspace_path.join("external.txt").exists(),
    "Workspace parent should be main's tip (with external.txt), but git HEAD was at the old commit"
  );

  let parent_log = JjVerifier::get_log_previous_commit(workspace_path.to_str().unwrap())
    .expect("Failed to get parent log");
  assert!(
    parent_log.contains("external commit on main"),
    "Workspace @- should be the external commit (main tip), got: {}",
    parent_log
  );
}

/// New workspace target_branch should be persisted in DB even for non-stacked workspaces,
/// so auto-rebase knows which branch to track.
#[test]
fn test_create_workspace_sets_target_branch_in_db() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/target-branch-check",
    Some("target branch test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  assert!(
    workspace.target_branch.is_some(),
    "target_branch should be set for non-stacked workspaces"
  );
  assert_eq!(
    workspace.target_branch.as_deref(),
    Some(default_branch),
    "target_branch should default to the repo default branch"
  );
}

#[test]
fn test_create_workspace_materializes_installed_library_skills() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let app_dir = repo.temp_dir.path().join("app-data");
  fs::create_dir_all(&app_dir).expect("app data");
  let previous = std::env::var("TREQ_APP_DATA_DIR").ok();
  std::env::set_var("TREQ_APP_DATA_DIR", app_dir.to_str().unwrap());

  let files = vec![(
    "SKILL.md".to_string(),
    b"---\nname: demo\ndescription: demo\n---\n# Demo\n".to_vec(),
  )];
  let checksum = treq_lib::core::skills::skill_checksum(&files);
  let entry = treq_lib::core::skills::SkillCatalogEntry {
    id: "test/demo".to_string(),
    name: "demo".to_string(),
    description: Some("demo".to_string()),
    source: "test".to_string(),
    category: None,
    license: None,
    proprietary: false,
    url: None,
    checksum: Some(checksum),
    files: vec![],
  };
  let install = treq_lib::core::skills::install_skill_files(
    &entry,
    &files,
    treq_lib::core::skills::SkillInstallScope::Application,
    None,
  );
  let created = if install.is_ok() {
    treq_lib::core::create_workspace(
      &repo.repo_path,
      "feat/with-skill",
      None,
      None,
      None,
      None,
      None,
    )
  } else {
    Err(install.err().unwrap())
  };

  match previous {
    Some(value) => std::env::set_var("TREQ_APP_DATA_DIR", value),
    None => std::env::remove_var("TREQ_APP_DATA_DIR"),
  }

  let workspace = created.expect("Failed to create workspace");
  let ws = repo.workspaces_dir().join(&workspace.workspace_path);
  assert!(
    ws.join(".agents/skills/demo/SKILL.md").exists(),
    "library skill should be copied into the workspace"
  );
  assert!(ws.join(".claude/skills/demo/SKILL.md").exists());
}
