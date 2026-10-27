//! Reading ahead: what the pointer rests on, or what a workspace switch
//! would read, is read before the click, which then draws it in the first
//! frame. Loading placeholders wait before they show, and reads that find
//! what shows already redraw nothing.

use std::{cell::Cell, rc::Rc, time::Duration};

use gpui::{Entity, Modifiers, TestAppContext};

use super::{
  kept_tabs::{drawn_now, first_frame, reads, set_workspace, workspace},
  *,
};
use crate::gpui_shell::screen::{code::CodeView, prefetch::HOVER_INTENT};

/// Rests the pointer on `selector` for `rest`.
fn hover(h: &mut Harness, selector: &'static str, rest: Duration) {
  h.cx.run_until_parked();
  let at = h
    .cx
    .debug_bounds(selector)
    .unwrap_or_else(|| panic!("{selector} is not drawn"))
    .center();
  h.cx.simulate_mouse_move(at, None, Modifiers::none());
  h.cx.executor().advance_clock(rest);
  h.cx.run_until_parked();
}

fn workspace_view(h: &mut Harness) -> Entity<workspace::WorkspaceView> {
  h.cx.run_until_parked();
  h.screen
    .read_with(&h.cx, |s, _| s.workspace.clone())
    .expect("workspace view")
}

/// The route the window model would show workspace `id` on: the current
/// one's, on that workspace.
fn route_to(h: &mut Harness, id: i64) -> workspace_data::Route {
  let mut route = h
    .screen
    .read_with(&h.cx, |s, _| route_of(s.snapshot()).clone());
  route.workspace_id = Some(id);
  route.workspace = Some(workspace(id));
  route
}

fn prefetch(h: &mut Harness, id: i64) {
  let route = route_to(h, id);
  h.screen
    .update(&mut h.cx, |s, cx| s.prefetch_workspace(route, cx));
  h.cx.run_until_parked();
}

fn two_workspaces(mut host: FakeHost) -> FakeHost {
  host.workspaces = vec![workspace(7), workspace(8)];
  host
}

/// Opens commit `id` as a click would, without running anything after it.
fn open_commit(h: &mut Harness, id: &str) {
  let commits = workspace_view(h).read_with(&h.cx, |w, _| w.commits().cloned().expect("commits"));
  commits.update(&mut h.cx, |view, cx| view.toggle(id, cx));
}

#[gpui::test]
fn resting_on_a_commit_draws_its_diff_at_once(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  hover(&mut h, "commit-row-a", HOVER_INTENT);
  assert_eq!(reads(&host, "read diff a"), 1);
  open_commit(&mut h, "a");
  first_frame(&mut h);
  assert!(drawn_now(&mut h, "commit-file-row"));
  h.cx.run_until_parked();
  assert_eq!(reads(&host, "read diff a"), 1, "opened from what was read");
}

#[gpui::test]
fn passing_over_a_commit_reads_nothing(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  hover(&mut h, "commit-row-a", HOVER_INTENT / 2);
  hover(&mut h, "commit-row-b", HOVER_INTENT / 2);
  h.cx.simulate_mouse_move(
    gpui::point(gpui::px(1.), gpui::px(1.)),
    None,
    Modifiers::none(),
  );
  h.cx.executor().advance_clock(HOVER_INTENT);
  h.cx.run_until_parked();
  assert_eq!(reads(&host, "read diff"), 0);
  // Without it, a commit opens on its loading state.
  open_commit(&mut h, "a");
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "commit-file-row"));
}

#[gpui::test]
fn resting_on_a_file_opens_it_at_once(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.file_texts = std::collections::HashMap::from([(
    "/repo/.treq/workspaces/login/a.rs".to_string(),
    "a\nb\nc".to_string(),
  )]);
  host.dirs = std::collections::HashMap::from([(
    "/repo/.treq/workspaces/login".to_string(),
    vec![crate::core::WorkspaceEntry {
      name: "a.rs".into(),
      path: "/repo/.treq/workspaces/login/a.rs".into(),
      is_directory: false,
      modified_at: None,
      submodule_pin: None,
      submodule_synced: None,
      status: None,
    }],
  )]);
  let mut route = route_json(Some(7), "main");
  route["fileBrowser"] = serde_json::json!({});
  let (mut h, host) = workspace_harness(cx, host, serde_json::from_value(route).expect("route"));
  hover(&mut h, "file-tree-a.rs", HOVER_INTENT);
  assert_eq!(
    reads(&host, "read file /repo/.treq/workspaces/login/a.rs"),
    1
  );
  let browser = workspace_view(&mut h).read_with(&h.cx, |w, _| w.file_browser().cloned());
  browser.expect("file browser").update(&mut h.cx, |b, cx| {
    b.select_file("/repo/.treq/workspaces/login/a.rs", cx)
  });
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "file-browser-loading"));
  assert!(drawn_now(&mut h, "code-line-3"));
  h.cx.run_until_parked();
  assert_eq!(
    reads(&host, "read file /repo/.treq/workspaces/login/a.rs"),
    1
  );
}

/// A file read that is still on the way draws nothing for a moment, its
/// spinner taking its room, then shows the spinner.
#[gpui::test]
fn a_file_opening_shows_its_spinner_only_after_a_moment(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.file_texts = std::collections::HashMap::from([(
    "/repo/.treq/workspaces/login/a.rs".to_string(),
    "a".to_string(),
  )]);
  let mut route = route_json(Some(7), "main");
  route["fileBrowser"] = serde_json::json!({});
  let (mut h, _) = workspace_harness(cx, host, serde_json::from_value(route).expect("route"));
  let browser = workspace_view(&mut h).read_with(&h.cx, |w, _| w.file_browser().cloned());
  browser.expect("file browser").update(&mut h.cx, |b, cx| {
    b.select_file("/repo/.treq/workspaces/login/a.rs", cx)
  });
  first_frame(&mut h);
  assert!(drawn_now(&mut h, "file-browser-loading"));
  assert!(!drawn_now(&mut h, "file-browser-loading-shown"));
  // The spinner's delay runs on the real clock, as animations do.
  std::thread::sleep(crate::gpui_shell::ui::SPINNER_DELAY + Duration::from_millis(50));
  first_frame(&mut h);
  assert!(drawn_now(&mut h, "file-browser-loading-shown"));
}

#[gpui::test]
fn a_workspace_read_ahead_draws_its_header_and_tab_at_once(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, two_workspaces(commits_host()), commits_route());
  set_workspace(&mut h, 7);
  assert!(h.drawn("commit-row-a"));
  prefetch(&mut h, 8);
  assert_eq!(reads(&host, "read log Some(8)"), 1);
  let header_reads = reads(&host, "read workspace");
  let view = workspace_view(&mut h);
  set_workspace(&mut h, 8);
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "workspace-tab-stand-in"));
  assert!(!drawn_now(&mut h, "commits-loading"));
  assert!(drawn_now(&mut h, "commit-row-a"));
  let branch = view.read_with(&h.cx, |w, _| w.branch().map(str::to_string));
  assert_eq!(branch.as_deref(), Some("feature/8"));
  // Both read again behind what they drew.
  h.cx.run_until_parked();
  assert_eq!(reads(&host, "read workspace"), header_reads + 1);
  assert_eq!(reads(&host, "read log Some(8)"), 2);
}

/// Without reading ahead, a workspace's first visit draws the last one,
/// dimmed, and the last workspace's header until its own reads land.
#[gpui::test]
fn a_workspace_not_read_ahead_stands_the_last_one_in(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, two_workspaces(commits_host()), commits_route());
  set_workspace(&mut h, 7);
  assert!(h.drawn("commit-row-a"));
  let view = workspace_view(&mut h);
  set_workspace(&mut h, 8);
  first_frame(&mut h);
  assert!(drawn_now(&mut h, "workspace-tab-stand-in"));
  let branch = view.read_with(&h.cx, |w, _| w.branch().map(str::to_string));
  assert_ne!(branch.as_deref(), Some("feature/8"));
}

/// Reading ahead changes nothing on screen.
#[gpui::test]
fn reading_a_workspace_ahead_leaves_the_screen_alone(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, two_workspaces(commits_host()), commits_route());
  set_workspace(&mut h, 7);
  assert!(h.drawn("commit-row-a"));
  let view = workspace_view(&mut h);
  let before = view.read_with(&h.cx, |w, _| w.branch().map(str::to_string));
  prefetch(&mut h, 8);
  let after = view.read_with(&h.cx, |w, _| w.branch().map(str::to_string));
  assert_eq!(before, after);
  assert!(!h.drawn("workspace-tab-stand-in"));
  let shown = view.read_with(&h.cx, |w, cx| {
    w.commits().map(|c| c.read(cx).parent.workspace_id())
  });
  assert_eq!(shown, Some(Some(7)));
}

#[gpui::test]
fn the_code_tab_draws_a_workspace_read_ahead_at_once(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, two_workspaces(code_host()), code_route());
  set_workspace(&mut h, 7);
  assert!(h.drawn("workspace-code"));
  prefetch(&mut h, 8);
  assert_eq!(reads(&host, "read overview Some(8)"), 1);
  set_workspace(&mut h, 8);
  first_frame(&mut h);
  let code = code_view(&mut h);
  let shown = code.read_with(&h.cx, |c, _| c.parent.workspace_id());
  assert_eq!(shown, Some(8));
}

fn code_view(h: &mut Harness) -> Entity<CodeView> {
  h.screen
    .read_with(&h.cx, |s, cx| s.workspace.as_ref()?.read(cx).code.clone())
    .expect("code view")
}

/// Counts `entity`'s redraw requests.
fn notifies<T: 'static>(h: &mut Harness, entity: &Entity<T>) -> Rc<Cell<usize>> {
  let count = Rc::new(Cell::new(0));
  let counted = count.clone();
  h.cx.update(|_, cx| {
    cx.observe(entity, move |_, _| counted.set(counted.get() + 1))
      .detach()
  });
  count
}

#[gpui::test]
fn the_code_tab_read_again_unchanged_redraws_nothing(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  assert!(h.drawn("workspace-code"));
  let code = code_view(&mut h);
  let redraws = notifies(&mut h, &code);
  let before = reads(&host, "read overview");
  code.update(&mut h.cx, |c, cx| c.revalidate(cx));
  h.cx.run_until_parked();
  assert_eq!(reads(&host, "read overview"), before + 1);
  assert_eq!(redraws.get(), 0);
}

#[gpui::test]
fn the_file_index_is_built_once_a_workspace_shows(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, two_workspaces(code_host()), code_route());
  set_workspace(&mut h, 7);
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"read file index Some(7)".to_string()));
  set_workspace(&mut h, 8);
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"read file index Some(8)".to_string()));
}

/// "Jump to File" lists for its empty query at once, with no debounce.
#[gpui::test]
fn the_file_picker_searches_its_empty_query_at_once(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  let scope = file_picker::Scope {
    repo_path: "/repo".into(),
    workspace_id: Some(7),
    workspace_path: "/repo/.treq/workspaces/login".into(),
  };
  h.screen
    .update(&mut h.cx, |s, cx| s.set_file_picker(Some(scope), cx));
  h.cx.run_until_parked();
  assert!(host.calls().contains(&r#"read files """#.to_string()));
}

/// A workspace shown lately draws its own header at once when shown again,
/// not the last workspace's while it reads.
#[gpui::test]
fn a_workspace_shown_again_draws_its_header_at_once(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, two_workspaces(commits_host()), commits_route());
  set_workspace(&mut h, 7);
  let view = workspace_view(&mut h);
  let seven = view.read_with(&h.cx, |w, _| w.branch().map(str::to_string));
  set_workspace(&mut h, 8);
  h.cx.run_until_parked();
  set_workspace(&mut h, 7);
  first_frame(&mut h);
  let branch = view.read_with(&h.cx, |w, _| w.branch().map(str::to_string));
  assert_eq!(branch, seven);
}

/// Once the shown tab has read, the workspace's other tabs are made ahead
/// while the window is in front, so switching tabs draws at once.
#[gpui::test]
fn an_idle_window_makes_the_other_tabs_ahead(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, two_workspaces(changes_host()), commits_route());
  set_workspace(&mut h, 7);
  assert!(h.drawn("workspace-commits"));
  assert_eq!(reads(&host, "read workspace diff"), 0);
  h.cx.update(|window, _| window.activate_window());
  idle(&mut h, Duration::from_secs(2));
  assert_eq!(reads(&host, "read workspace diff"), 1);
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.tab = workspace::Tab::Changes;
    }
  });
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "changes-loading-skeleton"));
  assert!(drawn_now(&mut h, "file-card-a.rs"));
}

#[gpui::test]
fn a_window_in_the_background_reads_nothing_ahead(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, two_workspaces(changes_host()), commits_route());
  set_workspace(&mut h, 7);
  assert!(h.drawn("workspace-commits"));
  // Test windows start in the background.
  idle(&mut h, Duration::from_secs(10));
  assert_eq!(reads(&host, "read workspace diff"), 0);
}

/// Lets the window idle for `time`, a tenth of a second at a time.
fn idle(h: &mut Harness, time: Duration) {
  let step = Duration::from_millis(100);
  for _ in 0..time.as_millis() / step.as_millis() {
    h.cx.executor().advance_clock(step);
    h.cx.run_until_parked();
  }
}
