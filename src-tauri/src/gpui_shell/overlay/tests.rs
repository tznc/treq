use std::{cell::RefCell, rc::Rc, sync::Mutex};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::*;

#[derive(Default)]
struct FakeHost {
  checks: PaletteChecks,
  pr: Option<PrLink>,
  workspaces: Vec<WorkspaceRow>,
  branches: Vec<JjBranch>,
  switch_error: Option<String>,
  calls: Mutex<Vec<String>>,
}

impl FakeHost {
  fn record(&self, call: String) {
    self.calls.lock().unwrap().push(call);
  }

  fn calls(&self) -> Vec<String> {
    self.calls.lock().unwrap().clone()
  }
}

impl OverlayHost for FakeHost {
  fn checks(&self, repo_path: &str, workspace_id: i64) -> PaletteChecks {
    self.record(format!("checks {repo_path} {workspace_id}"));
    self.checks.clone()
  }

  fn pr(&self, repo_path: &str, branch: &str) -> Option<PrLink> {
    self.record(format!("pr {repo_path} {branch}"));
    self.pr.clone()
  }

  fn workspaces(&self, repo_path: &str) -> Result<Vec<WorkspaceRow>, String> {
    self.record(format!("workspaces {repo_path}"));
    Ok(self.workspaces.clone())
  }

  fn branches(&self, repo_path: &str) -> Result<Vec<JjBranch>, String> {
    self.record(format!("branches {repo_path}"));
    Ok(self.branches.clone())
  }

  fn switch_branch(&self, repo_path: &str, name: &str) -> Result<(), String> {
    self.record(format!("switch {repo_path} {name}"));
    self.switch_error.clone().map_or(Ok(()), Err)
  }

  fn open_url(&self, url: &str) {
    self.record(format!("open {url}"));
  }
}

struct Harness {
  cx: VisualTestContext,
  overlay: Entity<Overlay>,
  sent: Rc<RefCell<Vec<ScreenAction>>>,
  host: Arc<FakeHost>,
}

impl Harness {
  fn new(cx: &mut TestAppContext, json: serde_json::Value, host: FakeHost) -> Self {
    let snapshot: OverlaySnapshot = serde_json::from_value(json).expect("parse");
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sink: ActionSink = {
      let sent = sent.clone();
      Rc::new(move |action, _| sent.borrow_mut().push(action))
    };
    cx.update(theme::init);
    let host = Arc::new(host);
    let dyn_host: Arc<dyn OverlayHost> = host.clone();
    let (overlay, cx) = cx.add_window_view(|_, cx| Overlay::new(snapshot, dyn_host, sink, cx));
    let cx = cx.clone();
    Self {
      cx,
      overlay,
      sent,
      host,
    }
  }

  fn drawn(&mut self, selector: &'static str) -> bool {
    self.cx.run_until_parked();
    self.cx.debug_bounds(selector).is_some()
  }

  fn click(&mut self, selector: &'static str) {
    self.cx.run_until_parked();
    let bounds = self
      .cx
      .debug_bounds(selector)
      .unwrap_or_else(|| panic!("{selector} is not drawn"));
    self.cx.simulate_click(bounds.center(), Modifiers::none());
    self.cx.run_until_parked();
  }

  fn keys(&mut self, keys: &str) {
    self.cx.run_until_parked();
    self.cx.simulate_keystrokes(keys);
    self.cx.run_until_parked();
  }

  fn type_text(&mut self, text: &str) {
    self.cx.run_until_parked();
    self.cx.simulate_input(text);
    self.cx.run_until_parked();
  }

  fn sent(&self) -> Vec<ScreenAction> {
    self.sent.borrow().clone()
  }

  /// The labels shown, in order.
  fn shown(&mut self) -> Vec<String> {
    self.cx.run_until_parked();
    self.overlay.read_with(&self.cx, |o, _| {
      let entries = o.entries();
      o.order()
        .into_iter()
        .map(|i| entries[i].value.clone())
        .collect()
    })
  }

  fn selected(&mut self) -> Option<String> {
    self.cx.run_until_parked();
    self.overlay.read_with(&self.cx, |o, _| {
      let entries = o.entries();
      o.selected.map(|i| entries[i].value.clone())
    })
  }
}

fn press(id: &str) -> ScreenAction {
  ScreenAction::Press { id: id.into() }
}

fn field(field: &str, value: serde_json::Value) -> ScreenAction {
  ScreenAction::Field {
    field: field.into(),
    value,
  }
}

fn palette_json(workspace: bool) -> serde_json::Value {
  serde_json::json!({
    "kind": "commandPalette",
    "dark": false,
    "repoPath": "/repo",
    "stash": true,
    "hasSelectedWorkspace": workspace,
    "workspace": workspace.then(|| serde_json::json!({
      "id": 4, "repoPath": "/repo", "branch": "feat/alpha"
    })),
  })
}

#[test]
fn reads_every_overlay_react_sends() {
  for json in [
    palette_json(true),
    serde_json::json!({"kind": "keyboardShortcuts", "dark": true}),
    serde_json::json!({"kind": "workspacePicker", "dark": false, "repoPath": "/repo"}),
    serde_json::json!({"kind": "branchSwitcher", "dark": false, "repoPath": "/repo"}),
  ] {
    let parsed: Result<OverlaySnapshot, _> = serde_json::from_value(json.clone());
    assert!(parsed.is_ok(), "{json}: {parsed:?}");
  }
}

#[test]
fn serializes_overlay_actions_as_react_reads_them() {
  assert_eq!(
    serde_json::to_value(press("overlay.close")).unwrap(),
    serde_json::json!({"kind": "press", "id": "overlay.close"})
  );
}

#[gpui::test]
fn palette_runs_the_first_command_on_enter(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, palette_json(false), FakeHost::default());
  assert!(h.drawn("overlay-dialog"));
  assert_eq!(h.selected().as_deref(), Some("Go to Home"));
  h.keys("enter");
  assert_eq!(h.sent(), vec![press("palette.dashboard")]);
}

#[gpui::test]
fn palette_moves_the_selection_with_the_arrow_keys(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, palette_json(false), FakeHost::default());
  h.keys("down down");
  assert_eq!(h.selected().as_deref(), Some("Open Repository"));
  h.keys("up");
  assert_eq!(h.selected().as_deref(), Some("Go to Settings"));
  h.keys("up up");
  assert_eq!(h.selected().as_deref(), Some("Go to Home"));
  h.keys("end");
  assert_eq!(h.selected().as_deref(), Some("View Stashed Changes"));
  h.keys("ctrl-p");
  assert_eq!(h.selected().as_deref(), Some("View Prompt History"));
  h.keys("enter");
  assert_eq!(h.sent(), vec![press("palette.open-prompt-history")]);
}

#[gpui::test]
fn palette_filters_as_cmdk_does(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, palette_json(false), FakeHost::default());
  h.type_text("branch");
  assert_eq!(h.shown(), vec!["Switch Branch"]);
  assert_eq!(h.selected().as_deref(), Some("Switch Branch"));
  h.type_text("zzz");
  assert!(h.shown().is_empty());
  h.keys("enter");
  assert!(h.sent().is_empty());
}

#[gpui::test]
fn palette_runs_a_clicked_command(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, palette_json(false), FakeHost::default());
  h.click("overlay-item-1");
  assert_eq!(h.sent(), vec![press("palette.settings")]);
}

#[gpui::test]
fn palette_closes_on_escape_and_on_its_backdrop(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, palette_json(false), FakeHost::default());
  // The search field's row, inside the dialog.
  let dialog = h.cx.debug_bounds("overlay-dialog").expect("dialog");
  h.cx.simulate_click(
    dialog.origin + gpui::point(px(8.), px(8.)),
    Modifiers::none(),
  );
  h.cx.run_until_parked();
  assert!(h.sent().is_empty());
  h.keys("escape");
  assert_eq!(h.sent(), vec![press("overlay.close")]);
  let backdrop = h.cx.debug_bounds("overlay-backdrop").expect("backdrop");
  h.cx.simulate_click(
    backdrop.origin + gpui::point(px(4.), px(4.)),
    Modifiers::none(),
  );
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    vec![press("overlay.close"), press("overlay.close")]
  );
}

#[gpui::test]
fn palette_reads_the_workspace_checks_and_pr(cx: &mut TestAppContext) {
  let host = FakeHost {
    checks: PaletteChecks {
      workflows: vec![("ci.yml".into(), "CI".into())],
      trusted: true,
      setup_configured: false,
      setup_running: false,
    },
    pr: Some(PrLink {
      number: 7,
      url: "https://github.com/o/r/pull/7".into(),
    }),
    ..FakeHost::default()
  };
  let mut h = Harness::new(cx, palette_json(true), host);
  assert_eq!(
    h.host.calls(),
    vec!["checks /repo 4", "pr /repo feat/alpha"]
  );
  let shown = h.shown();
  assert!(shown.contains(&"Run check: CI".to_string()));
  assert!(shown.contains(&"Toggle Terminal".to_string()));
  assert_eq!(h.selected().as_deref(), Some("Go to Home"));

  h.type_text("PR in browser");
  assert_eq!(
    h.selected().as_deref(),
    Some("Open Workspace PR in Browser")
  );
  h.keys("enter");
  assert!(h
    .host
    .calls()
    .contains(&"open https://github.com/o/r/pull/7".to_string()));
  assert_eq!(h.sent(), vec![press("overlay.close")]);
}

#[gpui::test]
fn palette_opens_the_pr_in_the_window_through_react(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr: Some(PrLink {
      number: 7,
      url: "https://github.com/o/r/pull/7".into(),
    }),
    ..FakeHost::default()
  };
  let mut h = Harness::new(cx, palette_json(true), host);
  h.keys("end");
  assert_eq!(h.selected().as_deref(), Some("Open Workspace PR"));
  h.keys("enter");
  assert_eq!(
    h.sent(),
    vec![field(
      "palette.navigate",
      "https://github.com/o/r/pull/7".into()
    )]
  );
}

#[gpui::test]
fn palette_selects_the_row_under_the_pointer(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, palette_json(false), FakeHost::default());
  let row = h.cx.debug_bounds("overlay-item-2").expect("row");
  h.cx
    .simulate_mouse_move(row.center(), None, Modifiers::none());
  assert_eq!(h.selected().as_deref(), Some("Open Repository"));
}

fn workspace_host() -> FakeHost {
  FakeHost {
    workspaces: vec![
      WorkspaceRow {
        id: 3,
        branch_name: "feat/alpha".into(),
        agents: 1,
      },
      WorkspaceRow {
        id: 5,
        branch_name: "fix/beta".into(),
        agents: 2,
      },
    ],
    ..FakeHost::default()
  }
}

#[gpui::test]
fn workspace_picker_opens_the_picked_workspace(cx: &mut TestAppContext) {
  let json = serde_json::json!({"kind": "workspacePicker", "dark": true, "repoPath": "/repo"});
  let mut h = Harness::new(cx, json, workspace_host());
  assert_eq!(h.host.calls(), vec!["workspaces /repo"]);
  assert_eq!(h.shown(), vec!["feat/alpha", "fix/beta"]);
  h.type_text("beta");
  assert_eq!(h.shown(), vec!["fix/beta"]);
  h.keys("enter");
  assert_eq!(h.sent(), vec![field("workspacePicker.select", 5.into())]);
}

#[test]
fn describes_a_workspace_as_react_does() {
  let row = |agents| WorkspaceRow {
    id: 1,
    branch_name: "a".into(),
    agents,
  };
  assert_eq!(
    workspace_description(&row(1)),
    "1 agent, 0 shells, 0 changes"
  );
  assert_eq!(
    workspace_description(&row(0)),
    "0 agents, 0 shells, 0 changes"
  );
}

fn branch(name: &str, current: bool) -> JjBranch {
  JjBranch {
    name: name.into(),
    is_current: current,
  }
}

fn branch_host() -> FakeHost {
  FakeHost {
    branches: vec![
      branch("feat/x", false),
      branch("main@origin", false),
      branch("main", true),
    ],
    ..FakeHost::default()
  }
}

fn branch_json() -> serde_json::Value {
  serde_json::json!({"kind": "branchSwitcher", "dark": false, "repoPath": "/repo"})
}

#[gpui::test]
fn branch_switcher_groups_current_local_and_remote(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, branch_json(), branch_host());
  assert_eq!(h.shown(), vec!["main", "feat/x", "main@origin"]);
  assert_eq!(h.selected().as_deref(), Some("main"));
  h.keys("alt-down");
  assert_eq!(h.selected().as_deref(), Some("feat/x"));
}

#[gpui::test]
fn branch_switcher_closes_on_the_current_branch(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, branch_json(), branch_host());
  h.keys("enter");
  assert_eq!(h.sent(), vec![press("overlay.close")]);
  assert!(!h.host.calls().iter().any(|c| c.starts_with("switch")));
}

#[gpui::test]
fn branch_switcher_switches_and_tells_react(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, branch_json(), branch_host());
  h.keys("down enter");
  assert!(h.host.calls().contains(&"switch /repo feat/x".to_string()));
  assert_eq!(
    h.sent(),
    vec![field("branchSwitcher.changed", "feat/x".into())]
  );
}

#[gpui::test]
fn branch_switcher_shows_a_failed_switch(cx: &mut TestAppContext) {
  let host = FakeHost {
    switch_error: Some("conflicted".into()),
    ..branch_host()
  };
  let mut h = Harness::new(cx, branch_json(), host);
  h.keys("down enter");
  assert!(h.sent().is_empty());
  let error = h.overlay.read_with(&h.cx, |o, _| match &o.body {
    Body::Branches { error, .. } => error.clone(),
    _ => None,
  });
  assert_eq!(error.as_deref(), Some("conflicted"));
  // React hides the list while the error shows.
  assert!(h.shown().is_empty());
}

#[gpui::test]
fn keyboard_shortcuts_close_on_escape_and_question_mark(cx: &mut TestAppContext) {
  let json = serde_json::json!({"kind": "keyboardShortcuts", "dark": false});
  let mut h = Harness::new(cx, json, FakeHost::default());
  assert!(h.drawn("keyboard-shortcuts-modal"));
  h.keys("escape");
  h.keys("shift-?");
  assert_eq!(
    h.sent(),
    vec![press("overlay.close"), press("overlay.close")]
  );
}
