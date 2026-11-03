//! Model-based property tests of commit manipulation in a real jj workspace:
//! random sequences of commit, describe, abandon, bookmark set and undo,
//! checked after every step against a plain model of the stack.

use crate::common::{proptest_config, TestRepo};
use proptest::prelude::*;
use std::collections::BTreeMap;
use treq_lib::jj;

#[derive(Debug, Clone)]
enum Op {
  /// Write `f{file}.txt` and commit it.
  Commit(u8),
  Describe(usize, u8),
  Abandon(usize),
  /// Point `pb/{slot}` at a stack commit.
  Bookmark(u8, usize),
  /// Undo the operation of the previous describe, abandon or bookmark step.
  Undo,
}

fn op() -> impl Strategy<Value = Op> {
  prop_oneof![
    3 => (0u8..3).prop_map(Op::Commit),
    2 => (any::<usize>(), 0u8..10).prop_map(|(i, m)| Op::Describe(i, m)),
    2 => any::<usize>().prop_map(Op::Abandon),
    2 => (0u8..2, any::<usize>()).prop_map(|(s, i)| Op::Bookmark(s, i)),
    2 => Just(Op::Undo),
  ]
}

#[derive(Debug, Clone, PartialEq)]
struct Model {
  /// (change id, description), oldest first.
  stack: Vec<(String, String)>,
  /// bookmark → change id (the base change when it fell off the stack).
  bookmarks: BTreeMap<String, String>,
}

fn check(ws: &str, base: &str, model: &Model) -> Result<(), TestCaseError> {
  let mut ids = jj::jj_log_revset_change_ids(ws, &format!("{base}..@-"))
    .map_err(|e| TestCaseError::fail(e.to_string()))?;
  ids.reverse();
  let expected: Vec<String> = model.stack.iter().map(|(c, _)| c.clone()).collect();
  prop_assert_eq!(&ids, &expected, "stack change ids");
  for (change, desc) in &model.stack {
    let actual =
      jj::jj_get_commit_description(ws, change).map_err(|e| TestCaseError::fail(e.to_string()))?;
    prop_assert_eq!(
      actual.trim_end(),
      desc.as_str(),
      "description of {}",
      change
    );
  }
  for (name, change) in &model.bookmarks {
    let on =
      jj::get_bookmarks_on_revision(ws, change).map_err(|e| TestCaseError::fail(e.to_string()))?;
    prop_assert!(on.contains(name), "{} not on {} ({:?})", name, change, on);
  }
  Ok(())
}

fn fail(e: impl ToString) -> TestCaseError {
  TestCaseError::fail(e.to_string())
}

proptest! {
  #![proptest_config(proptest_config(12))]

  #[test]
  fn commit_ops_follow_the_model(ops in prop::collection::vec(op(), 1..8)) {
    let repo = TestRepo::new().map_err(fail)?;
    let w = repo.create_workspace_simple("feat/ops").map_err(fail)?;
    let ws = repo.workspace_full_path(&w);
    let base = jj::jj_get_change_id(&ws, "@-").map_err(fail)?;
    let mut model = Model { stack: Vec::new(), bookmarks: BTreeMap::new() };
    let mut undo: Option<(String, Model)> = None;
    let mut seq = 0u32;
    for op in ops {
      let before = model.clone();
      let mut undoable = false;
      let mut op_to_undo: Option<String> = None;
      match op {
        Op::Commit(file) => {
          seq += 1;
          TestRepo::write_workspace_file(&ws, &format!("f{file}.txt"), &format!("v{seq}\n")).map_err(fail)?;
          let msg = format!("commit {seq}");
          jj::jj_commit(&ws, &msg).map_err(fail)?;
          let change = jj::jj_get_change_id(&ws, "@-").map_err(fail)?;
          model.stack.push((change, msg));
        }
        Op::Describe(i, m) => {
          if model.stack.is_empty() { continue; }
          let i = i % model.stack.len();
          let msg = format!("described {m}");
          jj::jj_describe(&ws, &model.stack[i].0, &msg).map_err(fail)?;
          model.stack[i].1 = msg;
          undoable = true;
        }
        Op::Abandon(i) => {
          if model.stack.is_empty() { continue; }
          let i = i % model.stack.len();
          let (gone, _) = model.stack.remove(i);
          // Undo the operation abandon reports, as the app does; the log
          // below can record a snapshot on top of it.
          op_to_undo = Some(jj::jj_abandon(&ws, &gone).map_err(fail)?);
          let parent = if i == 0 { base.clone() } else { model.stack[i - 1].0.clone() };
          for target in model.bookmarks.values_mut() {
            if *target == gone { *target = parent.clone(); }
          }
          let gone_from_stack = jj::jj_log_revset_change_ids(&ws, &format!("{base}..@ & change_id({gone})")).map_err(fail)?.is_empty();
          prop_assert!(gone_from_stack);
          undoable = true;
        }
        Op::Bookmark(slot, i) => {
          if model.stack.is_empty() { continue; }
          let i = i % model.stack.len();
          let name = format!("pb/{slot}");
          jj::jj_set_bookmark(&ws, &name, &model.stack[i].0).map_err(fail)?;
          model.bookmarks.insert(name, model.stack[i].0.clone());
          undoable = true;
        }
        Op::Undo => {
          let Some((op_id, prev)) = undo.take() else { continue; };
          jj::jj_undo_operation(&ws, &op_id).map_err(fail)?;
          model = prev;
          // The git backend keeps committer time to the second, so redoing the
          // undone rewrite within the same second rebuilds the same commit id,
          // which jj-lib rejects as "already exists".
          std::thread::sleep(std::time::Duration::from_millis(1100));
        }
      }
      undo = if undoable {
        let op_id = match op_to_undo {
          Some(id) => id,
          None => jj::current_operation_id(&ws).map_err(fail)?,
        };
        Some((op_id, before))
      } else {
        None
      };
      check(&ws, &base, &model)?;
    }
  }

  /// Stash moves working-copy edits out and applying the stash brings back
  /// the same file contents.
  #[test]
  fn stash_round_trips_file_contents(files in prop::collection::btree_map(0u8..4, "[a-z]{1,8}", 1..4)) {
    let repo = TestRepo::new().map_err(fail)?;
    let w = repo.create_workspace_simple("feat/stash-prop").map_err(fail)?;
    let ws = repo.workspace_full_path(&w);
    for (k, v) in &files {
      TestRepo::write_workspace_file(&ws, &format!("s{k}.txt"), &format!("{v}\n")).map_err(fail)?;
    }
    let entry = treq_lib::core::stash_workspace_changes(&repo.repo_path, Some(w.id)).map_err(fail)?;
    for k in files.keys() {
      let stashed_away = !std::path::Path::new(&ws).join(format!("s{k}.txt")).exists();
      prop_assert!(stashed_away);
    }
    jj::jj_apply_stash_commit(&ws, &entry.commit_id).map_err(fail)?;
    for (k, v) in &files {
      let got = std::fs::read_to_string(std::path::Path::new(&ws).join(format!("s{k}.txt"))).map_err(fail)?;
      prop_assert_eq!(got, format!("{v}\n"));
    }
  }
}
