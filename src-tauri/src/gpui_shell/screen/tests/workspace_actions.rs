//! The workspace header's actions GPUI runs itself (`workspace/actions.rs`):
//! checks, pull requests, retargeting, Reset to remote and Delete
//! Workspace.

use gpui::TestAppContext;

use super::*;

/// A feature workspace with no pull request yet, so Create PR shows.
fn no_pr_host() -> FakeHost {
  FakeHost {
    loaded: feature_loaded(),
    pr: workspace_data::PrCache {
      pr: Some(None),
      ci: None,
    },
    workspaces: vec![serde_json::from_value(workspace_row("feature/login")).expect("workspace")],
    ..FakeHost::default()
  }
}

fn act(h: &mut Harness, action: ScreenAction) {
  h.screen.update(&mut h.cx, |screen, cx| {
    let view = screen.workspace.clone().expect("workspace view");
    view.update(cx, |view, cx| view.handle(action, cx));
  });
  h.cx.run_until_parked();
}

fn calls_starting(host: &FakeHost, prefixes: &[&str]) -> Vec<String> {
  host
    .calls()
    .into_iter()
    .filter(|c| prefixes.iter().any(|p| c.starts_with(p)))
    .collect()
}

#[gpui::test]
fn runs_every_workflow_and_the_setup_script(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  h.cx.run_until_parked();
  act(&mut h, press("workspace.checksRunAll"));
  act(
    &mut h,
    ScreenAction::Field {
      field: "workspace.checksRun".into(),
      value: "b.yml".into(),
    },
  );
  act(&mut h, press("workspace.checksRerunSetup"));
  act(&mut h, press("workspace.checksTrust"));
  assert!(h.sent().is_empty(), "React runs none of them");
  let path = "/repo/.treq/workspaces/login";
  let mut calls = calls_starting(&host, &["run ", "trust "]);
  // Run All starts the three at once.
  calls[..3].sort();
  assert_eq!(
    calls,
    [
      format!("run 7:a.yml in {path}"),
      format!("run 7:b.yml in {path}"),
      format!("run 7:c.yml in {path}"),
      format!("run 7:b.yml in {path}"),
      format!("run 7:<setup> in {path}"),
      "trust /repo".to_string(),
    ]
  );
  assert!(host
    .calls()
    .contains(&"notify workspaceChanged".to_string()));
}

#[gpui::test]
fn draws_checks_running_anywhere_in_the_app(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  h.cx.update(|_, cx| {
    let mut running = crate::gpui_shell::checks_run::Running::default();
    for key in ["7:a.yml", "7:b.yml:test", "7:<setup>", "8:c.yml"] {
      running.0.start("/repo", key);
    }
    cx.set_global(running);
  });
  h.cx.run_until_parked();
  let route = h.screen.read_with(&h.cx, |screen, cx| {
    let view = screen.workspace.as_ref().expect("workspace view").read(cx);
    view.drawn_route(cx)
  });
  assert_eq!(route.running_checks, ["a.yml", "b.yml"]);
  assert_eq!(route.running_workflows, ["a.yml"]);
  assert!(route.setup_rerunning);
}

#[gpui::test]
fn creates_a_pull_request_pushing_first(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, no_pr_host(), workspace_route());
  h.click("workspace-create-pr");
  h.cx.run_until_parked();
  assert!(h.sent().is_empty());
  assert_eq!(
    calls_starting(&host, &["push ", "create pr", "refresh pr", "notify toast"]),
    [
      "push 7",
      "create pr tznc/treq \"feat: Add login\" \"\" feature/login->main draft=false",
      "refresh pr status feature/login",
      "notify toast Pull request created",
    ]
  );
}

#[gpui::test]
fn creates_a_draft_or_manual_pull_request_from_the_menu(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, no_pr_host(), workspace_route());
  h.click("workspace-create-pr-menu");
  h.pick("Create draft PR");
  assert!(host
    .calls()
    .iter()
    .any(|c| c.starts_with("create pr") && c.ends_with("draft=true")));
  assert!(host
    .calls()
    .contains(&"notify toast Draft PR created".to_string()));

  h.click("workspace-create-pr-menu");
  h.pick("Create PR manually");
  let open = calls_starting(&host, &["open https://github.com/tznc/treq/compare"]);
  assert_eq!(
    open,
    ["open https://github.com/tznc/treq/compare/main...feature%2Flogin?expand=1&title=feat%3A+Add+login&body="]
  );
}

#[gpui::test]
fn a_failed_pull_request_toasts(cx: &mut TestAppContext) {
  let mut host = no_pr_host();
  host.github.fail = vec!["create_pr"];
  let (mut h, host) = workspace_harness(cx, host, workspace_route());
  act(&mut h, press("workspace.createPr"));
  assert!(host
    .calls()
    .contains(&"notify toast Failed to create PR".to_string()));
}

#[gpui::test]
fn views_the_pull_request_in_treq(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  h.click("workspace-view-pr");
  h.cx.run_until_parked();
  assert!(h.sent().is_empty());
  assert!(host.calls().contains(&"notify open".to_string()));
}

#[gpui::test]
fn deletes_the_workspace_from_the_more_menu(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  h.click("workspace-more");
  h.pick("Delete Workspace");
  assert!(calls_starting(&host, &["delete workspace"]).is_empty());
  h.answer("Delete workspace");
  assert_eq!(
    calls_starting(&host, &["delete workspace", "notify"]),
    [
      "notify workspacesRemoving",
      "delete workspace 7",
      "notify workspaceChanged",
      "notify workspacesRemoved",
      "notify toast Workspace deleted",
    ]
  );
}

#[gpui::test]
fn restores_the_row_when_delete_fails(cx: &mut TestAppContext) {
  let host = FakeHost {
    failing: vec!["delete_workspace"],
    ..feature_host()
  };
  let (mut h, host) = workspace_harness(cx, host, workspace_route());
  h.click("workspace-more");
  h.pick("Delete Workspace");
  h.answer("Delete workspace");
  assert_eq!(
    calls_starting(&host, &["delete workspace", "notify"]),
    [
      "notify workspacesRemoving",
      "delete workspace 7",
      "notify workspacesRestored",
      "notify toast Delete Failed",
    ]
  );
}

#[gpui::test]
fn keeps_the_workspace_when_delete_is_declined(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  h.click("workspace-more");
  h.pick("Delete Workspace");
  h.answer("Cancel");
  assert!(!h.drawn("confirm"));
  assert!(!host
    .calls()
    .iter()
    .any(|c| c.starts_with("delete workspace")));
}

/// The labels of the menu shown last.
fn menu_labels(h: &mut Harness) -> Vec<String> {
  use crate::gpui_shell::popup::{MenuEntry, PopupRequest};
  h.cx.run_until_parked();
  match h.popups.borrow().last() {
    Some(PopupRequest::ShowDropdown { entries, .. }) => entries
      .iter()
      .filter_map(|e| match e {
        MenuEntry::Item { text, .. } => Some(text.clone()),
        _ => None,
      })
      .collect(),
    _ => panic!("no dropdown is shown"),
  }
}

#[gpui::test]
fn resets_to_the_remote_from_the_more_menu(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  h.click("workspace-more");
  assert_eq!(menu_labels(&mut h), ["Reset to remote", "Delete Workspace"]);
  h.pick("Reset to remote");
  assert!(calls_starting(&host, &["reset"]).is_empty(), "asks first");
  h.answer("Reset to remote");
  assert_eq!(
    calls_starting(&host, &["reset", "notify toast"]),
    ["reset 7", "notify toast Reset to remote"]
  );
}

#[gpui::test]
fn keeps_local_work_when_reset_is_declined(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  h.click("workspace-more");
  h.pick("Reset to remote");
  h.answer("Cancel");
  assert!(!h.drawn("confirm"));
  assert!(calls_starting(&host, &["reset"]).is_empty());
}

#[gpui::test]
fn offers_no_reset_without_an_upstream(cx: &mut TestAppContext) {
  let mut host = feature_host();
  if let Some(status) = host.loaded.status.as_mut() {
    status.remote_sync = crate::core::RemoteSyncStatus::NotOnRemote;
  }
  let (mut h, _) = workspace_harness(cx, host, workspace_route());
  h.click("workspace-more");
  assert_eq!(menu_labels(&mut h), ["Delete Workspace"]);
}

#[gpui::test]
fn retargets_onto_another_branch(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, feature_host(), workspace_route());
  h.cx.run_until_parked();
  let target = |branch: &str| ScreenAction::Field {
    field: "workspace.target".into(),
    value: branch.into(),
  };
  // The branch it already targets does nothing.
  act(&mut h, target("main"));
  act(&mut h, target("develop"));
  assert!(h.sent().is_empty());
  assert_eq!(
    calls_starting(&host, &["retarget", "notify toast"]),
    [
      "retarget feature/login -> develop",
      "notify toast Rebased onto develop",
    ]
  );
}

#[gpui::test]
fn hands_a_remote_repository_actions_to_react(cx: &mut TestAppContext) {
  let host = FakeHost {
    react_actions: true,
    ..no_pr_host()
  };
  let (mut h, host) = workspace_harness(cx, host, workspace_route());
  h.cx.run_until_parked();
  for id in ["workspace.checksRunAll", "workspace.createPr"] {
    act(&mut h, press(id));
  }
  assert_eq!(
    h.sent(),
    [press("workspace.checksRunAll"), press("workspace.createPr"),]
  );
  assert!(calls_starting(&host, &["run ", "create pr"]).is_empty());
}

#[gpui::test]
fn a_merged_pull_request_hides_create_pr(cx: &mut TestAppContext) {
  let mut host = feature_host();
  if let Some(Some(pr)) = host.pr.pr.as_mut() {
    pr.state = "MERGED".into();
  }
  let (mut h, _) = workspace_harness(cx, host, workspace_route());
  assert!(h.drawn("workspace-view-pr"));
  assert!(!h.drawn("workspace-create-pr"));
}

#[gpui::test]
fn a_closed_pull_request_can_be_replaced(cx: &mut TestAppContext) {
  let mut host = feature_host();
  if let Some(Some(pr)) = host.pr.pr.as_mut() {
    pr.state = "CLOSED".into();
  }
  let (mut h, _) = workspace_harness(cx, host, workspace_route());
  assert!(h.drawn("workspace-view-pr"));
  assert!(h.drawn("workspace-create-pr"));
}
