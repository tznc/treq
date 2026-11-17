//! Model-based property test of a default-branch workspace (one on the repo's
//! default branch, targeting itself) while another clone moves the remote.
//!
//! A real bare `origin`, the `TestRepo` clone with its home repo and a
//! default-branch workspace, and an independent "other clone" that pushes to
//! (and force-pushes over) `origin`. Random steps run against all three and
//! the invariants are checked after every step.
//!
//! Local pushes and home-repo commits are not modelled yet: they drop
//! `origin`'s commits until the fixes for diverged default branches land.

use crate::common::{proptest_config, TestRepo};
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use treq_lib::core::MergeCommit;
use treq_lib::local_db::Workspace;

#[derive(Debug, Clone)]
enum Op {
  /// The other clone pushes a new commit on top of `origin`'s default branch.
  RemoteAdvance,
  /// The other clone drops `origin`'s tip and force-pushes a replacement.
  RemoteRewrite,
  /// Pull from the home repo (`pull_workspace_from_remote(None)`).
  HomePull,
  /// Pull from the default-branch workspace.
  WorkspacePull,
  /// Commit a new file in the default-branch workspace.
  WorkspaceCommit,
  /// Leave an uncommitted file in the default-branch workspace.
  WorkspaceDirty,
  /// Create child `feat/c{slot}` stacked on the default branch.
  CreateChild(u8),
  ChildCommit(usize),
  Rebase(usize, bool),
  Merge(usize),
}

fn op() -> impl Strategy<Value = Op> {
  prop_oneof![
    3 => Just(Op::RemoteAdvance),
    2 => Just(Op::RemoteRewrite),
    2 => Just(Op::HomePull),
    2 => Just(Op::WorkspacePull),
    3 => Just(Op::WorkspaceCommit),
    1 => Just(Op::WorkspaceDirty),
    2 => (0u8..2).prop_map(Op::CreateChild),
    2 => any::<usize>().prop_map(Op::ChildCommit),
    2 => (any::<usize>(), any::<bool>()).prop_map(|(i, f)| Op::Rebase(i, f)),
    1 => any::<usize>().prop_map(Op::Merge),
  ]
}

struct World {
  repo: TestRepo,
  main: String,
  main_ws: Workspace,
  other: PathBuf,
  children: BTreeMap<String, Workspace>,
  /// Messages of local commits (never pushed here).
  unpushed: BTreeSet<String>,
  /// Files left uncommitted in the default-branch workspace.
  dirty: BTreeSet<String>,
  seq: usize,
}

fn fail(e: impl std::fmt::Display) -> TestCaseError {
  TestCaseError::fail(e.to_string())
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, TestCaseError> {
  TestRepo::run_git(&cwd.to_string_lossy(), args).map_err(fail)
}

fn ids(cwd: &str, revset: &str) -> Vec<String> {
  let _ = TestRepo::jj_update_stale(cwd);
  TestRepo::jj_commit_ids_in_revset(cwd, revset).unwrap_or_default()
}

impl World {
  fn new() -> Result<Self, TestCaseError> {
    let repo = TestRepo::with_remote().map_err(fail)?;
    let main = repo.default_branch().to_string();
    let main_ws = treq_lib::core::create_workspace(
      &repo.repo_path,
      &main,
      Some("default branch workspace".to_string()),
      None,
      None,
      None,
      None,
    )
    .map_err(fail)?;
    treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, main_ws.id, &main)
      .map_err(fail)?;
    let other = repo
      .remote_path()
      .parent()
      .expect("fixture dir")
      .join("other");
    git(
      other.parent().expect("fixture dir"),
      &[
        "clone",
        &repo.remote_path().to_string_lossy(),
        &other.to_string_lossy(),
      ],
    )?;
    git(&other, &["config", "user.email", "other@example.com"])?;
    git(&other, &["config", "user.name", "Other"])?;
    Ok(Self {
      repo,
      main,
      main_ws,
      other,
      children: BTreeMap::new(),
      unpushed: BTreeSet::new(),
      dirty: BTreeSet::new(),
      seq: 0,
    })
  }

  fn tag(&mut self, kind: &str) -> String {
    self.seq += 1;
    format!("{kind}-{}-", self.seq)
  }

  fn ws_path(&self, ws: &Workspace) -> String {
    self.repo.workspace_full_path(ws)
  }

  fn other_sync(&self) -> Result<(), TestCaseError> {
    git(&self.other, &["fetch", "origin"])?;
    git(&self.other, &["checkout", "-q", &self.main])?;
    git(
      &self.other,
      &["reset", "-q", "--hard", &format!("origin/{}", self.main)],
    )?;
    Ok(())
  }

  fn other_commit(&mut self, force: bool) -> Result<(), TestCaseError> {
    self.other_sync()?;
    if force && git(&self.other, &["rev-parse", "-q", "--verify", "HEAD~1"]).is_ok() {
      git(&self.other, &["reset", "-q", "--hard", "HEAD~1"])?;
    }
    let tag = self.tag(if force { "rewrite" } else { "remote" });
    std::fs::write(self.other.join(format!("{tag}.txt")), &tag).map_err(fail)?;
    git(&self.other, &["add", "-A"])?;
    git(&self.other, &["commit", "-q", "-m", &tag])?;
    let refspec = format!("{}:{}", self.main, self.main);
    if force {
      git(&self.other, &["push", "-q", "--force", "origin", &refspec])?;
    } else {
      git(&self.other, &["push", "-q", "origin", &refspec])?;
    }
    Ok(())
  }

  fn commit_in(&mut self, dir: &str, id: Option<i64>, kind: &str) -> Result<(), TestCaseError> {
    let tag = self.tag(kind);
    TestRepo::write_workspace_file(dir, &format!("{tag}.txt"), &tag).map_err(fail)?;
    treq_lib::core::commit_workspace(&self.repo.repo_path, id, &tag).map_err(fail)?;
    self.unpushed.insert(tag);
    Ok(())
  }

  fn child(&self, i: usize) -> Option<Workspace> {
    (!self.children.is_empty()).then(|| {
      self
        .children
        .values()
        .nth(i % self.children.len())
        .cloned()
        .expect("in range")
    })
  }

  fn apply(&mut self, op: &Op) -> Result<(), TestCaseError> {
    let repo_path = self.repo.repo_path.clone();
    match op {
      Op::RemoteAdvance => self.other_commit(false)?,
      Op::RemoteRewrite => self.other_commit(true)?,
      Op::HomePull => {
        let res =
          treq_lib::core::pull_workspace_from_remote(&repo_path, None, "git").map_err(fail)?;
        prop_assert!(res.success, "{}", res.message);
      }
      Op::WorkspacePull => {
        let res =
          treq_lib::core::pull_workspace_from_remote(&repo_path, Some(self.main_ws.id), "git")
            .map_err(fail)?;
        prop_assert!(res.success, "{}", res.message);
      }
      Op::WorkspaceCommit => {
        let dir = self.ws_path(&self.main_ws);
        self.commit_in(&dir, Some(self.main_ws.id), "ws")?;
        // The commit takes the uncommitted files with it.
        self.dirty.clear();
      }
      Op::WorkspaceDirty => {
        let tag = self.tag("dirty");
        let dir = self.ws_path(&self.main_ws);
        TestRepo::write_workspace_file(&dir, &format!("{tag}.txt"), &tag).map_err(fail)?;
        self.dirty.insert(format!("{tag}.txt"));
      }
      Op::CreateChild(slot) => {
        let branch = format!("feat/c{slot}");
        if self.children.contains_key(&branch) {
          return Ok(());
        }
        let ws = treq_lib::core::create_workspace(
          &repo_path,
          &branch,
          Some(branch.clone()),
          None,
          Some(&self.main),
          None,
          None,
        )
        .map_err(fail)?;
        self.children.insert(branch, ws);
      }
      Op::ChildCommit(i) => {
        if let Some(ws) = self.child(*i) {
          let dir = self.ws_path(&ws);
          self.commit_in(&dir, Some(ws.id), "child")?;
        }
      }
      Op::Rebase(i, force) => {
        if let Some(ws) = self.child(*i) {
          let res = treq_lib::core::check_and_rebase_workspaces(
            &repo_path,
            Some(ws.id),
            Some(self.main.clone()),
            Some(*force),
            "git",
          )
          .map_err(fail)?;
          let hold = treq_lib::core::rebase_guard::hold(&repo_path, ws.id);
          if !self.dirty.is_empty() && *force {
            prop_assert_eq!(
              hold.as_ref(),
              Some(&treq_lib::core::rebase_guard::RebaseHold::ParentDirty {
                branch: self.main.clone()
              })
            );
          }
          let main_conflicted = treq_lib::jj::jj_is_bookmark_conflicted(&repo_path, &self.main);
          if res.success && hold.is_none() && !main_conflicted && *force {
            let dir = self.ws_path(&ws);
            prop_assert!(
              !ids(&dir, &format!("{} & ::@", self.main)).is_empty(),
              "{} not on {} after rebase: {}",
              ws.branch_name,
              self.main,
              res.message
            );
          }
        }
      }
      Op::Merge(i) => {
        if let Some(ws) = self.child(*i) {
          if treq_lib::core::merge_workspace(
            &repo_path,
            ws.id,
            &self.tag("merge"),
            MergeCommit::Merge,
          )
          .is_ok()
          {
            self.children.remove(&ws.branch_name);
          }
        }
      }
    }
    Ok(())
  }

  fn check(&self) -> Result<(), TestCaseError> {
    let repo_path = &self.repo.repo_path;
    let ws_dir = self.ws_path(&self.main_ws);
    // Every local commit not yet pushed is still reachable from a bookmark
    // or a working copy.
    for tag in &self.unpushed {
      let revset = format!("description(substring:\"{tag}\") & ::(bookmarks() | working_copies())");
      prop_assert!(
        !ids(repo_path, &revset).is_empty(),
        "local commit {} lost",
        tag
      );
    }
    // Uncommitted files of the default-branch workspace stay there.
    for file in &self.dirty {
      prop_assert!(Path::new(&ws_dir).join(file).exists(), "{} vanished", file);
      prop_assert!(
        !Path::new(repo_path).join(file).exists(),
        "{} leaked home",
        file
      );
      for ws in self.children.values() {
        prop_assert!(
          !Path::new(&self.ws_path(ws)).join(file).exists(),
          "{} leaked into {}",
          file,
          ws.branch_name
        );
      }
    }
    // Target branches never change behind the user's back.
    let rows = treq_lib::local_db::get_workspaces(repo_path).map_err(fail)?;
    for row in rows.iter().filter(|w| !w.archived) {
      prop_assert_eq!(row.target_branch.as_deref(), Some(self.main.as_str()));
    }
    Ok(())
  }
}

proptest! {
  #![proptest_config(proptest_config(16))]

  #[test]
  fn default_branch_workspace_follows_a_moving_remote(ops in prop::collection::vec(op(), 1..10)) {
    let mut world = World::new()?;
    for op in &ops {
      world.apply(op)?;
      world.check()?;
    }
  }
}
