//! The standard text-editing shortcuts in the app's text fields: paste,
//! select all, copy, cut and undo, with Cmd on macOS and Ctrl elsewhere.
//! Each field is reached the way a user reaches it, and the keys go through
//! GPUI's keyboard dispatch, so a handler that swallows them fails here.

use gpui::{Role, TestAppContext};
use gpui_testing_library::{prelude::*, Node};

use super::*;

/// The platform's primary modifier, as the keyboard syntax names it.
const SECONDARY: &str = if cfg!(target_os = "macos") {
  "Meta"
} else {
  "Control"
};

fn screen(h: &mut Harness) -> gpui_testing_library::Screen {
  h.cx
    .background_executor
    .advance_clock(std::time::Duration::from_millis(400));
  h.cx.run_until_parked();
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  screen
}

/// The text field holding keyboard focus.
#[track_caller]
fn focused(screen: &gpui_testing_library::Screen) -> Node {
  screen.redraw();
  screen
    .elements()
    .into_iter()
    .find(|node| {
      node.is_focused()
        && matches!(
          node.role(),
          Some(Role::TextInput | Role::MultilineTextInput | Role::SearchInput)
        )
    })
    .expect("a focused text field")
}

fn value(screen: &gpui_testing_library::Screen) -> String {
  focused(screen).value().unwrap_or_default()
}

fn shortcut(screen: &gpui_testing_library::Screen, key: &str) {
  screen
    .user()
    .keyboard(&format!("{{{SECONDARY}>}}{key}{{/{SECONDARY}}}"));
}

/// Runs every shortcut against the focused field, whatever it holds.
#[track_caller]
fn edits_with_shortcuts(screen: &gpui_testing_library::Screen) {
  let user = screen.user();

  shortcut(screen, "a");
  user.paste("one");
  assert_eq!(
    value(screen),
    "one",
    "select all, then paste, replaces everything"
  );

  user.paste(" two");
  assert_eq!(value(screen), "one two", "paste inserts at the caret");

  shortcut(screen, "z");
  assert_eq!(value(screen), "one", "undo takes the paste back");

  shortcut(screen, "a");
  assert_eq!(user.copy().as_deref(), Some("one"));
  assert_eq!(value(screen), "one", "copy leaves the text");

  shortcut(screen, "a");
  assert_eq!(user.cut().as_deref(), Some("one"));
  assert_eq!(value(screen), "", "cut takes the selection");

  user.keyboard("typed");
  assert_eq!(
    value(screen),
    "typed",
    "typing still works after the shortcuts"
  );
}

fn open_workspaces(cx: &mut TestAppContext) -> Harness {
  let mut host = FakeHost::default();
  host.github.remote = github::acme();
  let snapshot: ScreenSnapshot = serde_json::from_value(serde_json::json!({
    "kind": "onboarding",
    "dark": false,
    "modal": {
      "kind": "openWorkspaces",
      "repoPath": "/repo",
      "repos": ["/repo"],
      "linear": false,
      "stackOn": null,
    },
  }))
  .expect("snapshot");
  Harness::with_host(cx, snapshot, Arc::new(host))
}

#[gpui::test]
fn new_workspaces_lines(cx: &mut TestAppContext) {
  let mut h = open_workspaces(cx);
  let screen = screen(&mut h);
  screen.user().click(&screen.get_by_label_text("Workspaces"));
  edits_with_shortcuts(&screen);
}

#[gpui::test]
fn new_workspaces_pull_request_search(cx: &mut TestAppContext) {
  let mut h = open_workspaces(cx);
  let screen = screen(&mut h);
  screen
    .user()
    .click(&screen.get_by_label_text("Search pull requests"));
  edits_with_shortcuts(&screen);
}

#[gpui::test]
fn agent_prompt(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  // The prompt view is built on its first draw.
  screen(&mut h);
  let screen = screen(&mut h);
  edits_with_shortcuts(&screen);
}

#[gpui::test]
fn commit_message(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  let screen = screen(&mut h);
  h.click("commit-message");
  edits_with_shortcuts(&screen);
}

#[gpui::test]
fn stack_dialog_title(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, workspace_dialogs::dialog(serde_json::json!({})));
  let screen = screen(&mut h);
  screen.user().click(&screen.get_by_label_text("Title"));
  edits_with_shortcuts(&screen);
}

#[gpui::test]
fn settings_field(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  let screen = screen(&mut h);
  h.click("settings-branch-pattern");
  edits_with_shortcuts(&screen);
}
