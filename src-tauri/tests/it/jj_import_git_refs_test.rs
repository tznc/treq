use crate::common::TestRepo;
use treq_lib::core::list_repo_branches;

/// Regression: `list_repo_branches` (which calls `jj_util_import_git_refs`) used to panic with
/// "BUG: Descendants have not been rebased after the last rewrites." when `git::import_refs`
/// abandoned a commit that had jj descendants. The transaction must call `rebase_descendants()`
/// before `tx.commit()`.
///
/// Reproducer: create a git branch with a commit, let jj import it (so jj has a record of the
/// commit and an `@` working-copy on top of repo HEAD), then make the commit unreachable from
/// any git ref. The next import runs `abandon_unreachable_commits` and rewrites history.
#[test]
fn test_list_repo_branches_handles_abandoned_commits() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  // Create a branch with a commit, then import it into jj.
  TestRepo::run_git(&repo.repo_path, &["checkout", "-b", "doomed"])
    .expect("Failed to create branch");
  repo
    .commit_file("doomed.txt", "doomed content", "commit on doomed branch")
    .expect("Failed to commit on doomed branch");
  list_repo_branches(&repo.repo_path).expect("first import should succeed");

  // Make the doomed commit unreachable from any git ref.
  TestRepo::run_git(&repo.repo_path, &["checkout", default_branch])
    .expect("Failed to return to main");
  TestRepo::run_git(&repo.repo_path, &["branch", "-D", "doomed"]).expect("Failed to delete branch");

  // Second import must not panic — jj-lib requires rebase_descendants before commit when
  // commits are rewritten (abandoned).
  let branches = list_repo_branches(&repo.repo_path).expect("list_repo_branches should not panic");
  assert!(
    !branches.iter().any(|b| b.name == "doomed"),
    "doomed branch should be gone after deletion, got: {:?}",
    branches
  );
}
