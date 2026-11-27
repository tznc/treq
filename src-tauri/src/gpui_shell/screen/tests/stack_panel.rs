//! The Code tab's stack panel (`workspace/stack-panel.test.tsx`): only for
//! a workspace in a stack, with its chain and none of its siblings.

use gpui::TestAppContext;

use super::*;

/// The window's workspaces, `(id, branch, target)`; 7 is the one on screen.
fn stack_host(rows: &[(i64, &str, &str)]) -> FakeHost {
  let workspaces = rows
    .iter()
    .map(|(id, branch, target)| {
      let mut row = workspace_row(branch);
      row["id"] = (*id).into();
      row["target_branch"] = (*target).into();
      row["workspace_path"] = format!("/repo/.treq/workspaces/{id}").into();
      row["title"] = (*branch).into();
      serde_json::from_value(row).expect("workspace")
    })
    .collect();
  FakeHost {
    workspaces,
    ..code_host()
  }
}

fn item(id: i64) -> &'static str {
  Box::leak(format!("workspace-stack-item-{id}").into_boxed_str())
}

/// Whether the stack card lists the workspace titled `branch`.
fn lists(screen: &gpui_testing_library::Screen, branch: &str) -> bool {
  screen
    .query_by_role(gpui_testing_library::by::role(gpui::Role::ListItem).name(branch))
    .is_some()
}

fn panel_shown(screen: &gpui_testing_library::Screen) -> bool {
  screen
    .query_by_role(gpui_testing_library::by::role(gpui::Role::Button).name("Stack new workspace"))
    .is_some()
}

#[gpui::test]
fn the_home_view_draws_no_stack_panel(cx: &mut TestAppContext) {
  let mut host = stack_host(&[(7, "feat/alpha", "main"), (8, "feat/beta", "feat/alpha")]);
  host.loaded.workspace = None;
  let mut route = route_json(None, "main");
  route["code"] = true.into();
  let (mut h, _) = workspace_harness(cx, host, serde_json::from_value(route).expect("route"));
  let screen = h.rendered();
  gpui_testing_library::expect(
    screen.get_by_role(gpui_testing_library::by::role(gpui::Role::Button).name("Start an agent")),
  )
  .to_be_visible();
  assert!(!panel_shown(&screen));
}

#[gpui::test]
fn a_lone_workspace_draws_a_one_row_stack_panel(cx: &mut TestAppContext) {
  let host = stack_host(&[(7, "feat/alpha", "main"), (8, "feat/other", "main")]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  let screen = h.rendered();
  assert!(panel_shown(&screen));
  // The workspace on screen is its one, selected row.
  let rows = screen.query_all_by_role(gpui_testing_library::by::role(gpui::Role::ListItem));
  assert_eq!(rows.len(), 1);
  assert!(screen
    .query_by_role(gpui_testing_library::by::role(gpui::Role::ListItem).selected(true))
    .is_some());
  assert!(!lists(&screen, "feat/other"), "not in its stack");
}

/// The branch, its target picker and Stack on this… sit at the top of the
/// stack card, not in the header.
#[gpui::test]
fn the_stack_panel_holds_the_target_picker_and_stack_button(cx: &mut TestAppContext) {
  let host = stack_host(&[(6, "feat/alpha", "main"), (7, "feat/beta", "feat/alpha")]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.cx.run_until_parked();
  let panel = h.cx.debug_bounds("workspace-stack-panel").expect("panel");
  let header = h.cx.debug_bounds("workspace-header").expect("header");
  for id in ["workspace-target", "workspace-stack"] {
    let b = h
      .cx
      .debug_bounds(id)
      .unwrap_or_else(|| panic!("{id} drawn"));
    assert!(panel.contains(&b.center()), "{id} in the stack card");
    assert!(!header.intersects(&b), "{id} not in the header");
  }
}

#[gpui::test]
fn the_stack_panel_target_picker_opens_the_target_popover(cx: &mut TestAppContext) {
  let host = stack_host(&[(6, "feat/alpha", "main"), (7, "feat/beta", "feat/alpha")]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.click("workspace-target");
  h.cx.run_until_parked();
  let screen = h.rendered();
  assert!(
    screen
      .query_by_role(gpui_testing_library::by::role(gpui::Role::Dialog))
      .is_some(),
    "the target popover opens over the window"
  );
}

#[gpui::test]
fn the_stack_panel_stack_button_stacks_on_the_workspace(cx: &mut TestAppContext) {
  let host = stack_host(&[(7, "feat/alpha", "main")]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.click("workspace-stack");
  h.cx.run_until_parked();
  assert!(
    h.sent().contains(&press("workspace.stack")),
    "{:?}",
    h.sent()
  );
}

#[gpui::test]
fn a_stacked_workspace_lists_its_chain_and_opens_a_row(cx: &mut TestAppContext) {
  let host = stack_host(&[(6, "feat/alpha", "main"), (7, "feat/beta", "feat/alpha")]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  let screen = h.rendered();
  assert!(panel_shown(&screen));
  assert!(lists(&screen, "feat/alpha") && lists(&screen, "feat/beta"));
  h.click(item(6));
  let opened = fields(&h, "code.stack");
  assert_eq!(opened.len(), 1);
  assert_eq!(opened[0]["branch_name"], "feat/alpha");
}

#[gpui::test]
fn the_first_workspace_of_a_stack_lists_the_ones_above(cx: &mut TestAppContext) {
  let host = stack_host(&[(7, "feat/alpha", "main"), (8, "feat/beta", "feat/alpha")]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  let screen = h.rendered();
  assert!(panel_shown(&screen));
  assert!(lists(&screen, "feat/alpha") && lists(&screen, "feat/beta"));
}

#[gpui::test]
fn the_stack_panel_leaves_out_sibling_workspaces(cx: &mut TestAppContext) {
  let host = stack_host(&[
    (6, "feat/alpha", "main"),
    (7, "feat/beta", "feat/alpha"),
    (8, "feat/gamma", "feat/alpha"),
  ]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  let screen = h.rendered();
  assert!(lists(&screen, "feat/alpha") && lists(&screen, "feat/beta"));
  assert!(!lists(&screen, "feat/gamma"), "a sibling");
}

fn pr(number: u64, branch: &str, state: &str) -> Option<crate::github::PrInfo> {
  Some(crate::github::PrInfo {
    number,
    title: branch.into(),
    state: state.into(),
    url: format!("https://github.com/o/r/pull/{number}"),
    head_ref_name: branch.into(),
    base_ref_name: "main".into(),
    merge_state_status: None,
    is_draft: false,
    body: String::new(),
  })
}

fn github_stack(number: u64) -> crate::github_stack::GhStack {
  crate::github_stack::GhStack {
    number,
    base_ref_name: "main".into(),
    entries: vec![],
  }
}

/// A two-level stack, `feat/alpha` (#40, on screen) under `feat/beta` (#41).
fn prs_host() -> FakeHost {
  let mut host = stack_host(&[(7, "feat/alpha", "main"), (8, "feat/beta", "feat/alpha")]);
  host.branch_prs = [
    ("feat/alpha".to_string(), pr(40, "feat/alpha", "OPEN")),
    ("feat/beta".to_string(), pr(41, "feat/beta", "MERGED")),
  ]
  .into();
  host
}

#[gpui::test]
fn the_stack_button_counts_levels_from_the_bottom(cx: &mut TestAppContext) {
  let host = stack_host(&[
    (7, "feat/alpha", "main"),
    (8, "feat/beta", "feat/alpha"),
    (9, "feat/gamma", "feat/beta"),
    (10, "feat/delta", "feat/gamma"),
  ]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.shows(
    gpui::Role::Button,
    "Stack level 1 of 4, not stacked on GitHub"
  ));
}

#[gpui::test]
fn the_stack_button_names_the_github_stack(cx: &mut TestAppContext) {
  let mut host = prs_host();
  host.gh_stack = Some(github_stack(12));
  let (mut h, host) = workspace_harness(cx, host, code_route());
  assert!(h.shows(gpui::Role::Button, "Stack level 1 of 2, GitHub stack #12"));
  // Read through the pull request of the workspace on screen.
  assert!(host.calls().contains(&"read github stack 40".to_string()));
}

#[gpui::test]
fn the_stack_menu_opens_the_stack_on_github(cx: &mut TestAppContext) {
  let mut host = prs_host();
  host.gh_stack = Some(github_stack(12));
  let (mut h, host) = workspace_harness(cx, host, code_route());
  h.click("workspace-stack-level");
  h.cx.run_until_parked();
  click_named(&mut h, gpui::Role::MenuItem, "Open on GitHub");
  h.cx.run_until_parked();
  assert!(
    host
      .calls()
      .contains(&"open https://github.com/o/r/pull/40".to_string()),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn stack_rows_show_their_pull_requests(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, prs_host(), code_route());
  let screen = h.rendered();
  for name in ["Pull request #40, open", "Pull request #41, merged"] {
    assert!(
      screen
        .query_by_role(gpui_testing_library::by::role(gpui::Role::Status).name(name))
        .is_some(),
      "{name}"
    );
  }
}
