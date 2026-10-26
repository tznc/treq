use gpui::TestAppContext;

use super::*;

const WORKSPACE_JSON: &str = r#"{
  "header": {
    "branch": "feature/login",
    "home": false,
    "linear": {"key": "ENG-12", "url": "https://linear.app/t/ENG-12", "title": "Login"},
    "target": {"branch": "main", "loading": false, "disabled": false},
    "homeTarget": null,
    "stack": null,
    "push": null,
    "viewPr": {"number": 42, "state": "OPEN", "draft": false},
    "createPr": null,
    "statusLoading": false,
    "sync": {"ahead": 2, "behind": 0, "disabled": false, "spinning": false},
    "checks": {"status": "passed", "count": "7/7", "name": "Checks: Treq 3/3, GitHub 4/4"},
    "merge": {"passing": true, "disabled": false, "tooltip": null},
    "details": true,
    "more": {"disabled": false}
  },
  "tabRow": {
    "tab": "overview",
    "commits": {"kind": "count", "count": 2, "tone": "muted"},
    "changes": {"count": 5, "tone": "uncommitted"},
    "logs": false,
    "busy": null,
    "loc": {"insertions": 12, "deletions": 4}
  }
}"#;

#[test]
fn workspace_header_is_what_react_drew() {
  let expected: workspace::Workspace = serde_json::from_str(WORKSPACE_JSON).expect("parse");
  let (header, tab_row) = workspace_data::workspace_view(
    route_of(&workspace_route()),
    &feature_loaded(),
    &open_pr(),
    workspace_data::Pending::None,
  );
  assert_eq!(header, expected.header);
  assert_eq!(tab_row, expected.tab_row);
}

#[test]
fn workspace_header_offers_the_home_rebase() {
  let route: ScreenSnapshot = serde_json::from_value(route_json(None, "feat/home")).expect("route");
  let loaded = workspace_data::Loaded {
    home_ahead: 2,
    ..Default::default()
  };
  let (header, _) = workspace_data::workspace_view(
    route_of(&route),
    &loaded,
    &workspace_data::PrCache::default(),
    workspace_data::Pending::None,
  );
  assert!(header.home);
  assert_eq!(header.branch, "feat/home");
  let target = header.home_target.expect("home target");
  assert_eq!(
    target.rebase.expect("rebase").tooltip,
    "Rebase feat/home onto main (2 new commits)"
  );
  assert_eq!(
    header.stack.expect("stack").tooltip,
    "Create stacked workspace from feat/home"
  );
}

#[gpui::test]
fn workspace_header_sends_what_it_does_not_do_itself(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  for selector in [
    "workspace-view-pr",
    "workspace-view-pr-menu",
    "workspace-merge",
    "workspace-more",
  ] {
    h.click(selector);
  }
  h.click("workspace-linear");
  h.cx.run_until_parked();
  // View PR and More run in GPUI (`workspace/actions.rs`).
  assert_eq!(h.sent(), vec![press("workspace.merge")]);
  assert_eq!(
    host.writes(),
    vec![
      "notify open",
      "open https://github.com/tznc/treq/pull/42",
      "open https://linear.app/t/ENG-12",
    ]
  );
}

#[gpui::test]
fn workspace_header_github_buttons_copy_their_link(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  let pr = "https://github.com/tznc/treq/pull/42";
  for (selector, url) in [
    ("workspace-view-pr", pr.to_string()),
    ("workspace-view-pr-menu", pr.to_string()),
    ("workspace-checks", format!("{pr}/checks")),
  ] {
    h.right_click(selector);
    h.pick("Copy link");
    let copied = h.cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some(url.as_str()), "{selector}");
  }
  assert!(h.sent().is_empty());
  assert_eq!(host.writes(), vec!["notify toast Link copied"; 3]);
}

#[gpui::test]
fn workspace_header_syncs_itself(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  h.click("workspace-sync");
  h.cx.run_until_parked();
  assert!(h.sent().is_empty());
  assert_eq!(
    host.writes(),
    vec![
      "sync Some(7)",
      "notify toast Sync failed",
      "notify workspaceChanged",
    ]
  );
  // The window reads again on `workspaceChanged` and bumps the route's
  // revision, which reads the header again
  // (`workspace_header_reads_again_when_the_model_refreshes`): the header
  // doesn't read it twice.
  let reads = host
    .calls()
    .iter()
    .filter(|c| c.starts_with("read workspace "))
    .count();
  assert_eq!(reads, 1);
}

#[gpui::test]
fn workspace_header_reads_again_when_the_model_refreshes(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  assert!(h.drawn("workspace-sync"));
  h.update(|s| s.dark = true);
  h.update(|s| match &mut s.view {
    View::Workspace(route) => route.revision = 3,
    _ => unreachable!(),
  });
  h.cx.run_until_parked();
  let reads: Vec<_> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("read workspace "))
    .collect();
  assert_eq!(reads, vec!["read workspace 0", "read workspace 3"]);
  assert!(host.calls().contains(&"read pr feature/login".to_string()));
}

#[gpui::test]
fn workspace_tabs_switch_through_react(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  assert!(h.drawn("commits-tab-count"));
  assert!(h.drawn("review-change-count"));
  assert!(h.drawn("loc-diff-marker"));
  assert!(!h.drawn("workspace-tab-logs"));
  h.click("workspace-tab-changes");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![field("workspace.tab", "changes".into())]);
}

#[gpui::test]
fn home_workspace_opens_the_branch_switcher(cx: &mut TestAppContext) {
  let route = serde_json::from_value(route_json(None, "main")).expect("route");
  let (mut h, _) = workspace_harness(cx, FakeHost::default(), route);
  h.click("workspace-branch");
  h.click("workspace-stack");
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    vec![press("workspace.branchSwitcher"), press("workspace.stack")]
  );
}
