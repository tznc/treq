//! Tests for the header popovers.

use std::{
  cell::{Cell, RefCell},
  rc::Rc,
  sync::Mutex,
};

use gpui::{point, size, Bounds, Entity, Modifiers, TestAppContext, VisualTestContext};

use super::{
  checks::{check_duration, ci_state},
  data::{ChecksData, Setup, StartingPrompt, Workflow},
  *,
};
use crate::{
  core::PrerequisiteStatus,
  github::{PrCheckEntry, PrCiStatus},
  gpui_shell::{
    native::NativeEvent,
    screen::{
      checklist::ToolsHost,
      workspace_data::{Loaded, Outcome, PrCache, Route},
    },
  },
  jj::JjBranch,
};

#[derive(Default)]
struct FakeHost {
  prompt: Option<StartingPrompt>,
  branches: Vec<&'static str>,
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

impl ToolsHost for FakeHost {
  fn check_prerequisites(&self) -> Vec<PrerequisiteStatus> {
    Vec::new()
  }

  fn open_url(&self, url: &str) {
    self.record(format!("open {url}"));
  }
}

impl crate::gpui_shell::screen::launch_data::LaunchHost for FakeHost {
  fn start_agent_task(
    &self,
    _: &crate::gpui_shell::screen::launch_data::TaskRequest,
  ) -> Result<bool, String> {
    unreachable!()
  }

  fn launch_session(
    &self,
    _: &crate::gpui_shell::screen::launch_data::LaunchRequest,
    _: bool,
  ) -> Result<crate::gpui_shell::screen::launch_data::SessionStart, String> {
    unreachable!()
  }

  fn session_started(&self, _: &crate::gpui_shell::screen::launch_data::SessionStart, _: bool) {
    unreachable!()
  }

  fn resolve_agent(&self, _: &str, _: Option<&str>) -> Option<String> {
    None
  }

  fn pending_session(
    &self,
    _: i64,
  ) -> Option<crate::gpui_shell::screen::launch_data::PendingSession> {
    None
  }

  fn send_assets(&self) -> Vec<crate::gpui_shell::screen::launch_data::SendAsset> {
    Vec::new()
  }

  fn dismiss_send_asset(&self, _: &str) {}

  fn record_review(&self, _: &str, _: Option<i64>, _: &str, _: &str, _: &str) {}
}

impl crate::gpui_shell::screen::WorkspaceHost for FakeHost {
  fn load(&self, _: &Route) -> Loaded {
    Loaded::default()
  }

  fn pr_cache(&self, _: &str, _: &str) -> PrCache {
    PrCache::default()
  }

  fn push(&self, _: &str, _: Option<i64>) -> Outcome {
    unreachable!()
  }

  fn sync(&self, _: &str, _: Option<i64>) -> Outcome {
    unreachable!()
  }

  fn reset_to_remote(&self, _: &str, _: i64) -> Outcome {
    unreachable!()
  }

  fn rebase_home(&self, _: &str, _: &str) -> Outcome {
    unreachable!()
  }

  fn notify(&self, event: NativeEvent) {
    if let NativeEvent::Toast { title, .. } = event {
      self.record(format!("toast {title}"));
    }
  }
}

impl PopoverHost for FakeHost {
  fn starting_prompt(&self, _: &str, _: i64) -> Option<StartingPrompt> {
    self.prompt.clone()
  }

  fn branches(&self, repo_path: &str) -> Result<Vec<JjBranch>, String> {
    self.record(format!("branches {repo_path}"));
    Ok(
      self
        .branches
        .iter()
        .map(|name| JjBranch {
          name: (*name).into(),
          is_current: false,
        })
        .collect(),
    )
  }
}

struct Harness {
  cx: VisualTestContext,
  popover: Entity<Popover>,
  sent: Rc<RefCell<Vec<ScreenAction>>>,
  closed: Rc<Cell<bool>>,
  host: Arc<FakeHost>,
  _closed: gpui::Subscription,
}

impl Harness {
  fn new(cx: &mut TestAppContext, kind: Kind, host: FakeHost) -> Self {
    Self::over(cx, kind, host, None)
  }

  /// A popover over `shell`, which its outside presses go on to.
  fn over(
    cx: &mut TestAppContext,
    kind: Kind,
    host: FakeHost,
    shell: Option<gpui::AnyWindowHandle>,
  ) -> Self {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sink: ActionSink = {
      let sent = sent.clone();
      Rc::new(move |action, _| sent.borrow_mut().push(action))
    };
    cx.update(theme::init);
    let host = Arc::new(host);
    let dyn_host: Arc<dyn PopoverHost> = host.clone();
    let anchor = Bounds::new(point(px(700.), px(10.)), size(px(80.), px(32.)));
    let (popover, cx) =
      cx.add_window_view(|_, cx| Popover::new(kind, anchor, shell, false, dyn_host, sink, cx));
    let mut cx = cx.clone();
    let closed = Rc::new(Cell::new(false));
    let subscription = {
      let closed = closed.clone();
      cx.update(|_, cx| cx.subscribe(&popover, move |_, _: &Closed, _| closed.set(true)))
    };
    Self {
      cx,
      popover,
      sent,
      closed,
      host,
      _closed: subscription,
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

  fn sent(&self) -> Vec<ScreenAction> {
    self.sent.borrow().clone()
  }
}

fn press(id: &str) -> ScreenAction {
  ScreenAction::Press { id: id.into() }
}

fn field(field: &str, value: &str) -> ScreenAction {
  ScreenAction::Field {
    field: field.into(),
    value: value.into(),
  }
}

fn details() -> Kind {
  Kind::Details(DetailsSpec {
    repo_path: "/repo".into(),
    workspace_id: 7,
    branch: "feature/login".into(),
    target: "main".into(),
    created_at: "2024-03-05T14:07:09Z".into(),
  })
}

fn prompt_host() -> FakeHost {
  FakeHost {
    prompt: Some(StartingPrompt {
      id: 12,
      text: "Add a login form".into(),
    }),
    ..Default::default()
  }
}

fn target(branches: Vec<&'static str>) -> (Kind, FakeHost) {
  (
    Kind::Target(TargetSpec {
      repo_path: "/repo".into(),
      current: Some("main".into()),
      branches: None,
    }),
    FakeHost {
      branches,
      ..Default::default()
    },
  )
}

fn workflow(filename: &str, latest: Option<&str>) -> Workflow {
  Workflow {
    filename: filename.into(),
    name: filename.trim_end_matches(".yml").into(),
    latest: latest.map(|status| (3, status.to_string())),
  }
}

fn checks_input() -> ChecksInput {
  ChecksInput {
    data: ChecksData {
      workflows: vec![
        workflow("lint.yml", Some("passed")),
        workflow("test.yml", None),
      ],
      trusted: true,
      setup: None,
    },
    ci: None,
    running: Vec::new(),
    running_workflows: Vec::new(),
    setup_rerunning: false,
    logs: true,
  }
}

fn checks(input: ChecksInput) -> Kind {
  Kind::Checks(input)
}

#[test]
fn check_durations_read_like_the_header_list() {
  assert_eq!(check_duration(9), "9s");
  assert_eq!(check_duration(61), "1m 1s");
  assert_eq!(check_duration(3725), "1h 2m 5s");
}

#[test]
fn ci_buckets_fold_into_three_states() {
  assert_eq!(ci_state("pass"), "success");
  assert_eq!(ci_state("skipping"), "success");
  assert_eq!(ci_state("fail"), "failure");
  assert_eq!(ci_state("cancel"), "failure");
  assert_eq!(ci_state("pending"), "pending");
}

#[test]
fn checks_roll_up_into_one_state() {
  let mut data = ChecksData::default();
  assert_eq!(data.rollup(&[]), ("none", 0, 0));
  data.workflows = vec![workflow("a.yml", Some("passed")), workflow("b.yml", None)];
  assert_eq!(data.rollup(&[]), ("not-run", 1, 2));
  data.workflows[1].latest = Some((4, "passed".into()));
  assert_eq!(data.rollup(&[]), ("passed", 2, 2));
  data.workflows[1].latest = Some((4, "failed".into()));
  assert_eq!(data.rollup(&[]), ("failed", 1, 2));
  assert_eq!(data.rollup(&["b.yml".to_string()]), ("running", 1, 2));
}

#[test]
fn setup_holds_checks_back_until_it_finishes() {
  let mut data = ChecksData {
    trusted: true,
    ..Default::default()
  };
  assert!(data.can_run());
  data.setup = Some(Setup {
    configured: true,
    status: "running".into(),
  });
  assert!(!data.can_run());
  data.setup = Some(Setup {
    configured: true,
    status: "passed".into(),
  });
  assert!(data.can_run());
  data.trusted = false;
  assert!(!data.can_run());
}

#[gpui::test]
fn details_list_the_workspace_and_its_starting_prompt(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, details(), prompt_host());
  assert!(h.drawn("workspace-details-popover"));
  assert!(h.drawn("details-prompt"));
  assert!(h.drawn("details-view-full"));
  assert!(!h.drawn("details-no-prompt"));
  let shown = h.popover.read_with(&h.cx, |p, _| match &p.content {
    Content::Details(state) => state.prompt().cloned(),
    _ => None,
  });
  assert_eq!(shown.map(|p| p.text), Some("Add a login form".to_string()));
}

#[gpui::test]
fn details_say_when_no_prompt_was_recorded(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, details(), FakeHost::default());
  assert!(h.drawn("details-no-prompt"));
  assert!(!h.drawn("details-copy"));
}

#[gpui::test]
fn details_copy_the_prompt_and_toast(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, details(), prompt_host());
  h.click("details-copy");
  assert_eq!(
    h.cx.read_from_clipboard().and_then(|item| item.text()),
    Some("Add a login form".to_string())
  );
  assert!(h.host.calls().contains(&"toast Prompt copied".to_string()));
  assert!(!h.closed.get());
}

#[gpui::test]
fn details_hand_the_full_prompt_to_react(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, details(), prompt_host());
  h.click("details-view-full");
  assert_eq!(
    h.sent(),
    vec![ScreenAction::Field {
      field: "workspace.viewFullPrompt".into(),
      value: 12.into()
    }]
  );
  assert!(h.closed.get());
}

#[gpui::test]
fn popovers_close_from_an_outside_press_and_escape(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, details(), prompt_host());
  h.cx.run_until_parked();
  h.cx
    .simulate_click(point(px(10.), px(600.)), Modifiers::none());
  h.cx.run_until_parked();
  assert!(h.closed.get());

  let mut h = Harness::new(cx, details(), prompt_host());
  h.keys("escape");
  assert!(h.closed.get());
}

/// The shell window under the backdrop: records the presses it gets.
struct Shell {
  presses: Rc<RefCell<Vec<gpui::Point<Pixels>>>>,
}

impl Render for Shell {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    let presses = self.presses.clone();
    div()
      .id("shell")
      .size_full()
      .on_click(move |event, _, _| presses.borrow_mut().push(event.position()))
  }
}

fn shell(
  cx: &mut TestAppContext,
) -> (gpui::AnyWindowHandle, Rc<RefCell<Vec<gpui::Point<Pixels>>>>) {
  let presses = Rc::new(RefCell::new(Vec::new()));
  let window = cx.add_window({
    let presses = presses.clone();
    |_, _| Shell { presses }
  });
  cx.run_until_parked();
  (window.into(), presses)
}

#[gpui::test]
fn an_outside_press_goes_on_to_the_shell_window(cx: &mut TestAppContext) {
  let (window, presses) = shell(cx);
  let mut h = Harness::over(cx, details(), prompt_host(), Some(window));
  h.cx.run_until_parked();
  h.cx
    .simulate_click(point(px(10.), px(600.)), Modifiers::none());
  h.cx.run_until_parked();
  assert!(h.closed.get());
  assert_eq!(*presses.borrow(), [point(px(10.), px(600.))]);
}

#[gpui::test]
fn a_press_on_the_popover_button_only_closes_it(cx: &mut TestAppContext) {
  let (window, presses) = shell(cx);
  let mut h = Harness::over(cx, details(), prompt_host(), Some(window));
  h.cx.run_until_parked();
  // The Harness's anchor.
  h.cx
    .simulate_click(point(px(720.), px(20.)), Modifiers::none());
  h.cx.run_until_parked();
  assert!(h.closed.get());
  assert!(presses.borrow().is_empty());
}

#[gpui::test]
fn a_press_inside_the_popover_keeps_it_open(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, details(), prompt_host());
  h.click("workspace-details-popover");
  assert!(!h.closed.get());
}

#[gpui::test]
fn target_lists_the_repository_branches(cx: &mut TestAppContext) {
  let (kind, host) = target(vec!["main", "develop", "release"]);
  let mut h = Harness::new(cx, kind, host);
  assert!(h.drawn("target-branch-popover"));
  assert!(h.drawn("branch-option-0"));
  assert!(h.drawn("branch-option-2"));
  assert!(!h.drawn("branch-option-3"));
  assert!(h.host.calls().contains(&"branches /repo".to_string()));
}

/// Branches read ahead while the pointer rested on the button list in the
/// first frame, with nothing loading, and are read again behind.
#[gpui::test]
fn target_lists_branches_read_ahead_at_once(cx: &mut TestAppContext) {
  let (mut kind, host) = target(vec!["main", "develop"]);
  if let Kind::Target(spec) = &mut kind {
    spec.branches = Some(vec!["main".into(), "develop".into()]);
  }
  let mut h = Harness::new(cx, kind, host);
  h.cx.update(|window, cx| {
    window.refresh();
    window.draw(cx).clear(cx);
  });
  assert!(h.cx.debug_bounds("branch-option-1").is_some());
  assert!(h.cx.debug_bounds("target-loading").is_none());
  assert!(h.drawn("branch-option-1"));
  assert!(h.host.calls().contains(&"branches /repo".to_string()));
}

#[gpui::test]
fn target_filters_with_the_search_field(cx: &mut TestAppContext) {
  let (kind, host) = target(vec!["main", "develop", "release"]);
  let mut h = Harness::new(cx, kind, host);
  h.cx.run_until_parked();
  h.cx.simulate_input("rel");
  h.cx.run_until_parked();
  assert!(h.drawn("branch-option-2"));
  assert!(!h.drawn("branch-option-0"));
  assert!(!h.drawn("branch-option-1"));
  assert_eq!(
    h.popover.read_with(&h.cx, |p, _| match &p.content {
      Content::Target(state) => state.selected().map(str::to_string),
      _ => None,
    }),
    Some("release".to_string())
  );
}

#[gpui::test]
fn target_says_when_no_branch_matches(cx: &mut TestAppContext) {
  let (kind, host) = target(vec!["main"]);
  let mut h = Harness::new(cx, kind, host);
  h.cx.run_until_parked();
  h.cx.simulate_input("zzz");
  h.cx.run_until_parked();
  assert!(h.drawn("target-empty"));
}

#[gpui::test]
fn target_picks_with_the_keyboard(cx: &mut TestAppContext) {
  let (kind, host) = target(vec!["main", "develop", "release"]);
  let mut h = Harness::new(cx, kind, host);
  h.cx.run_until_parked();
  h.keys("down");
  h.keys("down");
  h.keys("up");
  h.keys("enter");
  assert_eq!(h.sent(), vec![field("workspace.target", "develop")]);
  assert!(h.closed.get());
}

#[gpui::test]
fn target_picks_with_a_click(cx: &mut TestAppContext) {
  let (kind, host) = target(vec!["main", "develop"]);
  let mut h = Harness::new(cx, kind, host);
  h.click("branch-option-1");
  assert_eq!(h.sent(), vec![field("workspace.target", "develop")]);
  assert!(h.closed.get());
}

#[gpui::test]
fn checks_run_one_workflow_or_all(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, checks(checks_input()), FakeHost::default());
  assert!(h.drawn("checks-popover"));
  h.click("checks-run-1");
  h.click("checks-run-all");
  assert_eq!(
    h.sent(),
    vec![
      field("workspace.checksRun", "test.yml"),
      press("workspace.checksRunAll")
    ]
  );
  assert!(!h.closed.get());
}

#[gpui::test]
fn checks_hold_back_until_the_repository_is_trusted(cx: &mut TestAppContext) {
  let mut input = checks_input();
  input.data.trusted = false;
  let mut h = Harness::new(cx, checks(input), FakeHost::default());
  h.click("checks-run-1");
  h.click("checks-run-all");
  assert!(h.sent().is_empty());
  h.click("checks-trust");
  assert_eq!(h.sent(), vec![press("workspace.checksTrust")]);
}

#[gpui::test]
fn checks_disable_a_workflow_that_is_running(cx: &mut TestAppContext) {
  let mut input = checks_input();
  input.running = vec!["test.yml".into()];
  input.running_workflows = vec!["test.yml".into()];
  let mut h = Harness::new(cx, checks(input), FakeHost::default());
  h.click("checks-run-1");
  h.click("checks-run-all");
  assert!(h.sent().is_empty());
}

#[gpui::test]
fn checks_rerun_the_setup_script(cx: &mut TestAppContext) {
  let mut input = checks_input();
  input.data.setup = Some(Setup {
    configured: true,
    status: "failed".into(),
  });
  let mut h = Harness::new(cx, checks(input), FakeHost::default());
  h.click("checks-setup");
  assert_eq!(h.sent(), vec![press("workspace.checksRerunSetup")]);
}

#[gpui::test]
fn checks_close_to_show_the_logs(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, checks(checks_input()), FakeHost::default());
  h.click("checks-view-logs");
  assert_eq!(h.sent(), vec![press("workspace.checksLogs")]);
  assert!(h.closed.get());

  let mut input = checks_input();
  input.logs = false;
  let mut h = Harness::new(cx, checks(input), FakeHost::default());
  assert!(!h.drawn("checks-view-logs"));
}

#[gpui::test]
fn checks_list_pull_request_checks_failed_first(cx: &mut TestAppContext) {
  let mut input = checks_input();
  let check = |name: &str, bucket: &str, link: &str| PrCheckEntry {
    name: name.into(),
    bucket: bucket.into(),
    link: link.into(),
    duration_secs: Some(65),
  };
  input.ci = Some(PrCiStatus {
    state: "failure".into(),
    total: 2,
    passed: 1,
    failed: 1,
    pending: 0,
    checks: vec![
      check("build", "pass", ""),
      check("e2e", "fail", "https://github.com/tznc/treq/actions/runs/1"),
    ],
  });
  let mut h = Harness::new(cx, checks(input), FakeHost::default());
  assert!(h.drawn("checks-github"));
  let first = h.cx.debug_bounds("ci-check-0").expect("first");
  let second = h.cx.debug_bounds("ci-check-1").expect("second");
  assert!(first.top() < second.top());
  h.click("ci-check-0");
  assert!(h
    .host
    .calls()
    .contains(&"open https://github.com/tznc/treq/actions/runs/1".to_string()));
}

#[gpui::test]
fn checks_list_the_wip_status_last_as_inactive(cx: &mut TestAppContext) {
  let mut input = checks_input();
  let check = |name: &str, bucket: &str| PrCheckEntry {
    name: name.into(),
    bucket: bucket.into(),
    link: String::new(),
    duration_secs: None,
  };
  input.ci = Some(PrCiStatus {
    state: "success".into(),
    total: 1,
    passed: 1,
    failed: 0,
    pending: 0,
    checks: vec![
      check("WIP", crate::github::INACTIVE),
      check("build", "pass"),
    ],
  });
  let mut h = Harness::new(cx, checks(input), FakeHost::default());
  // Below the counted checks, labelled instead of spinning.
  assert!(h.drawn("ci-inactive-1"));
  assert!(!h.drawn("ci-inactive-0"));
}

#[gpui::test]
fn checks_follow_the_workspace_view(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, checks(checks_input()), FakeHost::default());
  assert!(!h.drawn("checks-workflow-2"));
  let mut input = checks_input();
  input.data.workflows.push(workflow("deploy.yml", None));
  h.popover
    .update(&mut h.cx, |popover, cx| popover.set_checks(input, cx));
  assert!(h.drawn("checks-workflow-2"));
}
