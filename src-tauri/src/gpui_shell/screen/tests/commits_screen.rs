use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn commits_tab_lists_commits_and_opens_their_diffs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  for selector in [
    "workspace-commits",
    "commit-row-a",
    "commit-row-b",
    "shift-commits-to-now",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(!h.drawn("workspace-code"));
  assert!(host
    .calls()
    .contains(&"read log Some(7) None None".to_string()));
  h.click("commit-row-a");
  assert!(h.drawn("commit-file-row"));
  assert!(host.calls().contains(&"read diff a".to_string()));
  // The newest commit can be undone; every own commit edited and moved.
  for selector in [
    "edit-description-a",
    "undo-commit-a",
    "revert-commit-a",
    "edit-timestamp-a",
    "move-commit-a",
    "stash-commit-button",
    "delete-commit-a",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  h.click("commit-file-row");
  assert!(!h.drawn("add-comment-0-1"));
  h.click("commit-row-a");
  assert!(!h.drawn("commit-file-row"));
}

#[gpui::test]
fn commits_tab_rewrites_commits_in_the_home_repo(cx: &mut TestAppContext) {
  let mut route = route_json(None, "main");
  route["code"] = true.into();
  route["tab"] = "commits".into();
  let route = serde_json::from_value(route).expect("home commits route");
  let (mut h, host) = workspace_harness(cx, commits_host(), route);
  h.click("commit-row-a");
  for selector in ["edit-description-a", "revert-commit-a", "delete-commit-a"] {
    assert!(h.drawn(selector), "{selector}");
  }
  h.click("revert-commit-a");
  h.answer("Revert commit");
  assert!(host.writes().contains(&"revert a".to_string()));
  h.click("edit-description-a");
  assert!(h.drawn("edit-description-dialog"));
  assert!(host.calls().contains(&"read description a".to_string()));
}

#[gpui::test]
fn commits_tab_abandons_once_confirmed(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("delete-commit-a");
  h.answer("Cancel");
  assert!(!host.writes().iter().any(|w| w.starts_with("abandon")));
  assert!(!h.drawn("confirm"));
}

#[gpui::test]
fn commits_tab_removes_an_abandoned_commit(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("delete-commit-a");
  assert!(!host.writes().iter().any(|w| w.starts_with("abandon")));
  h.answer("Delete commit");
  assert!(host.writes().contains(&"abandon a".to_string()));
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(300));
  assert!(!h.drawn("commit-row-a"));
  assert!(h.drawn("commit-row-b"));
}

#[gpui::test]
fn commits_tab_keeps_a_commit_it_failed_to_abandon(cx: &mut TestAppContext) {
  let mut host = commits_host();
  host.failing = vec!["abandon"];
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  h.click("commit-row-a");
  h.click("delete-commit-a");
  h.answer("Delete commit");
  assert!(host.writes().contains(&"abandon a".to_string()));
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(300));
  assert!(h.drawn("commit-row-a"), "put back");
  assert!(h.drawn("delete-commit-a"));
}

#[gpui::test]
fn commits_tab_opens_the_commit_react_asked_for(cx: &mut TestAppContext) {
  let mut route = commits_route();
  if let View::Workspace(r) = &mut route.view {
    r.scroll_to_commit = Some("b".into());
  }
  let (mut h, host) = workspace_harness(cx, commits_host(), route);
  assert!(h.drawn("commit-file-row"));
  assert!(host.calls().contains(&"read diff b".to_string()));
  assert_eq!(fields(&h, "commits.scrolled").len(), 1);
}

#[gpui::test]
fn commits_tab_drag_over_code_copies_the_text(cx: &mut TestAppContext) {
  use gpui::{Modifiers, MouseButton};
  let (mut h, _) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.cx.run_until_parked();
  let from = h.cx.debug_bounds("commit-code-src/a.rs-0-1").expect("code");
  let to = h.cx.debug_bounds("commit-code-src/a.rs-0-2").expect("code");
  let start = gpui::point(from.left() + gpui::px(1.), from.center().y);
  let end = gpui::point(to.right() - gpui::px(1.), to.center().y);
  h.cx
    .simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
  h.cx
    .simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
  h.cx
    .simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
  h.cx.simulate_keystrokes(if cfg!(target_os = "macos") {
    "cmd-c"
  } else {
    "ctrl-c"
  });
  h.cx.run_until_parked();
  assert_eq!(
    h.cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
    Some("b\nc")
  );
}

#[gpui::test]
fn commits_tab_sends_a_line_comment_to_an_agent(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("add-comment-0-1");
  assert!(h.drawn("comment-input"));
  h.cx.simulate_input("why?");
  h.click("comment-edit");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&r#"create session Some(7) "Code comment""#.to_string()),
    "{:?}",
    host.writes()
  );
  let created = host.started_sessions();
  assert_eq!(
    created[0]["pendingPrompt"],
    "src/a.rs:2 (commit a)\n```\n+b\n```\n> why?\n"
  );
  assert_eq!(created[0]["permissionMode"], "acceptEdits");
  assert!(!h.drawn("comment-input"));
}

#[gpui::test]
fn commits_tab_edits_a_description(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("edit-description-a");
  assert!(h.drawn("edit-description-dialog"));
  assert!(host.calls().contains(&"read description a".to_string()));
  h.cx.simulate_input("!");
  h.click("description-save");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .iter()
      .any(|w| w.starts_with("describe a ") && w.contains('!')),
    "{:?}",
    host.writes()
  );
  assert!(!h.drawn("edit-description-dialog"));
}

#[gpui::test]
fn commits_tab_shifts_the_stack_from_the_timestamp_dialog(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("edit-timestamp-a");
  assert!(h.drawn("edit-timestamp-dialog"));
  // Nothing to apply until a duration is entered.
  h.click("timestamp-apply");
  h.cx.run_until_parked();
  assert!(!host.writes().iter().any(|w| w.starts_with("shift a")));
  h.click("shift-stack-to-now");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"shift to now 7".to_string()));
  assert!(!h.drawn("edit-timestamp-dialog"));
}

#[gpui::test]
fn commits_tab_resolves_conflicts_with_an_agent(cx: &mut TestAppContext) {
  let mut host = commits_host();
  if let Some(log) = host.history.as_mut() {
    log.commits[0].has_conflicts = true;
  }
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  assert!(h.drawn("resolve-conflicts-banner"));
  h.click("resolve-conflicts-button");
  assert!(h.drawn("resolve-conflicts-dialog"));
  assert!(host
    .calls()
    .contains(&r#"start resolve Some(7) Some(["a"])"#.to_string()));
  type_into(&mut h, "resolve-conflicts-prompt", "keep both sides");
  h.click("resolve-conflicts-submit");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&r#"create session Some(7) "Resolve a""#.to_string()),
    "{:?}",
    host.writes()
  );
  let created = host.started_sessions();
  assert_eq!(created[0]["workspacePath"], "/r/cwd");
  assert_eq!(created[0]["pendingPrompt"], "resolve: keep both sides");
  assert!(!h.drawn("resolve-conflicts-dialog"));
}

#[gpui::test]
fn commits_tab_resolve_dialog_shows_why_it_cannot_resolve(cx: &mut TestAppContext) {
  let mut host = commits_host();
  if let Some(log) = host.history.as_mut() {
    log.commits[0].has_conflicts = true;
  }
  host.failing = vec!["start_resolve"];
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  h.click("resolve-conflicts-button");
  h.cx.run_until_parked();
  assert!(h.drawn("resolve-conflicts-error"));
  type_into(&mut h, "resolve-conflicts-prompt", "keep both sides");
  h.click("resolve-conflicts-submit");
  h.cx.run_until_parked();
  assert!(host.started_sessions().is_empty());
  assert!(h.drawn("resolve-conflicts-dialog"));
}

#[gpui::test]
fn commits_tab_discards_the_tentative_working_copy(cx: &mut TestAppContext) {
  let mut host = commits_host();
  if let Some(log) = host.history.as_mut() {
    log.tentative_working_copy = serde_json::from_value(serde_json::json!({
      "commit": commit("wc", 1, 0),
      "workspace_label": "login"
    }))
    .ok();
  }
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  h.click("commit-row-wc");
  h.click("delete-tentative-changes");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(
    writes.iter().any(|w| w.starts_with("snapshot ")),
    "{writes:?}"
  );
  assert!(
    writes.iter().any(|w| w.starts_with("discard ")),
    "{writes:?}"
  );
  assert!(!h.drawn("commit-row-wc"));
}

#[gpui::test]
fn commits_tab_hands_moves_to_react(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("move-commit-a");
  h.click("move-commit-existing");
  h.cx.run_until_parked();
  assert_eq!(fields(&h, "commits.moveToExisting")[0]["change_id"], "a");
  assert!(!h.drawn("move-commit-menu"));
}

fn remote_chrome(state: &str, cut_off: bool) -> serde_json::Value {
  serde_json::json!({
    "displayName": "app",
    "terminal": true,
    "state": state,
    "cutOff": cut_off,
    "notices": ["Merging a remote workspace is not available yet."],
  })
}

#[gpui::test]
fn a_remote_repository_hides_stash_all(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  assert!(h.drawn("stash-all-changes"));
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.remote_chrome = serde_json::from_value(remote_chrome("online", false)).ok();
    }
  });
  h.cx.run_until_parked();
  assert!(!h.drawn("stash-all-changes"));
  assert!(h.drawn("discard-all-changes"));
}

#[gpui::test]
fn a_remote_repository_draws_dashboards_strip(cx: &mut TestAppContext) {
  let mut route = route_json(Some(7), "main");
  route["remoteChrome"] = remote_chrome("offline", false);
  let snapshot = serde_json::from_value(route).expect("remote route");
  let (mut h, _) = workspace_harness(cx, code_host(), snapshot);
  for selector in [
    "remote-strip",
    "remote-terminal-link",
    "remote-refresh-link",
    "remote-status-banner",
    "remote-banner-reconnect",
    "remote-banner-refresh",
    "remote-capability-notice",
    "workspace-header",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(!h.drawn("remote-banner-wake"));
  assert!(!h.drawn("remote-cutoff-overlay"));
  h.click("remote-terminal-link");
  h.click("remote-banner-reconnect");
  h.click("remote-refresh-link");
  assert_eq!(
    h.sent(),
    vec![
      press("remote.terminal"),
      press("remote.reconnect"),
      press("remote.refresh")
    ]
  );
}

#[gpui::test]
fn a_cut_off_remote_repository_dims_the_window(cx: &mut TestAppContext) {
  let mut route = route_json(Some(7), "main");
  route["remoteChrome"] = remote_chrome("cutoff", true);
  let snapshot = serde_json::from_value(route).expect("remote route");
  let (mut h, _) = workspace_harness(cx, code_host(), snapshot);
  assert!(h.drawn("remote-cutoff-overlay"));
  assert!(h.drawn("remote-status-banner"));
  assert!(
    !h.drawn("remote-banner-refresh"),
    "no Refresh while cut off"
  );
  // The strip's links take no clicks.
  h.click("remote-terminal-link");
  h.click("remote-refresh-link");
  assert!(h.sent().is_empty(), "{:?}", h.sent());
}

#[gpui::test]
fn commits_tab_description_dialog_saves_on_secondary_enter_and_closes_on_escape(
  cx: &mut TestAppContext,
) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("edit-description-a");
  h.cx.simulate_input("!");
  h.cx.simulate_keystrokes("secondary-enter");
  h.cx.run_until_parked();
  assert!(host.writes().iter().any(|w| w.starts_with("describe a ")));
  assert!(!h.drawn("edit-description-dialog"));

  h.click("edit-description-a");
  assert!(h.drawn("edit-description-dialog"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("edit-description-dialog"));
}

#[gpui::test]
fn commits_tab_timestamp_dialog_closes_on_escape(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, commits_host(), commits_route());
  h.click("commit-row-a");
  h.click("edit-timestamp-a");
  assert!(h.drawn("edit-timestamp-dialog"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("edit-timestamp-dialog"));
}

#[gpui::test]
fn commits_tab_resolve_dialog_submits_on_secondary_enter_and_closes_on_escape(
  cx: &mut TestAppContext,
) {
  let mut host = commits_host();
  if let Some(log) = host.history.as_mut() {
    log.commits[0].has_conflicts = true;
  }
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  h.click("resolve-conflicts-button");
  type_into(&mut h, "resolve-conflicts-prompt", "keep both sides");
  h.cx.simulate_keystrokes("secondary-enter");
  h.cx.run_until_parked();
  assert_eq!(host.started_sessions().len(), 1);

  h.click("resolve-conflicts-button");
  assert!(h.drawn("resolve-conflicts-dialog"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("resolve-conflicts-dialog"));
}

fn screen(h: &mut Harness) -> gpui_testing_library::Screen {
  h.cx.run_until_parked();
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  screen
}

#[gpui::test]
fn commits_tab_says_when_the_log_fails_and_retries(cx: &mut TestAppContext) {
  use gpui::Role;
  use gpui_testing_library::prelude::*;
  let host = FakeHost {
    history: None,
    ..commits_host()
  };
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  let page = screen(&mut h);
  expect(page.get_by_role(by::role(Role::Status).name("Couldn't load commits"))).to_be_visible();
  assert!(page
    .query_by_role(by::role(Role::ProgressIndicator))
    .is_none());
  let reads = || {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("read log"))
      .count()
  };
  let before = reads();
  page
    .user()
    .click(&page.get_by_role(by::role(Role::Button).name("Retry")));
  h.cx.run_until_parked();
  assert!(reads() > before, "Retry reads the log again");
}

#[gpui::test]
fn a_diff_that_fails_to_load_offers_retry(cx: &mut TestAppContext) {
  use gpui::Role;
  use gpui_testing_library::prelude::*;
  let host = FakeHost {
    diff: None,
    ..commits_host()
  };
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  h.click("commit-row-a");
  let page = screen(&mut h);
  expect(page.get_by_role(by::role(Role::Status).name("Couldn't load the diff"))).to_be_visible();
  let reads = || host.calls().iter().filter(|c| *c == "read diff a").count();
  let before = reads();
  page
    .user()
    .click(&page.get_by_role(by::role(Role::Button).name("Retry")));
  h.cx.run_until_parked();
  assert_eq!(reads(), before + 1);
}

#[gpui::test]
fn commits_tab_finds_text_in_messages_and_open_diffs(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, commits_host(), commits_route());
  let count = |h: &mut Harness| {
    h.cx.run_until_parked();
    h.screen.read_with(&h.cx, |screen, cx| {
      let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
      let find = &workspace.commits().expect("commits").read(cx).find;
      (find.current, find.total)
    })
  };
  h.cx.simulate_keystrokes("cmd-f");
  assert!(h.drawn("commits-search"));
  // Both messages read "work".
  h.cx.simulate_input("work");
  assert_eq!(count(&mut h), (0, 2));
  h.click("commits-search-next");
  assert_eq!(count(&mut h), (1, 2));
  h.click("commits-search-next");
  assert_eq!(count(&mut h), (0, 2));
  // a's diff (" a", "+b", "+c") is searched once open.
  // Cmd+F again focuses the input and selects its text.
  h.cx.simulate_keystrokes("cmd-f");
  h.cx.simulate_input("b");
  assert_eq!(count(&mut h), (0, 0));
  h.click("commit-row-a");
  assert_eq!(count(&mut h), (0, 1));
  h.click("commits-search-close");
  assert!(!h.drawn("commits-search"));
}
