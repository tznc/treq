use std::{cell::RefCell, rc::Rc};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::*;

fn row(id: i64, title: &str, depth: u32) -> SidebarRow {
  SidebarRow {
    id,
    title: title.into(),
    depth,
    selected: false,
    conflicted: false,
    status: None,
    pr: None,
    queue: None,
    archiving: false,
    exiting: false,
  }
}

fn snapshot(workspaces: Vec<SidebarRow>) -> SidebarSnapshot {
  SidebarSnapshot {
    repo_name: "treq".into(),
    settings_open: false,
    github: false,
    linear: None,
    dark: false,
    width: 280.,
    agent_unavailable: None,
    shell_unavailable: None,
    bulk_archive: None,
    repos: vec![RepoSection {
      path: "/src/treq".into(),
      home_ref: Some("main".into()),
      home_selected: true,
      active: true,
      loading: false,
      workspaces,
    }],
  }
}

/// A sidebar in a test window, and the actions it sent.
struct Harness {
  cx: VisualTestContext,
  sidebar: Entity<Sidebar>,
  sent: Rc<RefCell<Vec<SidebarAction>>>,
}

impl Harness {
  fn new(cx: &mut TestAppContext, snapshot: SidebarSnapshot) -> Self {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sink: ActionSink = {
      let sent = sent.clone();
      Rc::new(move |action, _| sent.borrow_mut().push(action))
    };
    let (sidebar, cx) = cx.add_window_view(|_, _| Sidebar::new(snapshot, sink));
    let cx = cx.clone();
    Self { cx, sidebar, sent }
  }

  fn bounds(&mut self, selector: &'static str) -> Option<gpui::Bounds<gpui::Pixels>> {
    self.cx.run_until_parked();
    self.cx.debug_bounds(selector)
  }

  fn click(&mut self, selector: &'static str) {
    let bounds = self
      .bounds(selector)
      .unwrap_or_else(|| panic!("{selector} is not drawn"));
    self.cx.simulate_click(bounds.center(), Modifiers::none());
  }

  fn hover(&mut self, selector: &'static str) {
    let bounds = self
      .bounds(selector)
      .unwrap_or_else(|| panic!("{selector} is not drawn"));
    self
      .cx
      .simulate_mouse_move(bounds.center(), None, Modifiers::none());
  }

  fn sent(&self) -> Vec<SidebarAction> {
    self.sent.borrow().clone()
  }

  fn update(&mut self, f: impl FnOnce(&mut SidebarSnapshot)) {
    let mut snapshot = self
      .sidebar
      .read_with(&self.cx, |s, _| s.snapshot().clone());
    f(&mut snapshot);
    self
      .sidebar
      .update(&mut self.cx, |s, cx| s.set_snapshot(snapshot, cx));
  }
}

#[test]
fn parses_snapshot_from_react() {
  let json = r#"{
    "repoName": "treq",
    "settingsOpen": false,
    "github": false,
    "linear": null,
    "dark": true,
    "width": 280,
    "agentUnavailable": "Not supported here",
    "shellUnavailable": null,
    "bulkArchive": 2,
    "repos": [{
      "path": "/src/treq",
      "homeRef": "main",
      "homeSelected": false,
      "active": true,
      "loading": false,
      "workspaces": [
        {"id": 7, "title": "feat-a", "depth": 1, "selected": true,
         "conflicted": true, "status": {"color": "yellow", "spin": true},
         "pr": "draft", "queue": "testing", "archiving": false, "exiting": false},
        {"id": 8, "title": "feat-b", "depth": 2, "selected": false,
         "status": null, "pr": null}
      ]
    }]
  }"#;
  let snapshot: SidebarSnapshot = serde_json::from_str(json).expect("parse");
  assert_eq!(snapshot.bulk_archive, Some(2));
  assert_eq!(
    snapshot.agent_unavailable.as_deref(),
    Some("Not supported here")
  );
  let rows = &snapshot.repos[0].workspaces;
  assert_eq!(
    rows[0].status,
    Some(RowStatus {
      color: StatusColor::Yellow,
      spin: true
    })
  );
  assert!(rows[0].conflicted);
  assert_eq!(rows[0].pr, Some(PrState::Draft));
  assert_eq!(rows[0].queue.as_deref(), Some("testing"));
  // Optional row fields default when React leaves them out.
  assert!(!rows[1].conflicted && !rows[1].archiving && rows[1].queue.is_none());
}

#[test]
fn serializes_actions_for_react() {
  let json = |action| serde_json::to_value(action).expect("json");
  assert_eq!(
    json(SidebarAction::SelectWorkspace { id: 3 }),
    serde_json::json!({"kind": "selectWorkspace", "id": 3})
  );
  assert_eq!(
    json(SidebarAction::SelectHome {
      repo_path: "/r".into()
    }),
    serde_json::json!({"kind": "selectHome", "repoPath": "/r"})
  );
  assert_eq!(
    json(SidebarAction::StartAgent { id: None }),
    serde_json::json!({"kind": "startAgent", "id": null})
  );
  assert_eq!(
    json(SidebarAction::BulkArchive),
    serde_json::json!({"kind": "bulkArchive"})
  );
}

#[test]
fn indents_rows_like_the_react_sidebar() {
  assert_eq!(row_indent(0), 10.);
  assert_eq!(row_indent(1), 16.);
  assert_eq!(row_indent(3), 28.);
  assert_eq!(guide_left(2), 8.);
  assert_eq!(guide_left(4), 20.);
}

#[test]
fn labels_merge_queue_states_like_the_react_sidebar() {
  let palette = theme::palette(false);
  assert_eq!(queue_dot("queued", &palette).2, "In merge queue");
  assert!(queue_dot("testing", &palette).1);
  assert!(queue_dot("merging", &palette).1);
  assert_eq!(queue_dot("failed", &palette).2, "Failed in merge queue");
  assert_eq!(queue_dot("dequeued", &palette).2, "dequeued");
}

#[gpui::test]
fn header_buttons_open_the_palette_and_settings(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  h.click("command-palette");
  h.click("settings");
  assert_eq!(
    h.sent(),
    vec![
      SidebarAction::OpenCommandPalette,
      SidebarAction::OpenSettings
    ]
  );
}

#[gpui::test]
fn nav_rows_open_github_and_linear(cx: &mut TestAppContext) {
  let mut s = snapshot(vec![]);
  s.linear = Some(false);
  let mut h = Harness::new(cx, s);
  h.click("github");
  h.click("linear");
  assert_eq!(
    h.sent(),
    vec![SidebarAction::OpenGithub, SidebarAction::OpenLinear]
  );
}

#[gpui::test]
fn hides_linear_row_when_its_preview_is_off(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  assert!(h.bounds("linear").is_none());
}

#[gpui::test]
fn clicking_home_selects_its_repository(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  h.click("home");
  assert_eq!(
    h.sent(),
    vec![SidebarAction::SelectHome {
      repo_path: "/src/treq".into()
    }]
  );
}

#[gpui::test]
fn home_actions_show_without_hover_and_do_not_select_home(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  h.click("home-start-agent");
  h.click("home-open-shell");
  h.click("home-stack");
  assert_eq!(
    h.sent(),
    vec![
      SidebarAction::StartAgent { id: None },
      SidebarAction::OpenShell { id: None },
      SidebarAction::Stack { id: None },
    ]
  );
}

#[gpui::test]
fn clicking_a_workspace_selects_it(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0)]));
  h.click("workspace-7");
  assert_eq!(h.sent(), vec![SidebarAction::SelectWorkspace { id: 7 }]);
}

#[gpui::test]
fn workspace_actions_appear_on_hover(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0)]));
  h.hover("workspace-7");
  h.click("workspace-7-start-agent");
  h.hover("workspace-7");
  h.click("workspace-7-open-shell");
  h.hover("workspace-7");
  h.click("workspace-7-stack");
  assert_eq!(
    h.sent(),
    vec![
      SidebarAction::StartAgent { id: Some(7) },
      SidebarAction::OpenShell { id: Some(7) },
      SidebarAction::Stack { id: Some(7) },
    ]
  );
}

#[gpui::test]
fn unavailable_actions_do_nothing(cx: &mut TestAppContext) {
  let mut s = snapshot(vec![row(7, "feat-a", 0)]);
  s.agent_unavailable = Some("Agents are not supported here".into());
  s.shell_unavailable = Some("Shells are not supported here".into());
  let mut h = Harness::new(cx, s);
  h.click("home-start-agent");
  h.click("home-open-shell");
  h.hover("workspace-7");
  h.click("workspace-7-start-agent");
  assert!(h.sent().is_empty(), "sent {:?}", h.sent());
}

#[gpui::test]
fn archiving_workspace_ignores_clicks(cx: &mut TestAppContext) {
  let mut archiving = row(7, "feat-a", 0);
  archiving.archiving = true;
  let mut h = Harness::new(cx, snapshot(vec![archiving]));
  h.click("workspace-7");
  assert!(h.sent().is_empty());
  // No hover actions on an archiving row.
  h.hover("workspace-7");
  assert!(h.bounds("workspace-7-start-agent").is_none());
}

#[gpui::test]
fn shows_status_and_queue_dots(cx: &mut TestAppContext) {
  let mut dirty = row(7, "feat-a", 0);
  dirty.status = Some(RowStatus {
    color: StatusColor::Yellow,
    spin: false,
  });
  dirty.queue = Some("queued".into());
  let mut h = Harness::new(cx, snapshot(vec![dirty, row(8, "clean", 0)]));
  assert!(h.bounds("status-7").is_some());
  assert!(h.bounds("queue-7").is_some());
  assert!(h.bounds("status-8").is_none());
  assert!(h.bounds("queue-8").is_none());
}

#[gpui::test]
fn shows_skeleton_rows_while_loading(cx: &mut TestAppContext) {
  let mut s = snapshot(vec![]);
  s.repos[0].loading = true;
  let mut h = Harness::new(cx, s);
  assert!(h.bounds("skeleton-0").is_some());
  assert!(h.bounds("skeleton-5").is_some());
  h.update(|s| {
    s.repos[0].loading = false;
    s.repos[0].workspaces = vec![row(7, "feat-a", 0)];
  });
  assert!(h.bounds("skeleton-0").is_none());
  assert!(h.bounds("workspace-7").is_some());
}

#[gpui::test]
fn bulk_archive_button_archives_the_selection(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "a", 0), row(8, "b", 0)]));
  assert!(h.bounds("bulk-archive").is_none());
  h.update(|s| s.bulk_archive = Some(2));
  h.click("bulk-archive");
  assert_eq!(h.sent(), vec![SidebarAction::BulkArchive]);
}

#[gpui::test]
fn tooltip_shows_after_a_short_rest(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  h.hover("settings");
  h.sidebar
    .read_with(&h.cx, |s, _| assert!(!s.tooltip_shown("settings")));
  h.cx.executor().advance_clock(TOOLTIP_DELAY);
  h.cx.run_until_parked();
  h.sidebar
    .read_with(&h.cx, |s, _| assert!(s.tooltip_shown("settings")));
  // Moving away hides it.
  h.hover("command-palette");
  h.sidebar
    .read_with(&h.cx, |s, _| assert!(!s.tooltip_shown("settings")));
}

#[gpui::test]
fn row_button_tooltip_wins_over_its_row(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  h.hover("home-stack");
  h.cx.executor().advance_clock(TOOLTIP_DELAY);
  h.cx.run_until_parked();
  h.sidebar.read_with(&h.cx, |s, _| {
    assert!(s.tooltip_shown("home-stack"));
    assert!(!s.tooltip_shown("home:/src/treq"));
  });
  // Back on the row itself, the row's tooltip returns.
  let home = h.bounds("home").unwrap();
  h.cx.simulate_mouse_move(
    home.origin + gpui::point(gpui::px(40.), gpui::px(8.)),
    None,
    Modifiers::none(),
  );
  h.cx.executor().advance_clock(TOOLTIP_DELAY);
  h.cx.run_until_parked();
  h.sidebar.read_with(&h.cx, |s, _| {
    assert!(s.tooltip_shown("home:/src/treq"));
  });
}
