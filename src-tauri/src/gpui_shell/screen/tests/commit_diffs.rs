//! The Commits tab's diff size limits and its target section
//! (`workspace/commits.test.tsx`). The backend decides which file diffs
//! are deferred (over 500 lines) and which commits are blocked (over
//! 10,000); `it/core_commits_test.rs` covers that.

use gpui::TestAppContext;

use super::*;

fn host_with_diff(diff: serde_json::Value) -> FakeHost {
  let mut host = commits_host();
  host.diff = serde_json::from_value(diff).ok();
  host
}

#[gpui::test]
fn commits_tab_loads_a_deferred_file_diff_on_request(cx: &mut TestAppContext) {
  let mut host = host_with_diff(serde_json::json!({
    "committed_files": [{
      "path": "huge-diff.txt", "status": "A", "previous_path": null,
      "changed_line_count": 501, "diff_deferred": true
    }],
    "hunks_by_file": [],
    "too_large_to_render": false
  }));
  host.file_diffs = serde_json::from_value(serde_json::json!([{
    "path": "huge-diff.txt",
    "hunks": [{"id": "h", "header": "@@ -0,0 +1,1 @@", "lines": ["+large diff line 501"], "patch": ""}]
  }]))
  .expect("file diff");
  let (mut h, host) = workspace_harness(cx, host, commits_route());
  h.click("commit-row-a");
  assert!(h.drawn("commit-file-row"));
  assert!(h.drawn("load-deferred-diff"));
  h.click("load-deferred-diff");
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"read file diff a huge-diff.txt".to_string()));
  assert!(!h.drawn("load-deferred-diff"), "the diff shows instead");
}

#[gpui::test]
fn commits_tab_blocks_a_commit_too_large_to_render(cx: &mut TestAppContext) {
  let host = host_with_diff(serde_json::json!({
    "committed_files": [{
      "path": "huge-commit.txt", "status": "A", "previous_path": null,
      "changed_line_count": 10001, "diff_deferred": true
    }],
    "hunks_by_file": [],
    "too_large_to_render": true,
    "render_block_reason": "This commit changes more than 10,000 lines and is too large to render."
  }));
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  h.click("commit-row-a");
  assert!(h.drawn("commit-diff-blocked"));
  assert!(!h.drawn("commit-file-row"));
  assert!(!h.drawn("load-deferred-diff"));
}

fn on_target(id: &str) -> serde_json::Value {
  let mut c = commit(id, 1, 0);
  c["on_target_only"] = true.into();
  c
}

fn host_with_log(commits: Vec<serde_json::Value>, workspace_branch: &str) -> FakeHost {
  let mut host = commits_host();
  host.history = serde_json::from_value(serde_json::json!({
    "commits": commits,
    "target_branch": "main",
    "workspace_branch": workspace_branch
  }))
  .ok();
  host
}

#[gpui::test]
fn commits_tab_divides_the_workspace_from_its_target(cx: &mut TestAppContext) {
  let host = host_with_log(vec![commit("a", 1, 0), on_target("t")], "feature/login");
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  assert!(h.drawn("commit-row-a"));
  assert!(h.drawn("target-divider"));
  assert!(h.drawn("commit-row-t"));
  assert!(!h.drawn("own-commits-empty"));
}

#[gpui::test]
fn commits_tab_says_a_workspace_has_no_commits_above_its_target(cx: &mut TestAppContext) {
  let host = host_with_log(vec![on_target("t")], "feature/login");
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  assert!(h.drawn("own-commits-empty"));
  assert!(h.drawn("target-divider"));
  assert!(h.drawn("commit-row-t"));
}

#[gpui::test]
fn commits_tab_draws_no_target_section_on_the_default_branch(cx: &mut TestAppContext) {
  let host = host_with_log(vec![commit("a", 1, 0), on_target("t")], "main");
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  assert!(h.drawn("commit-row-a"));
  assert!(!h.drawn("target-divider"));
  assert!(!h.drawn("own-commits-empty"));
}

/// Trunk commits sit under one "On main (read-only)" divider, not an
/// "Immutable" badge on every row.
#[gpui::test]
fn commits_tab_labels_trunk_commits_read_only_once(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let mut target = on_target("t");
  target["is_immutable"] = true.into();
  let host = host_with_log(vec![commit("a", 1, 0), target], "feature/login");
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  assert!(h.drawn("commit-row-t"));
  h.cx.run_until_parked();
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  expect(screen.get_by_text("On main (read-only)")).to_be_visible();
  assert!(screen.query_by_text("Recent on main").is_none());
  assert!(screen.query_by_text("Immutable").is_none());
}

#[gpui::test]
fn commit_rows_open_a_context_menu(cx: &mut TestAppContext) {
  use crate::gpui_shell::popup::{MenuEntry, PopupRequest};
  use gpui::{Modifiers, MouseButton};
  let (mut h, host) = workspace_harness(cx, commits_host(), commits_route());
  h.cx.run_until_parked();
  let at = h.cx.debug_bounds("commit-row-a").expect("row").center();
  h.cx
    .simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
  h.cx
    .simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
  h.cx.run_until_parked();
  let texts: Vec<String> = match h.popups.borrow().last() {
    Some(PopupRequest::ShowMenu { entries, .. }) => entries
      .iter()
      .filter_map(|e| match e {
        MenuEntry::Item { text, .. } => Some(text.clone()),
        _ => None,
      })
      .collect(),
    _ => panic!("no menu is shown"),
  };
  assert_eq!(
    texts,
    [
      "Copy commit ID",
      "Copy change ID",
      "Copy message",
      "Edit description",
      "Undo commit",
      "Revert commit",
      "Edit timestamp",
      "Stash commit",
      "Delete commit",
    ]
  );
  assert!(!h.drawn("commit-file-row"), "a right-click doesn't expand");
  h.pick("Copy commit ID");
  assert_eq!(
    h.cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
    Some("a")
  );
  assert!(host
    .calls()
    .contains(&"notify toast Commit ID copied".to_string()));
  h.pick("Revert commit");
  assert!(
    !host.writes().contains(&"revert a".to_string()),
    "asks first"
  );
  h.answer("Revert commit");
  assert!(host.writes().contains(&"revert a".to_string()));
}

/// The labels of the menu a right-click on `row` opens.
fn row_menu(h: &mut Harness, row: &'static str) -> Vec<String> {
  use crate::gpui_shell::popup::{MenuEntry, PopupRequest};
  use gpui::{Modifiers, MouseButton};
  h.cx.run_until_parked();
  let at = h.cx.debug_bounds(row).expect("row").center();
  h.cx
    .simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
  h.cx
    .simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
  h.cx.run_until_parked();
  match h.popups.borrow().last() {
    Some(PopupRequest::ShowMenu { entries, .. }) => entries
      .iter()
      .filter_map(|e| match e {
        MenuEntry::Item { text, .. } => Some(text.clone()),
        _ => None,
      })
      .collect(),
    _ => panic!("no menu is shown"),
  }
}

/// In the home repo, a pushed (immutable) commit offers nothing that
/// rewrites it.
#[gpui::test]
fn home_offers_no_rewriting_actions_on_pushed_commits(cx: &mut TestAppContext) {
  let mut pushed = commit("a", 1, 0);
  pushed["is_immutable"] = true.into();
  let host = host_with_log(vec![pushed], "main");
  let mut route = route_json(None, "main");
  route["code"] = true.into();
  route["tab"] = "commits".into();
  let (mut h, _) = workspace_harness(cx, host, serde_json::from_value(route).expect("route"));
  let texts = row_menu(&mut h, "commit-row-a");
  for rewrite in [
    "Edit description",
    "Undo commit",
    "Edit timestamp",
    "Stash commit",
    "Delete commit",
  ] {
    assert!(
      !texts.iter().any(|t| t == rewrite),
      "{rewrite} offered: {texts:?}"
    );
  }
  assert!(texts.iter().any(|t| t == "Revert commit"), "{texts:?}");
}

const REWRITES: [&str; 5] = [
  "Edit description",
  "Undo commit",
  "Edit timestamp",
  "Stash commit",
  "Delete commit",
];

/// In a workspace, a commit its target branch already holds is not editable.
#[gpui::test]
fn workspace_commits_on_the_target_are_not_editable(cx: &mut TestAppContext) {
  let mut merged = commit("a", 1, 0);
  merged["is_immutable"] = true.into();
  let host = host_with_log(vec![merged], "feature/login");
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  let texts = row_menu(&mut h, "commit-row-a");
  for rewrite in REWRITES {
    assert!(
      !texts.iter().any(|t| t == rewrite),
      "{rewrite} offered: {texts:?}"
    );
  }
}

/// In a workspace, a commit only on its own branch stays editable, pushed or
/// not: the log marks immutable only what the target holds.
#[gpui::test]
fn workspace_branch_commits_stay_editable(cx: &mut TestAppContext) {
  let host = host_with_log(vec![commit("a", 1, 0)], "feature/login");
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  let texts = row_menu(&mut h, "commit-row-a");
  for edit in [
    "Edit description",
    "Edit timestamp",
    "Stash commit",
    "Delete commit",
  ] {
    assert!(texts.iter().any(|t| t == edit), "{edit} missing: {texts:?}");
  }
}

/// A workspace on the default branch blocks every rewrite of its immutable
/// commits.
#[gpui::test]
fn default_branch_workspace_blocks_immutable_commits(cx: &mut TestAppContext) {
  let mut pushed = commit("a", 1, 0);
  pushed["is_immutable"] = true.into();
  let host = host_with_log(vec![pushed], "main");
  let (mut h, _) = workspace_harness(cx, host, commits_route());
  let texts = row_menu(&mut h, "commit-row-a");
  for rewrite in REWRITES {
    assert!(
      !texts.iter().any(|t| t == rewrite),
      "{rewrite} offered: {texts:?}"
    );
  }
}
