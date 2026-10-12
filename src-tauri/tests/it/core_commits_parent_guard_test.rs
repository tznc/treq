//! A workspace stacked on another workspace's bookmark sees the parent's
//! commits in its `::@`. No history edit run from the child may rewrite them.
use crate::common::TestRepo;
use treq_lib::core;

const GUARD: &str = "belongs to parent branch 'feat/b'";

/// Runs `op(repo, child_id, parent_change, dest_id)` from child workspace A
/// (stacked on B) against B's commit. Expects an error containing `expected`
/// (or success when it is empty) and B's history unchanged.
fn guarded<T: std::fmt::Debug>(
  expected: &str,
  op: impl FnOnce(&str, i64, &str, i64) -> Result<T, String>,
) {
  let repo = TestRepo::new().unwrap();
  let parent = repo.create_workspace_with_commit("feat/b", "b.txt", "b\n", None);
  let parent_path = repo.workspace_full_path(&parent.unwrap());
  let parent_change = TestRepo::jj_change_id(&parent_path, "@-").unwrap();
  let child = repo.create_workspace_with_commit("feat/a", "a.txt", "a\n", Some("feat/b"));
  let child = child.unwrap();
  let child_path = repo.workspace_full_path(&child);
  let dest = repo.create_workspace_simple("feat/dest").unwrap();
  let ids =
    || [&parent_path, &child_path].map(|p| TestRepo::jj_commit_ids_in_revset(p, "::@-").unwrap());
  let before = ids();

  match op(&repo.repo_path, child.id, &parent_change, dest.id) {
    Err(err) => assert!(err.contains(expected), "unexpected error: {err}"),
    Ok(ok) => assert!(expected.is_empty(), "expected rejection, got {ok:?}"),
  }
  let after = ids();
  assert_eq!(after[0], before[0]);
  assert!(expected.is_empty() || after[1] == before[1]);
}

#[test]
fn abandon_refuses_parent_commit() {
  guarded(GUARD, |repo, a, c, _| core::abandon_commit(repo, a, c));
}

#[test]
fn describe_refuses_parent_commit() {
  guarded(GUARD, |repo, a, c, _| {
    core::describe_commit(repo, a, c, "renamed")
  });
}

#[test]
fn move_commit_refuses_parent_commit() {
  guarded(GUARD, core::move_commit_to_existing_workspace);
}

#[test]
fn stash_commit_refuses_parent_commit() {
  guarded(GUARD, |repo, a, c, _| core::stash_commit(repo, Some(a), c));
}

#[test]
fn undo_commit_refuses_parent_commit() {
  guarded(GUARD, |repo, a, c, _| core::undo_commit(repo, a, c));
}

#[test]
fn shift_timestamp_refuses_parent_commit() {
  guarded(GUARD, |repo, a, c, _| {
    core::shift_commit_timestamp(repo, a, c, 1, 0, 0, None)
  });
}

#[test]
fn move_changes_refuses_parent_commit() {
  guarded("not found in source workspace history", |repo, _, c, _| {
    let commits = vec![c.to_string()];
    let request = core::WorkspaceMoveRequest {
      files: vec![],
      hunks: vec![],
      commits,
    };
    core::move_workspace_changes(repo, "feat/a", "feat/dest", request)
  });
}

/// The range operation shifts A's own commits and stops at B's bookmark.
#[test]
fn shift_mutable_to_now_skips_parent_commits() {
  guarded("", |repo, a, _, _| {
    core::shift_mutable_commits_to_now(repo, a)
  });
}
