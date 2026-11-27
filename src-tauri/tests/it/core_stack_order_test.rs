/// Reordering a stack retargets and rebases its workspaces, and refuses a
/// stack that branches.
use crate::common::TestRepo;
use treq_lib::core::stack_order::{reorder_workspace, Reorder};

fn target_of(repo: &TestRepo, id: i64) -> Option<String> {
  treq_lib::local_db::get_workspace_by_id(&repo.repo_path, id)
    .expect("read workspace")
    .expect("workspace exists")
    .target_branch
}

#[test]
fn moving_a_workspace_up_swaps_it_with_the_one_above() {
  let repo = TestRepo::new().expect("test repo");
  let default = repo.default_branch().to_string();
  let a = repo
    .create_workspace_with_commit("so/a", "a.txt", "a\n", None)
    .expect("a");
  let b = repo
    .create_workspace_with_commit("so/b", "b.txt", "b\n", Some("so/a"))
    .expect("b");
  let c = repo
    .create_workspace_with_commit("so/c", "c.txt", "c\n", Some("so/b"))
    .expect("c");

  reorder_workspace(&repo.repo_path, b.id, Reorder::Up).expect("move b up");

  assert_eq!(target_of(&repo, a.id).as_deref(), Some(default.as_str()));
  assert_eq!(target_of(&repo, c.id).as_deref(), Some("so/a"));
  assert_eq!(target_of(&repo, b.id).as_deref(), Some("so/c"));
  let c_under_b =
    TestRepo::jj_commit_ids_in_revset(&repo.repo_path, "::so/b & so/c").expect("revset");
  let b_under_c =
    TestRepo::jj_commit_ids_in_revset(&repo.repo_path, "::so/c & so/b").expect("revset");
  assert_eq!(c_under_b.len(), 1, "c now sits under b");
  assert!(b_under_c.is_empty(), "b no longer sits under c");
}

#[test]
fn taking_a_workspace_out_closes_the_gap() {
  let repo = TestRepo::new().expect("test repo");
  let default = repo.default_branch().to_string();
  repo
    .create_workspace_with_commit("so/a", "a.txt", "a\n", None)
    .expect("a");
  let b = repo
    .create_workspace_with_commit("so/b", "b.txt", "b\n", Some("so/a"))
    .expect("b");
  let c = repo
    .create_workspace_with_commit("so/c", "c.txt", "c\n", Some("so/b"))
    .expect("c");

  reorder_workspace(&repo.repo_path, b.id, Reorder::Out).expect("take b out");

  assert_eq!(target_of(&repo, c.id).as_deref(), Some("so/a"));
  assert_eq!(target_of(&repo, b.id).as_deref(), Some(default.as_str()));
}

#[test]
fn a_branching_stack_is_not_reordered() {
  let repo = TestRepo::new().expect("test repo");
  repo
    .create_workspace_with_commit("so/a", "a.txt", "a\n", None)
    .expect("a");
  let b = repo
    .create_workspace_with_commit("so/b", "b.txt", "b\n", Some("so/a"))
    .expect("b");
  repo
    .create_workspace_with_commit("so/c", "c.txt", "c\n", Some("so/a"))
    .expect("c");

  let err = reorder_workspace(&repo.repo_path, b.id, Reorder::Down).unwrap_err();

  assert!(err.contains("branches"), "{err}");
  assert_eq!(target_of(&repo, b.id).as_deref(), Some("so/a"));
}
