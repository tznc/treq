//! Property tests: random clicks, typing and keys on the screens, found the
//! way a user finds them (gpui-testing-library's roles and names, plus the
//! screens' debug selectors), with invariants checked after every step.
//!
//! Each case opens a fresh test app. `PROPTEST_CASES` sets how many cases
//! run and `SEED` the run; a failure prints the shrunk step list.

use gpui::{
  proptest::{collection::vec, prelude::*, sample::Index},
  Role,
};
use gpui_testing_library::{prelude::*, Node, Snapshot};

use super::*;
use crate::gpui_shell::fuzz;

/// One thing the user does.
#[derive(Debug, Clone)]
enum Step {
  /// Clicks one of the controls on screen, by its index among them.
  Click(Index),
  /// Clicks one of the text fields on screen, then types.
  Type(Index, String),
  /// Types at the focus (user-event keyboard syntax).
  Key(&'static str),
  /// Time passes: debounced checks and saves run.
  Advance(u64),
}

/// No Tab: unhandled, gpui-testing-library moves the focus as a browser
/// does, but the shell's windows have no Tab navigation to emulate.
const KEYS: &[&str] = &[
  "{Escape}",
  "{Enter}",
  "{ArrowUp}",
  "{ArrowDown}",
  "{ArrowLeft}",
  "{ArrowRight}",
  "{Backspace}",
  "{Delete}",
  "{Home}",
  "{End}",
  "{Space}",
  "{Control>}a{/Control}",
  "{Meta>}a{/Meta}",
  "{Control>}z{/Control}",
];

/// Text a user might type or paste: names, unicode, emoji, path tricks,
/// branch-name edge cases and very long strings.
fn text() -> impl Strategy<Value = String> {
  prop_oneof![
    3 => "[a-zA-Z0-9 /._-]{0,12}",
    2 => "\\PC{0,8}",
    1 => gpui::proptest::sample::select(vec![
      "🚀", "👩‍👩‍👧", "é\u{301}", "日本語", "\u{202e}rtl", "\u{200b}", "@{", "..", "a b",
      "/", "-", "~^:?*[", "main", "treq/login", "  ", "\t",
    ])
    .prop_map(str::to_string),
    1 => (1usize..600).prop_map(|n| "x".repeat(n)),
    1 => (1usize..60).prop_map(|n| "🦀".repeat(n)),
  ]
}

fn step() -> impl Strategy<Value = Step> {
  prop_oneof![
    6 => any::<Index>().prop_map(Step::Click),
    3 => (any::<Index>(), text()).prop_map(|(i, t)| Step::Type(i, t)),
    3 => gpui::proptest::sample::select(KEYS).prop_map(Step::Key),
    1 => (0u64..1500).prop_map(Step::Advance),
  ]
}

/// The typed text in user-event syntax: braces and brackets doubled.
fn literal(text: &str) -> String {
  text.replace('{', "{{").replace('[', "[[")
}

/// A screen to fuzz: what it opens on, and the debug selectors of its
/// controls that are not observed by role.
struct Scenario {
  snapshot: ScreenSnapshot,
  host: FakeHost,
  wide: bool,
  clicks: &'static [&'static str],
  fields: &'static [&'static str],
  /// Debug selectors of layers drawn over the screen, which Escape closes.
  layers: &'static [&'static str],
}

const CLICK_ROLES: &[Role] = &[
  Role::Button,
  Role::Switch,
  Role::Tab,
  Role::CheckBox,
  Role::MenuItem,
  Role::ListBoxOption,
  Role::ComboBox,
  Role::RadioButton,
  Role::Link,
];

const TEXT_ROLES: &[Role] = &[Role::TextInput, Role::MultilineTextInput, Role::SearchInput];

/// A control on screen: an observed element or a debug selector.
#[derive(Debug, Clone)]
enum Target {
  Node(Box<Node>),
  Selector(&'static str),
}

impl Target {
  fn describe(&self) -> String {
    match self {
      Target::Node(node) => {
        let snapshot = node.initial_snapshot();
        format!(
          "{:?} {:?}",
          snapshot.role(),
          snapshot.name().unwrap_or_default()
        )
      }
      Target::Selector(s) => format!("#{s}"),
    }
  }
}

struct Frame {
  elements: Vec<Snapshot>,
  layers: Vec<(&'static str, gpui::Bounds<gpui::Pixels>)>,
}

impl Frame {
  fn dialogs(&self) -> Vec<&Snapshot> {
    self
      .elements
      .iter()
      .filter(|s| s.role() == Some(Role::Dialog) && s.is_visible())
      .collect()
  }

  fn focused(&self) -> Option<&Snapshot> {
    self.elements.iter().find(|s| s.focused() == Some(true))
  }

  /// Something on screen: a dialog or a layer.
  fn covered(&self) -> bool {
    !self.dialogs().is_empty() || !self.layers.is_empty()
  }

  /// Whether an element `before` showed is gone, as a closed popup is.
  fn lost_any_of(&self, before: &Frame) -> bool {
    before
      .elements
      .iter()
      .any(|b| !self.elements.iter().any(|a| a.path() == b.path()))
  }
}

struct Run {
  h: Harness,
  host: Arc<FakeHost>,
  screen: gpui_testing_library::Screen,
  scenario_clicks: &'static [&'static str],
  scenario_fields: &'static [&'static str],
  layers: &'static [&'static str],
  /// What the steps did, for failure messages.
  log: Vec<String>,
}

impl Run {
  fn open(cx: &mut TestAppContext, scenario: Scenario) -> Self {
    let host = Arc::new(scenario.host);
    let mut h = Harness::with_host(cx, scenario.snapshot, host.clone());
    if scenario.wide {
      h.cx
        .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
    }
    h.cx.run_until_parked();
    let window = h.cx.update(|window, _| window.window_handle());
    let screen = gpui_testing_library::Screen::new(&h.cx, window);
    screen.redraw();
    Self {
      h,
      host,
      screen,
      scenario_clicks: scenario.clicks,
      scenario_fields: scenario.fields,
      layers: scenario.layers,
      log: Vec::new(),
    }
  }

  fn settle(&mut self) {
    self.h.cx.run_until_parked();
    self.screen.redraw();
  }

  /// Reads the snapshot taken with the query: every live `Node` accessor
  /// draws a frame.
  fn usable(node: &Node) -> bool {
    let snapshot = node.initial_snapshot();
    snapshot.is_visible() && !snapshot.is_disabled()
  }

  fn targets(&mut self, roles: &[Role], selectors: &'static [&'static str]) -> Vec<Target> {
    let mut targets: Vec<Target> = self
      .screen
      .elements()
      .into_iter()
      .filter(|n| {
        n.initial_snapshot()
          .role()
          .is_some_and(|r| roles.contains(&r))
          && Self::usable(n)
      })
      .map(|n| Target::Node(Box::new(n)))
      .collect();
    for selector in selectors {
      if self.h.cx.debug_bounds(selector).is_some() {
        targets.push(Target::Selector(selector));
      }
    }
    targets
  }

  fn click(&mut self, target: &Target) {
    match target {
      Target::Node(node) => self.screen.user().click(node),
      Target::Selector(selector) => {
        if let Some(bounds) = self.h.cx.debug_bounds(selector) {
          self
            .h
            .cx
            .simulate_click(bounds.center(), gpui::Modifiers::none());
        }
      }
    }
  }

  /// Does `step`, returning what it did.
  fn step(&mut self, step: &Step) -> String {
    let done = match step {
      Step::Click(i) => {
        let targets = self.targets(CLICK_ROLES, self.scenario_clicks);
        if targets.is_empty() {
          return "click: nothing to click".into();
        }
        let target = &targets[i.index(targets.len())];
        self.click(target);
        format!("click {}", target.describe())
      }
      Step::Type(i, text) => {
        let targets = self.targets(TEXT_ROLES, self.scenario_fields);
        if targets.is_empty() {
          self.screen.user().keyboard(&literal(text));
          self.settle();
          return format!("type {text:?} at the focus");
        }
        let target = targets[i.index(targets.len())].clone();
        self.click(&target);
        self.settle();
        self.screen.user().keyboard(&literal(text));
        format!("type {text:?} into {}", target.describe())
      }
      Step::Key(keys) => {
        self.screen.user().keyboard(keys);
        format!("key {keys}")
      }
      Step::Advance(ms) => {
        self
          .h
          .cx
          .executor()
          .advance_clock(std::time::Duration::from_millis(*ms));
        format!("wait {ms}ms")
      }
    };
    self.settle();
    done
  }

  /// What is on screen: every observed element (one frame), and the
  /// layers drawn over the screen with their bounds.
  fn frame(&mut self) -> Frame {
    let elements = self
      .screen
      .elements()
      .iter()
      .map(|n| n.initial_snapshot().clone())
      .collect();
    let layers = self
      .layers
      .iter()
      .filter_map(|l| Some((*l, self.h.cx.debug_bounds(l)?)))
      .collect();
    Frame { elements, layers }
  }

  fn context(&self) -> String {
    format!("after {:#?}", self.log)
  }
}

/// The calls that change something: not reads, checks, dry runs or the
/// notices the screens post.
fn effects(host: &FakeHost) -> Vec<String> {
  host
    .writes()
    .into_iter()
    .filter(|c| {
      !c.contains("(dry run)")
        && !c.starts_with("notify ")
        && !c.starts_with("list ")
        && c != "fetch"
    })
    .collect()
}

/// Steps that cannot confirm anything: typing into a text field and
/// moving around. (`done` says where text went: typed at the focus, a
/// space can pick from an open menu.)
fn confirms_nothing(step: &Step, done: &str) -> bool {
  match step {
    Step::Type(_, text) => done.contains(" into ") || !text.contains(' '),
    Step::Key(keys) => !matches!(*keys, "{Enter}" | "{Space}"),
    Step::Click(_) | Step::Advance(_) => false,
  }
}

/// Runs `steps` on `scenario`, checking after each that:
/// - at most one dialog is open;
/// - the focus is inside the dialog while one is open;
/// - Escape closes the dialog, or a popup inside it;
/// - typing and moving around write nothing to the backend.
fn fuzz(cx: &mut TestAppContext, scenario: Scenario, steps: &[Step]) -> Run {
  fuzz_from(Run::open(cx, scenario), steps)
}

fn fuzz_from(mut run: Run, steps: &[Step]) -> Run {
  let mut before = run.frame();
  for step in steps {
    let written = effects(&run.host).len();
    let sent = run.h.sent().len();
    let done = run.step(step);
    let quiet = confirms_nothing(step, &done);
    run.log.push(done);
    let after = run.frame();

    let dialogs = after.dialogs();
    assert!(
      dialogs.len() <= 1 && after.layers.len() <= 1,
      "{:?} and {:?} are open together {}",
      dialogs.iter().map(|d| d.name()).collect::<Vec<_>>(),
      after.layers,
      run.context()
    );
    if let Some(focused) = after.focused() {
      if let Some(dialog) = dialogs.first() {
        assert!(
          dialog.contains(focused),
          "focus {:?} {:?} is outside dialog {:?} {}",
          focused.role(),
          focused.name(),
          dialog.name(),
          run.context()
        );
      }
      if let Some((layer, bounds)) = after.layers.first() {
        assert!(
          bounds.contains(&focused.bounds().center()),
          "focus {:?} {:?} is outside {layer} {}",
          focused.role(),
          focused.name(),
          run.context()
        );
      }
    }
    if matches!(step, Step::Key("{Escape}")) && before.covered() {
      let closed = run.h.sent()[sent..].iter().any(
        |a| matches!(a, ScreenAction::Press { id } if id.ends_with(".close") || id.ends_with(".cancel")),
      );
      assert!(
        closed || after.lost_any_of(&before),
        "Escape closed nothing: {:?} {:?} {}",
        before
          .dialogs()
          .iter()
          .map(|d| d.name())
          .collect::<Vec<_>>(),
        before.layers,
        run.context()
      );
    }
    let writes = effects(&run.host);
    if quiet {
      assert_eq!(
        writes.len(),
        written,
        "{:?} wrote {:?} {}",
        step,
        &writes[written..],
        run.context()
      );
    }
    before = after;
  }
  run
}

fn workspace_host() -> FakeHost {
  FakeHost {
    workspaces: vec![
      workspace_dialogs::workspace(7, "treq/login", Some("main")),
      workspace_dialogs::workspace(8, "treq/login-2", Some("treq/login")),
    ],
    branches: ["main", "release"]
      .map(|name| crate::jj::JjBranch {
        name: name.into(),
        is_current: false,
      })
      .to_vec(),
    local_branches: vec!["taken".into()],
    ..FakeHost::default()
  }
}

// Settings: toggles back and forth, typed values, save and reset.

const SETTINGS_CLICKS: &[&str] = &[
  "settings-tab-application",
  "settings-tab-repository",
  "settings-auto-push",
  "settings-auto-review-trigger",
  "settings-conflict-marker-style",
  "settings-default-agent",
  "settings-default-model",
  "settings-font-size",
  "settings-notify-agent-finished",
  "settings-repo-default-agent",
  "settings-repo-default-model",
  "settings-review-agent",
  "settings-setup-always-run",
  "settings-theme",
  "settings-yaml-reload",
  "settings-save",
  "settings-close",
];

const SETTINGS_FIELDS: &[&str] = &["settings-branch-pattern"];

fn settings_scenario(tab: &str) -> Scenario {
  Scenario {
    snapshot: settings_route(tab),
    host: FakeHost::with_settings(&[("/repo:auto_review_trigger", "on-push")]),
    wide: false,
    clicks: SETTINGS_CLICKS,
    fields: SETTINGS_FIELDS,
    layers: &[],
  }
}

/// The application settings Save writes: theme, zoom, font size, model,
/// agent, conflict style and the agent notification.
type AppFields = (String, f32, String, String, String, String, bool);

/// What a reopened settings screen must draw again.
fn saved_fields(s: &settings::Settings) -> (Option<settings::RepositoryFields>, Option<AppFields>) {
  (
    s.repository.as_ref().map(|r| r.fields.clone()),
    s.application.as_ref().map(|a| {
      (
        a.theme.clone(),
        a.zoom,
        a.font_size.clone(),
        a.default_model.clone(),
        a.default_agent.clone(),
        a.conflict_marker_style.clone(),
        a.notify_agent_finished,
      )
    }),
  )
}

#[test]
fn settings_survive_random_edits() {
  fuzz::check(
    "settings_survive_random_edits",
    4,
    (vec(step(), 1..20), any::<bool>()),
    |cx, (steps, repository_tab)| {
      let tab = if repository_tab {
        "repository"
      } else {
        "application"
      };
      let mut run = fuzz(cx, settings_scenario(tab), &steps);
      // Whatever was edited, saving it reads back the same in a reopened screen.
      run.settle();
      if !run.h.drawn("settings-save") {
        return;
      }
      run.h.click("settings-save");
      run.settle();
      let saved = drawn_settings(&mut run.h);
      if saved.save.as_ref().is_some_and(|s| !s.disabled) {
        // Nothing to compare: the save did not go through.
        return;
      }
      let mut reopened = Harness::with_host(cx, settings_route(tab), run.host.clone());
      let again = drawn_settings(&mut reopened);
      assert_eq!(
        saved_fields(&again),
        saved_fields(&saved),
        "settings round-trip {}",
        run.context()
      );
    },
  );
}

// The workspace dialogs: create, stack and rename, and their validation.

const DIALOG_CLICKS: &[&str] = &[
  "workspace-dialog-cancel",
  "workspace-dialog-submit",
  "workspace-dialog-target",
  "position-before",
  "move-to-existing",
  "dialog-commit-0",
  "dialog-commits-clear",
  "dialog-file-path-0",
  "rename-submit",
];

const DIALOG_FIELDS: &[&str] = &[
  "workspace-dialog-title",
  "workspace-dialog-branch",
  "workspace-dialog-description",
  "workspace-dialog-sparse",
  "workspace-dialog-symlinks",
  "rename-branch-name",
];

fn dialog_scenario(which: u8) -> Scenario {
  let snapshot = match which % 3 {
    0 => workspace_dialogs::dialog(serde_json::json!({})),
    1 => workspace_dialogs::dialog(serde_json::json!({
      "sourceBranch": "treq/login",
      "targetBranch": "treq/login",
    })),
    _ => workspace_dialogs::rename(),
  };
  Scenario {
    snapshot,
    host: workspace_host(),
    wide: false,
    clicks: DIALOG_CLICKS,
    fields: DIALOG_FIELDS,
    layers: &["workspace-dialog", "rename-workspace-dialog"],
  }
}

#[test]
fn workspace_dialogs_survive_random_input() {
  fuzz::check(
    "workspace_dialogs_survive_random_input",
    8,
    (vec(step(), 1..20), 0u8..3),
    |cx, (steps, which)| {
      let run = fuzz(cx, dialog_scenario(which), &steps);
      // Only a valid branch name is created, or renamed to once the dry
      // run accepts it.
      let workspaces = workspace_host().workspaces;
      for call in effects(&run.host) {
        let branch = call
          .strip_prefix("create ")
          .and_then(|rest| rest.split_whitespace().next())
          .or_else(|| call.strip_prefix("rename 7 -> "));
        if let Some(branch) = branch {
          assert_eq!(
            workspace_dialog_data::branch_name_error(branch, &workspaces),
            None,
            "{call} {}",
            run.context()
          );
        }
      }
    },
  );
}

// The Changes tab: review comments (add, edit, cancel, discard), the commit
// box, staging and discarding.

const CHANGES_CLICKS: &[&str] = &[
  "add-comment-a.rs-0-1",
  "add-comment-a.rs-0-2",
  "add-file-comment-button",
  "comment-input",
  "comment-submit",
  "inline-comment-card",
  "comment-edit",
  "comment-save",
  "comment-edit-cancel",
  "comment-discard",
  "delete-comment",
  "discard-review",
  "confirm-discard-review",
  "finish-review",
  "commit-message",
  "commit-button",
  "file-row-a.rs",
  "file-row-b.rs",
  "stage-a.rs",
  "unstage-a.rs",
  "viewed-a.rs",
  "discard-all-changes",
  "discard-all-confirm",
  "changes-filter",
];

const CHANGES_FIELDS: &[&str] = &["comment-input", "comment-edit", "commit-message"];

#[test]
fn the_changes_tab_survives_random_review() {
  fuzz::check(
    "the_changes_tab_survives_random_review",
    3,
    vec(step(), 1..20),
    |cx, steps| {
      let scenario = Scenario {
        snapshot: changes_route(),
        host: changes_host(),
        wide: true,
        clicks: CHANGES_CLICKS,
        fields: CHANGES_FIELDS,
        layers: &["discard-all-dialog"],
      };
      let run = fuzz(cx, scenario, &steps);
      let log = run.log.join("\n");
      for call in effects(&run.host) {
        // Discarding everything needs its confirmation.
        if call.starts_with("discard /") {
          assert!(
            log.contains("#discard-all-confirm") || log.contains("\"Discard all changes\""),
            "{call} {}",
            run.context()
          );
        }
        // Commits carry a message.
        if let Some(message) = call.strip_prefix("commit Some(7) ") {
          assert!(
            message.trim_matches('"').trim() != "",
            "{call} {}",
            run.context()
          );
        }
      }
    },
  );
}

// The Logs tab: its filters, explorer and query input.

const LOGS_CLICKS: &[&str] = &[
  "log-level-filter",
  "log-level-option-error",
  "multi-select-toggle",
  "repo-log-line",
  "logs-explorer-view",
  "template-menu",
  "sql-template-Errors by job",
  "sql-template-Recent lines",
  "run-query",
  "log-source",
];

const LOGS_FIELDS: &[&str] = &["sql-editor", "logs-find", "system-logs-search"];

#[test]
fn the_logs_tab_survives_random_queries() {
  fuzz::check(
    "the_logs_tab_survives_random_queries",
    3,
    (vec(step(), 1..20), any::<bool>()),
    |cx, (steps, system)| {
      let scenario = Scenario {
        snapshot: logs_route(system.then_some("system")),
        host: logs_host(),
        wide: true,
        clicks: LOGS_CLICKS,
        fields: LOGS_FIELDS,
        layers: &[],
      };
      fuzz(cx, scenario, &steps);
    },
  );
}

// The file picker: its search, list and Escape.

const PICKER_CLICKS: &[&str] = &[
  "task-add",
  "task-add-file",
  "file-picker-item-0",
  "file-picker-item-1",
  "file-picker-item-2",
];

#[test]
fn the_file_picker_survives_random_searches() {
  fuzz::check(
    "the_file_picker_survives_random_searches",
    3,
    vec(step(), 1..20),
    |cx, steps| {
      let scenario = Scenario {
        snapshot: code_route(),
        host: code_host(),
        wide: true,
        clicks: PICKER_CLICKS,
        fields: &[],
        layers: &["file-picker"],
      };
      let mut run = Run::open(cx, scenario);
      run.h.click("task-add");
      run.h.click("task-add-file");
      run.settle();
      fuzz_from(run, &steps);
    },
  );
}

// The Overview's machine popover: switching a host's terminals and
// resetting them only after the confirmation, whatever else is clicked.

const MACHINE_CLICKS: &[&str] = &[
  "machine-tools-ssh:ep-1",
  "machine-tools-local",
  "machine-terminal-backend",
  "machine-reset-terminals",
  "machine-remove",
];

#[test]
fn machine_terminal_controls_change_nothing_unconfirmed() {
  fuzz::check(
    "machine_terminal_controls_change_nothing_unconfirmed",
    4,
    vec(step(), 1..20),
    |cx, steps| {
      let scenario = Scenario {
        snapshot: onboarding(),
        host: FakeHost {
          listing: listing(),
          host_tools: Some(Ok(vec![])),
          terminal_status: Some(Ok(running_daemon(1))),
          ..FakeHost::default()
        },
        wide: true,
        clicks: MACHINE_CLICKS,
        fields: &[],
        layers: &[],
      };
      let mut run = fuzz(cx, scenario, &steps);
      run.settle();
      // A change to a host's terminals follows its confirmation, so there
      // are never more of them than confirmations answered with the action.
      let changes = run
        .host
        .calls()
        .iter()
        .filter(|c| c.starts_with("terminals ") || c.starts_with("reset terminals "))
        .count();
      // Confirmations: a click on the dialog's action, or Enter/Space on
      // its focused button.
      let confirmed = run
        .log
        .iter()
        .filter(|l| {
          l.ends_with("\"Use tmux\"")
            || l.ends_with("\"Use Treq daemon\"")
            || l.ends_with("\"Reset terminals\"")
            || *l == "key {Enter}"
            || *l == "key {Space}"
        })
        .count();
      assert!(
        changes <= confirmed,
        "{changes} terminal changes for {confirmed} confirmations {}",
        run.context()
      );
      // An open popover never stays on "Asking the host…" once the host
      // has answered.
      if run.h.drawn("machine-terminals-status") {
        let asking = run
          .screen
          .get_all_by_role(by::role(Role::Status))
          .iter()
          .any(|n| n.name().is_some_and(|name| name.starts_with("Asking")));
        assert!(!asking, "the status stayed on Asking {}", run.context());
      }
    },
  );
}
