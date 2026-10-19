use std::{
  cell::RefCell,
  rc::Rc,
  sync::{Arc, Mutex},
};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::*;

/// A screen in a test window, and the actions it sent.
struct Harness {
  cx: VisualTestContext,
  screen: Entity<Screen>,
  sent: Rc<RefCell<Vec<ScreenAction>>>,
}

/// The backend, as the screens see it, recording what they asked of it.
#[derive(Default)]
struct FakeHost {
  remote_ssh: bool,
  /// What `check_prerequisites` reports.
  installed: Vec<(&'static str, bool)>,
  calls: Mutex<Vec<String>>,
}

impl FakeHost {
  fn calls(&self) -> Vec<String> {
    self.calls.lock().unwrap().clone()
  }

  fn record(&self, call: String) {
    self.calls.lock().unwrap().push(call);
  }
}

impl onboarding::OnboardingHost for FakeHost {
  fn remote_ssh_enabled(&self) -> bool {
    self.remote_ssh
  }

  fn check_prerequisites(&self) -> Vec<crate::core::PrerequisiteStatus> {
    self.record("check".into());
    self
      .installed
      .iter()
      .map(|(binary, installed)| crate::core::PrerequisiteStatus {
        binary: binary.to_string(),
        installed: *installed,
      })
      .collect()
  }

  fn pick_repo(&self) {
    self.record("pick_repo".into());
  }

  fn open_url(&self, url: &str) {
    self.record(format!("open {url}"));
  }
}

impl Harness {
  fn new(cx: &mut TestAppContext, snapshot: ScreenSnapshot) -> Self {
    Self::with_host(cx, snapshot, Arc::new(FakeHost::default()))
  }

  fn with_host(cx: &mut TestAppContext, snapshot: ScreenSnapshot, host: Arc<FakeHost>) -> Self {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sink: ActionSink = {
      let sent = sent.clone();
      Rc::new(move |action, _| sent.borrow_mut().push(action))
    };
    cx.update(crate::gpui_shell::theme::init);
    let popups: crate::gpui_shell::popup::PopupSink = Rc::new(|_, _| {});
    let (screen, cx) = cx.add_window_view(|_, _| Screen::new(snapshot, sink, popups, None, host));
    let cx = cx.clone();
    Self { cx, screen, sent }
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
  }

  fn sent(&self) -> Vec<ScreenAction> {
    self.sent.borrow().clone()
  }

  fn update(&mut self, f: impl FnOnce(&mut ScreenSnapshot)) {
    let mut snapshot = self.screen.read_with(&self.cx, |s, _| s.snapshot().clone());
    f(&mut snapshot);
    self
      .screen
      .update(&mut self.cx, |s, cx| s.set_snapshot(snapshot, cx));
  }
}

fn onboarding() -> ScreenSnapshot {
  serde_json::from_str(r#"{"kind": "onboarding", "dark": false}"#).expect("parse")
}

#[test]
fn builds_the_checklist_from_the_check() {
  let list = checklist::from_statuses("Before you start", None, true);
  assert!(list.validating);
  assert_eq!(list.groups.len(), 3);
  // Nothing is reported missing before the first check returns.
  assert!(!list.groups[1].error);
  assert!(list
    .groups
    .iter()
    .flat_map(|g| &g.tools)
    .all(|t| t.installed.is_none()));

  let statuses = [("git", false), ("claude", false), ("gh", true)].map(|(binary, installed)| {
    crate::core::PrerequisiteStatus {
      binary: binary.into(),
      installed,
    }
  });
  let list = checklist::from_statuses("Before you start", Some(&statuses), false);
  let [required, agents, optional] = &list.groups[..] else {
    panic!("three groups");
  };
  assert_eq!(required.label, "Required");
  assert!(required.tools[0].required);
  assert_eq!(required.tools[0].installed, Some(false));
  assert!(agents.error, "no agent CLI is installed");
  assert_eq!(agents.tools.len(), 4);
  assert_eq!(optional.tools[0].installed, Some(true));
  assert_eq!(
    optional.tools[0].note.as_deref(),
    Some("Needed for pull requests and issues")
  );
  // The binaries are the ones the backend checks, in its order.
  let binaries: Vec<_> = list
    .groups
    .iter()
    .flat_map(|g| &g.tools)
    .map(|t| t.binary.as_str())
    .collect();
  assert_eq!(binaries, crate::core::PREREQUISITE_BINARIES);
}

#[test]
fn serializes_actions_as_react_reads_them() {
  let json = serde_json::to_value(ScreenAction::OpenUrl {
    url: "https://cli.github.com/".into(),
  })
  .expect("serialize");
  assert_eq!(
    json,
    serde_json::json!({"kind": "openUrl", "url": "https://cli.github.com/"})
  );
  assert_eq!(
    serde_json::to_value(ScreenAction::RecheckPrerequisites).expect("serialize"),
    serde_json::json!({"kind": "recheckPrerequisites"})
  );
}

#[gpui::test]
fn onboarding_runs_its_own_actions(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    installed: vec![("git", false), ("gh", true)],
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  // The check ran as the screen opened.
  assert!(h.drawn("install-git"));
  assert!(!h.drawn("install-gh"));
  h.click("onboarding-open-repo");
  h.click("prerequisites-recheck");
  h.click("install-git");
  // Only the remote dialog is still React's.
  h.click("onboarding-open-remote");
  assert_eq!(
    host.calls(),
    [
      "check",
      "pick_repo",
      "check",
      "open https://git-scm.com/downloads"
    ]
  );
  assert_eq!(h.sent(), vec![ScreenAction::OpenRemote]);
}

#[gpui::test]
fn onboarding_labels_the_remote_button_by_preview(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    remote_ssh: true,
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host);
  assert!(h.drawn("onboarding-open-remote"));
  let label = h.screen.read_with(&h.cx, |screen, cx| {
    screen
      .onboarding
      .as_ref()
      .map(|view| view.read(cx).remote_label.clone())
  });
  assert_eq!(label.as_deref(), Some("Open via SSH"));
}

const SETTINGS_JSON: &str = r#"{
  "kind": "settings",
  "dark": false,
  "tab": "repository",
  "tabs": ["repository", "application", "account", "integrations", "preview"],
  "save": {"label": "Save Settings", "disabled": false},
  "application": null,
  "repository": {
    "yamlCard": null,
    "currentBranch": "main",
    "loading": false,
    "fields": {
      "branchNamePattern": "treq/{name}",
      "includedFiles": "",
      "defaultModel": "",
      "defaultAgent": "",
      "autoPush": false,
      "ignoreGeneratedAgentFiles": true,
      "reviewPrompt": "",
      "reviewAgent": "",
      "autoReviewTrigger": "off"
    },
    "managed": [],
    "modelOptions": [{"value": "", "label": "Use Application Default"}],
    "agentOptions": [{"value": "", "label": "Use Application Default"}],
    "reviewAgentOptions": [{"value": "", "label": "Use Default Agent"}],
    "triggerOptions": [{"value": "off", "label": "Off"}],
    "error": null
  },
  "preview": null
}"#;

fn settings() -> ScreenSnapshot {
  serde_json::from_str(SETTINGS_JSON).expect("parse")
}

fn field(field: &str, value: serde_json::Value) -> ScreenAction {
  ScreenAction::Field {
    field: field.into(),
    value,
  }
}

#[gpui::test]
fn settings_controls_send_their_actions(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  h.click("settings-tab-application");
  h.click("settings-auto-push");
  h.click("settings-ignore-generated");
  h.click("settings-save");
  h.click("settings-close");
  assert_eq!(
    h.sent(),
    vec![
      field("settings.tab", "application".into()),
      field("repo.autoPush", true.into()),
      field("repo.ignoreGeneratedAgentFiles", false.into()),
      ScreenAction::Press {
        id: "settings.save".into()
      },
      ScreenAction::Press {
        id: "settings.close".into()
      },
    ]
  );
}

#[gpui::test]
fn settings_scrolls_its_content(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1100.), gpui::px(400.)));
  h.cx.run_until_parked();
  let before = h.cx.debug_bounds("settings-auto-push").expect("drawn");
  let scroll = h.cx.debug_bounds("settings-scroll").expect("drawn");
  h.cx.simulate_event(gpui::ScrollWheelEvent {
    position: scroll.center(),
    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-200.))),
    ..Default::default()
  });
  h.cx.run_until_parked();
  let after = h.cx.debug_bounds("settings-auto-push").expect("drawn");
  assert!(after.origin.y < before.origin.y, "{before:?} -> {after:?}");
}

#[gpui::test]
fn settings_fields_are_as_tall_as_react_ones(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  h.cx.run_until_parked();
  // `ui/input.tsx` and the `<select>`s are `h-10`.
  for selector in ["settings-branch-pattern", "settings-repo-default-model"] {
    let bounds = h.cx.debug_bounds(selector).expect("drawn");
    assert_eq!(bounds.size.height, gpui::px(40.), "{selector}");
  }
}

const WORKSPACE_JSON: &str = r#"{
  "kind": "workspace",
  "dark": false,
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
    "checks": {"status": "passed", "treq": "3/3", "github": {"label": "GH 4/4", "state": "success"}},
    "mergeQueue": null,
    "merge": {"passing": true, "disabled": false, "tooltip": null},
    "details": true,
    "more": {"disabled": false},
    "title": "Add login"
  },
  "tabRow": {
    "tab": "overview",
    "commits": {"kind": "count", "count": 3, "tone": "muted"},
    "changes": {"count": 5, "tone": "uncommitted"},
    "logs": false,
    "busy": null,
    "loc": {"insertions": 12, "deletions": 4}
  }
}"#;

fn workspace() -> ScreenSnapshot {
  serde_json::from_str(WORKSPACE_JSON).expect("workspace snapshot")
}

/// The workspace screen in a window as wide as the app's default.
fn workspace_harness(cx: &mut TestAppContext) -> Harness {
  let h = Harness::new(cx, workspace());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  h
}

fn press(id: &str) -> ScreenAction {
  ScreenAction::Press { id: id.into() }
}

#[gpui::test]
fn workspace_header_sends_its_actions(cx: &mut TestAppContext) {
  let mut h = workspace_harness(cx);
  for selector in [
    "workspace-target",
    "workspace-view-pr",
    "workspace-view-pr-web",
    "workspace-sync",
    "workspace-checks",
    "workspace-merge",
    "workspace-details",
    "workspace-more",
  ] {
    h.click(selector);
  }
  h.click("workspace-linear");
  assert_eq!(
    h.sent(),
    vec![
      press("workspace.target"),
      press("workspace.viewPr"),
      press("workspace.viewPrWeb"),
      press("workspace.sync"),
      press("workspace.checks"),
      press("workspace.merge"),
      press("workspace.details"),
      press("workspace.more"),
      ScreenAction::OpenUrl {
        url: "https://linear.app/t/ENG-12".into()
      },
    ]
  );
}

#[gpui::test]
fn workspace_tabs_switch_through_react(cx: &mut TestAppContext) {
  let mut h = workspace_harness(cx);
  assert!(h.drawn("commits-tab-count"));
  assert!(h.drawn("review-change-count"));
  assert!(h.drawn("loc-diff-marker"));
  assert!(!h.drawn("workspace-tab-logs"));
  h.click("workspace-tab-changes");
  assert_eq!(h.sent(), vec![field("workspace.tab", "changes".into())]);
}

#[gpui::test]
fn workspace_leaves_the_tab_row_to_react_without_one(cx: &mut TestAppContext) {
  let mut h = workspace_harness(cx);
  assert!(h.drawn("workspace-tab-row"));
  h.update(|s| match &mut s.view {
    View::Workspace(w) => w.tab_row = None,
    _ => unreachable!(),
  });
  assert!(!h.drawn("workspace-tab-row"));
  assert!(h.drawn("workspace-header"));
  assert!(h.drawn("workspace-frame"));
}

#[gpui::test]
fn workspace_frame_fills_the_space_below(cx: &mut TestAppContext) {
  let mut h = workspace_harness(cx);
  h.cx.run_until_parked();
  let header = h.cx.debug_bounds("workspace-header").expect("header");
  let row = h.cx.debug_bounds("workspace-tab-row").expect("tab row");
  let frame = h.cx.debug_bounds("workspace-frame").expect("frame");
  assert_eq!(row.top(), header.bottom());
  assert_eq!(frame.top(), row.bottom());
  assert!(frame.size.height > gpui::px(100.));
}

#[gpui::test]
fn home_workspace_opens_the_branch_switcher(cx: &mut TestAppContext) {
  let mut h = workspace_harness(cx);
  h.update(|s| match &mut s.view {
    View::Workspace(w) => {
      w.header.home = true;
      w.header.target = None;
      w.header.stack = Some(workspace::Tooltip {
        tooltip: "Create stacked workspace from main".into(),
      });
    }
    _ => unreachable!(),
  });
  h.click("workspace-branch");
  h.click("workspace-stack");
  assert_eq!(
    h.sent(),
    vec![press("workspace.branchSwitcher"), press("workspace.stack")]
  );
}
