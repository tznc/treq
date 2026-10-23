#![allow(clippy::expect_used, clippy::string_slice, reason = "bench code")]

//! The Changes tab's per-row and per-frame helpers on a large diff: line
//! numbers, comment and thread placement, intraline marks and syntax
//! highlighting. `changes_perf` (an ignored lib test) measures whole frames.

use std::collections::HashMap;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use treq_lib::github::GhReviewThread;
use treq_lib::gpui_bench as diff;
use treq_lib::jj::JjDiffHunk;

const FILES: usize = 6;
/// Replaced lines per file, in chunks of 10 removed, 10 added and 1 context.
const PAIRS: usize = 250;
/// Rows a window shows at once.
const VISIBLE: usize = 40;

fn line(file: usize, i: usize, new: bool) -> String {
  let value = if new { i * 3 + 1 } else { i * 3 };
  format!(
    "    let value_{i} = compute_{file}(\"item {i}\", {value}).map(|v| v + {value}); // note {i}"
  )
}

/// One file's diff: one hunk of `PAIRS` replaced lines, as `changes_perf` has.
fn hunk(file: usize) -> JjDiffHunk {
  let mut lines = Vec::new();
  for chunk in 0..PAIRS / 10 {
    for i in chunk * 10..chunk * 10 + 10 {
      lines.push(format!("-{}", line(file, i, false)));
    }
    for i in chunk * 10..chunk * 10 + 10 {
      lines.push(format!("+{}", line(file, i, true)));
    }
    lines.push(format!(" // context {chunk}"));
  }
  let old = lines.iter().filter(|l| !l.starts_with('+')).count();
  let new = lines.iter().filter(|l| !l.starts_with('-')).count();
  serde_json::from_value(serde_json::json!({
    "id": "h",
    "header": format!("@@ -1,{old} +1,{new} @@"),
    "lines": lines,
    "patch": "",
  }))
  .expect("hunk")
}

fn path(file: usize) -> String {
  format!("src/module_{file}.rs")
}

/// 20 single-line comments on file 0's new side.
fn comments() -> Vec<diff::LineComment> {
  (0..20)
    .map(|c| {
      serde_json::from_value(serde_json::json!({
        "id": format!("c{c}"), "file_path": path(0), "hunk_id": "h",
        "start_line": 1 + c * 13, "end_line": 1 + c * 13,
        "line_content": [], "text": "Kept", "created_at": "", "side": "new"
      }))
      .expect("comment")
    })
    .collect()
}

/// 5 threads on each file, the last ones on lines the diff does not have.
fn threads() -> Vec<GhReviewThread> {
  (0..FILES)
    .flat_map(|f| {
      (0..5).map(move |t| GhReviewThread {
        id: format!("t{f}-{t}"),
        is_resolved: false,
        is_outdated: false,
        path: path(f),
        line: Some(3 + t as i64 * 70),
        diff_side: "RIGHT".into(),
        comments: Vec::new(),
      })
    })
    .collect()
}

fn bench_lines(c: &mut Criterion) {
  let hunk = hunk(0);
  let hunks = vec![hunk.clone()];
  let comments = comments();
  let file = path(0);
  let mut group = c.benchmark_group("diff_view/lines");
  group.bench_function("line_numbers_550", |b| {
    b.iter(|| diff::line_numbers(black_box(&hunk)))
  });
  group.bench_function("line_at_visible_rows", |b| {
    b.iter(|| {
      for l in 0..VISIBLE {
        black_box(diff::line_at(&hunk, l));
      }
    })
  });
  // What each drawn row asks: its gutter icon and the cards under it.
  group.bench_function("comments_visible_rows", |b| {
    b.iter(|| {
      for l in 0..VISIBLE {
        black_box(diff::line_has_comment(&comments, &file, &hunk, l, &hunks));
        black_box(diff::comments_under(&comments, &file, &hunk, l, &hunks));
      }
    })
  });
  group.finish();
}

fn bench_threads(c: &mut Criterion) {
  let hunks: Vec<Vec<JjDiffHunk>> = (0..FILES).map(|f| vec![hunk(f)]).collect();
  let working: HashMap<String, &[JjDiffHunk]> =
    (0..FILES).map(|f| (path(f), &hunks[f][..])).collect();
  let committed = HashMap::new();
  let threads = threads();
  c.bench_function("diff_view/place_threads_30", |b| {
    b.iter(|| diff::place_threads(black_box(&threads), &[&working, &committed]))
  });
}

fn bench_intraline(c: &mut Criterion) {
  let hunk = hunk(0);
  c.bench_function("diff_view/intraline_250_pairs", |b| {
    b.iter(|| diff::changed_ranges(black_box(&hunk.lines)))
  });
  // Long lines with edits spread along them: the diffed middle is nearly
  // the whole 400 characters, the table's worst case.
  let long: Vec<String> = (0..PAIRS)
    .map(|i| format!("-{}", long_line(i, false)))
    .chain((0..PAIRS).map(|i| format!("+{}", long_line(i, true))))
    .collect();
  c.bench_function("diff_view/intraline_250_long_pairs", |b| {
    b.iter(|| diff::changed_ranges(black_box(&long)))
  });
}

/// A 400-character line; the new side changes one character in each
/// 40-character word.
fn long_line(i: usize, new: bool) -> String {
  let mark = if new { 'y' } else { 'x' };
  (0..10)
    .map(|w| format!("w{w}_{i:04}_{mark}_abcdefghijklmnopqrstuvwxyz0123"))
    .collect()
}

fn bench_highlight(c: &mut Criterion) {
  let lines: Vec<String> = hunk(0).lines.iter().map(|l| l[1..].to_string()).collect();
  let mut group = c.benchmark_group("diff_view/highlight");
  group.bench_function("warm_550_lines", |b| {
    b.iter(|| {
      for line in &lines {
        black_box(diff::highlight("rs", line));
      }
    })
  });
  // Lines not seen before: each goes through syntect.
  let mut round = 0usize;
  group.bench_function("cold_line", |b| {
    b.iter(|| {
      round += 1;
      black_box(diff::highlight(
        "rs",
        &format!("{} // {round}", lines[round % lines.len()]),
      ))
    })
  });
  group.finish();
}

/// What a comment card runs on its body each time it draws.
fn bench_comment_text(c: &mut Criterion) {
  let body = "Thanks :+1: this reads well.\n\nOne nit: `compute_0` could take a slice.\n\n\
              - rename `value_1`\n- drop the `map` :robot:\n";
  let mut group = c.benchmark_group("diff_view/comment_text");
  group.bench_function("looks_like_markdown", |b| {
    b.iter(|| diff::looks_like_markdown(black_box(body)))
  });
  group.bench_function("emojify", |b| b.iter(|| diff::emojify(black_box(body))));
  group.finish();
}

criterion_group!(
  benches,
  bench_comment_text,
  bench_lines,
  bench_threads,
  bench_intraline,
  bench_highlight
);
criterion_main!(benches);
