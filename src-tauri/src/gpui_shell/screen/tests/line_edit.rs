//! E edits the selected diff lines in place, writing the file directly.

use gpui::{Modifiers, MouseButton, TestAppContext};

use super::changes_screen::{changes_host_with_committed, committed_route};
use super::*;

/// The text in the line editor, while it is open.
fn line_edit(h: &mut Harness) -> Option<String> {
  h.cx.run_until_parked();
  h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    let changes = workspace.changes().expect("changes").read(cx);
    changes
      .review
      .line_edit
      .as_ref()
      .map(|_| changes.review.line_edit_text.clone())
  })
}

/// The file writes asked of the host.
fn writes(host: &FakeHost) -> Vec<String> {
  host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("apply suggestion"))
    .collect()
}

/// Types into the open editor and returns what it then holds.
fn type_into_editor(h: &mut Harness, text: &str) -> String {
  h.cx.simulate_input(text);
  line_edit(h).expect("editing")
}

#[gpui::test]
fn e_edits_the_selected_lines_in_place(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  // a.rs: " a", "+b", "+c"; the context line is part of the file too.
  h.click("gutter-a.rs-0-0");
  h.cx.simulate_keystrokes("shift-down shift-down e");
  assert!(h.shows(gpui::Role::MultilineTextInput, "Edit lines"));
  assert_eq!(line_edit(&mut h).as_deref(), Some("a\nb\nc"));
  let text = type_into_editor(&mut h, "x");
  assert_ne!(text, "a\nb\nc");
  h.cx.simulate_keystrokes("secondary-enter");
  let writes = writes(&host);
  assert_eq!(writes.len(), 1, "{writes:?}");
  assert!(
    writes[0].ends_with(&format!(
      "a.rs 1..3 [\"a\", \"b\", \"c\"] -> {text:?}"
    )),
    "{writes:?}"
  );
  assert_eq!(line_edit(&mut h), None, "saved, the diff shows again");
}

#[gpui::test]
fn escape_closes_the_line_editor_without_writing(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("gutter-a.rs-0-1");
  h.cx.simulate_keystrokes("e");
  assert_eq!(line_edit(&mut h).as_deref(), Some("b"));
  type_into_editor(&mut h, "x");
  h.cx.simulate_keystrokes("escape");
  assert_eq!(line_edit(&mut h), None);
  assert!(writes(&host).is_empty());
  // The diff has the keyboard again.
  h.cx.simulate_keystrokes("down");
  assert_eq!(selected_lines(&mut h), Some(("a.rs".into(), vec![(0, 2)])));
}

#[gpui::test]
fn saving_unchanged_lines_writes_nothing(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("gutter-a.rs-0-1");
  h.cx.simulate_keystrokes("e");
  h.click("line-edit-save");
  assert_eq!(line_edit(&mut h), None);
  assert!(writes(&host).is_empty());
}

#[gpui::test]
fn e_edits_the_kept_side_of_a_committed_file(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host_with_committed(), committed_route(true));
  // c.rs is committed: "-c", "+d".
  h.click("gutter-c.rs-0-0");
  h.cx.simulate_keystrokes("shift-down e");
  assert_eq!(
    line_edit(&mut h).as_deref(),
    Some("d"),
    "the removed line is not in the file"
  );
  let text = type_into_editor(&mut h, "x");
  h.cx.simulate_keystrokes("secondary-enter");
  let writes = writes(&host);
  assert!(
    writes
      .iter()
      .any(|w| w.ends_with(&format!("c.rs 1..1 [\"d\"] -> {text:?}"))),
    "{writes:?}"
  );
}

#[gpui::test]
fn e_on_removed_lines_alone_opens_nothing(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_committed(), committed_route(true));
  h.click("gutter-a.rs-0-0");
  h.cx.simulate_keystrokes("e");
  assert_eq!(line_edit(&mut h), None);
}

#[gpui::test]
fn e_edits_the_lines_of_a_text_selection(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  h.cx.run_until_parked();
  // A triple-click picks line +c's text.
  let at = h.cx.debug_bounds("code-a.rs-0-2").expect("code").center();
  h.cx.simulate_event(gpui::MouseDownEvent {
    position: at,
    button: MouseButton::Left,
    modifiers: Modifiers::none(),
    click_count: 3,
    first_mouse: false,
  });
  h.cx
    .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
  h.cx.simulate_keystrokes("e");
  assert_eq!(line_edit(&mut h).as_deref(), Some("c"));
}
