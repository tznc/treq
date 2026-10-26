use gpui::{Modifiers, MouseButton, TestAppContext};

use super::*;

/// Drags in the gutter from one line to another, through every line between.
fn drag_gutter(h: &mut Harness, from: usize, to: usize) {
  h.cx.run_until_parked();
  let at = |h: &mut Harness, l: usize| {
    h.cx
      .debug_bounds(Box::leak(format!("gutter-a.rs-0-{l}").into_boxed_str()))
      .expect("gutter")
      .center()
  };
  let start = at(h, from);
  h.cx
    .simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
  let path: Vec<usize> = if from <= to {
    (from..=to).collect()
  } else {
    (to..=from).rev().collect()
  };
  let mut last = start;
  for l in path {
    last = at(h, l);
    h.cx
      .simulate_mouse_move(last, Some(MouseButton::Left), Modifiers::none());
    h.cx.run_until_parked();
  }
  h.cx
    .simulate_mouse_up(last, MouseButton::Left, Modifiers::none());
}

#[gpui::test]
fn changes_tab_drag_selects_lines_downward(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  drag_gutter(&mut h, 0, 2);
  assert_eq!(
    selected_lines(&mut h).map(|(_, l)| l),
    Some(vec![(0, 0), (0, 1), (0, 2)])
  );
}

#[gpui::test]
fn changes_tab_drag_selects_lines_upward(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  drag_gutter(&mut h, 2, 0);
  assert_eq!(
    selected_lines(&mut h).map(|(_, l)| l),
    Some(vec![(0, 0), (0, 1), (0, 2)])
  );
}

/// a.rs as a replaced pair, more added lines and a trailing context line, the
/// shape of the QA app's alerting.ts diff.
fn changes_host_replaced() -> FakeHost {
  let mut host = changes_host();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": [
      {"path": "a.rs", "status": "M", "previous_path": null, "changed_line_count": 8, "diff_deferred": false}
    ],
    "hunks_by_file": [
      {"path": "a.rs", "hunks": [{"id": "h", "header": "@@ -1,3 +1,7 @@", "lines": [
        "-fn f(line: &str) {", "-  g(line)", "+fn f(line: &str, level: u8) {", "+  g(line, level)",
        "+}", "+", "+fn e(line: &str) {", "+  f(line, 3)", " }"
      ], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  host
}

#[gpui::test]
fn changes_tab_drag_selects_across_a_replaced_pair_both_ways(cx: &mut TestAppContext) {
  let all: Vec<(usize, usize)> = (0..9).map(|l| (0, l)).collect();
  let (mut h, _) = workspace_harness(cx, changes_host_replaced(), changes_route());
  drag_gutter(&mut h, 0, 8);
  assert_eq!(
    selected_lines(&mut h).map(|(_, l)| l),
    Some(all.clone()),
    "down"
  );
  h.cx.simulate_keystrokes("escape");
  drag_gutter(&mut h, 8, 0);
  assert_eq!(selected_lines(&mut h).map(|(_, l)| l), Some(all), "up");
}

#[gpui::test]
fn changes_tab_shift_click_extends_the_line_selection(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_replaced(), changes_route());
  h.click("gutter-a.rs-0-6");
  let at = h
    .cx
    .debug_bounds("gutter-a.rs-0-1")
    .expect("gutter")
    .center();
  h.cx.simulate_click(at, Modifiers::shift());
  assert_eq!(
    selected_lines(&mut h).map(|(_, l)| l),
    Some((1..=6).map(|l| (0, l)).collect())
  );
}

#[test]
fn terminal_label_shows_the_workspace_title_else_the_branch() {
  let place = terminal::Place {
    branches: vec![
      ("/ws/a".into(), "feat-a".into()),
      ("/ws/b".into(), "feat-b".into()),
    ],
    titles: vec![("/ws/a".into(), "Fix login flow".into())],
    ..terminal::Place::default()
  };
  assert_eq!(
    place.workspace_label("/ws/a").as_deref(),
    Some("Fix login flow")
  );
  assert_eq!(place.workspace_label("/ws/b").as_deref(), Some("feat-b"));
  assert_eq!(place.workspace_label("/elsewhere"), None);
}

#[gpui::test]
fn workspace_header_controls_share_one_height(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  let mut height = |s: &'static str| h.cx.debug_bounds(s).expect(s).size.height;
  let toolbar = height("workspace-view-pr");
  for selector in [
    "workspace-view-pr-menu",
    "workspace-checks",
    "workspace-sync",
    "workspace-merge",
    "workspace-more",
  ] {
    assert_eq!(height(selector), toolbar, "{selector}");
  }
  assert_eq!(toolbar, gpui::px(crate::gpui_shell::ui::theme::CONTROL_MD));
  // The pip names what it signals.
  assert!(h.drawn("workspace-sync-pip"));
}

/// Presses the left button on `at` with `clicks` clicks, as a fast repeat does.
fn press_code(h: &mut Harness, at: gpui::Point<gpui::Pixels>, clicks: usize) {
  h.cx.simulate_event(gpui::MouseDownEvent {
    position: at,
    button: MouseButton::Left,
    modifiers: Modifiers::none(),
    click_count: clicks,
    first_mouse: false,
  });
}

fn code_bounds(h: &mut Harness, l: usize) -> gpui::Bounds<gpui::Pixels> {
  h.cx.run_until_parked();
  h.cx
    .debug_bounds(Box::leak(format!("code-a.rs-0-{l}").into_boxed_str()))
    .expect("code")
}

fn copy(h: &mut Harness) -> Option<String> {
  h.cx.simulate_keystrokes(if cfg!(target_os = "macos") {
    "cmd-c"
  } else {
    "ctrl-c"
  });
  h.cx.run_until_parked();
  h.cx.read_from_clipboard().and_then(|c| c.text())
}

#[gpui::test]
fn changes_tab_drag_over_code_copies_the_plain_text(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_replaced(), changes_route());
  let from = code_bounds(&mut h, 2);
  let to = code_bounds(&mut h, 3);
  let start = gpui::point(from.left() + gpui::px(1.), from.center().y);
  let end = gpui::point(to.right() - gpui::px(1.), to.center().y);
  press_code(&mut h, start, 1);
  h.cx
    .simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
  h.cx
    .simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
  assert_eq!(
    copy(&mut h).as_deref(),
    Some("fn f(line: &str, level: u8) {\n  g(line, level)")
  );
  assert_eq!(selected_lines(&mut h), None, "no gutter range starts");
}

#[gpui::test]
fn changes_tab_double_and_triple_click_select_a_word_and_a_line(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_replaced(), changes_route());
  let b = code_bounds(&mut h, 3);
  let on_g = gpui::point(b.left() + gpui::px(1.), b.center().y);
  // "  g(line, level)": the first click lands on the leading spaces.
  press_code(&mut h, on_g, 2);
  h.cx
    .simulate_mouse_up(on_g, MouseButton::Left, Modifiers::none());
  assert_eq!(copy(&mut h).as_deref(), Some("  "));
  press_code(&mut h, on_g, 3);
  h.cx
    .simulate_mouse_up(on_g, MouseButton::Left, Modifiers::none());
  assert_eq!(copy(&mut h).as_deref(), Some("  g(line, level)"));
}

#[gpui::test]
fn changes_tab_escape_clears_the_text_selection(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_replaced(), changes_route());
  let b = code_bounds(&mut h, 3);
  let at = gpui::point(b.left() + gpui::px(1.), b.center().y);
  press_code(&mut h, at, 3);
  h.cx
    .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  let text = h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    workspace
      .changes()
      .expect("changes")
      .read(cx)
      .review
      .text
      .clone()
  });
  assert_eq!(text, None);
}
