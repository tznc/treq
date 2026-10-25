//! The Commits tab with a large commit open: several files of about 500
//! changed lines each. The timeline and the open diff are one virtual
//! list, so only the rows on screen are drawn. `commits_perf` (ignored)
//! prints the frame cost: `cargo test --features gpui-shell --lib
//! commits_perf -- --ignored --nocapture`.

use gpui::{Modifiers, MouseButton, TestAppContext};

use super::changes_large::{frame, leak, report, timed};
use super::*;

pub(super) const FILES: usize = 6;
/// Changed lines per file.
const LINES: usize = 500;

pub(super) fn lines() -> usize {
  std::env::var("PERF_LINES")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(LINES)
}

/// Every commit's diff: `FILES` files, each one hunk of `LINES` lines,
/// half removed and half added.
pub(super) fn large_host() -> FakeHost {
  let mut host = commits_host();
  let n = lines();
  let files: Vec<_> = (0..FILES)
    .map(|f| {
      serde_json::json!({
        "path": format!("src/module_{f}.rs"), "status": "M", "previous_path": null,
        "changed_line_count": n, "diff_deferred": false
      })
    })
    .collect();
  let hunks: Vec<_> = (0..FILES)
    .map(|f| {
      let lines: Vec<String> = (0..n)
        .map(|i| {
          let sign = if (i / 10) % 2 == 0 { '-' } else { '+' };
          format!("{sign}    let value_{i} = compute_{f}(\"item {i}\", {i}).map(|v| v + {i});")
        })
        .collect();
      serde_json::json!({
        "path": format!("src/module_{f}.rs"),
        "hunks": [{"id": "h", "header": format!("@@ -1,{} +1,{} @@", n / 2, n / 2), "lines": lines, "patch": ""}]
      })
    })
    .collect();
  host.diff = serde_json::from_value(serde_json::json!({
    "committed_files": files,
    "hunks_by_file": hunks,
    "too_large_to_render": false
  }))
  .ok();
  host
}

fn line(f: usize, l: usize) -> &'static str {
  leak(format!("commit-line-src/module_{f}.rs-0-{l}"))
}

/// The large commit, open.
pub(super) fn opened(cx: &mut TestAppContext) -> Harness {
  let (mut h, _) = workspace_harness(cx, large_host(), commits_route());
  h.click("commit-row-a");
  h.cx.run_until_parked();
  h
}

#[gpui::test]
fn a_large_commit_draws_only_the_rows_on_screen(cx: &mut TestAppContext) {
  let mut h = opened(cx);
  assert!(h.drawn(line(0, 0)));
  assert!(!h.drawn(line(0, 400)), "far below the window");
  assert!(!h.drawn(line(5, 0)));
  assert!(h.drawn("commit-row-a"));
}

#[gpui::test]
fn a_large_commit_scrolls_and_comments_on_a_line_far_down(cx: &mut TestAppContext) {
  let mut h = opened(cx);
  let at = h.cx.debug_bounds("commits-list").expect("list").center();
  for _ in 0..30 {
    h.cx.simulate_event(gpui::ScrollWheelEvent {
      position: at,
      delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-400.))),
      modifiers: Modifiers::none(),
      touch_phase: gpui::TouchPhase::Moved,
    });
    h.cx.run_until_parked();
  }
  assert!(!h.drawn(line(0, 0)), "scrolled away from the top");
  assert!(!h.drawn("commit-row-a"));
  // The first line on screen, wherever the scroll landed.
  let list = h.cx.debug_bounds("commits-list").expect("list");
  let (f, l) = (0..FILES)
    .flat_map(|f| (0..lines()).map(move |l| (f, l)))
    .find(|&(f, l)| {
      h.cx
        .debug_bounds(line(f, l))
        .is_some_and(|b| b.top() >= list.top() && b.bottom() <= list.bottom())
    })
    .expect("a line on screen");
  let row = h.cx.debug_bounds(line(f, l)).expect("line");
  h.cx
    .simulate_mouse_move(row.center(), None, Modifiers::none());
  h.click(leak(format!("add-comment-0-{l}")));
  assert!(h.drawn("comment-input"));
  let input = h.cx.debug_bounds("comment-input").expect("comment box");
  assert!(input.top() >= row.bottom() - gpui::px(1.), "under its line");
}

#[gpui::test]
fn a_large_commit_folds_and_unfolds_its_files(cx: &mut TestAppContext) {
  let mut h = opened(cx);
  h.click("commit-file-row");
  assert!(!h.drawn(line(0, 0)), "the first file folded");
  assert!(h.drawn(line(1, 0)), "the next file moves up");
  h.click("commit-row-a");
  assert!(!h.drawn("commit-file-row"));
  assert!(h.drawn("commit-row-b"));
  h.click("commit-row-a");
  assert!(h.drawn(line(1, 0)), "folds outlive the commit closing");
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn commits_perf(cx: &mut TestAppContext) {
  let mut h = opened(cx);
  assert!(h.drawn("commit-file-row"));
  let mut full: Vec<_> = (0..10).map(|_| frame(&mut h)).collect();
  report("full frame", &mut full);

  let origin = h
    .cx
    .debug_bounds("commit-file-row")
    .expect("file row")
    .center();
  let mut hover: Vec<_> = (0..30)
    .map(|i| {
      let at = gpui::point(origin.x, origin.y + gpui::px(40. + i as f32 * 7.));
      timed(&mut h, |h| {
        h.cx.simulate_mouse_move(at, None, Modifiers::none())
      })
    })
    .collect();
  report("hover a line", &mut hover);

  let mut click: Vec<_> = (0..5)
    .map(|i| {
      let at = gpui::point(origin.x, origin.y + gpui::px(40. + i as f32 * 20.));
      timed(&mut h, |h| {
        h.cx
          .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
        h.cx
          .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
      })
    })
    .collect();
  report("click a line", &mut click);

  let mut scroll: Vec<_> = (0..20)
    .map(|_| {
      timed(&mut h, |h| {
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
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn commits_steady_perf(cx: &mut TestAppContext) {
  let mut host = large_host();
  let commits: Vec<_> = (0..100).map(|i| commit(&format!("c{i}"), 10, 4)).collect();
  host.history = serde_json::from_value(serde_json::json!({
    "commits": commits,
    "target_branch": "main",
    "workspace_branch": "feature/login"
  }))
  .ok();
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  h.click("commit-row-c0");
  h.cx.run_until_parked();
  assert!(h.drawn("commit-file-row"));
  let mut steady: Vec<_> = (0..20).map(|_| frame(&mut h)).collect();
  report("steady frame, 100 commits", &mut steady);
}

/// 2,000 commits over 100 days, the first one's large diff aside.
fn history_host() -> FakeHost {
  let mut host = large_host();
  let commits: Vec<_> = (0..2000)
    .map(|i| {
      let mut c = commit(&format!("c{i}"), 10, 4);
      c["timestamp"] = format!("2026-07-{:02}T{:02}:00:00Z", 1 + (i / 20) % 28, i % 20).into();
      c["description"] = format!("Commit number {i}\n\nA body that runs on for a while.").into();
      c
    })
    .collect();
  host.history = serde_json::from_value(serde_json::json!({
    "commits": commits,
    "target_branch": "main",
    "workspace_branch": "feature/login"
  }))
  .ok();
  host
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn commits_history_perf(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, history_host(), commits_route());
  h.cx.run_until_parked();
  assert!(h.drawn("commit-row-c0"));
  let mut steady: Vec<_> = (0..20).map(|_| frame(&mut h)).collect();
  report("steady frame, 2000 commits", &mut steady);

  // Opening and closing a commit lays the rows out again.
  let mut toggle: Vec<_> = (0..10)
    .map(|_| timed(&mut h, |h| h.click("commit-row-c1")))
    .collect();
  report("open/close a commit", &mut toggle);

  let origin = h.cx.debug_bounds("commits-list").expect("list").center();
  let mut hover: Vec<_> = (0..30)
    .map(|i| {
      let at = gpui::point(origin.x, origin.y + gpui::px(i as f32 * 9.));
      timed(&mut h, |h| {
        h.cx.simulate_mouse_move(at, None, Modifiers::none())
      })
    })
    .collect();
  report("hover rows", &mut hover);

  let mut scroll: Vec<_> = (0..30)
    .map(|_| {
      timed(&mut h, |h| {
        h.cx.simulate_event(gpui::ScrollWheelEvent {
          position: origin,
          delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-400.))),
          modifiers: Modifiers::none(),
          touch_phase: gpui::TouchPhase::Moved,
        })
      })
    })
    .collect();
  report("scroll 400px", &mut scroll);
}
