use std::{cell::RefCell, rc::Rc};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::*;

/// A screen in a test window, and the actions it sent.
struct Harness {
  cx: VisualTestContext,
  screen: Entity<Screen>,
  sent: Rc<RefCell<Vec<ScreenAction>>>,
}

impl Harness {
  fn new(cx: &mut TestAppContext, snapshot: ScreenSnapshot) -> Self {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sink: ActionSink = {
      let sent = sent.clone();
      Rc::new(move |action, _| sent.borrow_mut().push(action))
    };
    cx.update(crate::gpui_shell::theme::init);
    let (screen, cx) = cx.add_window_view(|_, _| Screen::new(snapshot, sink));
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

const ONBOARDING_JSON: &str = r#"{
  "kind": "onboarding",
  "dark": false,
  "remoteLabel": "Open cloud workspace",
  "checklist": {
    "title": "Before you start",
    "validating": false,
    "groups": [
      {"label": "Required", "error": false, "tools": [
        {"binary": "git", "label": "Git", "note": null, "required": true,
         "installed": false, "installUrl": "https://git-scm.com/downloads"}
      ]},
      {"label": "Agent CLI (install at least one)", "error": true, "tools": [
        {"binary": "claude", "label": "Claude Code", "note": null,
         "required": false, "installed": null,
         "installUrl": "https://code.claude.com/docs/en/setup"}
      ]},
      {"label": "Optional", "error": false, "tools": [
        {"binary": "gh", "label": "GitHub CLI",
         "note": "Needed for pull requests and issues", "required": false,
         "installed": true, "installUrl": "https://cli.github.com/"}
      ]}
    ]
  }
}"#;

fn onboarding() -> ScreenSnapshot {
  serde_json::from_str(ONBOARDING_JSON).expect("parse")
}

#[test]
fn parses_onboarding_from_react() {
  let snapshot = onboarding();
  assert!(!snapshot.dark);
  let View::Onboarding(screen) = snapshot.view else {
    panic!("not onboarding");
  };
  assert_eq!(screen.remote_label.as_deref(), Some("Open cloud workspace"));
  let groups = &screen.checklist.groups;
  assert_eq!(groups.len(), 3);
  assert!(groups[1].error);
  assert_eq!(groups[0].tools[0].installed, Some(false));
  assert_eq!(groups[1].tools[0].installed, None);
  assert_eq!(
    groups[2].tools[0].note.as_deref(),
    Some("Needed for pull requests and issues")
  );
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
fn onboarding_buttons_send_their_actions(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, onboarding());
  h.click("onboarding-open-repo");
  h.click("onboarding-open-remote");
  h.click("prerequisites-recheck");
  // Only the missing tool gets an install link.
  assert!(h.drawn("install-git"));
  assert!(!h.drawn("install-gh"));
  assert!(!h.drawn("install-claude"));
  h.click("install-git");
  assert_eq!(
    h.sent(),
    vec![
      ScreenAction::OpenRepo,
      ScreenAction::OpenRemote,
      ScreenAction::RecheckPrerequisites,
      ScreenAction::OpenUrl {
        url: "https://git-scm.com/downloads".into()
      },
    ]
  );
}

#[gpui::test]
fn onboarding_hides_the_remote_button_without_a_label(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, onboarding());
  assert!(h.drawn("onboarding-open-remote"));
  h.update(|s| {
    let View::Onboarding(screen) = &mut s.view else {
      panic!("not onboarding");
    };
    screen.remote_label = None;
  });
  assert!(!h.drawn("onboarding-open-remote"));
}

#[gpui::test]
fn recheck_is_disabled_while_checking(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, onboarding());
  h.update(|s| {
    let View::Onboarding(screen) = &mut s.view else {
      panic!("not onboarding");
    };
    screen.checklist.validating = true;
  });
  h.click("prerequisites-recheck");
  assert!(h.sent().is_empty());
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
