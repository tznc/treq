//! `jj_split` on a workspace working copy: the two halves must be distinct
//! changes, and selecting a path with no changes must be refused.
use crate::common::TestRepo;

#[test]
fn split_gives_halves_distinct_change_ids_without_prior_snapshot() {
  let repo = TestRepo::new().unwrap();
  let ws = repo.create_workspace_simple("split-ws").unwrap();
  let path = repo.workspace_full_path(&ws);
  TestRepo::write_workspace_file(&path, "a.txt", "a\n").unwrap();
  TestRepo::write_workspace_file(&path, "b.txt", "b\n").unwrap();

  treq_lib::jj::jj_split(&path, "first half", vec!["a.txt".to_string()]).unwrap();

  let first = TestRepo::jj_change_id(&path, "@-").unwrap();
  let second = TestRepo::jj_change_id(&path, "@").unwrap();
  assert_ne!(first, second);
  let all = TestRepo::jj_change_ids_in_revset(&path, "all()").unwrap();
  let unique: std::collections::HashSet<_> = all.iter().collect();
  assert_eq!(all.len(), unique.len(), "divergent change ids: {all:?}");
  assert!(TestRepo::jj_bookmarks_on_revision(&path, "@-")
    .unwrap()
    .contains(&"split-ws".to_string()));
  assert_eq!(
    TestRepo::jj_files_at_revision(&path, "@-")
      .unwrap()
      .iter()
      .filter(|f| f.ends_with(".txt"))
      .collect::<Vec<_>>(),
    vec!["a.txt"]
  );
}

#[test]
fn split_rejects_path_without_changes() {
  let repo = TestRepo::new().unwrap();
  let ws = repo.create_workspace_simple("split-missing").unwrap();
  let path = repo.workspace_full_path(&ws);
  TestRepo::write_workspace_file(&path, "a.txt", "a\n").unwrap();
  let bookmark_before = treq_lib::jj::jj_get_commit_id(&path, "split-missing").unwrap();

  let error = treq_lib::jj::jj_split(&path, "nothing", vec!["missing.txt".to_string()])
    .unwrap_err()
    .to_string();

  assert!(error.contains("invalid_arguments:"), "{error}");
  assert_eq!(
    treq_lib::jj::jj_get_commit_id(&path, "split-missing").unwrap(),
    bookmark_before,
    "a rejected split must not move the bookmark"
  );
}
