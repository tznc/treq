use std::{cell::RefCell, rc::Rc, sync::Mutex};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::*;

mod fuzz;
mod perf;

#[derive(Default)]
struct FakeHost {
  checks: PaletteChecks,
  pr: Option<PrLink>,
  workspaces: Vec<WorkspaceRow>,
  branches: Vec<JjBranch>,
  switch_error: Option<String>,
  /// Checks fail with this error.
  run_error: Option<String>,
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

  fn checks_target(
    &self,
    repo_path: &str,
    workspace_id: i64,
  ) -> Option<crate::core::running_checks::ChecksTarget> {
    Some(crate::core::running_checks::ChecksTarget {
      repo_path: repo_path.into(),
      workspace_id,
      workspace_path: format!("{repo_path}/.treq/workspaces/ws{workspace_id}"),
    })
  }

  fn run_checks(
    &self,
    target: &crate::core::running_checks::ChecksTarget,
    run: &crate::core::running_checks::CheckRun,
  ) -> Result<Vec<crate::core::checks::JobResult>, String> {
    self.record(format!(
      "run {} in {}",
      run.key(target.workspace_id),
      target.workspace_path
    ));
    if let Some(err) = &self.run_error {
      return Err(err.clone());
    }
    Ok(vec![crate::core::checks::JobResult {
      job_id: "test".into(),
      steps: vec![],
      success: true,
    }])
  }

  fn trust_repo(&self, repo_path: &str) -> Result<(), String> {
    self.record(format!("trust {repo_path}"));
    Ok(())
  }

  fn notify(&self, event: crate::gpui_shell::native::NativeEvent) {
    let json = serde_json::to_value(&event).expect("serialize");
    let mut line = format!("notify {}", json["kind"].as_str().unwrap_or_default());
    if let Some(title) = json["title"].as_str() {
      line += &format!(" {title}");
    }
    self.record(line);
  }

  fn terminal(&self, command: crate::core::palette_commands::TerminalCommand) {
    self.record(format!("terminal {}", command.kind()));
  }
}

fn checks_host(run_error: Option<&str>) -> FakeHost {
  FakeHost {
    checks: PaletteChecks {
      workflows: vec![
        ("ci.yml".into(), "CI".into()),
        ("lint.yml".into(), "Lint".into()),
      ],
      trusted: true,
      setup_configured: true,
      setup_running: false,
    },
    run_error: run_error.map(Into::into),
    ..FakeHost::default()
  }
}

/// Picks the palette command labelled `label`.
fn pick(h: &mut Harness, label: &str) {
  h.type_text(label);
  assert_eq!(h.selected().as_deref(), Some(label));
  h.keys("enter");
}

fn effects(h: &Harness) -> Vec<String> {
  h.host
    .calls()
    .into_iter()
    .filter(|c| !c.starts_with("checks ") && !c.starts_with("pr "))
    .collect()
}

#[gpui::test]
fn palette_runs_one_check_and_reports_it(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, palette_json(true), checks_host(None));
  pick(&mut h, "Run check: Lint");
  assert_eq!(h.sent(), vec![press("overlay.close")]);
  assert_eq!(
    effects(&h),
    [
      "run 4:lint.yml in /repo/.treq/workspaces/ws4",
      "notify workspaceChanged",
      "notify toast Lint passed",
    ]
  );
}

#[gpui::test]
fn palette_runs_all_checks_with_one_report(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, palette_json(true), checks_host(Some("no runner")));
  pick(&mut h, "Run checks…");
  let effects = effects(&h);
  let toasts: Vec<&String> = effects.iter().filter(|c| c.contains("toast")).collect();
  assert_eq!(toasts, ["notify toast Checks could not run"]);
  assert_eq!(effects.iter().filter(|c| c.starts_with("run ")).count(), 2);
}

#[gpui::test]
fn palette_reruns_setup_and_trusts_without_a_toast(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, palette_json(true), checks_host(None));
  pick(&mut h, "Re-run setup script");
  assert_eq!(
    effects(&h),
    [
      "run 4:<setup> in /repo/.treq/workspaces/ws4",
      "notify workspaceChanged",
    ]
  );
}

#[gpui::test]
fn palette_gives_the_terminal_pane_its_commands(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, palette_json(true), FakeHost::default());
  pick(&mut h, "Maximize Terminal");
  assert_eq!(effects(&h), ["terminal toggleMaximize"]);
  assert_eq!(h.sent(), vec![press("overlay.close")]);
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

// command-palette.test.tsx, in Testing Library's terms: the palette's
// options are found by role and name, and clicked as a user would.
mod testing_library {
  use gpui::{Role, TestAppContext};
  use gpui_testing_library::{prelude::*, ByRole};

  use super::*;

  fn screen(h: &mut Harness) -> gpui_testing_library::Screen {
    h.cx.run_until_parked();
    let window = h.cx.update(|window, _| window.window_handle());
    let screen = gpui_testing_library::Screen::new(&h.cx, window);
    screen.redraw();
    screen
  }

  fn option(name: &str) -> ByRole {
    by::role(Role::ListBoxOption).name(name)
  }

  #[gpui::test]
  fn shows_search_files_in_the_palette_dialog(cx: &mut TestAppContext) {
    let mut h = Harness::new(cx, palette_json(false), FakeHost::default());
    let screen = screen(&mut h);
    expect(screen.get_by_role(by::role(Role::Dialog))).to_be_visible();
    let search = screen.get_by_role(option("Search Files"));
    expect(&search).to_be_visible();
    // gpui-testing-library 0.1 does not read gpui-kit's descriptions yet;
    // the e2e suite checks the description text.
  }

  #[gpui::test]
  fn always_shows_go_to_home_and_go_to_settings(cx: &mut TestAppContext) {
    for workspace in [false, true] {
      let mut h = Harness::new(cx, palette_json(workspace), FakeHost::default());
      let screen = screen(&mut h);
      expect(screen.get_by_role(option("Go to Home"))).to_be_visible();
      expect(screen.get_by_role(option("Go to Settings"))).to_be_visible();
      // The first command is selected, as cmdk selects it.
      expect(screen.get_by_role(option("Go to Home"))).to_be_selected();
    }
  }

  #[gpui::test]
  fn clicking_search_files_asks_for_the_file_picker(cx: &mut TestAppContext) {
    let mut h = Harness::new(cx, palette_json(false), FakeHost::default());
    let screen = screen(&mut h);
    screen
      .user()
      .click(&screen.get_by_role(option("Search Files")));
    h.cx.run_until_parked();
    assert_eq!(h.sent(), vec![press("palette.search-files")]);
  }

  #[gpui::test]
  fn typing_filters_the_options(cx: &mut TestAppContext) {
    let mut h = Harness::new(cx, palette_json(false), FakeHost::default());
    let screen = screen(&mut h);
    screen.user().keyboard("branch");
    screen.redraw();
    expect(screen.get_by_role(option("Switch Branch"))).to_be_selected();
    assert!(screen.query_by_role(option("Go to Home")).is_none());
  }
}
