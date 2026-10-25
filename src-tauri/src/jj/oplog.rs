//! Retention for the jj operation log.
//!
//! Every edit writes an operation whose view lists every workspace and
//! bookmark, so the log (and the commits only it references) grows without
//! bound. [`prune_operation_log`] abandons operations older than a cutoff, the
//! way `jj op abandon ..X` does, with the two extra steps that make it safe
//! for a repo with many workspaces:
//!
//! - `X` is never at or past the oldest operation any workspace's working copy
//!   records, so no workspace points at an abandoned operation.
//! - Abandoning rewrites every descendant of `X` under a new id, so each
//!   workspace's recorded operation id is remapped to its rewritten id.
//!   Without this other workspaces fail with "repo was loaded at operation X,
//!   which seems to be a sibling of the working copy's operation Y".
//!
//! Unreachable operations, views and commits are then garbage collected the
//! way `jj util gc` does.

use super::{
  block_on, create_user_settings, load_home_repo, with_working_copy_lock, JjError, WorkspacePaths,
};
use futures::TryStreamExt as _;
use jj_lib::default_index::DefaultIndexStore;
use jj_lib::op_store::OperationId;
use jj_lib::op_walk;
use jj_lib::repo::{Repo as _, StoreFactories};
use jj_lib::workspace::{default_working_copy_factories, Workspace};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long operations are kept.
pub const OPERATION_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Fewest prunable operations worth the one-time index rebuild a prune costs.
pub const MIN_PRUNABLE_OPERATIONS: usize = 200;

/// What [`prune_operation_log`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationPrune {
  /// Fewer than the minimum prunable operations, or nothing older than the cutoff.
  NothingToPrune,
  /// Operations were abandoned and unreachable data collected.
  Pruned { abandoned: usize },
  /// Skipped without changes, with the reason.
  Skipped(String),
}

fn millis(time: SystemTime) -> i64 {
  match time.duration_since(UNIX_EPOCH) {
    Ok(d) => d.as_millis() as i64,
    Err(_) => 0,
  }
}

fn io_err(context: &str, err: impl std::fmt::Display) -> JjError {
  JjError::IoError(format!("{context}: {err}"))
}

/// Loads every workspace whose directory exists: `(path, workspace)`.
fn load_workspaces(
  repo_path: &str,
  home: Workspace,
  paths: &WorkspacePaths,
  names: Vec<String>,
) -> Result<Vec<(String, Workspace)>, JjError> {
  let home_name = home.workspace_name().as_str().to_string();
  let mut loaded = vec![(repo_path.to_string(), home)];
  let settings = create_user_settings(repo_path)?;
  for name in names {
    if name == home_name {
      continue;
    }
    let Some(stored) = paths.get_by_name(&name) else {
      continue;
    };
    let full = if stored.is_absolute() {
      stored
    } else {
      Path::new(repo_path).join(".jj").join("repo").join(stored)
    };
    let treq_path = Path::new(repo_path)
      .join(".treq")
      .join("workspaces")
      .join(&name);
    let path = if treq_path.exists() { treq_path } else { full };
    if !path.exists() {
      // A deleted checkout cannot be used, so its recorded operation is moot.
      continue;
    }
    let workspace = Workspace::load(
      &settings,
      &path,
      &StoreFactories::default(),
      &default_working_copy_factories(),
    )
    .map_err(|e| io_err(&format!("Failed to load workspace '{name}'"), e))?;
    loaded.push((path.to_string_lossy().into_owned(), workspace));
  }
  Ok(loaded)
}

/// Abandons operations that ended before `cutoff` and are older than every
/// operation a workspace still records, then collects unreachable data.
///
/// Returns [`OperationPrune::NothingToPrune`] unless at least `min_prunable`
/// operations qualify. Callers serialize this per repo.
pub fn prune_operation_log(
  repo_path: &str,
  cutoff: SystemTime,
  min_prunable: usize,
) -> Result<OperationPrune, JjError> {
  let (home, repo, _store) = load_home_repo(repo_path)?;
  let paths = WorkspacePaths::load(home.repo_path())?;
  let head = repo.operation().clone();
  let op_store = repo.op_store().clone();
  let op_heads_store = repo.loader().op_heads_store().clone();
  let names: Vec<String> = repo
    .view()
    .wc_commit_ids()
    .keys()
    .map(|name| name.as_str().to_string())
    .collect();

  let heads =
    block_on(op_heads_store.get_op_heads()).map_err(|e| io_err("Failed to read op heads", e))?;
  if heads.len() != 1 || heads[0] != *head.id() {
    return Ok(OperationPrune::Skipped(
      "the operation log has several heads".into(),
    ));
  }

  // Newest first; ancestors always come after their descendants.
  let ops: Vec<_> = block_on(op_walk::walk_ancestors(std::slice::from_ref(&head)).try_collect())
    .map_err(|e| io_err("Failed to walk the operation log", e))?;
  let position: HashMap<&OperationId, usize> =
    ops.iter().enumerate().map(|(i, op)| (op.id(), i)).collect();

  // The newest candidate must sit after every operation a workspace records.
  let mut newest_recorded = 0usize;
  let workspaces = load_workspaces(repo_path, home, &paths, names)?;
  for (path, workspace) in &workspaces {
    let recorded = workspace.working_copy().operation_id();
    match position.get(recorded) {
      Some(&index) => newest_recorded = newest_recorded.max(index),
      None => {
        return Ok(OperationPrune::Skipped(format!(
          "workspace at {path} records an operation outside the log"
        )));
      }
    }
  }

  let cutoff_ms = millis(cutoff);
  let root_id = op_store.root_operation_id().clone();
  let Some(boundary) = ops.iter().enumerate().position(|(index, op)| {
    index > newest_recorded && op.id() != &root_id && op.metadata().time.end.timestamp.0 < cutoff_ms
  }) else {
    return Ok(OperationPrune::NothingToPrune);
  };
  let boundary_op = ops[boundary].clone();
  // Everything from the boundary down, except the root operation.
  let abandoned = ops.len() - boundary - 1;
  if abandoned < min_prunable {
    return Ok(OperationPrune::NothingToPrune);
  }

  // Reparent the surviving operations onto the root operation.
  let survivors: Vec<_> = block_on(
    op_walk::walk_ancestors_range(
      std::slice::from_ref(&head),
      std::slice::from_ref(&boundary_op),
    )
    .try_collect(),
  )
  .map_err(|e| io_err("Failed to walk the operations to keep", e))?;
  let mut rewritten: HashMap<OperationId, OperationId> = HashMap::new();
  for old in survivors.iter().rev() {
    let mut data = old.store_operation().clone();
    let mut root_once = Some(root_id.clone());
    data.parents = data
      .parents
      .iter()
      .filter_map(|id| rewritten.get(id).cloned().or_else(|| root_once.take()))
      .collect();
    let new_id = block_on(op_store.write_operation(&data))
      .map_err(|e| io_err("Failed to rewrite an operation", e))?;
    rewritten.insert(old.id().clone(), new_id);
  }
  let Some(new_head) = rewritten.get(head.id()).cloned() else {
    return Ok(OperationPrune::NothingToPrune);
  };

  {
    let _lock =
      block_on(op_heads_store.lock()).map_err(|e| io_err("Failed to lock op heads", e))?;
    let current =
      block_on(op_heads_store.get_op_heads()).map_err(|e| io_err("Failed to read op heads", e))?;
    if current != [head.id().clone()] {
      return Ok(OperationPrune::Skipped(
        "the operation log changed while pruning".into(),
      ));
    }
    block_on(op_heads_store.update_op_heads(&[head.id().clone()], &new_head))
      .map_err(|e| io_err("Failed to update op heads", e))?;
  }

  // Point every working copy at the rewritten id of its operation.
  for (path, mut workspace) in workspaces {
    with_working_copy_lock(&path, || -> Result<(), JjError> {
      let locked = workspace
        .start_working_copy_mutation()
        .map_err(|e| io_err(&format!("Failed to lock the working copy at {path}"), e))?;
      let mut locked = locked;
      let Some(new_id) = rewritten
        .get(locked.locked_wc().old_operation_id())
        .cloned()
      else {
        // Recorded an operation that was not rewritten; leave it alone.
        return Ok(());
      };
      block_on(locked.finish(new_id))
        .map_err(|e| io_err(&format!("Failed to save the working copy at {path}"), e))
    })?;
  }

  collect_garbage(repo_path, &new_head, cutoff)?;
  Ok(OperationPrune::Pruned { abandoned })
}

/// What `jj util gc` does: drop unreachable operations and views, rebuild the
/// index from the new head, then drop commits nothing references any more.
fn collect_garbage(
  repo_path: &str,
  head: &OperationId,
  commits_newer: SystemTime,
) -> Result<(), JjError> {
  let (_home, repo, _store) = load_home_repo(repo_path)?;
  if repo.operation().id() != head {
    return Err(JjError::IoError(
      "Operation head moved before garbage collection".into(),
    ));
  }
  // Operations rewritten by this prune are recent files; their unreachable
  // originals stay for a day, which also protects heads a concurrent process
  // may have just created.
  let ops_newer = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
  block_on(repo.op_store().gc(std::slice::from_ref(head), ops_newer))
    .map_err(|e| io_err("Failed to collect operations", e))?;
  if let Some(index_store) = repo
    .loader()
    .index_store()
    .downcast_ref::<DefaultIndexStore>()
  {
    index_store
      .reinit()
      .map_err(|e| io_err("Failed to reset the index", e))?;
  }
  let repo = block_on(repo.loader().load_at(repo.operation()))
    .map_err(|e| io_err("Failed to reload the repo", e))?;
  repo
    .store()
    .gc(repo.index(), commits_newer)
    .map_err(|e| io_err("Failed to collect commits", e))?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::process::Command;
  use tempfile::TempDir;

  fn jj(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("jj")
      .current_dir(dir)
      .env("JJ_USER", "Test User")
      .env("JJ_EMAIL", "test@example.com")
      .args(args)
      .output()
      .expect("jj should run");
    assert!(
      output.status.success(),
      "jj {args:?} in {dir:?} failed: {}",
      String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
  }

  fn op_count(dir: &Path) -> usize {
    jj(
      dir,
      &["op", "log", "--no-graph", "-T", "id.short() ++ \"\\n\""],
    )
    .lines()
    .count()
  }

  /// Edits a file in every workspace and snapshots it, so each records a
  /// fresh operation.
  fn touch_all(dirs: &[&Path], round: usize) {
    for (index, dir) in dirs.iter().enumerate() {
      std::fs::write(dir.join("f.txt"), format!("{round} {index}\n")).expect("write");
      jj(dir, &["status"]);
    }
  }

  fn repo_with_workspaces() -> (TempDir, Vec<std::path::PathBuf>) {
    let temp = TempDir::new().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir(&home).expect("home");
    jj(&home, &["git", "init", "--colocate", "."]);
    let mut dirs = vec![home.clone()];
    for name in ["ws1", "ws2"] {
      let dir = temp.path().join(name);
      jj(
        &home,
        &[
          "workspace",
          "add",
          dir.to_str().expect("utf8"),
          "--name",
          name,
        ],
      );
      dirs.push(dir);
    }
    (temp, dirs)
  }

  #[test]
  fn prune_keeps_recent_operations_and_leaves_every_workspace_usable() {
    let (_temp, dirs) = repo_with_workspaces();
    let refs: Vec<&Path> = dirs.iter().map(|d| d.as_path()).collect();
    for round in 0..4 {
      touch_all(&refs, round);
    }
    let before_cutoff = op_count(&dirs[0]);
    std::thread::sleep(Duration::from_millis(50));
    let cutoff = SystemTime::now();
    std::thread::sleep(Duration::from_millis(50));
    touch_all(&refs, 100);
    let total_before = op_count(&dirs[0]);
    let recent = total_before - before_cutoff;
    assert!(recent >= 3, "each workspace should add a recent operation");

    let home = dirs[0].to_str().expect("utf8");
    let outcome = prune_operation_log(home, cutoff, 1).expect("prune");
    assert!(
      matches!(outcome, OperationPrune::Pruned { .. }),
      "{outcome:?}"
    );

    let total_after = op_count(&dirs[0]);
    assert!(total_after < total_before, "old operations are gone");
    assert!(
      total_after >= recent,
      "recent operations are kept: {total_after} < {recent}"
    );

    // Every workspace still loads, snapshots and shows history.
    for (index, dir) in refs.iter().enumerate() {
      std::fs::write(dir.join("f.txt"), format!("after {index}\n")).expect("write");
      jj(dir, &["status"]);
      jj(
        dir,
        &[
          "log",
          "-r",
          "all()",
          "--no-graph",
          "-T",
          "commit_id ++ \"\\n\"",
        ],
      );
    }
    // And the lib path treq uses reads the repo.
    assert!(super::super::list_jj_workspaces(home).expect("list").len() >= 2);
  }

  #[test]
  fn prune_stops_before_the_oldest_operation_a_workspace_records() {
    let (_temp, dirs) = repo_with_workspaces();
    let refs: Vec<&Path> = dirs.iter().map(|d| d.as_path()).collect();
    touch_all(&refs, 0);
    // ws2 stays on this operation while the others keep working.
    let up_to_stale = op_count(&dirs[0]);
    for round in 1..5 {
      touch_all(&refs[..2], round);
    }
    std::thread::sleep(Duration::from_millis(50));
    let cutoff = SystemTime::now();
    touch_all(&refs[..2], 100);
    let total_before = op_count(&dirs[0]);

    let home = dirs[0].to_str().expect("utf8");
    prune_operation_log(home, cutoff, 1).expect("prune");

    let total_after = op_count(&dirs[0]);
    assert!(
      total_after > total_before - up_to_stale,
      "operations from the stale workspace's operation on are kept"
    );
    assert!(total_after < total_before, "older operations are gone");
    // The stale workspace's operation is still reachable, so it is usable.
    std::fs::write(dirs[2].join("f.txt"), "late\n").expect("write");
    jj(&dirs[2], &["status"]);
    jj(&dirs[1], &["status"]);
  }

  #[test]
  fn prune_does_nothing_below_the_minimum() {
    let (_temp, dirs) = repo_with_workspaces();
    let refs: Vec<&Path> = dirs.iter().map(|d| d.as_path()).collect();
    touch_all(&refs, 0);
    std::thread::sleep(Duration::from_millis(50));
    let cutoff = SystemTime::now();
    touch_all(&refs, 1);
    let before = op_count(&dirs[0]);
    let outcome =
      prune_operation_log(dirs[0].to_str().expect("utf8"), cutoff, 10_000).expect("prune");
    assert_eq!(outcome, OperationPrune::NothingToPrune);
    assert_eq!(op_count(&dirs[0]), before);
  }
}
