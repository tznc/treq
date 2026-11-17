//! Round 6 measurements: the diff views at their extremes (10k files, a
//! 50k-line file, a 1 MB line, thousands of renames, hundreds of conflicts)
//! and the PR review flow at 500+ threads and drafts. Everything here is
//! `#[ignore]`d and prints numbers:
//! `cargo test --features gpui-shell --lib diff_extremes -- --ignored --nocapture --test-threads=1`.

use std::{
  collections::HashMap,
  time::{Duration, Instant},
};

use gpui::TestAppContext;

use super::{
  allocs::{allocs_now, live},
  changes_large::{frame, report},
  *,
};
use crate::{
  gpui_shell::screen::{
    changes_data, changes_review as review, changes_threads, commits_data, highlight, intraline,
    pr_review_data,
  },
  jj::JjDiffHunk,
};

fn took<T>(f: impl FnOnce() -> T) -> (T, Duration) {
  let start = Instant::now();
  let out = f();
  (out, start.elapsed())
}

fn ms(d: Duration) -> f64 {
  d.as_secs_f64() * 1000.
}

fn hunk(header: &str, lines: Vec<String>) -> JjDiffHunk {
  serde_json::from_value(serde_json::json!({
    "id": "h", "header": header, "lines": lines, "patch": "",
  }))
  .expect("hunk")
}

fn rust_line(i: usize) -> String {
  format!(
    "    let value_{i} = compute(\"item {i}\", {}).map(|v| v + {i}); // note {i}",
    i * 3
  )
}

/// A new file: one hunk of `n` added lines.
fn added_hunk(n: usize) -> JjDiffHunk {
  hunk(
    &format!("@@ -0,0 +1,{n} @@"),
    (0..n).map(|i| format!("+{}", rust_line(i))).collect(),
  )
}

fn files_json(files: &[serde_json::Value], hunks: &[serde_json::Value]) -> FakeHost {
  let mut host = code_host();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": files,
    "hunks_by_file": hunks,
    "too_large_to_render": false
  }))
  .ok();
  host
}

fn file(path: &str, status: &str, previous: Option<&str>, lines: usize) -> serde_json::Value {
  serde_json::json!({
    "path": path, "status": status, "previous_path": previous,
    "changed_line_count": lines, "diff_deferred": false,
  })
}

fn hunks_of(path: &str, hunks: Vec<JjDiffHunk>) -> serde_json::Value {
  serde_json::json!({ "path": path, "hunks": hunks })
}

// ---- Pure functions at scale ------------------------------------------------

#[test]
#[ignore = "benchmark"]
fn diff_extremes_line_number_scaling() {
  let big = added_hunk(50_000);
  for l in [0usize, 10_000, 49_999] {
    let (_, d) = took(|| {
      for _ in 0..100 {
        std::hint::black_box(review::line_at(&big, l));
      }
    });
    println!("line_at(50k hunk, l={l:>6}) x100: {:.2} ms", ms(d));
  }
  let (_, d) = took(|| commits_data::line_numbers(&big));
  println!("line_numbers(50k hunk): {:.2} ms", ms(d));
  // What 60 drawn rows near the end of a 50k hunk ask for.
  let comments: Vec<review::LineComment> = (0..500)
    .map(|c| {
      serde_json::from_value(serde_json::json!({
        "id": format!("c{c}"), "file_path": "big.rs", "hunk_id": "h",
        "start_line": 49_000 + c, "end_line": 49_000 + c,
        "line_content": [], "text": "Kept", "created_at": "", "side": "new"
      }))
      .expect("comment")
    })
    .collect();
  let hunks = vec![big.clone()];
  // As the drawn rows ask: the file's lines numbered once, kept per file,
  // and a comment's outdatedness worked out once per layout.
  let numbers = commits_data::line_numbers(&big);
  let memo = std::cell::RefCell::new(HashMap::new());
  let outdated = |c: &review::LineComment| {
    *memo
      .borrow_mut()
      .entry(c.id.clone())
      .or_insert_with(|| review::is_outdated(c, Some(&hunks)))
  };
  let (_, d) = took(|| {
    for (l, &n) in numbers.iter().enumerate().take(49_060).skip(49_000) {
      let at = review::number_and_side(n, l);
      std::hint::black_box(review::line_has_comment(
        &comments, "big.rs", &big, at, &outdated,
      ));
      std::hint::black_box(review::comments_under(
        &comments, "big.rs", &big, at, &outdated,
      ));
    }
  });
  println!(
    "60 rows at line 49000 of a 50k hunk, 500 comments there: has_comment+under = {:.2} ms",
    ms(d)
  );
}

#[test]
#[ignore = "benchmark"]
fn diff_extremes_intraline() {
  // One huge replaced pair, changes spread along it.
  for len in [10_000usize, 100_000, 1_000_000] {
    let old: String = (0..len / 10)
      .map(|i| format!("w{:07}", i % 100_000))
      .collect();
    let new: String = (0..len / 10)
      .map(|i| {
        if i % 500 == 0 {
          format!("x{:07}", i % 100_000)
        } else {
          format!("w{:07}", i % 100_000)
        }
      })
      .collect();
    let lines = vec![format!("-{old}"), format!("+{new}")];
    let before = allocs_now();
    let (marks, d) = took(|| intraline::changed_ranges(&lines));
    println!(
      "intraline pair of {len} chars, change every 500 words: {:.2} ms, marks={}, allocs={}",
      ms(d),
      marks.values().map(Vec::len).sum::<usize>(),
      allocs_now() - before
    );
  }
  // 50k replaced pairs.
  let mut lines = Vec::new();
  for i in 0..25_000 {
    lines.push(format!("-{}", rust_line(i)));
  }
  for i in 0..25_000 {
    lines.push(format!("+{} // x", rust_line(i)));
  }
  let (marks, d) = took(|| intraline::changed_ranges(&lines));
  println!(
    "intraline 25k pairs: {:.2} ms, marked lines={}",
    ms(d),
    marks.len()
  );
}

fn minified(words: usize, tag: usize) -> String {
  // `a.b(c,d);function e(f){return f+1}` style, many token boundaries.
  (0..words)
    .map(|i| {
      format!(
        "function f{i}(a,b){{return a+b*{};}}var v{i}={tag}+{i};",
        i % 97
      )
    })
    .collect()
}

#[test]
#[ignore = "benchmark"]
fn diff_extremes_highlight_and_overlay() {
  // Syntect on lines just under LONG_LINE, cold.
  for len in [2_000usize, 10_000, 19_000, 25_000, 1_000_000] {
    let line = minified(len / 60 + 1, 1);
    let (n, d) = took(|| highlight::spans("js", &line).len());
    println!(
      "highlight cold {len:>8} chars (actual {}): {:.2} ms, spans={n}",
      line.len(),
      ms(d)
    );
  }
  // The overlay of intraline marks on a line under LONG_LINE, per draw.
  let old = minified(300, 1);
  let new = old.replacen("a+b*5;", "a-b*5;", 40);
  println!("overlay line len {}", old.len());
  let lines = vec![format!("-{old}"), format!("+{new}")];
  let marks = intraline::changed_ranges(&lines);
  let ranges = marks.get(&1).cloned().unwrap_or_default();
  println!("marks on the added line: {}", ranges.len());
  let colored: Vec<_> = ranges
    .iter()
    .map(|r| (r.clone(), gpui::hsla(0.3, 0.5, 0.5, 0.45)))
    .collect();
  let _ = highlight::styled_marked(&new, Some("js"), false, &colored);
  let (_, d) = took(|| {
    for _ in 0..5 {
      std::hint::black_box(highlight::styled_marked(&new, Some("js"), false, &colored));
    }
  });
  println!(
    "styled_marked warm, {} char line, {} marks, x5: {:.2} ms",
    new.len(),
    colored.len(),
    ms(d)
  );
  let (_, d) = took(|| {
    for _ in 0..5 {
      std::hint::black_box(highlight::styled_marked(&new, Some("js"), false, &[]));
    }
  });
  println!("styled (no marks) warm same line x5: {:.2} ms", ms(d));

  // Cache memory: 20k distinct lines of 19k chars.
  let before = live();
  let (_, d) = took(|| {
    for i in 0..100 {
      let line = minified(300, i);
      highlight::spans("js", &line);
    }
  });
  println!(
    "highlight cache, 100 distinct ~{} char lines: {:.0} ms, live +{:.1} MiB",
    minified(300, 0).len(),
    ms(d),
    (live() - before) as f64 / 1048576.
  );
}

#[test]
#[ignore = "benchmark"]
fn diff_extremes_pr_data() {
  // A PR of 300 files x 400 lines.
  let mut diff = String::new();
  for f in 0..300 {
    diff.push_str(&format!(
      "diff --git a/src/f{f}.rs b/src/f{f}.rs\nindex 1..2 100644\n--- a/src/f{f}.rs\n+++ b/src/f{f}.rs\n"
    ));
    for chunk in 0..4 {
      diff.push_str(&format!("@@ -{0},100 +{0},100 @@\n", chunk * 200 + 1));
      for i in 0..100 {
        if i % 2 == 0 {
          diff.push_str(&format!(" {}\n", rust_line(i)));
        } else {
          diff.push_str(&format!("-{}\n+{} // new\n", rust_line(i), rust_line(i)));
        }
      }
    }
  }
  println!("pr diff text: {:.1} MiB", diff.len() as f64 / 1048576.);
  let before = live();
  let (parsed, d) = took(|| pr_review_data::parse_unified_diff(&diff));
  println!(
    "parse_unified_diff: {:.1} ms, files={}, live +{:.1} MiB",
    ms(d),
    parsed.files.len(),
    (live() - before) as f64 / 1048576.
  );
  let (_, d) = took(|| {
    let hunks: HashMap<String, Vec<JjDiffHunk>> = parsed
      .hunks
      .iter()
      .map(|(p, h)| (p.clone(), h.clone()))
      .collect();
    std::hint::black_box(hunks)
  });
  println!(
    "place_drafts' hunks clone (every poll, 3 s): {:.1} ms",
    ms(d)
  );
  let wrapped: HashMap<String, std::sync::Arc<Vec<JjDiffHunk>>> = parsed
    .hunks
    .iter()
    .map(|(p, h)| (p.clone(), std::sync::Arc::new(h.clone())))
    .collect();
  let (eq, d) = took(|| wrapped == wrapped.clone());
  println!("Arc map == clone (ptr-eq fast path?): {eq} {:.2} ms", ms(d));

  // 500 drafts over those files, hashed lines, a third moved.
  let drafts: Vec<crate::local_db::AgentReviewComment> = (0..500)
    .map(|i| {
      let f = i % 300;
      let hunks = &parsed.hunks[&format!("src/f{f}.rs")];
      let h = &hunks[i % hunks.len()];
      let numbers = commits_data::line_numbers(h);
      let at = h
        .lines
        .iter()
        .zip(&numbers)
        .position(|(l, n)| l.starts_with('+') && n.1.is_some())
        .unwrap_or(0);
      let line = numbers[at].1.unwrap_or(1) as i64;
      let hash = pr_review_data::hash_diff_lines(&[h.lines[at].clone()]);
      // Every third draft is stored at a line that no longer holds its text.
      let stored = if i % 3 == 0 { line + 7 } else { line };
      serde_json::from_value(serde_json::json!({
        "id": format!("d{i}"), "repo_path": "/repo", "target_type": "pr", "target_id": "5",
        "file_path": format!("src/f{f}.rs"), "hunk_id": null, "start_line": stored,
        "end_line": stored, "side": "new", "comment_text": "Nit", "suggested_replacement": null,
        "status": "open", "source": "local-human", "created_at": "", "resolved_at": null,
        "line_hash": hash,
      }))
      .expect("draft")
    })
    .collect();
  let (shown, d) = took(|| pr_review_data::drafts(&drafts, &parsed.hunks));
  println!(
    "drafts(500 over 300 files x 400 lines, 1/3 moved): {:.1} ms, outdated={}",
    ms(d),
    shown.iter().filter(|d| d.outdated).count()
  );

  // One file of 50k lines with 100 drafts (all moved).
  let big = HashMap::from([("big.rs".to_string(), vec![added_hunk(50_000)])]);
  let drafts: Vec<crate::local_db::AgentReviewComment> = (0..100)
    .map(|i| {
      let line = 100 + i * 400;
      let hash = pr_review_data::hash_diff_lines(&[format!("+{}", rust_line(line - 1))]);
      serde_json::from_value(serde_json::json!({
        "id": format!("d{i}"), "repo_path": "/repo", "target_type": "pr", "target_id": "5",
        "file_path": "big.rs", "hunk_id": null, "start_line": line + 3,
        "end_line": line + 3, "side": "new", "comment_text": "Nit", "suggested_replacement": null,
        "status": "open", "source": "local-human", "created_at": "", "resolved_at": null,
        "line_hash": hash,
      }))
      .expect("draft")
    })
    .collect();
  let (shown, d) = took(|| pr_review_data::drafts(&drafts, &big));
  println!(
    "drafts(100 moved drafts in one 50k-line file): {:.1} ms, outdated={}",
    ms(d),
    shown.iter().filter(|d| d.outdated).count()
  );
}

#[test]
#[ignore = "benchmark"]
fn diff_extremes_thread_placement() {
  // 600 threads over 30 files of 2 hunks x 1000 lines, and over one 50k file.
  let hunks: HashMap<String, Vec<JjDiffHunk>> = (0..30)
    .map(|f| {
      (
        format!("src/f{f}.rs"),
        (0..2)
          .map(|h| {
            let start = h * 1500 + 1;
            hunk(
              &format!("@@ -{start},1000 +{start},1000 @@"),
              (0..1000).map(|i| format!(" {}", rust_line(i))).collect(),
            )
          })
          .collect(),
      )
    })
    .collect();
  let working: HashMap<String, &[JjDiffHunk]> =
    hunks.iter().map(|(p, h)| (p.clone(), &h[..])).collect();
  let committed = HashMap::new();
  let threads: Vec<_> = (0..600)
    .map(|t| {
      let mut th = gh_thread(&format!("t{t}"), (1 + (t * 5) % 2400) as i64, false);
      th.path = format!("src/f{}.rs", t % 30);
      th
    })
    .collect();
  let (placed, d) = took(|| changes_threads::place_threads(&threads, &[&working, &committed]));
  println!(
    "place_threads(600 over 30 files x 2000 lines): {:.1} ms, placed={}",
    ms(d),
    placed.by_line.len()
  );

  let big = [added_hunk(50_000)];
  let working: HashMap<String, &[JjDiffHunk]> = HashMap::from([("big.rs".to_string(), &big[..])]);
  let threads: Vec<_> = (0..600)
    .map(|t| {
      let mut th = gh_thread(&format!("t{t}"), (1 + t * 80) as i64, false);
      th.path = "big.rs".into();
      th
    })
    .collect();
  let (placed, d) = took(|| changes_threads::place_threads(&threads, &[&working, &committed]));
  println!(
    "place_threads(600 over one 50k-line hunk): {:.1} ms, placed={}",
    ms(d),
    placed.by_line.len()
  );
  // 600 threads on a file of 200 hunks x 250 lines (a long file, many hunks).
  let many: Vec<JjDiffHunk> = (0..200)
    .map(|h| {
      let start = h * 400 + 1;
      hunk(
        &format!("@@ -{start},250 +{start},250 @@"),
        (0..250).map(|i| format!(" {}", rust_line(i))).collect(),
      )
    })
    .collect();
  let working: HashMap<String, &[JjDiffHunk]> = HashMap::from([("many.rs".to_string(), &many[..])]);
  let threads: Vec<_> = (0..600)
    .map(|t| {
      let mut th = gh_thread(&format!("t{t}"), (1 + (t * 130) % 80_000) as i64, false);
      th.path = "many.rs".into();
      th
    })
    .collect();
  let (placed, d) = took(|| changes_threads::place_threads(&threads, &[&working, &committed]));
  println!(
    "place_threads(600 over 200 hunks x 250 lines): {:.1} ms, placed={}",
    ms(d),
    placed.by_line.len()
  );
}

#[test]
#[ignore = "benchmark"]
fn diff_extremes_conflicts_data() {
  let content: String = (1..=300)
    .map(|n| {
      format!(
        "ctx {n}\n<<<<<<< Side #1 (Conflict {n} of 300)\nours {n}\n||||||| Base\nbase {n}\n=======\ntheirs {n}\n>>>>>>> Side #2 (Conflict {n} of 300)\n"
      )
    })
    .collect();
  let (regions, d) = took(|| crate::conflict_markers::parse_conflict_markers(&content, "f.rs"));
  println!(
    "parse_conflict_markers 300 conflicts: {:.2} ms, regions={}",
    ms(d),
    regions.len()
  );
  let lines: Vec<String> = content.lines().map(|l| format!("+{l}")).collect();
  let mut h = hunk(&format!("@@ -0,0 +1,{} @@", lines.len()), lines);
  h.conflict_regions = regions;
  let hunks = vec![h];
  let (found, d) = took(|| changes_data::conflicts("f.rs", &hunks));
  println!("conflicts(): {:.2} ms n={}", ms(d), found.len());
  let (_, d) = took(|| changes_data::conflict_lines(&found));
  println!("conflict_lines(): {:.2} ms", ms(d));
  let (_, d) = took(|| serde_json::to_string(&hunks).map(|s| s.len()));
  println!("serde of the conflicted hunk: {:.2} ms", ms(d));
}

// ---- Frames ---------------------------------------------------------------

fn open(h: &mut Harness, path: &str) {
  if h.drawn("view-large-changeset") {
    h.click("view-large-changeset");
  }
  h.click(leak(format!("file-row-{path}")));
  h.cx.run_until_parked();
}

fn leak(s: String) -> &'static str {
  Box::leak(s.into_boxed_str())
}

fn frames(h: &mut Harness, name: &str, n: usize) {
  let mut samples: Vec<_> = (0..n).map(|_| frame(h)).collect();
  report(name, &mut samples);
}

fn scroll_by(h: &mut Harness, px: f32) -> Duration {
  let list = h.cx.debug_bounds("changes-diff-list");
  let at = list.map_or(gpui::point(gpui::px(700.), gpui::px(400.)), |b| b.center());
  let start = Instant::now();
  h.cx.simulate_event(gpui::ScrollWheelEvent {
    position: at,
    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-px))),
    modifiers: gpui::Modifiers::none(),
    touch_phase: gpui::TouchPhase::Moved,
  });
  h.cx.run_until_parked();
  h.cx.update(|window, cx| window.draw(cx).clear(cx));
  start.elapsed()
}

#[gpui::test]
#[ignore = "benchmark"]
fn diff_extremes_frame_huge_file(cx: &mut TestAppContext) {
  let n: usize = std::env::var("LINES")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(50_000);
  let host = files_json(
    &[file("src/big.rs", "A", None, n)],
    &[hunks_of("src/big.rs", vec![added_hunk(n)])],
  );
  let before = live();
  let (mut h, _) = {
    let (r, d) = took(|| workspace_harness(cx, host, changes_route()));
    println!("{n}-line file: harness + load: {:.0} ms", ms(d));
    r
  };
  let d = {
    let start = Instant::now();
    open(&mut h, "src/big.rs");
    start.elapsed()
  };
  println!(
    "open the {n}-line file: {:.0} ms; live +{:.1} MiB",
    ms(d),
    (live() - before) as f64 / 1048576.
  );
  frames(&mut h, "full frame, top", 8);
  let d = scroll_by(&mut h, (n as f32 * 20.) * 0.5);
  println!("scroll to the middle: {:.0} ms", ms(d));
  frames(&mut h, "full frame, middle", 8);
  let d = scroll_by(&mut h, (n as f32 * 20.) * 0.6);
  println!("scroll to the end: {:.0} ms", ms(d));
  frames(&mut h, "full frame, end", 8);
  let mut keys: Vec<_> = (0..10)
    .map(|_| super::changes_large::timed(&mut h, |h| h.cx.simulate_keystrokes("down")))
    .collect();
  report("arrow down at the end", &mut keys);
  let start = Instant::now();
  h.cx.simulate_keystrokes("cmd-f");
  h.cx.simulate_input("value_4999");
  h.cx.executor().advance_clock(Duration::from_millis(150));
  h.cx.run_until_parked();
  println!(
    "find 'value_4999' in {n} lines: {:.0} ms",
    ms(start.elapsed())
  );
}

/// `LINE_CHARS` per line, `LINES` replaced pairs.
#[gpui::test]
#[ignore = "benchmark"]
fn diff_extremes_frame_long_lines(cx: &mut TestAppContext) {
  let chars: usize = std::env::var("LINE_CHARS")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(1_000_000);
  let pairs: usize = std::env::var("PAIRS")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(1);
  let words = chars / 60 + 1;
  let mut lines = Vec::new();
  for p in 0..pairs {
    lines.push(format!("-{}", minified(words, p)));
  }
  for p in 0..pairs {
    lines.push(format!("+{}", minified(words, p + 1)));
  }
  let host = files_json(
    &[file("dist/app.js", "M", None, pairs * 2)],
    &[hunks_of(
      "dist/app.js",
      vec![hunk(&format!("@@ -1,{pairs} +1,{pairs} @@"), lines)],
    )],
  );
  let before = live();
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  let start = Instant::now();
  open(&mut h, "dist/app.js");
  println!(
    "{pairs} pair(s) of {chars}-char lines: open {:.0} ms; live +{:.1} MiB",
    ms(start.elapsed()),
    (live() - before) as f64 / 1048576.
  );
  frames(&mut h, "full frame", 6);
}

#[gpui::test]
#[ignore = "benchmark"]
fn diff_extremes_frame_many_files(cx: &mut TestAppContext) {
  let n: usize = std::env::var("FILES")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(10_000);
  let renames: bool = std::env::var("RENAMES").is_ok();
  let binary: bool = std::env::var("BINARY").is_ok();
  let ext = if binary { "png" } else { "rs" };
  let files: Vec<_> = (0..n)
    .map(|i| {
      let path = format!("src/d{}/f{i}.{ext}", i % 100);
      if renames {
        file(&path, "R", Some(&format!("old/d{}/f{i}.{ext}", i % 100)), 0)
      } else {
        file(&path, "M", None, 3)
      }
    })
    .collect();
  let hunks: Vec<_> = if renames || binary {
    Vec::new()
  } else {
    (0..n)
      .map(|i| {
        hunks_of(
          &format!("src/d{}/f{i}.{ext}", i % 100),
          vec![hunk(
            "@@ -1,2 +1,3 @@",
            vec![" a".into(), "+b".into(), "+c".into()],
          )],
        )
      })
      .collect()
  };
  let host = files_json(&files, &hunks);
  let before = live();
  let (mut h, _) = {
    let (r, d) = took(|| workspace_harness(cx, host, changes_route()));
    println!(
      "{n} files (renames={renames} binary={binary}): harness + load: {:.0} ms",
      ms(d)
    );
    r
  };
  h.cx.run_until_parked();
  println!(
    "live +{:.1} MiB after load",
    (live() - before) as f64 / 1048576.
  );
  frames(&mut h, "full frame (gated or not)", 5);
  let start = Instant::now();
  if h.drawn("view-large-changeset") {
    h.click("view-large-changeset");
  }
  h.cx.run_until_parked();
  println!(
    "View changes: {:.0} ms; live +{:.1} MiB",
    ms(start.elapsed()),
    (live() - before) as f64 / 1048576.
  );
  frames(&mut h, "full frame, expanded", 5);
  let d = scroll_by(&mut h, 100_000.);
  println!("scroll 100k px: {:.0} ms", ms(d));
  frames(&mut h, "full frame, scrolled", 5);
  let d = scroll_by(&mut h, 100_000.);
  println!("scroll 100k px: {:.0} ms", ms(d));
}

#[gpui::test]
#[ignore = "benchmark"]
fn diff_extremes_frame_many_conflicts(cx: &mut TestAppContext) {
  let n: usize = std::env::var("CONFLICTS")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(300);
  let content: String = (1..=n)
    .map(|k| {
      format!(
        "ctx {k}\n<<<<<<< Side #1 (Conflict {k} of {n})\nours {k}\n||||||| Base\nbase {k}\n=======\ntheirs {k}\n>>>>>>> Side #2 (Conflict {k} of {n})\n"
      )
    })
    .collect();
  let regions = crate::conflict_markers::parse_conflict_markers(&content, "README.md");
  let lines: Vec<String> = content.lines().map(|l| format!("+{l}")).collect();
  let mut host = changes_host();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": [file("README.md", "M", None, lines.len())],
    "conflicted_files": ["README.md"],
    "hunks_by_file": [{"path": "README.md", "hunks": [{
      "id": "r", "header": format!("@@ -1,1 +1,{} @@", lines.len()), "lines": lines,
      "patch": "", "conflict_regions": regions
    }]}],
    "too_large_to_render": false
  }))
  .ok();
  let before = live();
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  open(&mut h, "README.md");
  println!(
    "{n} conflicts: live +{:.1} MiB",
    (live() - before) as f64 / 1048576.
  );
  frames(&mut h, "full frame", 8);
  let d = scroll_by(&mut h, 20_000.);
  println!("scroll 20k px: {:.0} ms", ms(d));
  frames(&mut h, "full frame, scrolled", 8);
}
