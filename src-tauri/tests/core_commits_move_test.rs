//! Moving a commit to another workspace must take it out of the source
//! history, not leave it there with an inverse diff in the working copy.
mod common;

use common::{JjVerifier, TestRepo};
use treq_lib::core::move_commit_to_existing_workspace as move_commit;

fn source_with_commit(repo: &TestRepo) -> (treq_lib::local_db::Workspace, String, String) {
  let source = repo
    .create_workspace_with_commit("feat/move-src", "moved.txt", "moved\n", None)
    .unwrap();
  let path = repo.workspace_full_path(&source);
  let change_id = TestRepo::jj_change_id(&path, "@-").unwrap();
  (source, path, change_id)
}

#[test]
fn move_commit_removes_it_from_source_without_inverse_diff() {
  let repo = TestRepo::new().unwrap();
  let (source, source_path, change_id) = source_with_commit(&repo);
  let target = repo.create_workspace_simple("feat/move-dst").unwrap();
  let target_path = repo.workspace_full_path(&target);

  move_commit(&repo.repo_path, source.id, &change_id, target.id).unwrap();

  assert!(JjVerifier::file_exists_in_workspace(
    &target_path,
    "moved.txt"
  ));
  let target_changes = treq_lib::jj::jj_get_changed_files(&target_path).unwrap();
  assert_eq!(target_changes.len(), 1, "{target_changes:?}");
  assert!(!JjVerifier::file_exists_in_workspace(
    &source_path,
    "moved.txt"
  ));
  assert!(TestRepo::jj_working_copy_is_clean(&source_path));
  let source_log = TestRepo::jj_log_descriptions(&source_path, "::@", None).unwrap();
  assert!(
    source_log.iter().all(|(d, _)| d != "Add moved.txt"),
    "{source_log:?}"
  );
}

#[test]
fn move_commit_into_its_own_workspace_is_refused() {
  let repo = TestRepo::new().unwrap();
  let (source, source_path, change_id) = source_with_commit(&repo);

  let result = move_commit(&repo.repo_path, source.id, &change_id, source.id);

  assert!(result.is_err());
  assert!(TestRepo::jj_working_copy_is_clean(&source_path));
  assert!(JjVerifier::file_exists_in_workspace(
    &source_path,
    "moved.txt"
  ));
}

#[test]
fn move_refuses_parent_owned_or_conflicting_commits_without_changes() {
  let repo = TestRepo::new().unwrap();
  let c = repo.create_workspace_with_commit("feat/move-c", "c.txt", "c\n", None);
  let c_path = repo.workspace_full_path(&c.unwrap());
  let c_change = TestRepo::jj_change_id(&c_path, "@-").unwrap();
  let src = Some("feat/move-c");
  let a = repo
    .create_workspace_with_commit("feat/move-src", "moved.txt", "moved\n", src)
    .unwrap();
  let a_path = repo.workspace_full_path(&a);
  let a_change = TestRepo::jj_change_id(&a_path, "@-").unwrap();
  let b = repo.create_workspace_simple("feat/move-dst").unwrap();
  let b_path = repo.workspace_full_path(&b);
  TestRepo::write_workspace_file(&b_path, "moved.txt", "target\n").unwrap();
  treq_lib::jj::jj_get_changed_files(&b_path).unwrap();
  let logs = || [&a_path, &b_path, &c_path].map(|p| TestRepo::jj_log_descriptions(p, "::@", None));
  let before = logs();
  assert!(format!("{:?}", before[0]).contains("Add c.txt"));
  let mv = |id: &str| move_commit(&repo.repo_path, a.id, id, b.id);

  assert!(mv(&c_change).unwrap_err().contains("cannot be rewritten"));
  assert!(mv(&a_change).unwrap_err().contains("conflicts with target"));
  assert_eq!(logs(), before);
  let b_file = std::fs::read_to_string(std::path::Path::new(&b_path).join("moved.txt"));
  assert_eq!(b_file.unwrap(), "target\n");
}
