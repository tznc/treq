use std::{cell::RefCell, rc::Rc};

use gpui::{px, Entity, Modifiers, TestAppContext, VisualTestContext};

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
      label: None,
      missing: false,
      collapsed: false,
    }],
    modal: None,
  }
}

/// The main repository "treq" on screen, and a supporting "lib" whose
/// workspace shares an id with treq's.
fn two_repos() -> SidebarSnapshot {
  let mut s = snapshot(vec![row(7, "feat-a", 0)]);
  s.repos[0].label = Some("treq".into());
  s.repos.push(RepoSection {
    path: "/src/lib".into(),
    home_ref: None,
    home_selected: false,
    active: false,
    loading: false,
    workspaces: vec![row(7, "lib-feat", 0)],
    label: Some("lib".into()),
    missing: false,
    collapsed: false,
  });
  s
}

/// A sidebar in a test window, and the actions it sent.
struct Harness {
  cx: VisualTestContext,
  sidebar: Entity<Sidebar>,
  sent: Rc<RefCell<Vec<SidebarAction>>>,
  popups: Rc<RefCell<Vec<PopupRequest>>>,
}

impl Harness {
  fn new(cx: &mut TestAppContext, snapshot: SidebarSnapshot) -> Self {
    cx.update(crate::gpui_shell::theme::init);
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sink: ActionSink = {
      let sent = sent.clone();
      Rc::new(move |action, _| sent.borrow_mut().push(action))
    };
    let popups = Rc::new(RefCell::new(Vec::new()));
    let popup_sink: PopupSink = {
      let popups = popups.clone();
      Rc::new(move |request, _| popups.borrow_mut().push(request))
    };
    let (sidebar, cx) = cx.add_window_view(|_, _| Sidebar::new(snapshot, sink, popup_sink));
    let cx = cx.clone();
    Self {
      cx,
      sidebar,
      sent,
      popups,
    }
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

  /// A mouse down and up at `position`, the `click_count`th in a row.
  fn press(
    &mut self,
    position: gpui::Point<gpui::Pixels>,
    button: MouseButton,
    modifiers: Modifiers,
    click_count: usize,
  ) {
    self.cx.simulate_event(gpui::MouseDownEvent {
      position,
      button,
      modifiers,
      click_count,
      first_mouse: false,
    });
    self.cx.simulate_event(gpui::MouseUpEvent {
      position,
      button,
      modifiers,
      click_count,
    });
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
    json(SidebarAction::SelectWorkspace {
      id: 3,
      shift: true,
      toggle: false
    }),
    serde_json::json!({"kind": "selectWorkspace", "id": 3, "shift": true, "toggle": false})
  );
  assert_eq!(
    json(SidebarAction::SelectStack {
      id: 3,
      descendants: true
    }),
    serde_json::json!({"kind": "selectStack", "id": 3, "descendants": true})
  );
  assert_eq!(
    json(SidebarAction::ClearSelection),
    serde_json::json!({"kind": "clearSelection"})
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
  assert_eq!(
    json(SidebarAction::HomeMenu {
      repo_path: "/r".into()
    }),
    serde_json::json!({"kind": "homeMenu", "repoPath": "/r"})
  );
  assert_eq!(
    json(SidebarAction::DismissModal),
    serde_json::json!({"kind": "dismissModal"})
  );
  assert_eq!(
    json(SidebarAction::WorkspaceMenu { id: 4 }),
    serde_json::json!({"kind": "workspaceMenu", "id": 4})
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
fn the_open_button_opens_workspaces(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  h.click("open-workspaces");
  assert_eq!(h.sent(), vec![SidebarAction::OpenWorkspaces]);
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
  assert_eq!(h.sent(), vec![select(7, false, false)]);
}

fn select(id: i64, shift: bool, toggle: bool) -> SidebarAction {
  SidebarAction::SelectWorkspace { id, shift, toggle }
}

#[gpui::test]
fn modified_clicks_carry_shift_and_toggle(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0)]));
  let at = h.bounds("workspace-7").expect("drawn").center();
  h.press(at, MouseButton::Left, Modifiers::shift(), 1);
  h.press(at, MouseButton::Left, Modifiers::control(), 1);
  h.press(at, MouseButton::Left, Modifiers::command(), 1);
  assert_eq!(
    h.sent(),
    vec![
      select(7, true, false),
      select(7, false, true),
      select(7, false, true)
    ]
  );
}

#[gpui::test]
fn double_click_selects_the_stack(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0)]));
  let at = h.bounds("workspace-7").expect("drawn").center();
  h.press(at, MouseButton::Left, Modifiers::none(), 1);
  h.press(at, MouseButton::Left, Modifiers::none(), 2);
  h.press(at, MouseButton::Left, Modifiers::shift(), 2);
  assert_eq!(
    h.sent(),
    vec![
      select(7, false, false),
      SidebarAction::SelectStack {
        id: 7,
        descendants: false
      },
      SidebarAction::SelectStack {
        id: 7,
        descendants: true
      },
    ]
  );
}

#[gpui::test]
fn clicking_the_list_background_clears_the_selection(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0)]));
  let list = h.bounds("workspaces").expect("drawn");
  let below_rows = gpui::point(list.center().x, list.bottom() - px(4.));
  h.press(below_rows, MouseButton::Left, Modifiers::none(), 1);
  assert_eq!(h.sent(), vec![SidebarAction::ClearSelection]);
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
fn long_workspace_title_truncates_before_hover_actions(cx: &mut TestAppContext) {
  let title = "zn/intelligent-mayer-276u4-with-a-very-long-branch-name";
  let mut h = Harness::new(cx, snapshot(vec![row(7, title, 0)]));
  h.hover("workspace-7");
  let title = h.bounds("workspace-7-title").expect("drawn");
  let agent = h.bounds("workspace-7-start-agent").expect("drawn");
  assert!(
    title.right() <= agent.left(),
    "title ends at {:?}, past the agent button at {:?}",
    title.right(),
    agent.left()
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

#[gpui::test]
fn workspace_tooltip_opens_a_popup_beside_the_row(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0)]));
  h.hover("workspace-7");
  h.cx.executor().advance_clock(TOOLTIP_DELAY);
  h.cx.run_until_parked();
  let row = h.bounds("workspace-7").unwrap();
  assert_eq!(
    h.popups.borrow().as_slice(),
    &[PopupRequest::ShowTooltip {
      anchor: row,
      side: Side::Right,
      content: TooltipContent::Branch {
        title: "feat-a".into(),
        conflicted: false,
        pr: None,
      },
      dark: false,
    }]
  );
}

#[gpui::test]
fn button_tooltip_opens_below_and_closes_on_leave(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  h.hover("settings");
  h.cx.executor().advance_clock(TOOLTIP_DELAY);
  h.cx.run_until_parked();
  let settings = h.bounds("settings").unwrap();
  assert!(matches!(
    h.popups.borrow().as_slice(),
    [PopupRequest::ShowTooltip { anchor, side: Side::Bottom, content: TooltipContent::Text(_), .. }]
      if *anchor == settings
  ));
  h.hover("command-palette");
  h.cx.run_until_parked();
  assert_eq!(h.popups.borrow().last(), Some(&PopupRequest::HideTooltip));
}

fn right_click(h: &mut Harness, selector: &'static str) {
  let bounds = h
    .bounds(selector)
    .unwrap_or_else(|| panic!("{selector} is not drawn"));
  h.press(bounds.center(), MouseButton::Right, Modifiers::none(), 1);
}

#[gpui::test]
fn right_click_opens_row_menus_without_selecting(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0)]));
  right_click(&mut h, "home");
  right_click(&mut h, "workspace-7");
  assert_eq!(
    h.sent(),
    vec![
      SidebarAction::HomeMenu {
        repo_path: "/src/treq".into()
      },
      SidebarAction::WorkspaceMenu { id: 7 },
    ]
  );
}

#[gpui::test]
fn right_click_hides_the_tooltip(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0)]));
  h.hover("workspace-7");
  h.cx.executor().advance_clock(TOOLTIP_DELAY);
  h.cx.run_until_parked();
  h.sidebar
    .read_with(&h.cx, |s, _| assert!(s.tooltip_shown("workspace:7")));
  right_click(&mut h, "workspace-7");
  h.sidebar
    .read_with(&h.cx, |s, _| assert!(!s.tooltip_shown("workspace:7")));
}

#[gpui::test]
fn modal_backdrop_covers_the_sidebar_and_dismisses(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0)]));
  assert!(h.bounds("modal-backdrop").is_none());
  h.update(|s| s.modal = Some(0.4));
  let backdrop = h.bounds("modal-backdrop").expect("backdrop drawn");
  let row = h.bounds("workspace-7").unwrap();
  assert!(backdrop.contains(&row.center()));
  // It reaches the sidebar's right edge, border included.
  let sidebar = h.bounds("sidebar").expect("sidebar drawn");
  assert_eq!(backdrop.right(), sidebar.right());
  // A click over a row reaches the backdrop, not the row.
  h.click("workspace-7");
  assert_eq!(h.sent(), vec![SidebarAction::DismissModal]);
}

#[gpui::test]
fn opening_a_modal_hides_the_tooltip(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  h.hover("settings");
  h.cx.executor().advance_clock(TOOLTIP_DELAY);
  h.cx.run_until_parked();
  h.sidebar
    .read_with(&h.cx, |s, _| assert!(s.tooltip_shown("settings")));
  h.update(|s| s.modal = Some(0.4));
  h.sidebar
    .read_with(&h.cx, |s, _| assert!(!s.tooltip_shown("settings")));
}

impl Harness {
  /// Presses at `from`, moves to `to` in steps, and lets go there.
  fn drag(&mut self, from: gpui::Point<gpui::Pixels>, to: gpui::Point<gpui::Pixels>) {
    self.cx.simulate_event(gpui::MouseDownEvent {
      position: from,
      button: MouseButton::Left,
      modifiers: Modifiers::none(),
      click_count: 1,
      first_mouse: false,
    });
    for step in 1..=4 {
      let t = step as f32 / 4.;
      let position = from + (to - from) * t;
      self
        .cx
        .simulate_mouse_move(position, Some(MouseButton::Left), Modifiers::none());
    }
    self.cx.simulate_event(gpui::MouseUpEvent {
      position: to,
      button: MouseButton::Left,
      modifiers: Modifiers::none(),
      click_count: 1,
    });
    self.cx.run_until_parked();
  }

  fn center(&mut self, selector: &'static str) -> gpui::Point<gpui::Pixels> {
    self
      .bounds(selector)
      .unwrap_or_else(|| panic!("{selector} is not drawn"))
      .center()
  }
}

#[test]
fn serializes_drag_actions_for_react() {
  let json = |action| serde_json::to_value(action).expect("json");
  assert_eq!(
    json(SidebarAction::MoveWorkspace { id: 3, onto: None }),
    serde_json::json!({"kind": "moveWorkspace", "id": 3, "onto": null})
  );
  assert_eq!(
    json(SidebarAction::ResizeSidebar { width: 320. }),
    serde_json::json!({"kind": "resizeSidebar", "width": 320.0})
  );
  assert_eq!(
    json(SidebarAction::ResetSidebarWidth),
    serde_json::json!({"kind": "resetSidebarWidth"})
  );
}

#[gpui::test]
fn dropping_a_workspace_on_another_stacks_it_there(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0), row(8, "feat-b", 0)]));
  let (from, to) = (h.center("workspace-8"), h.center("workspace-7"));
  h.drag(from, to);
  assert_eq!(
    h.sent(),
    vec![SidebarAction::MoveWorkspace {
      id: 8,
      onto: Some(7)
    }]
  );
}

#[gpui::test]
fn dropping_a_workspace_on_the_list_moves_it_home(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0), row(8, "feat-b", 1)]));
  let list = h.bounds("workspaces").expect("drawn");
  let below_rows = gpui::point(list.center().x, list.bottom() - px(4.));
  let from = h.center("workspace-8");
  h.drag(from, below_rows);
  assert_eq!(
    h.sent(),
    vec![SidebarAction::MoveWorkspace { id: 8, onto: None }]
  );
}

#[gpui::test]
fn dropping_a_workspace_on_itself_does_nothing(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0)]));
  let at = h.center("workspace-7");
  h.drag(at, at + gpui::point(px(30.), px(2.)));
  assert!(h.sent().is_empty());
}

#[gpui::test]
fn dragging_the_edge_resizes_within_bounds(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  let handle = h.center("resize-handle");
  h.drag(handle, gpui::point(px(350.), handle.y));
  assert_eq!(h.sent(), vec![SidebarAction::ResizeSidebar { width: 350. }]);
  h.sidebar
    .read_with(&h.cx, |s, _| assert_eq!(s.width(), 350.));

  let handle = h.center("resize-handle");
  h.drag(handle, gpui::point(px(900.), handle.y));
  assert_eq!(
    h.sent().last(),
    Some(&SidebarAction::ResizeSidebar { width: 480. })
  );
}

#[gpui::test]
fn double_clicking_the_edge_resets_the_width(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, snapshot(vec![]));
  let handle = h.center("resize-handle");
  h.press(handle, MouseButton::Left, Modifiers::none(), 1);
  h.press(handle, MouseButton::Left, Modifiers::none(), 2);
  assert_eq!(h.sent(), vec![SidebarAction::ResetSidebarWidth]);
}

#[test]
fn serializes_repo_actions_for_react() {
  let json = |action| serde_json::to_value(action).expect("json");
  assert_eq!(
    json(SidebarAction::OpenRepoWorkspace {
      repo_path: "/r".into(),
      id: 3
    }),
    serde_json::json!({"kind": "openRepoWorkspace", "repoPath": "/r", "id": 3})
  );
  assert_eq!(
    json(SidebarAction::RepoWorkspaceMenu {
      repo_path: "/r".into(),
      id: 3
    }),
    serde_json::json!({"kind": "repoWorkspaceMenu", "repoPath": "/r", "id": 3})
  );
  assert_eq!(
    json(SidebarAction::ToggleRepo {
      repo_path: "/r".into()
    }),
    serde_json::json!({"kind": "toggleRepo", "repoPath": "/r"})
  );
}

#[gpui::test]
fn rows_of_another_repository_open_it(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, two_repos());
  h.click("repo1-workspace-7");
  h.click("workspace-7");
  assert_eq!(
    h.sent(),
    vec![
      SidebarAction::OpenRepoWorkspace {
        repo_path: "/src/lib".into(),
        id: 7
      },
      select(7, false, false),
    ]
  );
}

#[gpui::test]
fn rows_of_another_repository_have_their_own_menu(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, two_repos());
  right_click(&mut h, "repo1-workspace-7");
  assert_eq!(
    h.sent(),
    vec![SidebarAction::RepoWorkspaceMenu {
      repo_path: "/src/lib".into(),
      id: 7
    }]
  );
}

#[gpui::test]
fn rows_of_another_repository_have_no_actions(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, two_repos());
  h.hover("repo1-workspace-7");
  assert!(h.bounds("workspace-7-start-agent").is_none());
}

#[gpui::test]
fn group_labels_fold_their_repository(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, two_repos());
  assert!(h.bounds("group-lib").is_some());
  assert!(h.bounds("home-lib").is_some());
  h.click("group-lib");
  assert_eq!(
    h.sent(),
    vec![SidebarAction::ToggleRepo {
      repo_path: "/src/lib".into()
    }]
  );

  h.update(|s| s.repos[1].collapsed = true);
  assert!(h.bounds("repo1-workspace-7").is_none());
  assert!(h.bounds("workspace-7").is_some());
}

#[test]
fn parses_repo_labels_with_defaults() {
  let json = r#"{"path": "/r", "homeRef": null, "homeSelected": false,
    "active": false, "loading": false, "workspaces": [],
    "label": "lib", "missing": true}"#;
  let repo: RepoSection = serde_json::from_str(json).expect("parse");
  assert_eq!(repo.label.as_deref(), Some("lib"));
  assert!(repo.missing && !repo.collapsed);
}

/// Resting on another workspace's row asks for it to be read ahead;
/// passing over it, or resting on the selected row, does not.
#[gpui::test]
fn resting_on_a_workspace_row_reads_it_ahead(cx: &mut TestAppContext) {
  use crate::gpui_shell::screen::prefetch::HOVER_INTENT;
  let mut selected = row(8, "feat-b", 0);
  selected.selected = true;
  let mut h = Harness::new(cx, snapshot(vec![row(7, "feat-a", 0), selected]));
  let prefetches = |h: &Harness| {
    h.sent()
      .into_iter()
      .filter(|a| matches!(a, SidebarAction::PrefetchWorkspace { .. }))
      .collect::<Vec<_>>()
  };
  h.hover("workspace-7");
  h.cx.executor().advance_clock(HOVER_INTENT / 2);
  h.hover("workspace-8");
  h.cx.executor().advance_clock(HOVER_INTENT * 2);
  h.cx.run_until_parked();
  assert!(prefetches(&h).is_empty());
  h.hover("workspace-7");
  h.cx.executor().advance_clock(HOVER_INTENT);
  h.cx.run_until_parked();
  assert_eq!(prefetches(&h), [SidebarAction::PrefetchWorkspace { id: 7 }]);
}
