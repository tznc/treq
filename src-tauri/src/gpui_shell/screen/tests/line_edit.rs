//! E edits the selected diff lines in place, writing the file directly.

use gpui::{Role, TestAppContext};
use gpui_testing_library::by;

use super::changes_screen::{changes_host_with_committed, committed_route};
use super::*;

/// The platform's primary modifier, as the keyboard syntax names it.
const SECONDARY: &str = if cfg!(target_os = "macos") {
  "Meta"
} else {
  "Control"
};

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

/// Clicks the gutter of the line the user knows by `name`, as "Line 2 of a.rs".
fn click_gutter(h: &mut Harness, name: &str) {
  let screen = h.rendered();
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name(name)));
}

fn keyboard(h: &mut Harness, keys: &str) {
  h.rendered().user().keyboard(keys);
  h.cx.run_until_parked();
}

/// Types into the open editor and returns what it then holds.
fn type_into_editor(h: &mut Harness, text: &str) -> String {
  keyboard(h, text);
  line_edit(h).expect("editing")
}

fn save(h: &mut Harness) {
  keyboard(h, &format!("{{{SECONDARY}>}}{{Enter}}{{/{SECONDARY}}}"));
}

#[gpui::test]
fn e_edits_the_selected_lines_in_place(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  // a.rs: " a", "+b", "+c"; the context line is part of the file too.
  click_gutter(&mut h, "Line 1 of a.rs");
  keyboard(&mut h, "{Shift>}{ArrowDown}{ArrowDown}{/Shift}e");
  assert!(h.shows(Role::MultilineTextInput, "Edit lines"));
  assert_eq!(line_edit(&mut h).as_deref(), Some("a\nb\nc"));
  let text = type_into_editor(&mut h, "x");
  assert_ne!(text, "a\nb\nc");
  save(&mut h);
  let writes = writes(&host);
  assert_eq!(writes.len(), 1, "{writes:?}");
  assert!(
    writes[0].ends_with(&format!("a.rs 1..3 [\"a\", \"b\", \"c\"] -> {text:?}")),
    "{writes:?}"
  );
  assert_eq!(line_edit(&mut h), None, "saved, the diff shows again");
}

#[gpui::test]
fn escape_closes_the_line_editor_without_writing(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  click_gutter(&mut h, "Line 2 of a.rs");
  keyboard(&mut h, "e");
  assert_eq!(line_edit(&mut h).as_deref(), Some("b"));
  type_into_editor(&mut h, "x");
  keyboard(&mut h, "{Escape}");
  assert_eq!(line_edit(&mut h), None);
  assert!(writes(&host).is_empty());
  // The diff has the keyboard again.
  keyboard(&mut h, "{ArrowDown}");
  assert_eq!(selected_lines(&mut h), Some(("a.rs".into(), vec![(0, 2)])));
}

#[gpui::test]
fn saving_unchanged_lines_writes_nothing(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  click_gutter(&mut h, "Line 2 of a.rs");
  keyboard(&mut h, "e");
  let screen = h.rendered();
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Save")));
  assert_eq!(line_edit(&mut h), None);
  assert!(writes(&host).is_empty());
}

#[gpui::test]
fn e_edits_the_kept_side_of_a_committed_file(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host_with_committed(), committed_route(true));
  // c.rs is committed: "-c", "+d".
  click_gutter(&mut h, "Removed line 1 of c.rs");
  keyboard(&mut h, "{Shift>}{ArrowDown}{/Shift}e");
  assert_eq!(
    line_edit(&mut h).as_deref(),
    Some("d"),
    "the removed line is not in the file"
  );
  let text = type_into_editor(&mut h, "x");
  save(&mut h);
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
  click_gutter(&mut h, "Removed line 1 of a.rs");
  keyboard(&mut h, "e");
  assert_eq!(line_edit(&mut h), None);
}

#[gpui::test]
fn e_edits_the_lines_of_a_text_selection(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  // A triple-click on line +c's code picks its text.
  let screen = h.rendered();
  screen
    .user()
    .triple_click(&screen.get_by_role(by::role(Role::Row).name("+c")));
  keyboard(&mut h, "e");
  assert_eq!(line_edit(&mut h).as_deref(), Some("c"));
}
