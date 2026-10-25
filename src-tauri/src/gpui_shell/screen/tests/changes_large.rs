//! The Changes tab on a large working copy: several files of about 500
//! changed lines each, every one expanded. The diff is a virtual list, so
//! only the rows on screen are drawn; jumping to a file, the arrow keys and
//! find still reach rows that are not. `changes_perf` (ignored) prints the
//! frame cost: `cargo test --features gpui-shell --lib changes_perf --
//! --ignored --nocapture`.

use std::time::{Duration, Instant};

use gpui::{Modifiers, MouseButton, TestAppContext};

use super::*;

pub(super) const FILES: usize = 6;
/// Replaced lines per file: as many removed as added.
const PAIRS: usize = 250;

pub(super) fn pairs() -> usize {
  std::env::var("PERF_PAIRS")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(PAIRS)
}

fn line(file: usize, i: usize, new: bool) -> String {
  let value = if new { i * 3 + 1 } else { i * 3 };
  format!(
    "    let value_{i} = compute_{file}(\"item {i}\", {value}).map(|v| v + {value}); // note {i}"
  )
}

/// `FILES` Rust files, each one hunk of `PAIRS` removed and added lines.
pub(super) fn large_host() -> FakeHost {
  let mut host = code_host();
  let files: Vec<_> = (0..FILES)
    .map(|f| {
      serde_json::json!({
        "path": format!("src/module_{f}.rs"),
        "status": "M",
        "previous_path": null,
        "changed_line_count": pairs() * 2,
        "diff_deferred": false,
      })
    })
    .collect();
  let hunks: Vec<_> = (0..FILES)
    .map(|f| {
      let mut lines = Vec::new();
      for chunk in 0..pairs() / 10 {
        for i in chunk * 10..chunk * 10 + 10 {
          lines.push(format!("-{}", line(f, i, false)));
        }
        for i in chunk * 10..chunk * 10 + 10 {
          lines.push(format!("+{}", line(f, i, true)));
        }
        lines.push(format!(" // context {chunk}"));
      }
      let old = lines.iter().filter(|l| !l.starts_with('+')).count();
      let new = lines.iter().filter(|l| !l.starts_with('-')).count();
      serde_json::json!({
        "path": format!("src/module_{f}.rs"),
        "hunks": [{
          "id": "h",
          "header": format!("@@ -1,{old} +1,{new} @@"),
          "lines": lines,
          "patch": "",
        }],
      })
    })
    .collect();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": files,
    "hunks_by_file": hunks,
    "too_large_to_render": false
  }))
  .ok();
  host
}

/// A full frame: the Changes view rendered, laid out and painted again.
pub(super) fn frame(h: &mut Harness) -> Duration {
  h.cx.run_until_parked();
  h.cx.update(|window, cx| {
    let start = Instant::now();
    window.refresh();
    window.draw(cx).clear(cx);
    start.elapsed()
  })
}

pub(super) fn report(name: &str, samples: &mut [Duration]) {
  samples.sort();
  let ms = |d: Duration| d.as_secs_f64() * 1000.;
  let mean = samples.iter().map(|d| ms(*d)).sum::<f64>() / samples.len() as f64;
  println!(
    "{name:<28} n={:<3} mean={mean:>8.2}ms  p50={:>8.2}ms  max={:>8.2}ms",
    samples.len(),
    ms(samples[samples.len() / 2]),
    ms(samples[samples.len() - 1]),
  );
}

pub(super) fn timed(h: &mut Harness, act: impl FnOnce(&mut Harness)) -> Duration {
  let start = Instant::now();
  act(h);
  h.cx.run_until_parked();
  h.cx.update(|window, cx| window.draw(cx).clear(cx));
  start.elapsed()
}

pub(super) fn leak(s: String) -> &'static str {
  Box::leak(s.into_boxed_str())
}

/// The large working copy with every file's diff shown.
pub(super) fn expanded(cx: &mut TestAppContext) -> Harness {
  expanded_with(cx, large_host())
}

fn expanded_with(cx: &mut TestAppContext, host: FakeHost) -> Harness {
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  if h.drawn("view-large-changeset") {
    h.click("view-large-changeset");
  }
  // Picking a file in the list shows its diff, however large.
  for f in (0..FILES).rev() {
    h.click(leak(format!("file-row-src/module_{f}.rs")));
  }
  h.cx.run_until_parked();
  h
}

fn gutter(f: usize, l: usize) -> &'static str {
  leak(format!("gutter-src/module_{f}.rs-0-{l}"))
}

#[gpui::test]
fn a_large_diff_draws_only_the_rows_on_screen(cx: &mut TestAppContext) {
  let mut h = expanded(cx);
  assert!(h.drawn("file-card-src/module_0.rs"));
  assert!(h.drawn(gutter(0, 0)));
  assert!(!h.drawn(gutter(0, 400)), "far below the window");
  assert!(!h.drawn("file-card-src/module_5.rs"));
}

#[gpui::test]
fn a_large_diff_scrolls_to_a_file_picked_in_the_list(cx: &mut TestAppContext) {
  let mut h = expanded(cx);
  h.click("file-row-src/module_4.rs");
  h.cx.run_until_parked();
  assert!(h.drawn("file-card-src/module_4.rs"));
  assert!(h.drawn(gutter(4, 0)));
  assert!(!h.drawn(gutter(0, 0)), "scrolled away from the first file");
  let card = h
    .cx
    .debug_bounds("file-card-src/module_4.rs")
    .expect("card");
  let list = h.cx.debug_bounds("changes-diff-list").expect("list");
  assert!(
    card.top() >= list.top() && card.top() < list.top() + gpui::px(40.),
    "{card:?} at the top of {list:?}"
  );
}

#[gpui::test]
fn a_large_diff_follows_the_cursor_line_off_screen(cx: &mut TestAppContext) {
  let mut h = expanded(cx);
  h.click(gutter(0, 0));
  for _ in 0..120 {
    h.cx.simulate_keystrokes("down");
  }
  h.cx.run_until_parked();
  assert!(h.drawn(gutter(0, 120)), "the cursor's line is drawn");
  assert!(!h.drawn(gutter(0, 0)));
  let line = h.cx.debug_bounds(gutter(0, 120)).expect("line");
  let list = h.cx.debug_bounds("changes-diff-list").expect("list");
  assert!(
    line.top() >= list.top() && line.bottom() <= list.bottom(),
    "{line:?} inside {list:?}"
  );
}

#[gpui::test]
fn a_large_diff_finds_text_on_rows_not_drawn(cx: &mut TestAppContext) {
  let mut h = expanded(cx);
  h.click("file-card-src/module_0.rs");
  h.cx.simulate_keystrokes("cmd-f");
  assert!(h.drawn("diff-search"));
  // Only module 5's last pair has `value_249 = compute_5`.
  h.cx.simulate_input("value_249 = compute_5");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(150));
  h.cx.run_until_parked();
  // Enter reveals a match: the removed or the added line 249, in the last
  // chunk of 21 lines (removed at +9, added at +19). Either way the added
  // one is on screen.
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(
    h.drawn(gutter(5, 24 * 21 + 19)),
    "the match's line is drawn"
  );
  assert!(!h.drawn(gutter(0, 0)));
}

/// `large_host` under review: a kept review of `COMMENTS` line comments and
/// `THREADS` GitHub review threads on every file.
fn reviewed_host() -> FakeHost {
  const COMMENTS: usize = 20;
  const THREADS: usize = 5;
  let mut host = large_host();
  // New-side numbers run to `pairs() * 11 / 10`; spread the lines over them.
  let step = (pairs() * 11 / 10 / COMMENTS).max(1);
  let comments: Vec<_> = (0..FILES)
    .flat_map(|f| {
      (0..COMMENTS).map(move |c| {
        let line = 1 + c * step;
        serde_json::json!({
          "id": format!("c{f}-{c}"), "file_path": format!("src/module_{f}.rs"),
          "hunk_id": "h", "start_line": line, "end_line": line,
          "line_content": [], "text": "Kept", "created_at": "", "side": "new"
        })
      })
    })
    .collect();
  *host.review.lock().unwrap() = Some(serde_json::Value::Array(comments).to_string());
  host.threads = (0..FILES)
    .flat_map(|f| {
      (0..THREADS).map(move |t| {
        let mut thread = gh_thread(&format!("t{f}-{t}"), (3 + t * step * 4) as i64, false);
        thread.path = format!("src/module_{f}.rs");
        thread
      })
    })
    .collect();
  host
}

/// Opens find and applies `query`.
fn search(h: &mut Harness, query: &str) {
  h.cx.simulate_keystrokes("cmd-f");
  h.cx.simulate_input(query);
  h.cx.executor().advance_clock(Duration::from_millis(150));
  h.cx.run_until_parked();
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn changes_perf(cx: &mut TestAppContext) {
  let mut h = expanded(cx);
  measure(&mut h);
}

/// `changes_perf` with line comments, review threads and an applied find.
#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn changes_review_perf(cx: &mut TestAppContext) {
  let mut h = expanded_with(cx, reviewed_host());
  assert!(h.drawn("inline-comment-card") || h.drawn("github-comment-card"));
  println!("-- with comments and threads");
  measure(&mut h);
  search(&mut h, "value_1");
  println!("-- and a find applied");
  let mut full: Vec<_> = (0..10).map(|_| frame(&mut h)).collect();
  report("full frame", &mut full);
}

fn measure(h: &mut Harness) {
  assert!(h.drawn(gutter(0, 0)));
  let mut full: Vec<_> = (0..10).map(|_| frame(h)).collect();
  report("full frame", &mut full);

  let origin = h
    .cx
    .debug_bounds("gutter-src/module_0.rs-0-0")
    .expect("gutter")
    .center();
  let mut hover: Vec<_> = (0..30)
    .map(|i| {
      let at = gpui::point(
        origin.x + gpui::px(300.),
        origin.y + gpui::px(i as f32 * 7.),
      );
      timed(h, |h| h.cx.simulate_mouse_move(at, None, Modifiers::none()))
    })
    .collect();
  report("hover a line", &mut hover);

  let mut click: Vec<_> = (0..5)
    .map(|i| {
      let at = gpui::point(origin.x, origin.y + gpui::px(i as f32 * 20.));
      timed(h, |h| {
        h.cx
          .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
        h.cx
          .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
      })
    })
    .collect();
  report("click a line", &mut click);

  let mut keys: Vec<_> = (0..20)
    .map(|_| timed(h, |h| h.cx.simulate_keystrokes("down")))
    .collect();
  report("arrow down", &mut keys);

  let mut scroll: Vec<_> = (0..20)
    .map(|_| {
      timed(h, |h| {
        h.cx.simulate_event(gpui::ScrollWheelEvent {
          position: origin,
          delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-200.))),
          modifiers: Modifiers::none(),
          touch_phase: gpui::TouchPhase::Moved,
        })
      })
    })
    .collect();
  report("scroll 200px", &mut scroll);

  // A sidebar frame redraws the shell; the cached screen replays.
  let mut beside: Vec<_> = (0..20)
    .map(|_| {
      timed(h, |h| {
        h.cx.update(|window, cx| {
          if let Some(Some(root)) = window.root::<Framed>() {
            root.update(cx, |_, cx| cx.notify());
          }
        })
      })
    })
    .collect();
  report("a frame beside the screen", &mut beside);
  let screen = h.screen.clone();
  let mut own: Vec<_> = (0..20)
    .map(|_| timed(h, |h| screen.update(&mut h.cx, |_, cx| cx.notify())))
    .collect();
  report("the screen's own frame", &mut own);

  h.click("commit-message");
  let mut typing: Vec<_> = (0..20)
    .map(|_| timed(h, |h| h.cx.simulate_input("x")))
    .collect();
  report("type in the message", &mut typing);
}

/// A refresh (window focus) of the large working copy that reads back the
/// same diff: from the revision bump to the settled frame. Without a token
/// every file's hunks are read and compared again; with one, none are.
#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn changes_refresh_perf(cx: &mut TestAppContext) {
  refresh_perf(cx, None);
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn changes_refresh_token_perf(cx: &mut TestAppContext) {
  refresh_perf(cx, Some("t1"));
}

fn refresh_perf(cx: &mut TestAppContext, token: Option<&str>) {
  let mut host = large_host();
  if let Some(working) = host.working.as_mut() {
    working.working_copy_token = token.map(str::to_string);
  }
  let mut h = expanded_with(cx, host);
  let mut samples: Vec<_> = (0..10)
    .map(|_| {
      timed(&mut h, |h| {
        h.update(|s| {
          if let View::Workspace(r) = &mut s.view {
            r.changes_revision += 1;
          }
        })
      })
    })
    .collect();
  report(&format!("refresh, token {token:?}"), &mut samples);
}
