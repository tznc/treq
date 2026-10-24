#![allow(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  clippy::string_slice,
  clippy::print_stdout,
  clippy::print_stderr,
  reason = "measurement code"
)]
//! Round 6 measurements for multi-window and merge/sync/push flows. Ignored:
//! run one scenario at a time, under strace to count process spawns and
//! fsyncs per phase:
//!
//! ```text
//! strace -f -tt -e trace=execve,fdatasync,fsync,statx,newfstatat,inotify_add_watch,openat \
//!   -o /tmp/x.strace target/debug/deps/perf_r6_windows_flows-* --ignored --exact poll --nocapture
//! ```
//!
//! `mark(name)` stats `/__mark__/<name>`, which shows up in the trace.

use std::{fs, time::Instant};
use notify::Watcher as _;

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
  eprintln!("PHASE {name}: {ms:.1} ms");
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

/// Opening the same repository in 3 windows: each runs `prepare_repo`.
#[test]
#[ignore = "measurement"]
fn open_same_repo_three_times() {
  let repo = with_workspaces(25);
  let path = repo.repo_path.as_str();
  for i in 0..3 {
    timed(&format!("init_deferring_rebase_{i}"), || {
      core::init_deferring_rebase(path).unwrap();
    });
  }
  timed("init_with_rebase_sync_25ws", || {
    core::init(path).unwrap();
  });
  // Let the 3 background rebase passes finish.
  std::thread::sleep(std::time::Duration::from_secs(8));
  mark("rebase_passes_done");
}

/// pull + push (the Sync button), and a push of a new branch (Create PR).
#[test]
#[ignore = "measurement"]
fn sync_push() {
  let repo = TestRepo::with_remote().unwrap();
  let ws = repo
    .setup_workspace_with_pushed_commit("feat/sync", "a.txt", "a\n")
    .unwrap();
  let full = repo.workspace_full_path(&ws);
  // Remote moves on the default branch (unrelated to the workspace).
  repo
    .remote_commit_file("remote.txt", "r\n", "remote commit")
    .unwrap();
  // New local commit to push.
  TestRepo::write_workspace_file(&full, "b.txt", "b\n").unwrap();
  core::commit_workspace(&repo.repo_path, ws.id, "second").unwrap();
  let style = "diff";
  timed("pull", || {
    core::pull_workspace_from_remote(&repo.repo_path, Some(ws.id), style).unwrap();
  });
  timed("push", || {
    core::push_workspace_to_remote(&repo.repo_path, Some(ws.id)).unwrap();
  });
  // Nothing to do: the no-op sync a user triggers by clicking twice.
  timed("sync_noop_pull", || {
    core::pull_workspace_from_remote(&repo.repo_path, Some(ws.id), style).unwrap();
  });
  timed("sync_noop_push", || {
    core::push_workspace_to_remote(&repo.repo_path, Some(ws.id)).unwrap();
  });
  // Create PR on a branch that is not on the remote yet.
  let ws2 = repo
    .create_workspace_with_commit("feat/newpr", "c.txt", "c\n", None)
    .unwrap();
  timed("push_new_branch", || {
    core::push_workspace_to_remote(&repo.repo_path, Some(ws2.id)).unwrap();
  });
}

/// The three merge strategies on a workspace with 5 commits.
#[test]
#[ignore = "measurement"]
fn merge() {
  for (name, strategy) in [
    ("merge", core::MergeCommit::Merge),
    ("squash", core::MergeCommit::SquashAndMerge),
    ("rebase", core::MergeCommit::RebaseAndMerge),
  ] {
    let repo = TestRepo::new().unwrap();
    let ws = repo.create_workspace_simple("feat/m").unwrap();
    let full = repo.workspace_full_path(&ws);
    for i in 1..=5 {
      TestRepo::write_workspace_file(&full, "h.txt", &format!("v{i}\n")).unwrap();
      core::commit_workspace(&repo.repo_path, ws.id, &format!("c{i}")).unwrap();
    }
    timed(&format!("preview_{name}"), || {
      let _ = treq_lib::core::merge_preview::merge_preview(&repo.repo_path, ws.id);
    });
    timed(&format!("merge_{name}"), || {
      core::merge_workspace_removing_directory_later(&repo.repo_path, ws.id, "msg", strategy)
        .unwrap();
    });
  }
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
    let _ = treq_lib::auto_rebase::rebase_workspaces_for_target(&repo.repo_path, "feat/parent", "diff");
  });
}

/// inotify cost of a recursive watch over a tree with `dirs` directories.
#[test]
#[ignore = "measurement"]
fn watch_tree() {
  let dirs: usize = std::env::var("DIRS").ok().and_then(|v| v.parse().ok()).unwrap_or(5000);
  let tmp = tempfile::tempdir().unwrap();
  let root = tmp.path();
  fs::create_dir_all(root.join("src")).unwrap();
  for i in 0..dirs {
    fs::create_dir_all(root.join(format!("node_modules/pkg{}/lib/sub", i % (dirs / 3 + 1)))).unwrap();
  }
  let start = Instant::now();
  mark("begin_watch");
  let mut debouncer = notify_debouncer_full::new_debouncer(
    std::time::Duration::from_millis(500),
    None,
    |_r: notify_debouncer_full::DebounceEventResult| {},
  )
  .unwrap();
  debouncer.watch(root, notify::RecursiveMode::Recursive).unwrap();
  mark("end_watch");
  eprintln!("PHASE watch_setup: {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
  drop(debouncer);
}

/// Does a status poll with nothing changed still commit to local.db?
#[test]
#[ignore = "measurement"]
fn poll_writes() {
  let repo = with_workspaces(3);
  let path = repo.repo_path.as_str();
  let _ = core::list_workspace_statuses(path).unwrap();
  let db = format!("{path}/.treq/local.db");
  let observer = rusqlite::Connection::open(&db).unwrap();
  let dump = |c: &rusqlite::Connection| -> String {
    let mut s = c.prepare("SELECT * FROM workspaces").unwrap();
    let n = s.column_count();
    let _ = &mut s;
    let rows = s
      .query_map([], |r| {
        Ok((0..n).map(|i| format!("{:?}", r.get_ref(i).unwrap())).collect::<Vec<_>>().join("|"))
      })
      .unwrap();
    rows.map(|r| r.unwrap()).collect::<Vec<_>>().join("\n")
  };
  let version = |c: &rusqlite::Connection| -> i64 {
    c.query_row("PRAGMA data_version", [], |r| r.get(0)).unwrap()
  };
  let (v0, d0) = (version(&observer), dump(&observer));
  let _ = core::list_workspace_statuses(path).unwrap();
  let (v1, d1) = (version(&observer), dump(&observer));
  eprintln!("data_version {v0} -> {v1}; rows changed: {}", d0 != d1);
  let page_changes = observer.query_row("PRAGMA total_changes", [], |r| r.get::<_, i64>(0)).unwrap();
  eprintln!("observer total_changes {page_changes}");
  let tables: Vec<String> = observer.prepare("SELECT name FROM sqlite_master WHERE type='table'").unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
  for t in &tables {
    let n: i64 = observer.query_row(&format!("SELECT count(*) FROM \"{t}\""), [], |r| r.get(0)).unwrap();
    eprintln!("table {t}: {n} rows");
  }
  if d0 != d1 {
    for (a, b) in d0.lines().zip(d1.lines()).filter(|(a, b)| a != b) {
      eprintln!("- {a}\n+ {b}");
    }
  }
}

/// Which part of a status poll commits to local.db?
#[test]
#[ignore = "measurement"]
fn poll_writes_split() {
  let repo = with_workspaces(3);
  let path = repo.repo_path.as_str();
  let _ = core::list_workspace_statuses(path).unwrap();
  timed("discover", || {
    let _ = treq_lib::jj::discover_workspaces_with_conflicts(path).unwrap();
  });
  let d = treq_lib::jj::discover_workspaces_with_conflicts(path).unwrap();
  let now = "2026-01-01T00:00:00Z";
  timed("sync_discovered_1", || {
    let _ = treq_lib::local_db::sync_discovered_workspaces(path, &d, now).unwrap();
  });
  timed("sync_discovered_2", || {
    let _ = treq_lib::local_db::sync_discovered_workspaces(path, &d, now).unwrap();
  });
}

/// `merge_workspace`'s steps one by one (squash), to split its time.
#[test]
#[ignore = "measurement"]
fn merge_steps() {
  use treq_lib::jj;
  let repo = TestRepo::new().unwrap();
  let ws = repo.create_workspace_simple("feat/m").unwrap();
  let full = repo.workspace_full_path(&ws);
  for i in 1..=5 {
    TestRepo::write_workspace_file(&full, "h.txt", &format!("v{i}\n")).unwrap();
    core::commit_workspace(&repo.repo_path, ws.id, &format!("c{i}")).unwrap();
  }
  let target = repo.default_branch().to_string();
  let r = repo.repo_path.as_str();
  timed("step_abandon_empty", || {
    let _ = jj::jj_abandon_empty_commits(&full, &target);
  });
  timed("step_commits_ahead", || {
    let _ = jj::jj_get_commits_ahead(&full, &target).unwrap();
  });
  timed("step_squash", || {
    jj::jj_squash_merge_commit(&full, "feat/m", &target, "msg").unwrap();
  });
  timed("step_edit_bookmark_home", || {
    jj::jj_edit_bookmark(r, &target).unwrap();
  });
  timed("step_forget_workspace", || {
    jj::forget_workspace(r, &full).unwrap();
  });
  timed("step_db_delete", || {
    treq_lib::local_db::delete_workspace(r, ws.id).unwrap();
  });
}
