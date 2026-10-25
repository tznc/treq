//! Round 7 scale/growth probes (ignored). Disk and load-time growth of the jj
//! op log, and poll cost at hundreds of workspaces.
//!
//! ```text
//! GROWTH_OPS=3000 cargo test --lib growth_perf::oplog_grow -- --ignored --nocapture
//! GROWTH_WS=/path/ws GROWTH_REPO=/path cargo test --lib growth_perf::probe_clean -- --ignored --nocapture
//! WS_COUNT=500 cargo test --lib growth_perf::many_workspaces -- --ignored --nocapture
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stderr, reason = "perf probe")]

use std::{fs, path::Path, time::Instant};

use crate::test_repo::TestRepo;

fn dir_stats(p: &Path) -> (u64, usize) {
  let (mut bytes, mut files) = (0, 0);
  if let Ok(rd) = fs::read_dir(p) {
    for e in rd.flatten() {
      let m = e.metadata().unwrap();
      if m.is_dir() {
        let (b, f) = dir_stats(&e.path());
        bytes += b;
        files += f;
      } else {
        bytes += m.len();
        files += 1;
      }
    }
  }
  (bytes, files)
}

fn env_n(k: &str, d: usize) -> usize {
  std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn mark(s: &str) {
  let _ = fs::metadata(format!("/__MARK_{s}"));
}

fn median(mut v: Vec<f64>) -> f64 {
  v.sort_by(|a, b| a.partial_cmp(b).unwrap());
  v[v.len() / 2]
}

/// One treq snapshot op per iteration (a file edit seen by a poll), as an
/// agent editing files while the window polls would produce.
#[test]
#[ignore]
fn oplog_grow() {
  let n = env_n("GROWTH_OPS", 3000);
  let repo = TestRepo::new().unwrap();
  let ws = crate::core::create_workspace(&repo.repo_path, "grow", None, None, None, None, None).unwrap();
  let ws_path = repo.workspaces_dir().join(&ws.workspace_path);
  let wsp = ws_path.to_str().unwrap().to_string();
  let op_dir = Path::new(&repo.repo_path).join(".jj/repo/op_store");
  eprintln!("repo={} ws={}", repo.repo_path, wsp);
  for i in 0..env_n("GROWTH_EXTRA_WS", 0) {
    crate::core::create_workspace(&repo.repo_path, &format!("extra{i}"), None, None, None, None, None).unwrap();
  }
  let nfiles = env_n("GROWTH_FILES", 2000);
  for f in 0..nfiles {
    let d = ws_path.join(format!("d{}/e{}", f % 40, f % 7));
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join(format!("file{f}.rs")), format!("fn f{f}() {{}}\n")).unwrap();
  }
  crate::jj::jj_get_changed_files(&wsp).unwrap();
  let git_dirs = [Path::new(&repo.repo_path).join(".git"), Path::new(&repo.repo_path).join(".jj/repo/store")];
  let before_git: u64 = git_dirs.iter().map(|d| dir_stats(d).0).sum();
  eprintln!("seeded {nfiles} files; extra ws {}; git+store {} KiB", env_n("GROWTH_EXTRA_WS", 0), before_git / 1024);
  let mut win = Vec::new();
  for i in 1..=n {
    fs::write(ws_path.join("d3/e3").join("file3.rs"), format!("edit {i}\n")).unwrap();
    let t = Instant::now();
    crate::jj::jj_get_changed_files(&wsp).unwrap();
    win.push(t.elapsed().as_secs_f64() * 1e3);
    if i == 1 || i % 500 == 0 || i == n {
      let (b, f) = dir_stats(&op_dir);
      let heads = fs::read_dir(Path::new(&repo.repo_path).join(".jj/repo/op_heads/heads")).unwrap().count();
      let from = win.len().saturating_sub(20);
      let snap = median(win[from..].to_vec());
      // clean read (no new op)
      let mut clean = Vec::new();
      for _ in 0..7 {
        let t = Instant::now();
        crate::jj::jj_get_changed_files(&wsp).unwrap();
        clean.push(t.elapsed().as_secs_f64() * 1e3);
      }
      let t = Instant::now();
      crate::core::list_workspace_statuses(&repo.repo_path).unwrap();
      let st = t.elapsed().as_secs_f64() * 1e3;
      let git_now: u64 = git_dirs.iter().map(|d| dir_stats(d).0).sum();
      let idx = dir_stats(&Path::new(&repo.repo_path).join(".jj/repo/index"));
      let objs = dir_stats(&Path::new(&repo.repo_path).join(".git/objects")).1;
      eprintln!("  loose-ish files in .git/objects: {objs}");
      eprintln!("  git+store grew {} KiB ({} B/op), index {} KiB in {} files", (git_now - before_git) / 1024, (git_now - before_git) / i as u64, idx.0 / 1024, idx.1);
      eprintln!("ops~{i}: op_store {} KiB in {f} files, heads {heads}, snapshot-with-edit p50 {snap:.1} ms, clean read p50 {:.1} ms, list_workspace_statuses {st:.1} ms", b / 1024, median(clean));
    }
  }
  std::mem::forget(repo);
}

#[test]
#[ignore]
fn probe_clean() {
  let ws = std::env::var("GROWTH_WS").unwrap();
  let repo = std::env::var("GROWTH_REPO").unwrap();
  probe_inner(&repo, &ws);
}

#[inline(never)]
fn probe_inner(repo: &str, ws: &str) {
  crate::jj::jj_get_changed_files(ws).unwrap();
  let _ = repo;
}

/// Poll cost with many workspaces (jj workspaces only, no edits).
#[test]
#[ignore]
fn many_workspaces() {
  let n = env_n("WS_COUNT", 500);
  let repo = TestRepo::new().unwrap();
  let t0 = Instant::now();
  for i in 0..n {
    crate::core::create_workspace(&repo.repo_path, &format!("w{i}"), None, None, None, None, None).unwrap();
    if (i + 1) % 100 == 0 {
      eprintln!("created {} in {:.1}s", i + 1, t0.elapsed().as_secs_f64());
    }
  }
  eprintln!("repo={}", repo.repo_path);
  let dbp = Path::new(&repo.repo_path).join(".treq/local.db");
  eprintln!("local.db {} KiB", fs::metadata(&dbp).unwrap().len() / 1024);
  let (b, f) = dir_stats(&Path::new(&repo.repo_path).join(".jj/repo"));
  eprintln!("op/repo store {} KiB in {f} files", b / 1024);
  for round in 0..3 {
    mark(&format!("list_workspaces_{round}"));
    let t = Instant::now();
    crate::core::list_workspaces(&repo.repo_path).unwrap();
    mark(&format!("list_statuses_{round}"));
    let a = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    crate::core::list_workspace_statuses(&repo.repo_path).unwrap();
    let b = t.elapsed().as_secs_f64() * 1e3;
    eprintln!("round {round}: list_workspaces {a:.1} ms, list_workspace_statuses {b:.1} ms");
  }
  mark("have_changes");
  let t = Instant::now();
  let all = crate::core::list_workspaces(&repo.repo_path).unwrap();
  let r = crate::core::workspaces_have_changes(&repo.repo_path, &all);
  eprintln!("workspaces_have_changes({}) {:.1} ms", r.len(), t.elapsed().as_secs_f64() * 1e3);
  std::mem::forget(repo);
}

fn screen(i: usize) -> String {
  (0..50).map(|l| format!("line {l} of screen {i} ................ some agent output text here ......\n")).collect()
}

/// `record_screen` (once a second while an agent streams) against chat length.
#[test]
#[ignore]
fn agent_chat_record_cost() {
  use crate::core::agent_chat as ac;
  let repo = TestRepo::new().unwrap();
  let r = &repo.repo_path;
  ac::register_agent_chat(r, 1, "pty", "chat", "claude", None, None).unwrap();
  let mut samples = Vec::new();
  let mut small = String::new();
  for i in 0..1000usize {
    ac::record_user_message(r, 1, &screen(i), &format!("user message {i}")).unwrap();
    let t = Instant::now();
    ac::record_screen(r, 1, &screen(i + 100_000)).unwrap();
    samples.push(t.elapsed().as_secs_f64() * 1e3);
    if i == 99 {
      small = fs::read_to_string(Path::new(r).join(".treq/agent-chats/1.json")).unwrap();
    }
    if (i + 1) % 300 == 0 || i == 0 {
      let size = fs::metadata(Path::new(r).join(".treq/agent-chats/1.json")).unwrap().len();
      let from = samples.len().saturating_sub(10);
      eprintln!("msgs~{}: file {} KiB, record_screen p50 {:.2} ms", 2 * (i + 1), size / 1024, median(samples[from..].to_vec()));
    }
  }
  // many chat files: the logs explorer's list
  let dir = Path::new(r).join(".treq/agent-chats");
  let src = small;
  let big_n = env_n("CHATS", 300);
  for id in 2..=big_n {
    fs::write(dir.join(format!("{id}.json")), src.replace("\"session_id\":1", &format!("\"session_id\":{id}"))).unwrap();
  }
  let t = Instant::now();
  let l = ac::list_agent_chats(r).unwrap();
  eprintln!("list_agent_chats: {} chats x {} KiB = {:.1} ms", l.len(), src.len() / 1024, t.elapsed().as_secs_f64() * 1e3);
}

/// prompt_history and commit_diff_stats growth.
#[test]
#[ignore]
fn local_db_growth() {
  use crate::local_db as ldb;
  let repo = TestRepo::new().unwrap();
  let r = &repo.repo_path;
  let n = env_n("PROMPTS", 10_000);
  let text = "please refactor this module and fix the failing test, here is the log:\n".repeat(30);
  let t = Instant::now();
  for i in 0..n {
    ldb::add_prompt_history(r, None, None, &format!("{i} {text}"), Some("claude")).unwrap();
  }
  eprintln!("{n} prompts inserted in {:.1}s", t.elapsed().as_secs_f64());
  for _ in 0..3 {
    let t = Instant::now();
    let h = ldb::get_prompt_history(r).unwrap();
    eprintln!("get_prompt_history: {} rows, {} KiB text, {:.1} ms", h.len(), h.iter().map(|e| e.prompt_text.len()).sum::<usize>() / 1024, t.elapsed().as_secs_f64() * 1e3);
  }
  let rows: Vec<ldb::CommitRow> = (0..50_000).map(|i| ldb::CommitRow {
    commit_id: format!("{i:040x}"), insertions: 3, deletions: 2, change_id: format!("{i:012x}"),
    description: "Implement the thing and fix the bug".into(), author_name: "Test User".into(), timestamp: "2026-10-25T10:00:00+00:00".into(),
  }).collect();
  ldb::cache_commit_rows(r, &rows).unwrap();
  let ids: Vec<String> = (0..300).map(|i| format!("{:040x}", i * 97)).collect();
  let t = Instant::now();
  ldb::get_cached_commit_diff_stats_batch(r, &ids).unwrap();
  eprintln!("commit_diff_stats 50k rows: 300-id lookup {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
  let db = Path::new(r).join(".treq/local.db");
  eprintln!("local.db {} KiB, wal {} KiB", fs::metadata(&db).unwrap().len() / 1024, fs::metadata(db.with_extension("db-wal")).map(|m| m.len() / 1024).unwrap_or(0));
}

fn rss_kib() -> u64 {
  let s = fs::read_to_string("/proc/self/statm").unwrap();
  s.split_whitespace().nth(1).unwrap().parse::<u64>().unwrap() * 4
}

/// macOS and Windows build `notify_debouncer_full::new_debouncer` with a
/// `FileIdMap` (Linux uses `NoCache`): `watch(root, Recursive)` stats every
/// file under the root and keeps one map entry per file, node_modules,
/// target, .git and every other workspace's checkout under `.treq/` included.
#[test]
#[ignore]
fn file_id_map_cost() {
  use notify_debouncer_full::{FileIdCache, FileIdMap};
  for files in [env_n("TREE_FILES", 100_000), env_n("TREE_FILES", 100_000) * 5] {
    let dir = tempfile::tempdir().unwrap();
    for f in 0..files {
      let d = dir.path().join(format!("d{}/e{}", f % 400, f % 17));
      if f < 6800 {
        fs::create_dir_all(&d).unwrap();
      }
      fs::write(d.join(format!("f{f}.js")), "x").unwrap();
    }
    let before = rss_kib();
    let t = Instant::now();
    let mut map = FileIdMap::new();
    map.add_path(dir.path(), notify::RecursiveMode::Recursive);
    let ms = t.elapsed().as_secs_f64() * 1e3;
    eprintln!("{files} files: FileIdMap walk {ms:.0} ms, RSS +{} MiB", (rss_kib() - before) / 1024);
    drop(map);
  }
}

fn op_count(repo: &str) -> usize {
  fs::read_dir(Path::new(repo).join(".jj/repo/op_store/operations")).unwrap().count()
}

/// How many jj operations (op log entries) treq's own actions and idle polls add.
#[test]
#[ignore]
fn ops_per_action() {
  let n = env_n("WS_COUNT", 20);
  let repo = TestRepo::new().unwrap();
  let r = repo.repo_path.clone();
  let mut first = None;
  for i in 0..n {
    let w = crate::core::create_workspace(&r, &format!("w{i}"), None, None, None, None, None).unwrap();
    first.get_or_insert(w);
  }
  let w = first.unwrap();
  let wsp = repo.workspaces_dir().join(&w.workspace_path);
  let wsps = wsp.to_str().unwrap();
  let report = |what: &str, since: usize| eprintln!("{what}: +{} ops ({} workspaces)", op_count(&r) - since, n);
  let c = op_count(&r);
  eprintln!("after creating {n} workspaces: {c} ops");
  let c = op_count(&r);
  for _ in 0..10 {
    crate::core::list_workspaces(&r).unwrap();
    crate::core::list_workspace_statuses(&r).unwrap();
  }
  report("10 idle polls (list + statuses)", c);
  let c = op_count(&r);
  let all = crate::core::list_workspaces(&r).unwrap();
  crate::core::workspaces_have_changes(&r, &all);
  report("one has_changes pass, nothing edited", c);
  let c = op_count(&r);
  fs::write(wsp.join("a.txt"), "1\n").unwrap();
  crate::jj::jj_get_changed_files(wsps).unwrap();
  report("edit + snapshot", c);
  let c = op_count(&r);
  crate::core::commit_workspace(&r, w.id, "msg").unwrap();
  report("commit_workspace", c);
  let c = op_count(&r);
  fs::write(wsp.join("b.txt"), "1\n").unwrap();
  crate::core::list_workspace_statuses(&r).unwrap();
  crate::core::workspaces_have_changes(&r, &all);
  report("edit then poll + has_changes", c);
  std::mem::forget(repo);
}

/// One `list_workspace_statuses` on a repo left by `many_workspaces` (for callgrind:
/// `--toggle-collect='*probe_statuses_inner*'`).
#[test]
#[ignore]
fn probe_statuses() {
  let repo = std::env::var("GROWTH_REPO").unwrap();
  probe_statuses_inner(&repo);
}

#[inline(never)]
fn probe_statuses_inner(repo: &str) {
  crate::core::list_workspace_statuses(repo).unwrap();
}
