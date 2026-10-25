#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  clippy::print_stdout,
  reason = "measurement code"
)]
//! Round 6 measurements for multi-window and merge/sync/push flows. Ignored:
//! run one scenario at a time, under strace to count process spawns and
//! fsyncs per phase:
//!
//! ```text
//! strace -f -tt -e trace=execve,fdatasync,fsync,statx,newfstatat,inotify_add_watch,openat \
//!   -o /tmp/x.strace target/debug/deps/perf_r6_storage_cli-* --ignored --exact poll --nocapture
//! ```
//!
//! `mark(name)` stats `/__mark__/<name>`, which shows up in the trace.

use std::{fs, time::Instant};

#[path = "common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;
use treq_lib::core;

fn mark(name: &str) {
  let _ = fs::metadata(format!("/__mark__/{name}"));
}

fn timed<T>(name: &str, f: impl FnOnce() -> T) -> T {
  mark(&format!("begin_{name}"));
  let start = Instant::now();
  let out = f();
  let ms = start.elapsed().as_secs_f64() * 1000.0;
  mark(&format!("end_{name}"));
  println!("PHASE {name}: {ms:.1} ms");
  out
}

fn with_workspaces(count: usize) -> TestRepo {
  let repo = TestRepo::new().unwrap();
  for index in 1..=count {
    let ws = core::create_workspace(
      &repo.repo_path,
      &format!("feat/w-{index}"),
      None,
      None,
      None,
      None,
      None,
    )
    .unwrap();
    fs::write(
      repo
        .workspaces_dir()
        .join(&ws.workspace_path)
        .join(format!("w-{index}.txt")),
      "agent edit\n",
    )
    .unwrap();
  }
  repo
}

/// One window's `Read::POLL` (workspaces + statuses), three times.
#[test]
#[ignore = "measurement"]
fn poll() {
  let repo = with_workspaces(5);
  let path = repo.repo_path.as_str();
  // warm
  let _ = core::list_workspace_statuses(path);
  for i in 0..3 {
    timed(&format!("poll_{i}"), || {
      let _ = core::list_workspaces(path).unwrap();
      let _ = core::list_workspace_statuses(path).unwrap();
    });
  }
  timed("list_workspaces_only", || {
    let _ = core::list_workspaces(path).unwrap();
  });
  timed("list_statuses_only", || {
    let _ = core::list_workspace_statuses(path).unwrap();
  });
  // Two windows polling at the same moment.
  timed("poll_two_windows_concurrent", || {
    std::thread::scope(|s| {
      for _ in 0..2 {
        s.spawn(|| {
          let _ = core::list_workspaces(path).unwrap();
          let _ = core::list_workspace_statuses(path).unwrap();
        });
      }
    });
  });
}

/// A commit on a parent branch that has `children` workspaces targeting it.
#[test]
#[ignore = "measurement"]
fn rebase_children() {
  let repo = TestRepo::new().unwrap();
  let parent = repo
    .create_workspace_with_commit("feat/parent", "p.txt", "p\n", None)
    .unwrap();
  for i in 0..6 {
    let child = core::create_workspace(
      &repo.repo_path,
      &format!("feat/child-{i}"),
      None,
      None,
      Some("feat/parent"),
      None,
      None,
    )
    .unwrap();
    treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, child.id, "feat/parent")
      .unwrap();
  }
  let full = repo.workspace_full_path(&parent);
  TestRepo::write_workspace_file(&full, "p2.txt", "p2\n").unwrap();
  timed("commit_with_6_children", || {
    core::commit_workspace(&repo.repo_path, parent.id, "p2").unwrap();
  });
  TestRepo::write_workspace_file(&full, "p3.txt", "p3\n").unwrap();
  timed("commit_with_6_children_2", || {
    core::commit_workspace(&repo.repo_path, parent.id, "p3").unwrap();
  });
  timed("rebase_for_target_noop", || {
    let _ =
      treq_lib::auto_rebase::rebase_workspaces_for_target(&repo.repo_path, "feat/parent", "diff");
  });
}

/// Squash merge: time until `merge_workspace_removing_directory_later`
/// returns, and until the home checkout holds the merged file.
#[test]
#[ignore = "measurement"]
fn merge() {
  let repo = TestRepo::new().unwrap();
  let ws = repo.create_workspace_simple("feat/m").unwrap();
  let full = repo.workspace_full_path(&ws);
  for i in 1..=5 {
    TestRepo::write_workspace_file(&full, "h.txt", &format!("v{i}\n")).unwrap();
    core::commit_workspace(&repo.repo_path, ws.id, &format!("c{i}")).unwrap();
  }
  timed("merge_returns", || {
    core::merge_workspace_removing_directory_later(
      &repo.repo_path,
      ws.id,
      "msg",
      core::MergeCommit::SquashAndMerge,
    )
    .unwrap();
  });
  timed("poll_after_merge", || {
    let _ = core::list_workspace_statuses(&repo.repo_path).unwrap();
  });
}
