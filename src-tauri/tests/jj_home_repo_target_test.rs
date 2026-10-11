mod e2e_test_helpers;

use e2e_test_helpers::TestRepo;
use treq_lib::core::list_commits;
use treq_lib::jj::{
  jj_dry_run_home_repo_rebase, jj_get_home_repo_diverged_log, jj_rebase_home_repo_branch,
};

// Helper: create a branch with one commit on top of another
fn create_branch_with_commit(
  repo: &TestRepo,
  branch: &str,
  base: &str,
  filename: &str,
  content: &str,
  message: &str,
) {
  TestRepo::run_git(&repo.repo_path, &["checkout", base]).expect("checkout base");
  TestRepo::run_git(&repo.repo_path, &["checkout", "-b", branch]).expect("create branch");
  repo
    .commit_file(filename, content, message)
    .expect("commit file");
}

/// When on a non-default branch, jj_get_home_repo_diverged_log should return:
/// - `commits` = commits unique to the current branch, then target-ahead commits
///   (`on_target_only = true`)
/// - `merge_base_id` = Some(...)
#[test]
fn test_diverged_log_splits_branch_and_target_commits() {
  let repo = TestRepo::new().expect("create repo");
  let default_branch = repo.default_branch();

  // Add a commit on main so it can be "ahead" later
  repo
    .commit_file("main_a.txt", "main A", "main commit A")
    .expect("main commit A");

  // Create feature branch from this point
  create_branch_with_commit(
    &repo,
    "feature",
    default_branch,
    "feat.txt",
    "feature content",
    "feature commit",
  );

  // Go back to main and add another commit (target is now ahead)
  TestRepo::run_git(&repo.repo_path, &["checkout", default_branch]).expect("checkout main");
  repo
    .commit_file("main_b.txt", "main B", "main commit B")
    .expect("main commit B");

  // Back to feature to simulate home repo on feature branch
  TestRepo::run_git(&repo.repo_path, &["checkout", "feature"]).expect("checkout feature");

  let result = jj_get_home_repo_diverged_log(&repo.repo_path, "feature", default_branch, Some(20))
    .expect("diverged log should succeed");

  let branch_commits: Vec<_> = result
    .commits
    .iter()
    .filter(|c| !c.on_target_only)
    .collect();
  let target_commits: Vec<_> = result.commits.iter().filter(|c| c.on_target_only).collect();

  assert!(
    !branch_commits.is_empty(),
    "should have branch-unique commits"
  );
  assert!(
    branch_commits
      .iter()
      .any(|c| c.description.contains("feature commit")),
    "feature commit should be in branch-unique commits"
  );

  assert!(
    !target_commits.is_empty(),
    "should have target-ahead commits"
  );
  assert!(
    target_commits
      .iter()
      .any(|c| c.description.contains("main commit B")),
    "main commit B should be in target-ahead commits"
  );

  // merge_base_id should be populated
  assert!(
    result.merge_base_id.is_some(),
    "merge_base_id should be populated"
  );
}

/// When on the default branch itself, list_commits should return a normal history
/// (not the diverged view — no target-only commits and no merge_base_id).
#[test]
fn test_list_commits_on_default_branch_returns_normal_history() {
  let repo = TestRepo::new().expect("create repo");
  repo
    .commit_file("file.txt", "content", "extra commit")
    .expect("extra commit");

  let result = list_commits(&repo.repo_path, None, false, None, Some(10))
    .expect("list_commits should succeed");

  // On default branch: no target-ahead commits and no merge base
  assert!(
    result.commits.iter().all(|c| !c.on_target_only),
    "on default branch, commits should not include target-only entries"
  );
  assert!(
    result.merge_base_id.is_none(),
    "on default branch, merge_base_id should be None"
  );
  assert!(
    !result.commits.is_empty(),
    "should have commits on default branch"
  );
}

/// When on a non-default branch, list_commits returns branch-unique commits and merge base.
/// Target-ahead commits are only included when `include_target_branch_history` is true.
#[test]
fn test_list_commits_on_feature_branch_returns_diverged_view() {
  let repo = TestRepo::new().expect("create repo");
  let default_branch = repo.default_branch();

  // Add commit to main
  repo
    .commit_file("main_extra.txt", "main extra", "main extra commit")
    .expect("main extra");

  // Create feature branch with a commit
  create_branch_with_commit(
    &repo,
    "myfeature",
    default_branch,
    "feature_file.txt",
    "feature content",
    "feature work",
  );

  // Advance main beyond the feature branch point
  TestRepo::run_git(&repo.repo_path, &["checkout", default_branch]).expect("checkout main");
  repo
    .commit_file("main_new.txt", "main new", "main new commit")
    .expect("main new commit");

  // Switch to feature branch (simulating home repo on feature branch)
  TestRepo::run_git(&repo.repo_path, &["checkout", "myfeature"]).expect("checkout feature branch");

  let direct =
    jj_get_home_repo_diverged_log(&repo.repo_path, "myfeature", default_branch, Some(20))
      .expect("direct diverged log should succeed");
  assert!(
    direct.merge_base_id.is_some(),
    "direct diverged log should populate merge_base_id"
  );

  let result = list_commits(&repo.repo_path, None, false, None, Some(20))
    .expect("list_commits should succeed");

  let branch_commits: Vec<_> = result
    .commits
    .iter()
    .filter(|c| !c.on_target_only)
    .collect();
  let target_commits: Vec<_> = result.commits.iter().filter(|c| c.on_target_only).collect();

  assert!(
    branch_commits
      .iter()
      .any(|c| c.description.contains("feature work")),
    "feature work should appear in branch-unique commits"
  );
  assert!(
    target_commits.is_empty(),
    "target-ahead commits should be omitted when include_target_branch_history is false"
  );
  assert!(
    result.merge_base_id.is_some(),
    "merge_base_id should be set on non-default branch"
  );

  let with_target = list_commits(&repo.repo_path, None, true, None, Some(20))
    .expect("list_commits with target history should succeed");
  let target_with_history: Vec<_> = with_target
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .collect();
  assert!(
    target_with_history
      .iter()
      .any(|c| c.description.contains("main new commit")),
    "main new commit should appear when include_target_branch_history is true, got: {:?}",
    target_with_history
      .iter()
      .map(|c| &c.description)
      .collect::<Vec<_>>()
  );
}

/// Rebasing a home repo branch onto the default branch should succeed.
#[test]
fn test_rebase_home_repo_branch_onto_main() {
  let repo = TestRepo::new().expect("create repo");
  let default_branch = repo.default_branch();

  // Create feature branch
  create_branch_with_commit(
    &repo,
    "rebase-me",
    default_branch,
    "feature.txt",
    "feature content",
    "feature commit for rebase",
  );

  // Advance main
  TestRepo::run_git(&repo.repo_path, &["checkout", default_branch]).expect("checkout main");
  repo
    .commit_file("main_advance.txt", "advance", "advance main")
    .expect("advance main");

  // Switch to feature
  TestRepo::run_git(&repo.repo_path, &["checkout", "rebase-me"]).expect("checkout rebase-me");

  // Rebase
  let result = jj_rebase_home_repo_branch(&repo.repo_path, "rebase-me", default_branch)
    .expect("rebase should succeed");
  assert!(result.success, "rebase should succeed: {}", result.message);

  // After rebase, the feature branch should be based on the advanced main
  let log = jj_get_home_repo_diverged_log(&repo.repo_path, "rebase-me", default_branch, Some(20))
    .expect("diverged log after rebase");
  assert!(
    log.commits.iter().all(|c| !c.on_target_only),
    "after rebase, target should not be ahead"
  );
}

/// Dry-run should detect potential conflicts when both sides modify the same file.
#[test]
fn test_dry_run_detects_conflicting_changes() {
  let repo = TestRepo::new().expect("create repo");
  let default_branch = repo.default_branch();

  // Create base file on main
  repo
    .commit_file("shared.txt", "base content", "base commit")
    .expect("base commit");

  // Create feature branch modifying shared.txt
  create_branch_with_commit(
    &repo,
    "conflict-branch",
    default_branch,
    "shared.txt",
    "feature modification",
    "feature modifies shared",
  );

  // Advance main with a conflicting change to shared.txt
  TestRepo::run_git(&repo.repo_path, &["checkout", default_branch]).expect("checkout main");
  repo
    .commit_file("shared.txt", "main modification", "main modifies shared")
    .expect("main modification");

  // Switch to conflict branch
  TestRepo::run_git(&repo.repo_path, &["checkout", "conflict-branch"])
    .expect("checkout conflict-branch");

  let dry_run = jj_dry_run_home_repo_rebase(&repo.repo_path, "conflict-branch", default_branch)
    .expect("dry run should not fail");
  assert!(
    dry_run.would_conflict,
    "dry run should detect conflict: {:?}",
    dry_run.conflicted_files
  );
  assert!(
    !dry_run.conflicted_files.is_empty(),
    "conflicted files should list shared.txt"
  );
}

/// Dry-run should report no conflicts when changes are on disjoint files.
#[test]
fn test_dry_run_no_conflict_with_disjoint_changes() {
  let repo = TestRepo::new().expect("create repo");
  let default_branch = repo.default_branch();

  // Create feature branch modifying feature-only file
  create_branch_with_commit(
    &repo,
    "clean-branch",
    default_branch,
    "feature_only.txt",
    "feature content",
    "feature adds file",
  );

  // Advance main with a different file
  TestRepo::run_git(&repo.repo_path, &["checkout", default_branch]).expect("checkout main");
  repo
    .commit_file("main_only.txt", "main content", "main adds file")
    .expect("main only file");

  // Switch to clean branch
  TestRepo::run_git(&repo.repo_path, &["checkout", "clean-branch"]).expect("checkout clean");

  let dry_run = jj_dry_run_home_repo_rebase(&repo.repo_path, "clean-branch", default_branch)
    .expect("dry run should not fail");
  assert!(
    !dry_run.would_conflict,
    "dry run should not detect conflict for disjoint changes"
  );
}

/// A home-repo branch cut from the default branch tip has no target-ahead commits,
/// but `include_target_branch_history` should still return the default branch's
/// recent history so the Commits tab can show it below the branch's own commits.
#[test]
fn test_list_commits_on_feature_branch_includes_default_branch_history() {
  let repo = TestRepo::new().expect("create repo");
  let default_branch = repo.default_branch();

  repo
    .commit_file("main_history.txt", "main history", "main history commit")
    .expect("main history commit");

  create_branch_with_commit(
    &repo,
    "tipfeature",
    default_branch,
    "tip_feature.txt",
    "tip feature",
    "tip feature work",
  );

  let result = list_commits(&repo.repo_path, None, true, None, Some(20))
    .expect("list_commits with target history should succeed");

  let branch_descriptions: Vec<_> = result
    .commits
    .iter()
    .filter(|c| !c.on_target_only)
    .map(|c| c.description.clone())
    .collect();
  let target_descriptions: Vec<_> = result
    .commits
    .iter()
    .filter(|c| c.on_target_only)
    .map(|c| c.description.clone())
    .collect();

  assert!(
    branch_descriptions
      .iter()
      .any(|d| d.contains("tip feature work")),
    "branch commit should be listed first, got: {:?}",
    branch_descriptions
  );
  assert!(
    target_descriptions
      .iter()
      .any(|d| d.contains("main history commit")),
    "default branch history should be included, got: {:?}",
    target_descriptions
  );
  assert!(
    !target_descriptions
      .iter()
      .any(|d| d.contains("tip feature work")),
    "branch commits must not be duplicated into target history"
  );

  let without_target = list_commits(&repo.repo_path, None, false, None, Some(20))
    .expect("list_commits without target history should succeed");
  assert!(
    without_target.commits.iter().all(|c| !c.on_target_only),
    "default branch history should be omitted when include_target_branch_history is false"
  );
}
