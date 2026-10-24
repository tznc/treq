//! Property tests: random sequences of what a user and the backend do to a
//! window controller, with the window's invariants checked after each.
//!
//! Each case opens a fresh controller on a fresh test app; GPUI's scheduler
//! seed varies the order in which reads in flight land. `PROPTEST_CASES`
//! sets how many cases run and `SEED` the run (see `gpui_shell::fuzz`); a
//! failure prints the shrunk action list.

use std::sync::atomic::Ordering;

use gpui::proptest::{collection::vec, prelude::*};

use super::*;
use crate::gpui_shell::fuzz;
use crate::gpui_shell::window::{
  model::{Page, MAX_SIDEBAR_WIDTH, MAX_ZOOM, MIN_SIDEBAR_WIDTH, MIN_ZOOM},
  snapshots,
};

/// One thing that happens to the window.
#[derive(Debug, Clone)]
enum Op {
  Shortcut(Shortcut),
  /// Escape as the shell delivers it: the window's shortcut, then the
  /// overlay on top takes its own Escape.
  Escape,
  Menu(Menu),
  Sidebar(SidebarAction),
  Press(&'static str),
  Field(&'static str, serde_json::Value),
  Overlay(ScreenAction),
  /// The backend's workspaces change under the window.
  Delete(i64),
  Add(i64),
  /// The next workspaces read panics.
  FailNextRead,
  Focused,
  FilesChanged,
  PrsChanged,
  /// Time passes; the pollers read.
  Advance(u64),
  /// Every read in flight lands.
  Park,
  /// The confirmation on screen, if any, is answered: its verb, unless
  /// every confirmation is declined, or Cancel.
  Answer(bool),
}

/// Ids around the backend's: some listed, some never, some deleted later.
fn id() -> impl Strategy<Value = i64> {
  prop_oneof![4 => 1..=4i64, 1 => Just(0i64), 1 => 5..=9i64]
}

fn shortcut() -> impl Strategy<Value = Shortcut> {
  prop_oneof![
    Just(Shortcut::NewWorkspace),
    Just(Shortcut::Stack),
    Just(Shortcut::Palette),
    Just(Shortcut::AgentPrompt),
    Just(Shortcut::FilePicker),
    Just(Shortcut::ToggleShortcuts),
    Just(Shortcut::Escape),
    Just(Shortcut::ToggleTerminal),
    Just(Shortcut::MaximizeTerminal),
    Just(Shortcut::NewAgent),
    Just(Shortcut::NewShell),
    Just(Shortcut::ZoomIn),
    Just(Shortcut::ZoomOut),
  ]
}

fn menu() -> impl Strategy<Value = Menu> {
  prop_oneof![
    Just(Menu::Dashboard),
    Just(Menu::Settings),
    Just(Menu::AddRepository),
    Just(Menu::OpenSsh),
    Just(Menu::ViewLogs),
    Just(Menu::FactoryReset),
    Just(Menu::ForceRebase),
    Just(Menu::OpenInNewWindowInvalid),
  ]
}

fn width() -> impl Strategy<Value = f32> {
  prop_oneof![
    -1e6f32..1e6,
    Just(f32::NAN),
    Just(f32::INFINITY),
    Just(f32::NEG_INFINITY),
    Just(0.),
  ]
}

fn sidebar() -> impl Strategy<Value = SidebarAction> {
  let repo = || Just("/repo".to_string());
  prop_oneof![
    repo().prop_map(|repo_path| SidebarAction::SelectHome { repo_path }),
    (id(), any::<bool>(), any::<bool>())
      .prop_map(|(id, shift, toggle)| SidebarAction::SelectWorkspace { id, shift, toggle }),
    (id(), any::<bool>())
      .prop_map(|(id, descendants)| SidebarAction::SelectStack { id, descendants }),
    Just(SidebarAction::ClearSelection),
    Just(SidebarAction::OpenSettings),
    Just(SidebarAction::OpenCommandPalette),
    Just(SidebarAction::OpenGithub),
    Just(SidebarAction::OpenLinear),
    gpui::proptest::option::of(id()).prop_map(|id| SidebarAction::StartAgent { id }),
    gpui::proptest::option::of(id()).prop_map(|id| SidebarAction::OpenShell { id }),
    gpui::proptest::option::of(id()).prop_map(|id| SidebarAction::Stack { id }),
    Just(SidebarAction::BulkArchive),
    Just(SidebarAction::DismissModal),
    (id(), gpui::proptest::option::of(id()))
      .prop_map(|(id, onto)| SidebarAction::MoveWorkspace { id, onto }),
    width().prop_map(|width| SidebarAction::ResizeSidebar { width }),
    Just(SidebarAction::ResetSidebarWidth),
    repo().prop_map(|repo_path| SidebarAction::ToggleRepo { repo_path }),
    (repo(), id()).prop_map(|(repo_path, id)| SidebarAction::OpenRepoWorkspace { repo_path, id }),
  ]
}

const PRESSES: &[&str] = &[
  "workspace.branchSwitcher",
  "workspace.stack",
  "workspace.merge",
  "code.filePicker",
  "fileBrowser.back",
  "workspaceDialog.close",
  "renameWorkspace.close",
  "workspaceDeletion.close",
  "changeMove.done",
  "changeMove.cancel",
  "agentPrompt.close",
  "promptHistory.close",
  "stash.close",
  "filePicker.close",
  "settings.close",
  "mergePreview.cancel",
  "mergePreview.merged",
  "prReview.close",
  "github.tab:issues",
  "github.tab:prs",
  "github.new",
  "github.close",
  "github.select:3",
  "github.openWorkspace:2",
  "github.openWorkspace:9",
];

fn value() -> impl Strategy<Value = serde_json::Value> {
  prop_oneof![
    Just(serde_json::Value::Null),
    any::<bool>().prop_map(Into::into),
    id().prop_map(Into::into),
    "[a-z]{0,8}".prop_map(Into::into),
    prop_oneof![
      Just("overview"),
      Just("commits"),
      Just("changes"),
      Just("logs")
    ]
    .prop_map(Into::into),
  ]
}

fn field() -> impl Strategy<Value = (&'static str, serde_json::Value)> {
  let names = prop_oneof![
    Just("workspace.tab"),
    Just("workspace.viewFullPrompt"),
    Just("code.commit"),
    Just("commits.scrolled"),
    Just("commits.moveToNew"),
    Just("commits.stashed"),
    Just("changes.moveToNew"),
    Just("changes.showCommitted"),
    Just("workspaceDialog.created"),
    Just("settings.tab"),
  ];
  (names, value())
}

fn overlay() -> impl Strategy<Value = ScreenAction> {
  let press = |id: &'static str| ScreenAction::Press { id: id.into() };
  prop_oneof![
    Just(press("overlay.commandPalette")),
    Just(press("overlay.close")),
    Just(press("palette.settings")),
    value().prop_map(|value| ScreenAction::Field {
      field: "palette.navigate".into(),
      value
    }),
    value().prop_map(|value| ScreenAction::Field {
      field: "workspacePicker.select".into(),
      value
    }),
    Just(ScreenAction::Field {
      field: "branchSwitcher.changed".into(),
      value: serde_json::Value::Null
    }),
  ]
}

fn op() -> impl Strategy<Value = Op> {
  prop_oneof![
    4 => shortcut().prop_map(Op::Shortcut),
    2 => Just(Op::Escape),
    1 => menu().prop_map(Op::Menu),
    6 => sidebar().prop_map(Op::Sidebar),
    3 => gpui::proptest::sample::select(PRESSES).prop_map(Op::Press),
    2 => field().prop_map(|(f, v)| Op::Field(f, v)),
    2 => overlay().prop_map(Op::Overlay),
    1 => id().prop_map(Op::Delete),
    1 => id().prop_map(Op::Add),
    1 => Just(Op::FailNextRead),
    1 => Just(Op::Focused),
    1 => Just(Op::FilesChanged),
    1 => Just(Op::PrsChanged),
    2 => (0u64..25_000).prop_map(Op::Advance),
    3 => Just(Op::Park),
    2 => any::<bool>().prop_map(Op::Answer),
  ]
}

fn fuzz_host() -> Arc<FakeHost> {
  Arc::new(FakeHost::on(
    "/repo",
    vec![
      workspace(1, "a", Some("main")),
      workspace(2, "b", Some("a")),
      workspace(3, "c", Some("b")),
      workspace(4, "x", Some("main")),
    ],
  ))
}

fn run(window: &Window, host: &FakeHost, op: &Op, decline: bool, cx: &mut TestAppContext) {
  let controller = &window.controller;
  match op {
    Op::Shortcut(s) => controller.update(cx, |c, cx| c.shortcut(*s, cx)),
    Op::Escape => {
      controller.update(cx, |c, cx| c.shortcut(Shortcut::Escape, cx));
      let top = controller.read_with(cx, |c, _| snapshots::top_overlay(c.model()));
      if top.is_some() {
        let close = ScreenAction::Press {
          id: "overlay.close".into(),
        };
        controller.update(cx, |c, cx| c.overlay_action(&close, cx));
      }
    }
    Op::Menu(m) => controller.update(cx, |c, cx| c.menu(*m, cx)),
    Op::Sidebar(a) => {
      controller.update(cx, |c, cx| c.sidebar_action(a, cx));
    }
    Op::Press(id) => {
      let action = ScreenAction::Press {
        id: (*id).to_string(),
      };
      controller.update(cx, |c, cx| c.screen_action(&action, cx));
    }
    Op::Field(field, value) => {
      let action = ScreenAction::Field {
        field: (*field).to_string(),
        value: value.clone(),
      };
      controller.update(cx, |c, cx| c.screen_action(&action, cx));
    }
    Op::Overlay(action) => {
      controller.update(cx, |c, cx| c.overlay_action(action, cx));
    }
    Op::Delete(id) => host
      .workspaces
      .lock()
      .unwrap()
      .entry("/repo".into())
      .or_default()
      .retain(|w| w.id != *id),
    Op::Add(id) => {
      let mut all = host.workspaces.lock().unwrap();
      let list = all.entry("/repo".into()).or_default();
      if !list.iter().any(|w| w.id == *id) {
        list.push(workspace(*id, &format!("w{id}"), Some("main")));
      }
    }
    Op::FailNextRead => host.panic_once.store(true, Ordering::SeqCst),
    Op::Focused => controller.update(cx, |c, cx| c.focused(cx)),
    Op::FilesChanged => controller.update(cx, |c, cx| c.files_changed(cx)),
    Op::PrsChanged => controller.update(cx, |c, cx| c.prs_changed("/repo", false, cx)),
    Op::Advance(ms) => {
      cx.executor()
        .advance_clock(std::time::Duration::from_millis(*ms));
    }
    Op::Park => cx.run_until_parked(),
    Op::Answer(yes) => {
      let yes = *yes && !decline;
      let asked = controller.read_with(cx, |c, _| c.model().confirm.clone());
      if let Some(confirm) = asked {
        let answer = if yes { "confirm" } else { "cancel" };
        host.record(format!("{answer} {}", confirm.question.question));
        controller.update(cx, |c, cx| c.answer(yes, cx));
      }
    }
  }
}

/// What must hold whatever the user did.
fn check(window: &Window, host: &FakeHost, decline: bool, cx: &mut TestAppContext) {
  let (model, listed) = window.controller.read_with(cx, |c, _| {
    let model = c.model().clone();
    let listed: Vec<i64> = c
      .data()
      .workspaces(model.repo())
      .iter()
      .map(|w| w.id)
      .collect();
    (model, listed)
  });
  let shown = window.last();

  // Zoom and width stay in their clamps.
  assert!(
    (MIN_ZOOM..=MAX_ZOOM).contains(&shown.zoom),
    "zoom {}",
    shown.zoom
  );
  if let Some(sidebar) = &shown.sidebar {
    assert!(
      (MIN_SIDEBAR_WIDTH..=MAX_SIDEBAR_WIDTH).contains(&sidebar.width),
      "sidebar width {}",
      sidebar.width
    );
  }

  // The multi-selection only holds workspaces the window lists.
  for id in &model.multi {
    assert!(
      listed.contains(id),
      "multi-selection {id} is not listed {listed:?}"
    );
  }

  // The workspace route shows the selection.
  if let (Page::Workspace, Some(screen)) = (&model.page, &shown.screen) {
    if let View::Workspace(route) = &screen.view {
      assert_eq!(route.workspace_id, model.selected, "route and selection");
    }
  }

  // Nothing destructive without a yes. (Force rebase is the Developer
  // menu's own command and asks nothing, as in React.)
  let calls = host.calls();
  for (i, call) in calls.iter().enumerate() {
    let destructive = call == "factory reset" || call.starts_with("remove ");
    if destructive {
      assert!(!decline, "{call} although every confirmation was declined");
      assert!(
        calls[..i].iter().any(|c| c.starts_with("confirm ")),
        "{call} without asking: {calls:?}"
      );
    }
  }
}

/// The selection, once the reads land, is a workspace the window lists.
fn check_settled(window: &Window, cx: &mut TestAppContext) {
  let (selected, listed) = window.controller.read_with(cx, |c, _| {
    let model = c.model();
    let listed: Vec<i64> = c
      .data()
      .workspaces(model.repo())
      .iter()
      .map(|w| w.id)
      .collect();
    (model.selected, listed)
  });
  if let Some(id) = selected {
    assert!(
      listed.contains(&id),
      "selected {id} is not listed {listed:?}"
    );
  }
}

#[test]
fn the_window_survives_random_actions() {
  fuzz::check(
    "the_window_survives_random_actions",
    64,
    (vec(op(), 1..40), any::<bool>()),
    |cx, (ops, decline)| {
      let host = fuzz_host();
      let window = Window::open(cx, host.clone(), Some("/repo"));
      for op in &ops {
        run(&window, &host, op, decline, cx);
        check(&window, &host, decline, cx);
        if matches!(op, Op::Park) {
          check_settled(&window, cx);
        }
      }
      cx.run_until_parked();
      check(&window, &host, decline, cx);
      check_settled(&window, cx);
    },
  );
}
