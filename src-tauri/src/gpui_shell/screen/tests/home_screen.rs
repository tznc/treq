use gpui::TestAppContext;

use super::*;

const WORKSPACE_JSON: &str = r#"{
  "header": {
    "branch": "feature/login",
    "title": "Add login",
    "home": false,
    "linear": {"key": "ENG-12", "url": "https://linear.app/t/ENG-12", "title": "Login"},
    "target": {"branch": "main", "loading": false, "disabled": false},
    "homeTarget": null,
    "stack": {"tooltip": "Stack a new workspace on feature/login"},
    "push": null,
    "viewPr": {"number": 42, "state": "OPEN", "draft": false},
    "createPr": null,
    "statusLoading": false,
    "sync": {"ahead": 2, "behind": 0, "disabled": false, "spinning": false},
    "checks": {"status": "passed", "count": "7/7 checks passed", "name": "Checks: Treq 3/3, GitHub 4/4"},
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
fn labels_header_create_button_new_at_home_and_stack_in_a_workspace() {
  assert_eq!(super::super::workspace::stack_label(true), "New workspace…");
  assert_eq!(
    super::super::workspace::stack_label(false),
    "Stack on this…"
  );
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
    "Create a workspace from feat/home"
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
  assert!(h.drawn("workspace-tab-commits-count"));
  assert!(h.drawn("workspace-tab-changes-count"));
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

#[gpui::test]
fn a_disabled_merge_button_says_why_first(cx: &mut TestAppContext) {
  use crate::gpui_shell::popup::{PopupRequest, TooltipContent};
  let mut host = feature_host();
  if let Some(status) = host.loaded.status.as_mut() {
    status.conflicted_files = vec!["src/a.rs".into(), "src/b.rs".into()];
  }
  let (mut h, _) = workspace_harness(cx, host, workspace_route());
  h.cx.run_until_parked();
  let at = h
    .cx
    .debug_bounds("workspace-merge")
    .expect("merge")
    .center();
  h.cx.simulate_mouse_move(at, None, gpui::Modifiers::none());
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(300));
  h.cx.run_until_parked();
  let popups = h.popups.borrow();
  let text = popups
    .iter()
    .rev()
    .find_map(|p| match p {
      PopupRequest::ShowTooltip {
        content: TooltipContent::Text(text),
        ..
      } => Some(text.to_string()),
      _ => None,
    })
    .expect("a tooltip on the disabled Merge… button");
  assert!(text.starts_with("Resolve conflicts to merge"), "{text}");
}
