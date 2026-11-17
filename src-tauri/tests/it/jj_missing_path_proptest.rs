//! The read-only `jj` queries that take a workspace or repo path return their
//! empty/default value for a path that does not exist, instead of an IO error
//! (AGENTS.md: a deleted workspace must not break the screens that poll it).

use crate::common::proptest_config;
use proptest::prelude::*;

proptest! {
  #![proptest_config(proptest_config(16))]

  #[test]
  fn read_only_queries_default_on_missing_paths(name in "[a-zA-Z0-9_-]{1,12}(/[a-zA-Z0-9_.-]{1,8}){0,2}") {
    let dir = tempfile::tempdir().map_err(|e| TestCaseError::fail(e.to_string()))?;
    let missing = dir.path().join("gone").join(&name);
    let path = missing.to_string_lossy();
    prop_assert_eq!(treq_lib::jj::get_workspace_branch(&path).ok(), Some(String::new()));
    prop_assert_eq!(treq_lib::jj::list_workspaces(&path).ok().map(|v| v.len()), Some(0));
    prop_assert_eq!(treq_lib::jj::jj_working_copy_commit_hex(&path), None);
    prop_assert!(!treq_lib::jj::is_jj_workspace(&path));
    prop_assert!(!treq_lib::jj::jj_is_bookmark_conflicted(&path, "main"));
    prop_assert!(!treq_lib::jj::is_local_branch_behind_remote(&path, "main"));
    prop_assert!(treq_lib::jj::get_git_remotes(&path).is_empty());
    prop_assert!(treq_lib::jj::remove_workspace_directory_only(&path).is_ok());
  }
}
