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

#[gpui::test]
fn the_home_view_draws_no_stack_panel(cx: &mut TestAppContext) {
  let mut host = stack_host(&[(7, "feat/alpha", "main"), (8, "feat/beta", "feat/alpha")]);
  host.loaded.workspace = None;
  let mut route = route_json(None, "main");
  route["code"] = true.into();
  let (mut h, _) = workspace_harness(cx, host, serde_json::from_value(route).expect("route"));
  assert!(h.drawn("code-action-row"), "the home Code tab");
  assert!(!h.drawn("workspace-stack-panel"));
}

#[gpui::test]
fn a_lone_workspace_draws_no_stack_panel(cx: &mut TestAppContext) {
  let host = stack_host(&[(7, "feat/alpha", "main"), (8, "feat/other", "main")]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn("workspace-code"));
  assert!(!h.drawn("workspace-stack-panel"));
}

#[gpui::test]
fn a_stacked_workspace_lists_its_chain_and_opens_a_row(cx: &mut TestAppContext) {
  let host = stack_host(&[(6, "feat/alpha", "main"), (7, "feat/beta", "feat/alpha")]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn("workspace-stack-panel"));
  assert!(h.drawn(item(6)) && h.drawn(item(7)));
  h.click(item(6));
  let opened = fields(&h, "code.stack");
  assert_eq!(opened.len(), 1);
  assert_eq!(opened[0]["branch_name"], "feat/alpha");
}

#[gpui::test]
fn the_first_workspace_of_a_stack_lists_the_ones_above(cx: &mut TestAppContext) {
  let host = stack_host(&[(7, "feat/alpha", "main"), (8, "feat/beta", "feat/alpha")]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn("workspace-stack-panel"));
  assert!(h.drawn(item(7)) && h.drawn(item(8)));
}

#[gpui::test]
fn the_stack_panel_leaves_out_sibling_workspaces(cx: &mut TestAppContext) {
  let host = stack_host(&[
    (6, "feat/alpha", "main"),
    (7, "feat/beta", "feat/alpha"),
    (8, "feat/gamma", "feat/alpha"),
  ]);
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn(item(6)) && h.drawn(item(7)));
  assert!(!h.drawn(item(8)), "a sibling");
}
