use crate::common::TestRepo;

/// A fork clone: `origin` is the user's fork, `upstream` the original repo,
/// which has moved ahead of the fork's default branch.
fn fork_with_upstream_ahead() -> (TestRepo, tempfile::TempDir) {
  let repo = TestRepo::with_remote().expect("repo with origin");
  let dir = tempfile::TempDir::new().unwrap();
  let upstream = dir.path().join("upstream.git");
  TestRepo::run_git(
    &repo.repo_path,
    &["clone", "-q", "--bare", &repo.remote_path().to_string_lossy(), &upstream.to_string_lossy()],
  )
  .expect("clone upstream");
  TestRepo::gix_commit_file_on_ref(
    &upstream.to_string_lossy(),
    repo.default_branch(),
    "upstream.txt",
    "new upstream work\n",
    "upstream moved",
  )
  .expect("commit on upstream");
  TestRepo::run_git(
    &repo.repo_path,
    &["remote", "add", "upstream", &upstream.to_string_lossy()],
  )
  .expect("add upstream");
  (repo, dir)
}

#[test]
fn base_remote_is_upstream_when_present_else_origin() {
  let plain = TestRepo::with_remote().unwrap();
  assert_eq!(treq_lib::jj::jj_base_remote(&plain.repo_path), "origin");
  let (fork, _dir) = fork_with_upstream_ahead();
  assert_eq!(treq_lib::jj::jj_base_remote(&fork.repo_path), "upstream");
}

#[test]
fn fetch_brings_in_upstream_default_branch() {
  let (fork, _dir) = fork_with_upstream_ahead();
  treq_lib::jj::jj_git_fetch(&fork.repo_path).expect("fetch");
  let rev = format!("{}@upstream", fork.default_branch());
  let ids = TestRepo::jj_commit_ids_in_revset(&fork.repo_path, &rev).expect("upstream ref");
  let origin = format!("{}@origin", fork.default_branch());
  let origin_ids = TestRepo::jj_commit_ids_in_revset(&fork.repo_path, &origin).unwrap();
  assert_eq!(ids.len(), 1);
  assert_ne!(ids, origin_ids, "upstream is ahead of the fork");
}

#[test]
fn rebase_target_follows_upstream_when_it_is_ahead() {
  let (fork, _dir) = fork_with_upstream_ahead();
  treq_lib::jj::jj_git_fetch(&fork.repo_path).expect("fetch");
  let target = treq_lib::jj::resolve_branch_revision_for_rebase(&fork.repo_path, fork.default_branch())
    .expect("resolve target");
  let resolved = TestRepo::jj_commit_ids_in_revset(&fork.repo_path, &target).unwrap();
  let upstream = format!("{}@upstream", fork.default_branch());
  let upstream_ids = TestRepo::jj_commit_ids_in_revset(&fork.repo_path, &upstream).unwrap();
  assert_eq!(resolved, upstream_ids);
}
