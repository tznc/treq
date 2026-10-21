//! The workspace tab row's badges (`workspace/tab-indicators.test.tsx`):
//! the LOC marker and the Changes tab's file count.

use gpui::TestAppContext;

use super::*;

fn inside(h: &mut Harness, inner: &'static str, outer: &'static str) -> bool {
  h.cx.run_until_parked();
  match (h.cx.debug_bounds(inner), h.cx.debug_bounds(outer)) {
    (Some(inner), Some(outer)) => outer.contains(&inner.center()),
    _ => false,
  }
}

#[gpui::test]
fn the_tab_row_shows_the_loc_marker_and_change_count(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  assert!(h.drawn("workspace-tab-row"));
  assert!(inside(&mut h, "loc-diff-marker", "workspace-tab-row"));
  assert!(inside(
    &mut h,
    "review-change-count",
    "workspace-tab-changes"
  ));
}

#[gpui::test]
fn a_workspace_without_changes_shows_neither_badge(cx: &mut TestAppContext) {
  let mut loaded = feature_loaded();
  loaded.review_count = 0;
  loaded.uncommitted_count = 0;
  loaded.commits = serde_json::from_value(serde_json::json!({
    "commits": [],
    "target_branch": "main",
    "workspace_branch": "feature/login"
  }))
  .ok();
  let host = FakeHost {
    loaded,
    pr: open_pr(),
    ..FakeHost::default()
  };
  let (mut h, _) = workspace_harness(cx, host, workspace_route());
  assert!(h.drawn("workspace-tab-changes"));
  assert!(!h.drawn("loc-diff-marker"));
  assert!(!h.drawn("review-change-count"));
}

/// `sumWorkspaceLocFromLog`: the workspace's commits and its tentative
/// working copy, never the target's.
#[test]
fn the_loc_marker_sums_own_commits_and_the_working_copy() {
  let mut target = commit("t", 100, 100);
  target["on_target_only"] = true.into();
  let mut loaded = feature_loaded();
  loaded.commits = serde_json::from_value(serde_json::json!({
    "commits": [commit("a", 5, 1), target],
    "target_branch": "main",
    "workspace_branch": "feature/login",
    "tentative_working_copy": {"commit": commit("wc", 3, 0), "workspace_label": "login"}
  }))
  .ok();
  let (_, tab_row) = workspace_data::workspace_view(
    route_of(&workspace_route()),
    &loaded,
    &open_pr(),
    workspace_data::Pending::None,
  );
  let loc = tab_row.and_then(|row| row.loc).expect("loc");
  assert_eq!((loc.insertions, loc.deletions), (8, 1));
}
