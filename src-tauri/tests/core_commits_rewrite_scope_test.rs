//! `abandon_commit` / `describe_commit` take a revision string from the CLI and
//! the remote exec channel, so they must refuse commits outside the workspace's
//! own editable lineage instead of rewriting (or panicking on) them.
mod common;

use common::TestRepo;

fn main_commit_id(repo: &TestRepo) -> String {
  TestRepo::run_git(&repo.repo_path, &["rev-parse", repo.default_branch()])
    .unwrap()
    .trim()
    .to_string()
}

#[test]
fn abandon_and_describe_reject_the_root_commit() {
  let repo = TestRepo::new().unwrap();
  let ws = repo.create_workspace_simple("feat/root").unwrap();

  for revision in ["root()", "zzzzzzzz"] {
    let err = treq_lib::core::abandon_commit(&repo.repo_path, ws.id, revision).unwrap_err();
    assert!(err.contains("root commit"), "{err}");
    let err = treq_lib::core::describe_commit(&repo.repo_path, ws.id, revision, "msg").unwrap_err();
    assert!(err.contains("root commit"), "{err}");
  }
}

#[test]
fn abandon_and_describe_reject_default_branch_commits() {
  let repo = TestRepo::new().unwrap();
  let ws = repo
    .create_workspace_with_commit("feat/trunk", "a.txt", "a", None)
    .unwrap();
  let before = main_commit_id(&repo);
  let main = repo.default_branch().to_string();

  let err = treq_lib::core::abandon_commit(&repo.repo_path, ws.id, &main).unwrap_err();
  assert!(err.contains("default branch"), "{err}");
  let err = treq_lib::core::describe_commit(&repo.repo_path, ws.id, &main, "msg").unwrap_err();
  assert!(err.contains("default branch"), "{err}");

  assert_eq!(main_commit_id(&repo), before);
}

#[test]
fn abandon_and_describe_reject_commits_from_another_workspace() {
  let repo = TestRepo::new().unwrap();
  let ws = repo
    .create_workspace_with_commit("feat/mine", "a.txt", "a", None)
    .unwrap();
  let other = repo
    .create_workspace_with_commit("feat/other", "b.txt", "b", None)
    .unwrap();
  let other_path = repo.workspace_full_path(&other);
  let other_commit = TestRepo::jj_change_id(&other_path, "@-").unwrap();

  let err = treq_lib::core::abandon_commit(&repo.repo_path, ws.id, &other_commit).unwrap_err();
  assert!(err.contains("not in this workspace"), "{err}");
  let err =
    treq_lib::core::describe_commit(&repo.repo_path, ws.id, &other_commit, "msg").unwrap_err();
  assert!(err.contains("not in this workspace"), "{err}");

  assert_eq!(
    TestRepo::jj_change_id(&other_path, "@-").unwrap(),
    other_commit
  );
}

#[test]
fn abandon_still_works_for_the_workspace_own_commit() {
  let repo = TestRepo::new().unwrap();
  let ws = repo
    .create_workspace_with_commit("feat/own", "a.txt", "a", None)
    .unwrap();
  let path = repo.workspace_full_path(&ws);
  let own = TestRepo::jj_change_id(&path, "@-").unwrap();

  treq_lib::core::describe_commit(&repo.repo_path, ws.id, &own, "renamed").unwrap();
  treq_lib::core::abandon_commit(&repo.repo_path, ws.id, &own).unwrap();
}
