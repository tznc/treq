//! Model-based property test of the workspace lifecycle: random sequences of
//! create, delete, archive and open ("switch to") against a real jj repo,
//! checked after every step against a plain set of the live branches.

use crate::common::{proptest_config, JjVerifier, TestRepo};
use proptest::prelude::*;
use std::collections::BTreeMap;
use treq_lib::local_db::Workspace;

#[derive(Debug, Clone)]
enum Op {
  /// Create a workspace for branch `feat/p{slot}` unless one is live.
  Create(u8),
  Delete(usize),
  Archive(usize),
  /// Open a live workspace the way switching to it does: read its `@`.
  Switch(usize),
}

fn op() -> impl Strategy<Value = Op> {
  prop_oneof![
    3 => (0u8..3).prop_map(Op::Create),
    2 => any::<usize>().prop_map(Op::Delete),
    1 => any::<usize>().prop_map(Op::Archive),
    1 => any::<usize>().prop_map(Op::Switch),
  ]
}

fn pick(live: &BTreeMap<String, Workspace>, i: usize) -> Option<Workspace> {
  (!live.is_empty()).then(|| {
    live
      .values()
      .nth(i % live.len())
      .cloned()
      .expect("in range")
  })
}

fn check(
  repo: &TestRepo,
  live: &BTreeMap<String, Workspace>,
  gone: &[Workspace],
) -> Result<(), TestCaseError> {
  let rows = treq_lib::local_db::get_workspaces(&repo.repo_path).map_err(TestCaseError::fail)?;
  let mut db_branches: Vec<String> = rows
    .iter()
    .filter(|w| !w.archived)
    .map(|w| w.branch_name.clone())
    .collect();
  db_branches.sort();
  prop_assert_eq!(db_branches, live.keys().cloned().collect::<Vec<_>>());
  let jj_names = JjVerifier::list_workspaces(&repo.repo_path).map_err(TestCaseError::fail)?;
  for w in live.values() {
    prop_assert!(
      jj_names.contains(&w.workspace_name),
      "{} missing from jj {:?}",
      w.workspace_name,
      jj_names
    );
    prop_assert!(
      repo.workspaces_dir().join(&w.workspace_path).is_dir(),
      "{} has no directory",
      w.workspace_path
    );
  }
  for w in gone {
    let reused = live.values().any(|l| l.workspace_name == w.workspace_name);
    prop_assert!(
      reused || !jj_names.contains(&w.workspace_name),
      "{} still in jj {:?}",
      w.workspace_name,
      jj_names
    );
  }
  Ok(())
}

proptest! {
  #![proptest_config(proptest_config(16))]

  #[test]
  fn workspace_lifecycle_follows_the_model(ops in prop::collection::vec(op(), 1..7)) {
    let repo = TestRepo::new().map_err(TestCaseError::fail)?;
    let mut live: BTreeMap<String, Workspace> = BTreeMap::new();
    let mut gone: Vec<Workspace> = Vec::new();
    for op in ops {
      match op {
        Op::Create(slot) => {
          let branch = format!("feat/p{slot}");
          if live.contains_key(&branch) {
            continue;
          }
          let w = repo.create_workspace_simple(&branch).map_err(TestCaseError::fail)?;
          prop_assert_eq!(&w.branch_name, &branch);
          live.insert(branch, w);
        }
        Op::Delete(i) => if let Some(w) = pick(&live, i) {
          let deleted = treq_lib::core::delete_workspace(&repo.repo_path, &w.id).map_err(TestCaseError::fail)?;
          prop_assert!(deleted);
          prop_assert!(!repo.workspaces_dir().join(&w.workspace_path).exists());
          live.remove(&w.branch_name);
          gone.push(w);
        },
        Op::Archive(i) => if let Some(w) = pick(&live, i) {
          treq_lib::core::archive_workspace(&repo.repo_path, &w.id).map_err(TestCaseError::fail)?;
          live.remove(&w.branch_name);
          gone.push(w);
        },
        Op::Switch(i) => if let Some(w) = pick(&live, i) {
          let path = repo.workspaces_dir().join(&w.workspace_path);
          let path = path.to_string_lossy();
          prop_assert!(treq_lib::jj::is_jj_workspace(&path));
          prop_assert!(treq_lib::jj::jj_working_copy_commit_hex(&path).is_some());
        },
      }
      check(&repo, &live, &gone)?;
    }
  }
}
