//! The Commits, Changes and file browser tabs stay alive while another tab
//! or workspace shows: shown again, they draw what they last read in the
//! first frame, with no loading placeholder, and read again behind it. A
//! workspace's first visit keeps the last one drawn until it has read.

use gpui::TestAppContext;

use super::*;

/// Draws one frame without running any task, as the first frame after a
/// click is drawn before any read lands.
pub(super) fn first_frame(h: &mut Harness) {
  h.cx.update(|window, cx| {
    window.refresh();
    window.draw(cx).clear(cx);
  });
}

pub(super) fn drawn_now(h: &mut Harness, selector: &'static str) -> bool {
  h.cx.debug_bounds(selector).is_some()
}

fn set_tab(h: &mut Harness, tab: &str) {
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.tab = serde_json::from_value(tab.into()).expect("tab");
    }
  });
}

/// Shows workspace `id` of the window's list, as the window model routes it.
pub(super) fn set_workspace(h: &mut Harness, id: i64) {
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.workspace_id = Some(id);
      route.workspace = Some(workspace(id));
    }
  });
}

pub(super) fn workspace(id: i64) -> crate::local_db::Workspace {
  let mut row = workspace_row(&format!("feature/{id}"));
  row["id"] = id.into();
  serde_json::from_value(row).expect("workspace")
}

pub(super) fn reads(host: &FakeHost, prefix: &str) -> usize {
  host
    .calls()
    .iter()
    .filter(|c| c.starts_with(prefix))
    .count()
}

#[gpui::test]
fn commits_tab_draws_at_once_when_shown_again(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  assert!(h.drawn("commit-row-a"));
  set_tab(&mut h, "changes");
  assert!(h.drawn("changes-diff-viewer"));
  let before = reads(&host, "read log");
  set_tab(&mut h, "commits");
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "commits-loading"));
  assert!(drawn_now(&mut h, "commit-row-a"));
  // And it read again behind what it drew.
  h.cx.run_until_parked();
  assert_eq!(reads(&host, "read log"), before + 1);
}

#[gpui::test]
fn changes_tab_draws_at_once_when_shown_again(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  assert!(h.drawn("file-card-a.rs"));
  set_tab(&mut h, "commits");
  assert!(h.drawn("workspace-commits"));
  let before = reads(&host, "read workspace diff");
  set_tab(&mut h, "changes");
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "changes-loading-skeleton"));
  assert!(drawn_now(&mut h, "file-card-a.rs"));
  h.cx.run_until_parked();
  assert_eq!(reads(&host, "read workspace diff"), before + 1);
}

/// A hidden Changes tab stops polling its review threads.
#[gpui::test]
fn hidden_changes_tab_stops_polling(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  assert!(h.drawn("file-card-a.rs"));
  set_tab(&mut h, "commits");
  assert!(h.drawn("workspace-commits"));
  let before = reads(&host, "read agent comments");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_secs(120));
  h.cx.run_until_parked();
  assert_eq!(reads(&host, "read agent comments"), before);
}

#[gpui::test]
fn commits_tab_keeps_the_last_workspace_until_the_next_one_reads(cx: &mut TestAppContext) {
  let mut host = commits_host();
  host.workspaces = vec![workspace(7), workspace(8)];
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  assert!(h.drawn("commit-row-a"));
  // A workspace not shown before: the last one stays drawn, dimmed.
  set_workspace(&mut h, 8);
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "commits-loading"));
  assert!(drawn_now(&mut h, "workspace-tab-stand-in"));
  assert!(drawn_now(&mut h, "commit-row-a"));
  h.cx.run_until_parked();
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "workspace-tab-stand-in"));
  assert!(drawn_now(&mut h, "commit-row-a"));
  // Back on the first: drawn at once from what it read.
  set_workspace(&mut h, 7);
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "commits-loading"));
  assert!(!drawn_now(&mut h, "workspace-tab-stand-in"));
  assert!(drawn_now(&mut h, "commit-row-a"));
}

#[gpui::test]
fn changes_tab_draws_a_workspace_shown_again_at_once(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.workspaces = vec![workspace(7), workspace(8)];
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  assert!(h.drawn("file-card-a.rs"));
  set_workspace(&mut h, 8);
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "changes-loading-skeleton"));
  h.cx.run_until_parked();
  set_workspace(&mut h, 7);
  first_frame(&mut h);
  assert!(!drawn_now(&mut h, "changes-loading-skeleton"));
  assert!(!drawn_now(&mut h, "workspace-tab-stand-in"));
  assert!(drawn_now(&mut h, "file-card-a.rs"));
}

/// A tab stands in for another workspace only while it reads: one it
/// stands in for can't be clicked.
#[gpui::test]
fn a_stand_in_tab_takes_no_clicks(cx: &mut TestAppContext) {
  let mut host = commits_host();
  host.workspaces = vec![workspace(7), workspace(8)];
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  assert!(h.drawn("commit-row-a"));
  set_workspace(&mut h, 8);
  first_frame(&mut h);
  let row = h.cx.debug_bounds("commit-row-a").expect("row");
  h.cx.simulate_click(row.center(), gpui::Modifiers::none());
  first_frame(&mut h);
  assert!(
    !drawn_now(&mut h, "commit-file-row"),
    "the commit stays closed"
  );
}

/// Cmd+F belongs to the tab on screen: a hidden Changes tab doesn't open
/// its find bar.
#[gpui::test]
fn hidden_changes_tab_ignores_cmd_f(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  assert!(h.drawn("file-card-a.rs"));
  set_tab(&mut h, "commits");
  assert!(h.drawn("workspace-commits"));
  h.cx.simulate_keystrokes("cmd-f");
  set_tab(&mut h, "changes");
  assert!(h.drawn("file-card-a.rs"));
  assert!(!h.drawn("diff-search"));
}
