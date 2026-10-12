//! Switching branches and committing in the home repo must leave git on that
//! branch (HEAD attached, index matching the commit), so `git status` and the
//! app's branch display agree with jj.
use crate::common::TestRepo;
use treq_lib::core::{get_repo_current_branch, switch_repo_branch};

fn git(repo: &TestRepo, args: &[&str]) -> String {
  TestRepo::run_git(&repo.repo_path, args)
    .unwrap()
    .trim()
    .to_string()
}

#[test]
fn switching_branch_attaches_git_head_to_it() {
  let repo = TestRepo::new().unwrap();
  let default_branch = repo.default_branch().to_string();
  git(&repo, &["checkout", "-b", "feature-x"]);
  TestRepo::write_workspace_file(&repo.repo_path, "x.txt", "x\n").unwrap();
  git(&repo, &["add", "x.txt"]);
  git(
    &repo,
    &[
      "-c",
      "user.name=T",
      "-c",
      "user.email=t@t",
      "commit",
      "-m",
      "x",
    ],
  );
  let feature_tip = git(&repo, &["rev-parse", "HEAD"]);
  git(&repo, &["checkout", &default_branch]);

  switch_repo_branch(&repo.repo_path, "feature-x").unwrap();

  assert_eq!(
    git(&repo, &["symbolic-ref", "HEAD"]),
    "refs/heads/feature-x"
  );
  assert_eq!(git(&repo, &["rev-parse", "HEAD"]), feature_tip);
  let current = get_repo_current_branch(&repo.repo_path).unwrap();
  assert_eq!(current.current_branch.as_deref(), Some("feature-x"));
  assert!(std::path::Path::new(&repo.repo_path).join("x.txt").exists());
  assert_eq!(
    git(&repo, &["status", "--porcelain", "--untracked-files=no"]),
    "",
    "index should match the checked-out commit"
  );
}

#[test]
fn committing_in_the_home_repo_keeps_git_head_on_the_branch() {
  let repo = TestRepo::new().unwrap();
  let default_branch = repo.default_branch().to_string();
  TestRepo::write_workspace_file(&repo.repo_path, "c.txt", "c\n").unwrap();

  // The app lists changes (snapshotting the working copy) before committing.
  treq_lib::core::list_changed_files(&repo.repo_path, None).unwrap();
  treq_lib::jj::jj_split(&repo.repo_path, "home commit", vec!["c.txt".to_string()]).unwrap();

  assert_eq!(
    git(&repo, &["symbolic-ref", "HEAD"]),
    format!("refs/heads/{default_branch}")
  );
  assert_eq!(
    git(&repo, &["log", "-1", "--format=%s", "HEAD"]),
    "home commit"
  );
  assert_eq!(
    git(&repo, &["status", "--porcelain", "--untracked-files=no"]),
    "",
    "index should match the new commit"
  );
}
