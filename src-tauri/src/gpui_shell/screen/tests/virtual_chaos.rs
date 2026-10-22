//! Chaos tests for the virtual lists: random scrolls, clicks, hovers, keys,
//! window resizes and zoom on a large Changes diff and a large commit,
//! checking after every step that the rows drawn stack in order without
//! overlapping, stay inside the list, and that the list never goes blank.
//!
//! `PROPTEST_CASES` sets how many cases run and `SEED` the run; a failure
//! prints the shrunk step list.

use gpui::{
  proptest::{collection::vec, prelude::*},
  Bounds, Modifiers, MouseButton, Pixels,
};

use super::*;
use crate::gpui_shell::fuzz;

/// One thing the user does.
#[derive(Debug, Clone)]
enum Chaos {
  /// The wheel, by this many pixels (down is negative).
  Scroll(i32),
  /// A click at this point of the list, in thousandths of its size.
  Click(u16, u16),
  /// The pointer moves to this point of the list.
  Hover(u16, u16),
  Key(&'static str),
  /// The window takes this size.
  Resize(u16, u16),
  /// The rem size, as `ui_zoom` sets it, in percent.
  Zoom(u16),
}

const KEYS: &[&str] = &[
  "up",
  "down",
  "shift-down",
  "shift-up",
  "pageup",
  "pagedown",
  "home",
  "end",
  "escape",
  "enter",
  "space",
  "cmd-f",
  "c",
];

fn chaos() -> impl Strategy<Value = Chaos> {
  prop_oneof![
    5 => (-6000i32..6000).prop_map(Chaos::Scroll),
    3 => (0u16..1000, 0u16..1000).prop_map(|(x, y)| Chaos::Click(x, y)),
    2 => (0u16..1000, 0u16..1000).prop_map(|(x, y)| Chaos::Hover(x, y)),
    4 => gpui::proptest::sample::select(KEYS).prop_map(Chaos::Key),
    1 => (500u16..1800, 360u16..1200).prop_map(|(w, h)| Chaos::Resize(w, h)),
    1 => gpui::proptest::sample::select(&[50u16, 80, 100, 125, 150, 200][..]).prop_map(Chaos::Zoom),
  ]
}

/// A large list and how to find its rows.
struct Scenario {
  /// The list's debug selector.
  list: &'static str,
  /// The rows' debug selectors, in list order.
  rows: Vec<&'static str>,
}

fn leak(s: String) -> &'static str {
  Box::leak(s.into_boxed_str())
}

fn changes() -> Scenario {
  let lines = changes_large::pairs() / 10 * 21;
  let mut rows = Vec::new();
  for f in 0..changes_large::FILES {
    rows.push(leak(format!("file-card-src/module_{f}.rs")));
    rows.extend((0..lines).map(|l| leak(format!("gutter-src/module_{f}.rs-0-{l}"))));
  }
  Scenario {
    list: "changes-diff-list",
    rows,
  }
}

fn commits() -> Scenario {
  let mut rows = vec!["commit-row-a"];
  for f in 0..commits_large::FILES {
    rows.extend(
      (0..commits_large::lines()).map(|l| leak(format!("commit-line-src/module_{f}.rs-0-{l}"))),
    );
  }
  rows.push("commit-row-b");
  Scenario {
    list: "commits-list",
    rows,
  }
}

fn act(h: &mut Harness, list: Bounds<Pixels>, step: &Chaos) {
  let at = |x: u16, y: u16| {
    gpui::point(
      list.left() + list.size.width * (x as f32 / 1000.),
      list.top() + list.size.height * (y as f32 / 1000.),
    )
  };
  match *step {
    Chaos::Scroll(dy) => h.cx.simulate_event(gpui::ScrollWheelEvent {
      position: list.center(),
      delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(dy as f32))),
      modifiers: Modifiers::none(),
      touch_phase: gpui::TouchPhase::Moved,
    }),
    Chaos::Click(x, y) => {
      h.cx
        .simulate_mouse_down(at(x, y), MouseButton::Left, Modifiers::none());
      h.cx
        .simulate_mouse_up(at(x, y), MouseButton::Left, Modifiers::none());
    }
    Chaos::Hover(x, y) => h.cx.simulate_mouse_move(at(x, y), None, Modifiers::none()),
    Chaos::Key(keys) => h.cx.simulate_keystrokes(keys),
    Chaos::Resize(w, hh) => h
      .cx
      .simulate_resize(gpui::size(gpui::px(w as f32), gpui::px(hh as f32))),
    Chaos::Zoom(z) => h.cx.update(|window, _| {
      window.set_rem_size(gpui::px(crate::gpui_shell::ui::BASE_REM * z as f32 / 100.));
      window.refresh();
    }),
  }
  h.cx.run_until_parked();
  // A step can open a dialog or move focus elsewhere; the next frame is
  // what the user sees.
  h.cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The rows drawn, in list order, with their bounds.
fn drawn(h: &mut Harness, rows: &[&'static str]) -> Vec<(&'static str, Bounds<Pixels>)> {
  rows
    .iter()
    .filter_map(|r| Some((*r, h.cx.debug_bounds(r)?)))
    .collect()
}

/// Checks the list after a step.
fn check(h: &mut Harness, scenario: &Scenario, log: &[String]) {
  let Some(list) = h.cx.debug_bounds(scenario.list) else {
    // A step closed the list's screen (Escape on a dialog, say): nothing
    // to check.
    return;
  };
  let rows = drawn(h, &scenario.rows);
  let eps = gpui::px(0.5);
  // Rows of one file keep their order; a commit's lines can sit under
  // either commit, so overlap is checked in screen order.
  let file = |row: &str| row.rsplit_once('-').map(|(file, _)| file.to_string());
  for pair in rows.windows(2) {
    let ((a, ab), (b, bb)) = (pair[0], pair[1]);
    if file(a) == file(b) {
      assert!(
        bb.top() >= ab.bottom() - eps,
        "{b} {bb:?} is not below {a} {ab:?} after {log:#?}"
      );
    }
  }
  let mut by_top = rows.clone();
  by_top.sort_by(|a, b| a.1.top().partial_cmp(&b.1.top()).expect("finite"));
  for pair in by_top.windows(2) {
    let ((a, ab), (b, bb)) = (pair[0], pair[1]);
    assert!(
      bb.top() >= ab.bottom() - eps,
      "{b} {bb:?} overlaps {a} {ab:?} after {log:#?}"
    );
  }
  // A window too narrow for the zoom leaves the list narrower than a
  // line's gutter; only a list that fits its rows is checked.
  if list.size.width >= gpui::px(400.) {
    for (row, bounds) in &rows {
      assert!(
        bounds.left() >= list.left() - eps && bounds.right() <= list.right() + eps,
        "{row} {bounds:?} sticks out of {list:?} after {log:#?}"
      );
    }
  }
  if list.size.height > gpui::px(200.) {
    let shown = rows
      .iter()
      .any(|(_, b)| b.bottom() > list.top() && b.top() < list.bottom());
    assert!(shown, "{} is blank: {list:?} after {log:#?}", scenario.list);
  }
}

fn run(h: &mut Harness, scenario: &Scenario, steps: &[Chaos]) {
  let mut log = Vec::new();
  check(h, scenario, &log);
  for step in steps {
    log.push(format!("{step:?}"));
    let Some(list) = h.cx.debug_bounds(scenario.list) else {
      return;
    };
    act(h, list, step);
    check(h, scenario, &log);
  }
}

#[test]
fn a_large_changes_diff_survives_chaos() {
  fuzz::check(
    "a_large_changes_diff_survives_chaos",
    4,
    vec(chaos(), 1..40),
    |cx, steps| {
      let mut h = changes_large::expanded(cx);
      run(&mut h, &changes(), &steps);
    },
  );
}

#[test]
fn a_large_commit_survives_chaos() {
  fuzz::check(
    "a_large_commit_survives_chaos",
    4,
    vec(chaos(), 1..40),
    |cx, steps| {
      let mut h = commits_large::opened(cx);
      run(&mut h, &commits(), &steps);
    },
  );
}
