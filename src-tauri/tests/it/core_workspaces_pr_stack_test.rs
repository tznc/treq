/// Workspaces opened from pull requests follow the PR's base branch: they
/// target it at creation, count commits against it, and move when GitHub
/// retargets the PR or merges the PR underneath.
use std::collections::HashMap;

use crate::common::TestRepo;
use treq_lib::github::PrInfo;

fn pr(number: u64, head: &str, base: &str, state: &str) -> (String, Option<PrInfo>) {
  (
    head.to_string(),
    Some(PrInfo {
      number,
      title: format!("PR {number}"),
      state: state.into(),
      url: format!("https://github.com/o/r/pull/{number}"),
      head_ref_name: head.into(),
      base_ref_name: base.into(),
      merge_state_status: None,
      is_draft: false,
    }),
  )
}

fn target_of(repo: &TestRepo, id: i64) -> Option<String> {
  treq_lib::local_db::get_workspace_by_id(&repo.repo_path, id)
    .expect("read workspace")
    .expect("workspace exists")
    .target_branch
}

#[test]
fn pr_workspace_targets_a_base_that_is_only_on_the_remote() {
  let repo = TestRepo::with_remote().expect("test repo with remote");
  let default = repo.default_branch().to_string();
  repo
    .remote_branch_on("feat/a", &default, "a.txt", "a")
    .expect("push feat/a");
  repo
    .remote_branch_on("feat/b", "feat/a", "b.txt", "b")
    .expect("push feat/b");

  let (workspace, created) = treq_lib::core::open_or_create_workspace_from_pr(
    &repo.repo_path,
    "feat/b",
    "feat/a",
    None,
    None,
  )
  .expect("open PR workspace on a remote-only base");

  assert!(created);
  assert_eq!(workspace.target_branch.as_deref(), Some("feat/a"));
  let status = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace status");
  assert_eq!(
    status.commits_ahead_of_target.len(),
    1,
    "only feat/b's own commit is ahead of feat/a"
  );
}

#[test]
fn workspace_status_counts_commits_against_a_target_with_no_workspace() {
  let repo = TestRepo::with_remote().expect("test repo with remote");
  let default = repo.default_branch().to_string();
  repo
    .remote_branch_on("feat/a", &default, "a.txt", "a")
    .expect("push feat/a");
  repo
    .remote_branch_on("feat/b", "feat/a", "b.txt", "b")
    .expect("push feat/b");
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("fetch");
  treq_lib::jj::jj_bookmark_track(&repo.repo_path, "feat/a", "origin").expect("track feat/a");

  let (workspace, _) = treq_lib::core::open_or_create_workspace_from_pr(
    &repo.repo_path,
    "feat/b",
    "feat/a",
    None,
    None,
  )
  .expect("open PR workspace");

  let status = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace status");
  assert_eq!(status.commits_ahead_of_target.len(), 1);
  assert_eq!(status.partial.commits_ahead, 1);
}

#[test]
fn pr_stack_sync_moves_a_child_off_a_merged_base() {
  let repo = TestRepo::with_remote().expect("test repo with remote");
  let default = repo.default_branch().to_string();
  let a = repo
    .create_workspace_with_commit("feat/a", "a.txt", "a", None)
    .expect("workspace a");
  let b = repo
    .create_workspace_with_commit("feat/b", "b.txt", "b", Some("feat/a"))
    .expect("workspace b");
  assert_eq!(target_of(&repo, b.id).as_deref(), Some("feat/a"));

  let prs: HashMap<_, _> = [
    pr(1, "feat/a", &default, "MERGED"),
    pr(2, "feat/b", "feat/a", "OPEN"),
  ]
  .into_iter()
  .collect();
  assert!(treq_lib::core::pr_stacks::sync_pr_bases(
    &repo.repo_path,
    &prs
  ));

  assert_eq!(target_of(&repo, b.id).as_deref(), Some(default.as_str()));
  assert_eq!(target_of(&repo, a.id).as_deref(), Some(default.as_str()));
}

#[test]
fn pr_stack_sync_follows_a_pr_retargeted_on_github() {
  let repo = TestRepo::with_remote().expect("test repo with remote");
  let default = repo.default_branch().to_string();
  repo
    .create_workspace_with_commit("feat/a", "a.txt", "a", None)
    .expect("workspace a");
  let b = repo
    .create_workspace_with_commit("feat/b", "b.txt", "b", None)
    .expect("workspace b");

  let before: HashMap<_, _> = [pr(2, "feat/b", &default, "OPEN")].into_iter().collect();
  assert!(!treq_lib::core::pr_stacks::sync_pr_bases(
    &repo.repo_path,
    &before
  ));
  assert_eq!(target_of(&repo, b.id).as_deref(), Some(default.as_str()));

  let after: HashMap<_, _> = [pr(2, "feat/b", "feat/a", "OPEN")].into_iter().collect();
  assert!(treq_lib::core::pr_stacks::sync_pr_bases(
    &repo.repo_path,
    &after
  ));
  assert_eq!(target_of(&repo, b.id).as_deref(), Some("feat/a"));
}
