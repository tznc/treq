#![allow(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  clippy::string_slice,
  clippy::print_stdout,
  reason = "measurement code"
)]
//! Round 6: the Changes/Commits data path at extremes. Ignored; prints
//! numbers: `cargo test --test diff_extremes_data -- --ignored --nocapture --test-threads=1`.

use std::{fs, path::PathBuf, time::Instant};

mod common;
use common::TestRepo;

fn rss_mib() -> f64 {
  let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
  let kb = |key: &str| {
    status
      .lines()
      .find(|l| l.starts_with(key))
      .and_then(|l| l.split_whitespace().nth(1))
      .and_then(|v| v.parse::<f64>().ok())
      .unwrap_or(0.)
  };
  kb("VmRSS:") / 1024.
}

fn hwm_mib() -> f64 {
  let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
  status
    .lines()
    .find(|l| l.starts_with("VmHWM:"))
    .and_then(|l| l.split_whitespace().nth(1))
    .and_then(|v| v.parse::<f64>().ok())
    .unwrap_or(0.)
    / 1024.
}

fn init_app_db(repo: &TestRepo) {
  let app_dir: PathBuf = repo.temp_dir.path().join("app-data");
  fs::create_dir_all(&app_dir).unwrap();
  std::env::set_var("TREQ_APP_DATA_DIR", app_dir.to_string_lossy().to_string());
  let db = treq_lib::db::Database::new(app_dir.join("treq.db")).unwrap();
  db.init().unwrap();
  db.set_setting("conflict_marker_style", "git").unwrap();
}

fn rust_line(i: usize) -> String {
  format!("    let value_{i} = compute(\"item {i}\", {}).map(|v| v + {i}); // note {i}", i * 3)
}

fn body(file: usize, lines: usize, tag: &str) -> String {
  (0..lines)
    .map(|i| format!("{} {tag}{file}\n", rust_line(i)))
    .collect()
}

struct Ws {
  repo: TestRepo,
  id: i64,
  dir: PathBuf,
}

fn workspace(name: &str, prep_repo: impl FnOnce(&TestRepo)) -> Ws {
  let repo = TestRepo::new().unwrap();
  prep_repo(&repo);
  let ws = treq_lib::core::create_workspace(&repo.repo_path, name, None, None, None, None, None)
    .unwrap();
  init_app_db(&repo);
  let dir = repo.workspaces_dir().join(&ws.workspace_path);
  Ws { id: ws.id, dir, repo }
}

fn write(ws: &Ws, path: &str, content: impl AsRef<[u8]>) {
  let full = ws.dir.join(path);
  fs::create_dir_all(full.parent().unwrap()).unwrap();
  fs::write(full, content).unwrap();
}

fn diff(name: &str, ws: &Ws) -> treq_lib::jj::JjRevisionDiff {
  let start = Instant::now();
  let d = treq_lib::core::workspace_diff(&ws.repo.repo_path, ws.id).unwrap();
  let took = start.elapsed();
  let lines: usize = d
    .hunks_by_file
    .iter()
    .flat_map(|f| &f.hunks)
    .map(|h| h.lines.len())
    .sum();
  let (_, ser) = {
    let s = Instant::now();
    let json = serde_json::to_string(&d).unwrap();
    (json.len(), s.elapsed())
  };
  println!(
    "{name}: workspace_diff {:.0} ms; committed={} uncommitted={} hunk lines={} serde {:.0} ms; rss {:.0} MiB hwm {:.0}",
    took.as_secs_f64() * 1000.,
    d.committed_files.len(),
    d.uncommitted_files.len(),
    lines,
    ser.as_secs_f64() * 1000.,
    rss_mib(),
    hwm_mib()
  );
  d
}

fn commit_diff(name: &str, ws: &Ws) {
  let change = TestRepo::jj_change_id(ws.dir.to_str().unwrap(), "@-").unwrap();
  let start = Instant::now();
  let d = treq_lib::core::get_commit_diff_with_conflict_style(
    &ws.repo.repo_path,
    Some(ws.id),
    &change,
    "git",
  )
  .unwrap();
  let took = start.elapsed();
  let lines: usize = d
    .hunks_by_file
    .iter()
    .flat_map(|f| &f.hunks)
    .map(|h| h.lines.len())
    .sum();
  println!(
    "{name}: commit diff {:.0} ms; files={} too_large={} hunk lines={} rss {:.0} MiB",
    took.as_secs_f64() * 1000.,
    d.committed_files.len(),
    d.too_large_to_render,
    lines,
    rss_mib()
  );
}

fn commit(ws: &Ws, msg: &str) {
  treq_lib::core::commit_workspace(&ws.repo.repo_path, ws.id, msg).unwrap();
}

#[test]
#[ignore = "benchmark"]
fn extremes_committed_10k_files() {
  let n: usize = std::env::var("FILES").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000);
  let ws = workspace("feat-10k", |_| {});
  let s = Instant::now();
  for i in 0..n {
    write(&ws, &format!("src/d{}/f{i}.rs", i % 100), body(i, 20, "n"));
  }
  commit(&ws, "10k files");
  println!("setup+commit {n} files: {:.1} s", s.elapsed().as_secs_f64());
  diff("committed 10k files (1st)", &ws);
  diff("committed 10k files (2nd, nothing changed)", &ws);
  commit_diff("10k files", &ws);
  // A one-line edit in the working copy, the usual reload.
  write(&ws, "src/d0/f0.rs", body(0, 20, "edited"));
  diff("committed 10k + 1 working-copy edit", &ws);
}

#[test]
#[ignore = "benchmark"]
fn extremes_committed_big_files() {
  // jj does not track NEW files over 1 MiB, so the big files live in the base.
  let minified: String = (0..40_000)
    .map(|i| format!("function f{i}(a,b){{return a+b*{};}}var v{i}={i};", i % 97))
    .collect();
  println!("minified bytes {}", minified.len());
  let mut bin = vec![0u8; 8 << 20];
  for (i, b) in bin.iter_mut().enumerate() {
    *b = (i.wrapping_mul(2654435761) >> 7) as u8;
  }
  let ws = workspace("feat-big", |repo| {
    let root = std::path::Path::new(&repo.repo_path);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("dist")).unwrap();
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::write(root.join("src/big.rs"), body(0, 50_000, "n")).unwrap();
    fs::write(root.join("dist/app.min.js"), &minified).unwrap();
    for i in 0..5 {
      fs::write(root.join(format!("assets/img{i}.png")), &bin).unwrap();
    }
    repo.commit_file("README.md", "x\n", "tracked").unwrap();
  });
  // Committed: every 10th line of big.rs changes, the minified file changes
  // in the middle, the binaries change a byte.
  let edited: String = (0..50_000)
    .map(|i| {
      if i % 10 == 0 {
        format!("{} edited{i}\n", rust_line(i))
      } else {
        format!("{} n0\n", rust_line(i))
      }
    })
    .collect();
  write(&ws, "src/big.rs", &edited);
  write(&ws, "dist/app.min.js", minified.replace("a+b*5;", "a-b*5;"));
  for i in 0..5 {
    let mut b = bin.clone();
    b[10] ^= 1;
    write(&ws, &format!("assets/img{i}.png"), b);
  }
  commit(&ws, "big files");
  let d = diff("committed big (50k lines/10% changed, 2 MB line, 5x8MB binary)", &ws);
  println!("committed files {:?}", d.committed_files.iter().map(|f| (f.path.clone(), f.changed_line_count)).collect::<Vec<_>>());
  diff("committed big, 2nd", &ws);
  commit_diff("big", &ws);
  // Now the working copy rewrites the 50k-line file fully and the minified one.
  write(&ws, "src/big.rs", body(0, 50_000, "rewritten"));
  write(&ws, "dist/app.min.js", minified.replace("a+b", "a/b"));
  let d = diff("committed big + working-copy rewrite of both", &ws);
  let paths: Vec<String> = d.uncommitted_files.iter().map(|f| f.path.clone()).collect();
  println!("uncommitted {paths:?}");
  let s = Instant::now();
  let batch =
    treq_lib::core::list_file_hunks_batch(&ws.repo.repo_path, Some(ws.id), paths, "git").unwrap();
  let lines: usize = batch.files.iter().flat_map(|f| &f.hunks).map(|h| h.lines.len()).sum();
  let bytes: usize = batch.files.iter().flat_map(|f| &f.hunks).map(|h| h.lines.iter().map(String::len).sum::<usize>() + h.patch.len()).sum();
  println!(
    "hunks batch of the two rewritten big files: {:.0} ms, lines {lines}, bytes (lines+patch) {:.1} MiB, rss {:.0} MiB",
    s.elapsed().as_secs_f64() * 1000.,
    bytes as f64 / 1048576.,
    rss_mib()
  );
  let patch: usize = batch.files.iter().flat_map(|f| &f.hunks).map(|h| h.patch.len()).sum();
  println!("  of which `patch` strings: {:.1} MiB", patch as f64 / 1048576.);
  // The uncommitted binary edit: what does a batch cost for a binary file?
  write(&ws, "assets/img0.png", {
    let mut b = bin.clone();
    b[100] ^= 1;
    b
  });
  let s = Instant::now();
  let batch = treq_lib::core::list_file_hunks_batch(
    &ws.repo.repo_path,
    Some(ws.id),
    vec!["assets/img0.png".into()],
    "git",
  )
  .unwrap();
  println!(
    "hunks batch of one 8 MB binary file: {:.0} ms, hunks {}",
    s.elapsed().as_secs_f64() * 1000.,
    batch.files[0].hunks.len()
  );
}

#[test]
#[ignore = "benchmark"]
fn extremes_renames() {
  let n: usize = std::env::var("FILES").ok().and_then(|v| v.parse().ok()).unwrap_or(3_000);
  let ws = workspace("feat-renames", |repo| {
    for i in 0..n {
      repo
        .create_file(&format!("old/d{}/f{i}.rs", i % 50), &body(i, 30, "n"))
        .unwrap();
    }
    repo.commit_file("README.md", "x\n", "tracked").unwrap();
  });
  for i in 0..n {
    let from = ws.dir.join(format!("old/d{}/f{i}.rs", i % 50));
    let to = ws.dir.join(format!("new/d{}/g{i}.rs", i % 50));
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    let mut content = fs::read_to_string(&from).unwrap();
    content.push_str("// moved\n");
    fs::write(&to, content).unwrap();
    fs::remove_file(&from).unwrap();
  }
  commit(&ws, "rename everything");
  let d = diff("committed renames", &ws);
  println!(
    "renamed statuses: {}",
    d.committed_files.iter().filter(|f| f.status == "R").count()
  );
  diff("committed renames, 2nd", &ws);
  commit_diff("renames", &ws);
}

#[test]
#[ignore = "benchmark"]
fn extremes_working_copy_edits() {
  let n: usize = std::env::var("FILES").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000);
  let ws = workspace("feat-wc", |repo| {
    for i in 0..n {
      repo
        .create_file(&format!("src/d{}/f{i}.rs", i % 100), &body(i, 10, "n"))
        .unwrap();
    }
    repo.commit_file("README.md", "x\n", "tracked").unwrap();
  });
  for i in 0..n {
    write(&ws, &format!("src/d{}/f{i}.rs", i % 100), body(i, 10, "e"));
  }
  let d = diff("working copy 10k edits", &ws);
  let paths: Vec<String> = d.uncommitted_files.iter().map(|f| f.path.clone()).collect();
  let batches: Vec<&[String]> = {
    let (first, rest) = paths.split_at(2);
    std::iter::once(first).chain(rest.chunks(32)).collect()
  };
  let s = Instant::now();
  let mut first_ms = 0.;
  for (k, batch) in batches.iter().enumerate() {
    let b = treq_lib::core::list_file_hunks_batch(
      &ws.repo.repo_path,
      Some(ws.id),
      batch.to_vec(),
      "git",
    )
    .unwrap();
    std::hint::black_box(&b);
    if k == 1 {
      first_ms = s.elapsed().as_secs_f64() * 1000.;
    }
    if k == 20 {
      println!("  20 batches: {:.0} ms", s.elapsed().as_secs_f64() * 1000.);
      if std::env::var("ALL").is_err() {
        println!("  extrapolated {} batches: {:.1} s", batches.len(), s.elapsed().as_secs_f64() / 21. * batches.len() as f64);
        return;
      }
    }
  }
  println!(
    "all {} batches: {:.1} s (first two {:.0} ms); rss {:.0} MiB",
    batches.len(),
    s.elapsed().as_secs_f64(),
    first_ms,
    rss_mib()
  );
}

#[test]
#[ignore = "benchmark"]
fn extremes_binary_files() {
  let n: usize = std::env::var("FILES").ok().and_then(|v| v.parse().ok()).unwrap_or(50);
  let ws = workspace("feat-bin", |_| {});
  let mut bin = vec![0u8; 4 << 20];
  for (i, b) in bin.iter_mut().enumerate() {
    *b = (i.wrapping_mul(2654435761) >> 7) as u8;
  }
  for i in 0..n {
    bin[0] = i as u8;
    bin[1] = (i >> 8) as u8;
    write(&ws, &format!("assets/img{i}.png"), &bin);
  }
  commit(&ws, "binaries");
  diff(&format!("{n} x 4 MB binaries committed"), &ws);
  commit_diff("binaries", &ws);
}

/// Files the Changes tab never draws a diff for (binary extensions, deleted
/// files, renames) still go through the hunk batch.
#[test]
#[ignore = "benchmark"]
fn extremes_batch_of_undrawn_files() {
  let ws = workspace("feat-undrawn", |repo| {
    let mut bin = vec![0u8; 4 << 20];
    for (i, b) in bin.iter_mut().enumerate() {
      *b = (i.wrapping_mul(2654435761) >> 7) as u8;
    }
    for i in 0..20 {
      bin[0] = i as u8;
      let full = std::path::Path::new(&repo.repo_path).join(format!("assets/img{i}.png"));
      fs::create_dir_all(full.parent().unwrap()).unwrap();
      fs::write(full, &bin).unwrap();
      repo
        .create_file(&format!("gen/deleted{i}.rs"), &body(i, 20_000, "d"))
        .unwrap();
      repo
        .create_file(&format!("text/t{i}.rs"), &body(i, 200, "t"))
        .unwrap();
    }
    repo.commit_file("README.md", "x\n", "tracked").unwrap();
  });
  for i in 0..20 {
    let mut bin = fs::read(ws.dir.join(format!("assets/img{i}.png"))).unwrap();
    bin[5] ^= 1;
    write(&ws, &format!("assets/img{i}.png"), bin);
    fs::remove_file(ws.dir.join(format!("gen/deleted{i}.rs"))).unwrap();
    write(&ws, &format!("text/t{i}.rs"), body(i, 200, "edited"));
  }
  let d = diff("wc: 20 binaries, 20 deleted 20k-line files, 20 text edits", &ws);
  let run = |name: &str, filter: &dyn Fn(&str) -> bool| {
    let paths: Vec<String> = d
      .uncommitted_files
      .iter()
      .map(|f| f.path.clone())
      .filter(|p| filter(p))
      .collect();
    let n = paths.len();
    let s = Instant::now();
    let b = treq_lib::core::list_file_hunks_batch(&ws.repo.repo_path, Some(ws.id), paths, "git")
      .unwrap();
    let lines: usize = b.files.iter().flat_map(|f| &f.hunks).map(|h| h.lines.len()).sum();
    println!(
      "batch {name}: {n} files, {:.0} ms, hunk lines {lines}, rss {:.0} MiB",
      s.elapsed().as_secs_f64() * 1000.,
      rss_mib()
    );
  };
  run("text only", &|p| p.starts_with("text/"));
  run("binaries only", &|p| p.starts_with("assets/"));
  run("deleted only", &|p| p.starts_with("gen/"));
  run("all 60", &|_| true);
}
