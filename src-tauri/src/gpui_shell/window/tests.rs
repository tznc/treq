//! The window controller against a fake backend: what it reads, when, and
//! what it hands the shell to draw.

use std::{
  cell::RefCell,
  collections::HashMap,
  rc::Rc,
  sync::{Arc, Mutex},
};

use futures::channel::mpsc::UnboundedSender;
use gpui::{prelude::*, Entity, FocusHandle, Role, TestAppContext};
use gpui_testing_library::prelude::*;
use tauri_plugin_gpui::gpui;

use super::{
  actions::{Menu, Shortcut},
  controller::{Presentation, WindowController},
  data::{Flags, BACKGROUND_EVERY, POLL, SESSIONS_POLL},
  host::{FileWatch, WindowHost},
  remote::{RemoteHost, UserManagedValues},
  tree::tests::workspace,
};
use crate::{
  core::{
    agent_launch::LaunchRequest,
    cloud::ManagedEndpointEvent,
    remote::{RepositoryInspection, TreqCommandRequest},
    remote_control_plane::SshEndpoint,
    remote_records::{self, RestoreFailure, SavedRepository},
    repo::RepoBranch,
    supporting_repos::SupportingRepo,
    workspaces::WorkspaceSummary,
    SingleRebaseResult, WorkspaceSidebarStatus,
  },
  github::{GitRemoteInfo, PrInfo},
  gpui_shell::{
    confirm::Question,
    native::{NativeEvent, ToastTone},
    remote_repo::{Mutation, RemoteRepository},
    screen::{account_data::CloudAction, ScreenAction, View},
    sidebar::SidebarAction,
  },
  local_db::{Session, Workspace},
};

mod fuzz;

#[derive(Default)]
struct FakeHost {
  workspaces: Mutex<HashMap<String, Vec<Workspace>>>,
  supporting: Mutex<Vec<SupportingRepo>>,
  settings: Mutex<HashMap<String, String>>,
  /// The folder `pick_folder` answers with.
  folder: Option<String>,
  /// The remote repository `last_remote` restores.
  last_remote: Option<SavedRepository>,
  /// What a probe finds: a repository, unless set.
  no_repo: bool,
  /// A local folder that is neither a Git nor a jj repository.
  not_a_repo: bool,
  /// The next workspaces read panics, as a backend read before the app's
  /// state is managed did.
  panic_once: std::sync::atomic::AtomicBool,
  /// The next branch read fails, as one before `prepare_repo`'s jj init
  /// does.
  no_branch_once: std::sync::atomic::AtomicBool,
  /// The workspaces come from a database the window can list at once.
  from_db: bool,
  /// What `summaries` answers, when the host reads in one round trip.
  summaries: Mutex<Option<Vec<WorkspaceSummary>>>,
  /// Every call, in order.
  calls: Mutex<Vec<String>>,
  /// The watched checkouts, each with the sender that reports an edit.
  watches: Mutex<Vec<(String, UnboundedSender<()>)>>,
}

impl FakeHost {
  fn on(repo: &str, workspaces: Vec<Workspace>) -> Self {
    let host = Self::default();
    host
      .workspaces
      .lock()
      .unwrap()
      .insert(repo.into(), workspaces);
    host
  }

  fn record(&self, call: impl Into<String>) {
    self.calls.lock().unwrap().push(call.into());
  }

  fn calls(&self) -> Vec<String> {
    self.calls.lock().unwrap().clone()
  }

  fn count(&self, call: &str) -> usize {
    self.calls().iter().filter(|c| *c == call).count()
  }
}

impl WindowHost for FakeHost {
  fn workspaces(&self, repo: &str) -> Result<Vec<Workspace>, String> {
    self.record(format!("workspaces {repo}"));
    if self
      .panic_once
      .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
      panic!("state() called before manage()");
    }
    Ok(
      self
        .workspaces
        .lock()
        .unwrap()
        .get(repo)
        .cloned()
        .unwrap_or_default(),
    )
  }

  fn listed_workspaces(&self, repo: &str) -> Option<Vec<Workspace>> {
    if !self.from_db {
      return None;
    }
    self.record(format!("listed {repo}"));
    self.workspaces.lock().unwrap().get(repo).cloned()
  }

  fn statuses(&self, _: &str) -> Result<Vec<WorkspaceSidebarStatus>, String> {
    Ok(Vec::new())
  }

  fn has_changes(&self, _: &str, id: i64) -> Option<bool> {
    self.record(format!("detail {id}"));
    Some(false)
  }

  fn summaries(&self, repo: &str) -> Option<Vec<WorkspaceSummary>> {
    self.record(format!("summaries {repo}"));
    self.summaries.lock().unwrap().clone()
  }

  fn sessions(&self, repo: &str) -> Vec<Session> {
    self.record(format!("sessions {repo}"));
    Vec::new()
  }

  fn branch(&self, _: &str) -> Option<RepoBranch> {
    if self
      .no_branch_once
      .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
      return None;
    }
    Some(RepoBranch {
      current_branch: Some("main".into()),
      display_ref: "main".into(),
      is_detached: false,
    })
  }

  fn default_branch(&self, _: &str) -> Option<String> {
    Some("main".into())
  }

  fn remote(&self, _: &str) -> Option<GitRemoteInfo> {
    None
  }

  fn prs(&self, _: &str) -> HashMap<String, Option<PrInfo>> {
    HashMap::new()
  }

  fn supporting(&self, _: &str) -> Vec<SupportingRepo> {
    self.supporting.lock().unwrap().clone()
  }

  fn setting(&self, key: &str) -> Option<String> {
    self.settings.lock().unwrap().get(key).cloned()
  }

  fn flags(&self) -> Flags {
    Flags::default()
  }

  fn save_setting(&self, key: &str, value: &str) {
    self.record(format!("save {key}={value}"));
    self
      .settings
      .lock()
      .unwrap()
      .insert(key.into(), value.into());
  }

  fn is_repo(&self, _: &str) -> bool {
    !self.not_a_repo
  }

  fn init_repo(&self, path: &str) -> Result<(), String> {
    self.record(format!("init {path}"));
    Ok(())
  }

  fn prepare_repo(&self, repo: &str) {
    self.record(format!("prepare {repo}"));
  }

  fn set_supporting_paths(&self, paths: Vec<String>) {
    self.record(format!("supporting {paths:?}"));
  }

  fn pick_folder(&self) -> Option<String> {
    self.folder.clone()
  }

  fn open_repo_at(&self, path: &str) {
    self.record(format!("open {path}"));
  }

  fn open_in_new_window(&self, path: &str) {
    self.record(format!("new window {path}"));
  }

  fn add_supporting(&self, _: &str, path: &str) -> Result<String, String> {
    self.record(format!("add {path}"));
    self.supporting.lock().unwrap().push(SupportingRepo {
      path: path.into(),
      exists: true,
    });
    Ok(path.into())
  }

  fn remove_supporting(&self, _: &str, path: &str) -> Result<(), String> {
    self.record(format!("remove {path}"));
    self.supporting.lock().unwrap().retain(|r| r.path != path);
    Ok(())
  }

  fn launch(&self, request: &LaunchRequest, select: bool) -> Result<(), String> {
    self.record(format!(
      "launch {} {:?} {select}",
      request.repo_path, request.workspace_id
    ));
    Ok(())
  }

  fn toast(&self, _: ToastTone, title: &str, _: &str) {
    self.record(format!("toast {title}"));
  }

  fn open_url(&self, url: &str) {
    self.record(format!("url {url}"));
  }

  fn open_log_dir(&self) {
    self.record("log dir");
  }

  fn watch_files(&self, path: &str) -> Option<FileWatch> {
    let (tx, changes) = futures::channel::mpsc::unbounded();
    self.watches.lock().unwrap().push((path.into(), tx));
    Some(FileWatch {
      changes,
      _watcher: Box::new(()),
    })
  }

  fn force_rebase(&self, _: &str, id: i64, branch: &str) -> Result<SingleRebaseResult, String> {
    self.record(format!("rebase {id} {branch}"));
    Ok(SingleRebaseResult {
      rebased: true,
      success: true,
      message: String::new(),
      bookmark_conflicts: Vec::new(),
    })
  }

  fn factory_reset(&self) {
    self.record("factory reset");
  }

  fn dismiss_send_asset(&self, id: &str) {
    self.record(format!("dismiss {id}"));
  }

  fn detect_editors(&self) {
    self.record("editors");
  }
}

fn endpoint() -> SshEndpoint {
  crate::gpui_shell::remote_repo::tests::repository()
    .endpoint
    .unwrap()
}

fn inspection(path: &str) -> RepositoryInspection {
  serde_json::from_value(serde_json::json!({
    "root": path,
    "repository_type": "jj",
    "current_branch": "main",
    "default_branch": "main",
    "current_change_id": "abc",
    "current_commit_id": "def",
    "descriptor": {
      "id": path,
      "location": {"type": "ssh", "host": "devbox.example", "path": path},
      "display_name": "app",
    },
  }))
  .unwrap()
}

fn saved(endpoint_id: &str, generation: u64, path: &str) -> SavedRepository {
  let canonical = remote_records::canonicalize_remote_path(path);
  SavedRepository {
    id: remote_records::repository_id(endpoint_id, generation, &canonical),
    endpoint_id: endpoint_id.into(),
    endpoint_generation: generation,
    display_name: canonical.rsplit('/').next().unwrap_or_default().into(),
    canonical_remote_path: canonical,
    last_successful_trust_validation: None,
  }
}

impl RemoteHost for FakeHost {
  fn saved_repositories(&self, endpoint_id: &str, generation: u64) -> Vec<SavedRepository> {
    self.record(format!("saved {endpoint_id} {generation}"));
    vec![saved(endpoint_id, generation, "/srv/app")]
  }

  fn register_user_managed(&self, values: &UserManagedValues) -> SshEndpoint {
    self.record(format!("register {}", values.hostname));
    endpoint()
  }

  fn remote_read(
    &self,
    _: &SshEndpoint,
    request: TreqCommandRequest,
  ) -> Result<serde_json::Value, String> {
    self.record(format!("read {}", request.kind_name()));
    match request {
      TreqCommandRequest::ProbeRepo { repo } => Ok(serde_json::json!({
        "host": "devbox.example", "path": repo, "exists": !self.no_repo,
        "is_repo": !self.no_repo, "needs_clone": self.no_repo,
      })),
      TreqCommandRequest::InspectRepository { repo } => {
        Ok(serde_json::to_value(inspection(&repo)).unwrap())
      }
      TreqCommandRequest::ListDirectories { path } => {
        let path = if path == "~" {
          "/home/me".to_string()
        } else {
          path
        };
        let parent = path
          .rsplit_once('/')
          .map(|(p, _)| if p.is_empty() { "/" } else { p });
        Ok(serde_json::json!({
          "path": path, "parent": parent, "is_repo": path == "/srv/app" && !self.no_repo,
          "entries": [
            {"name": ".cache", "path": format!("{path}/.cache"), "hidden": true, "is_repo": false},
            {"name": "app", "path": format!("{path}/app"), "hidden": false, "is_repo": true},
          ],
        }))
      }
      _ => Err("unexpected read".into()),
    }
  }

  fn remote_change(
    &self,
    _: &SshEndpoint,
    request: TreqCommandRequest,
  ) -> Result<Mutation, String> {
    self.record(format!("change {}", request.kind_name()));
    Ok(Mutation::Applied(serde_json::Value::Null))
  }

  fn save_remote_repository(
    &self,
    endpoint_id: &str,
    generation: u64,
    path: &str,
  ) -> Result<SavedRepository, String> {
    self.record(format!("save remote {path}"));
    Ok(saved(endpoint_id, generation, path))
  }

  fn restore_remote(
    &self,
    descriptor: &SavedRepository,
    _: &SshEndpoint,
    _: u64,
    _: bool,
  ) -> Result<(SavedRepository, RepositoryInspection), RestoreFailure> {
    self.record(format!("restore {}", descriptor.canonical_remote_path));
    if self.no_repo {
      return Err(RestoreFailure::ReconnectFailed);
    }
    Ok((
      descriptor.clone(),
      inspection(&descriptor.canonical_remote_path),
    ))
  }

  fn last_remote(&self) -> Option<(SavedRepository, SshEndpoint, u64)> {
    let descriptor = self.last_remote.clone()?;
    let generation = descriptor.endpoint_generation;
    Some((descriptor, endpoint(), generation))
  }

  fn saved_remote(&self, id: &str) -> Option<(SavedRepository, SshEndpoint, u64)> {
    self
      .last_remote()
      .filter(|(descriptor, ..)| descriptor.id == id)
  }

  fn machine_endpoint(&self, id: &str) -> Option<(SshEndpoint, u64)> {
    Some((endpoint(), 0)).filter(|(e, _)| e.id == id)
  }

  fn cloud_action(&self, action: CloudAction) {
    self.record(format!("cloud {action:?}"));
  }

  fn show_remote(&self, repository: Option<RemoteRepository>) {
    let path = repository.map_or("none".to_string(), |r| r.canonical_path);
    self.record(format!("show {path}"));
  }
}

/// A controller and what it presented.
struct Window {
  controller: Entity<WindowController>,
  shown: Rc<RefCell<Vec<Presentation>>>,
}

impl Window {
  fn open(cx: &mut TestAppContext, host: Arc<FakeHost>, repo: Option<&str>) -> Self {
    let shown = Rc::new(RefCell::new(Vec::new()));
    let s = shown.clone();
    let repo = repo.map(String::from);
    let controller = cx.update(|cx| {
      cx.new(|cx| {
        WindowController::new(
          host,
          repo,
          Rc::new(move |p, _| s.borrow_mut().push(p)),
          None,
          cx,
        )
      })
    });
    cx.run_until_parked();
    Self { controller, shown }
  }

  fn press(&self, cx: &mut TestAppContext, id: &str) -> bool {
    self.screen(cx, ScreenAction::Press { id: id.into() })
  }

  fn field(&self, cx: &mut TestAppContext, field: &str, value: serde_json::Value) -> bool {
    self.screen(
      cx,
      ScreenAction::Field {
        field: field.into(),
        value,
      },
    )
  }

  fn screen_snapshot(&self) -> crate::gpui_shell::ScreenSnapshot {
    self.last().screen.expect("no screen")
  }

  fn last(&self) -> Presentation {
    self.shown.borrow().last().cloned().expect("nothing shown")
  }

  fn sidebar_ids(&self) -> Vec<i64> {
    let sidebar = self.last().sidebar.expect("no sidebar");
    sidebar.repos[0].workspaces.iter().map(|w| w.id).collect()
  }

  fn route_workspace(&self) -> Option<i64> {
    match self.last().screen.expect("no screen").view {
      View::Workspace(route) => route.workspace_id,
      _ => panic!("not the workspace view"),
    }
  }

  fn sidebar(&self, cx: &mut TestAppContext, action: SidebarAction) -> bool {
    let handled = self
      .controller
      .update(cx, |c, cx| c.sidebar_action(&action, cx));
    cx.run_until_parked();
    handled
  }

  fn screen(&self, cx: &mut TestAppContext, action: ScreenAction) -> bool {
    let handled = self
      .controller
      .update(cx, |c, cx| c.screen_action(&action, cx));
    cx.run_until_parked();
    handled
  }
}

/// The window's confirmation, drawn as the shell draws it and answering
/// the controller.
struct ConfirmView {
  controller: Entity<WindowController>,
  question: Question,
  focus: FocusHandle,
}

impl gpui::Render for ConfirmView {
  fn render(&mut self, window: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
    let controller = self.controller.clone();
    let p = crate::gpui_shell::theme::palette(false);
    gpui::div().size_full().child(self.question.confirm_dialog(
      &self.focus,
      window,
      &p,
      move |yes, _, cx| {
        controller.update(cx, |c, cx| c.answer(yes, cx));
      },
    ))
  }
}

impl Window {
  /// The confirmation on screen: its question.
  fn asked(&self) -> Option<String> {
    self.last().confirm.map(|c| c.question)
  }

  /// Clicks `button` in the confirmation on screen, found by role and
  /// name, as the user would.
  fn click_in_confirm(&self, cx: &mut TestAppContext, button: &str) {
    let question = self.last().confirm.expect("no confirmation shown");
    cx.update(crate::gpui_shell::theme::init);
    let controller = self.controller.clone();
    let (_, vcx) = cx.add_window_view(|_, cx| ConfirmView {
      controller,
      focus: cx.focus_handle(),
      question: question.clone(),
    });
    vcx.run_until_parked();
    let handle = vcx.update(|window, _| window.window_handle());
    let screen = gpui_testing_library::Screen::new(vcx, handle);
    screen.redraw();
    assert!(vcx.debug_bounds("confirm").is_some(), "the dialog is drawn");
    screen
      .user()
      .click(&screen.get_by_role(by::role(Role::Button).name(button)));
    cx.run_until_parked();
  }
}

fn repo_host() -> Arc<FakeHost> {
  Arc::new(FakeHost::on(
    "/repo",
    vec![
      workspace(1, "a", Some("main")),
      workspace(2, "b", Some("a")),
    ],
  ))
}

#[gpui::test]
fn opens_on_its_repository_and_draws_it(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  let calls = host.calls();
  assert!(calls.contains(&"prepare /repo".to_string()));
  assert!(calls.contains(&"editors".to_string()));
  assert!(calls.contains(&"supporting []".to_string()));
  assert_eq!(window.sidebar_ids(), [1, 2]);
  assert_eq!(window.route_workspace(), None, "the home view");
  assert!(window.last().modal.is_none());
}

#[gpui::test]
fn draws_the_rows_before_the_branch_and_the_jj_reads(cx: &mut TestAppContext) {
  let mut host = FakeHost::on("/repo", vec![workspace(1, "a", Some("main"))]);
  host.from_db = true;
  let host = Arc::new(host);
  let window = Window::open(cx, host.clone(), Some("/repo"));
  let first_rows = window
    .shown
    .borrow()
    .iter()
    .position(|p| {
      p.sidebar
        .as_ref()
        .is_some_and(|s| !s.repos[0].workspaces.is_empty())
    })
    .expect("rows are drawn");
  let rows_without_branch = window.shown.borrow()[first_rows]
    .sidebar
    .as_ref()
    .is_some_and(|s| s.repos[0].home_ref.is_none());
  assert!(rows_without_branch, "the rows come before the branch");
  assert_eq!(window.sidebar_ids(), [1]);
  assert_eq!(
    window.last().sidebar.unwrap().repos[0].home_ref.as_deref(),
    Some("main")
  );
  let calls = host.calls();
  let at = |call: &str| calls.iter().position(|c| c == call).expect(call);
  assert!(at("listed /repo") < at("detail 1"));
  assert_eq!(host.count("workspaces /repo"), 1, "listed once more, whole");
}

#[gpui::test]
fn draws_the_saved_theme_in_the_first_presentation(cx: &mut TestAppContext) {
  let host = repo_host();
  host
    .settings
    .lock()
    .unwrap()
    .insert("theme".into(), "dark".into());
  let window = Window::open(cx, host, Some("/repo"));
  assert!(window.shown.borrow().iter().all(|p| p.dark), "never light");
}

#[gpui::test]
fn shows_onboarding_without_a_repository(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let window = Window::open(cx, host.clone(), None);
  let shown = window.last();
  assert!(shown.sidebar.is_none());
  assert!(matches!(shown.screen.unwrap().view, View::Onboarding(_)));
  assert_eq!(host.count("workspaces /repo"), 0);
}

#[gpui::test]
fn a_folder_that_is_not_a_repository_shows_onboarding_and_offers_to_open_it(
  cx: &mut TestAppContext,
) {
  let host = Arc::new(FakeHost {
    not_a_repo: true,
    ..FakeHost::default()
  });
  let window = Window::open(cx, host.clone(), Some("/plain"));
  assert!(matches!(
    window.last().screen.unwrap().view,
    View::Onboarding(_)
  ));
  assert!(host
    .calls()
    .contains(&"save last_opened_repo_path=".to_string()));
  assert_eq!(host.count("prepare /plain"), 0, "nothing set up in it");
  assert_eq!(host.count("workspaces /plain"), 0, "nothing read from it");
  // Opening it asks to initialize one first.
  assert_eq!(
    window.asked().as_deref(),
    Some("Initialize a Git repository here?")
  );
  assert_eq!(host.count("open /plain"), 0, "not before the answer");
  window.click_in_confirm(cx, "Initialize");
  let calls = host.calls();
  let init = calls.iter().position(|c| c == "init /plain");
  let open = calls.iter().position(|c| c == "open /plain");
  assert!(init.is_some() && init < open, "{calls:?}");
  assert_eq!(window.asked(), None);
}

#[gpui::test]
fn declining_to_initialize_a_folder_leaves_it_alone(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    not_a_repo: true,
    ..FakeHost::default()
  });
  let window = Window::open(cx, host.clone(), Some("/plain"));
  window.click_in_confirm(cx, "Cancel");
  assert_eq!(host.count("init /plain"), 0);
  assert_eq!(host.count("open /plain"), 0);
  assert_eq!(host.count("toast Not a Git Repository"), 1);
  assert_eq!(window.asked(), None);
}

#[gpui::test]
fn a_folder_opened_in_a_new_window_is_initialized_once_confirmed(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    not_a_repo: true,
    folder: Some("/plain".into()),
    ..FakeHost::default()
  });
  let window = Window::open(cx, host.clone(), None);
  window.controller.update(cx, |c, cx| {
    c.overlay_action(
      &ScreenAction::Press {
        id: "palette.open-repository-new-window".into(),
      },
      cx,
    )
  });
  cx.run_until_parked();
  assert_eq!(
    window.asked().as_deref(),
    Some("Initialize a Git repository here?")
  );
  assert_eq!(host.count("new window /plain"), 0);
  window.click_in_confirm(cx, "Initialize");
  assert_eq!(host.count("init /plain"), 1);
  assert_eq!(host.count("new window /plain"), 1);
}

#[gpui::test]
fn follows_the_theme_zoom_and_sidebar_width_settings(cx: &mut TestAppContext) {
  let host = repo_host();
  host.settings.lock().unwrap().extend([
    ("theme".into(), "dark".into()),
    ("ui_zoom".into(), "120".into()),
  ]);
  let window = Window::open(cx, host.clone(), Some("/repo"));
  assert!(window.last().dark);
  assert_eq!(window.last().zoom, 120);
  window
    .controller
    .update(cx, |c, cx| c.shortcut(Shortcut::ZoomIn, cx));
  cx.run_until_parked();
  assert_eq!(window.last().zoom, 125);
  assert!(host.calls().contains(&"save ui_zoom=125".to_string()));
  window.sidebar(cx, SidebarAction::ResizeSidebar { width: 333. });
  assert_eq!(window.last().sidebar.unwrap().width, 333.);
  assert!(host
    .calls()
    .contains(&"save workspace_sidebar_width=333".to_string()));
}

#[gpui::test]
fn a_sidebar_width_that_is_not_a_number_keeps_the_sidebar_drawable(cx: &mut TestAppContext) {
  let host = repo_host();
  host
    .settings
    .lock()
    .unwrap()
    .insert("workspace_sidebar_width".into(), "NaN".into());
  let window = Window::open(cx, host.clone(), Some("/repo"));
  assert_eq!(window.last().sidebar.unwrap().width, 280.);
  window.sidebar(cx, SidebarAction::ResizeSidebar { width: f32::NAN });
  assert_eq!(window.last().sidebar.unwrap().width, 280.);
  assert!(host
    .calls()
    .contains(&"save workspace_sidebar_width=280".to_string()));
}

#[gpui::test]
fn follows_the_system_appearance_on_the_system_theme(cx: &mut TestAppContext) {
  let window = Window::open(cx, repo_host(), Some("/repo"));
  assert!(!window.last().dark);
  window
    .controller
    .update(cx, |c, cx| c.set_system_dark(true, cx));
  assert!(window.last().dark);
}

#[gpui::test]
fn a_read_that_panics_does_not_stop_the_window_reading(cx: &mut TestAppContext) {
  let host = repo_host();
  host
    .panic_once
    .store(true, std::sync::atomic::Ordering::SeqCst);
  let window = Window::open(cx, host.clone(), Some("/repo"));
  // The first read failed; `prepare_repo` finishing after it reads again,
  // and otherwise the poll does.
  cx.executor().advance_clock(POLL);
  cx.run_until_parked();
  assert_eq!(window.sidebar_ids(), [1, 2]);
  assert!(!host.panic_once.load(std::sync::atomic::Ordering::SeqCst));
}

#[gpui::test]
fn reads_the_branch_again_when_the_first_read_raced_the_repo_init(cx: &mut TestAppContext) {
  let host = repo_host();
  host
    .no_branch_once
    .store(true, std::sync::atomic::Ordering::SeqCst);
  let window = Window::open(cx, host, Some("/repo"));
  let repos = window.last().sidebar.unwrap().repos;
  assert_eq!(repos[0].home_ref.as_deref(), Some("main"));
}

#[gpui::test]
fn polls_on_swr_intervals(cx: &mut TestAppContext) {
  let host = repo_host();
  let _window = Window::open(cx, host.clone(), Some("/repo"));
  assert_eq!(host.count("workspaces /repo"), 1);
  assert_eq!(host.count("sessions /repo"), 1);
  assert_eq!(host.count("detail 1"), 1);
  cx.executor().advance_clock(POLL);
  cx.run_until_parked();
  assert_eq!(host.count("workspaces /repo"), 2);
  assert_eq!(host.count("sessions /repo"), 1, "sessions poll less often");
  assert_eq!(host.count("detail 1"), 1, "git detail waits for a change");
  cx.executor().advance_clock(SESSIONS_POLL - POLL);
  cx.run_until_parked();
  assert_eq!(host.count("sessions /repo"), 2);
}

/// A window in the background polls a third as often, and reads what it
/// skipped as soon as it is back in front.
#[gpui::test]
fn a_background_window_polls_less_and_catches_up_on_focus(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  window.controller.update(cx, |c, _| c.blurred());
  for _ in 0..BACKGROUND_EVERY {
    cx.executor().advance_clock(POLL);
    cx.run_until_parked();
  }
  assert_eq!(host.count("workspaces /repo"), 2, "one read in three ticks");
  cx.executor().advance_clock(POLL);
  cx.run_until_parked();
  assert_eq!(host.count("workspaces /repo"), 2);
  window.controller.update(cx, |c, cx| c.focused(cx));
  cx.run_until_parked();
  assert_eq!(host.count("workspaces /repo"), 3, "the skipped tick, read");
  window.controller.update(cx, |c, cx| c.focused(cx));
  cx.run_until_parked();
  assert_eq!(
    host.count("workspaces /repo"),
    3,
    "nothing more to catch up"
  );
  cx.executor().advance_clock(POLL);
  cx.run_until_parked();
  assert_eq!(host.count("workspaces /repo"), 4, "every tick in front");
}

#[gpui::test]
fn a_pr_poll_that_moved_stacks_reads_the_workspaces_again(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  cx.run_until_parked();
  window
    .controller
    .update(cx, |c, cx| c.prs_changed("/repo", false, cx));
  cx.run_until_parked();
  assert_eq!(
    host.count("workspaces /repo"),
    1,
    "PR state alone reads no workspaces"
  );
  window
    .controller
    .update(cx, |c, cx| c.prs_changed("/repo", true, cx));
  cx.run_until_parked();
  assert_eq!(host.count("workspaces /repo"), 2);
}

#[gpui::test]
fn a_workspace_change_reads_everything_again(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  let revision = window.controller.read_with(cx, |c, _| c.data().revision);
  host
    .workspaces
    .lock()
    .unwrap()
    .get_mut("/repo")
    .unwrap()
    .push(workspace(3, "c", Some("b")));
  window.controller.update(cx, |c, cx| {
    c.native_event(
      &NativeEvent::WorkspaceChanged {
        repo_path: "/repo".into(),
      },
      cx,
    )
  });
  cx.run_until_parked();
  assert_eq!(window.sidebar_ids(), [1, 2, 3]);
  assert_eq!(host.count("detail 3"), 1);
  assert_eq!(
    host.count("detail 1"),
    2,
    "an invalidation reads the detail"
  );
  assert_eq!(
    window.controller.read_with(cx, |c, _| c.data().revision),
    revision + 1
  );
}

#[gpui::test]
fn selects_a_workspace_from_the_sidebar(cx: &mut TestAppContext) {
  let window = Window::open(cx, repo_host(), Some("/repo"));
  assert!(window.sidebar(
    cx,
    SidebarAction::SelectWorkspace {
      id: 2,
      shift: false,
      toggle: false
    }
  ));
  assert_eq!(window.route_workspace(), Some(2));
  let sidebar = window.last().sidebar.unwrap();
  assert!(sidebar.repos[0].workspaces[1].selected);
}

/// What a click on a workspace row would show, for reading it ahead, is
/// the route the click then presents; the model itself does not move.
#[gpui::test]
fn reads_ahead_the_route_a_workspace_click_shows(cx: &mut TestAppContext) {
  let window = Window::open(cx, repo_host(), Some("/repo"));
  let ahead = window
    .controller
    .read_with(cx, |c, _| c.workspace_route(2))
    .expect("route");
  assert_eq!(window.route_workspace(), None);
  assert!(window.sidebar(
    cx,
    SidebarAction::SelectWorkspace {
      id: 2,
      shift: false,
      toggle: false
    }
  ));
  match window.screen_snapshot().view {
    View::Workspace(route) => assert_eq!(*route, ahead),
    _ => panic!("not the workspace view"),
  }
}

#[gpui::test]
fn selects_a_created_workspace_once_it_is_listed(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  host
    .workspaces
    .lock()
    .unwrap()
    .get_mut("/repo")
    .unwrap()
    .push(workspace(5, "new", Some("main")));
  window.screen(
    cx,
    ScreenAction::Field {
      field: "workspaceDialog.created".into(),
      value: 5.into(),
    },
  );
  assert_eq!(window.route_workspace(), Some(5));
}

#[gpui::test]
fn the_open_workspaces_dialog_selects_the_first_workspace_opened(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  window.sidebar(cx, SidebarAction::OpenWorkspaces);
  let modal = format!("{:?}", window.screen_snapshot().modal);
  assert!(
    modal
      .contains(r#"OpenWorkspaces(Route { repo_path: "/repo", repos: ["/repo"], linear: false, stack_on: None })"#),
    "{modal}"
  );

  host
    .workspaces
    .lock()
    .unwrap()
    .get_mut("/repo")
    .unwrap()
    .push(workspace(5, "opened", Some("main")));
  window.field(
    cx,
    "openWorkspaces.opened",
    serde_json::json!({"repoPath": "/repo", "id": 5}),
  );
  assert_eq!(window.route_workspace(), Some(5));
  assert!(window.screen_snapshot().modal.is_none());
}

#[gpui::test]
fn the_window_regaining_focus_reads_the_changes_again(cx: &mut TestAppContext) {
  let window = Window::open(cx, repo_host(), Some("/repo"));
  let revision = |cx: &mut TestAppContext| {
    window
      .controller
      .read_with(cx, |c, _| c.model().view.changes_revision)
  };
  let before = revision(cx);
  window.controller.update(cx, |c, cx| c.focused(cx));
  assert_eq!(revision(cx), before + 1);
}

#[gpui::test]
fn edits_on_disk_in_the_selected_checkout_read_the_changes_again(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  cx.run_until_parked();
  let revision = |cx: &mut TestAppContext| {
    window
      .controller
      .read_with(cx, |c, _| c.model().view.changes_revision)
  };
  let watched = |host: &FakeHost| -> Vec<String> {
    let watches = host.watches.lock().unwrap();
    watches
      .iter()
      .filter(|(_, tx)| !tx.is_closed())
      .map(|(path, _)| path.clone())
      .collect()
  };
  assert_eq!(watched(&host), ["/repo"]);

  let before = revision(cx);
  let edit = |host: &FakeHost, path: &str| {
    let watches = host.watches.lock().unwrap();
    let (_, tx) = watches.iter().rev().find(|(p, _)| p == path).unwrap();
    let _ = tx.unbounded_send(());
  };
  edit(&host, "/repo");
  cx.run_until_parked();
  assert_eq!(revision(cx), before + 1);

  select(&window, cx, 1, false);
  cx.run_until_parked();
  assert_eq!(
    watched(&host),
    [super::model::full_path(&workspace(1, "a", Some("main")))],
    "the selection's checkout replaces the repository's"
  );
  edit(&host, "/repo");
  cx.run_until_parked();
  assert_eq!(revision(cx), before + 1, "the old watch stopped");
}

#[gpui::test]
fn opens_the_dialogs_and_file_picker_from_shortcuts(cx: &mut TestAppContext) {
  let window = Window::open(cx, repo_host(), Some("/repo"));
  window
    .controller
    .update(cx, |c, cx| c.shortcut(Shortcut::FilePicker, cx));
  let picker = window.last().file_picker.expect("no file picker");
  assert_eq!(picker.workspace_path, "/repo");
  window.screen(
    cx,
    ScreenAction::Press {
      id: "filePicker.close".into(),
    },
  );
  assert!(window.last().file_picker.is_none());
  window
    .controller
    .update(cx, |c, cx| c.shortcut(Shortcut::Palette, cx));
  assert!(window.last().overlay.is_some());
}

#[gpui::test]
fn shortcuts_do_not_stack_a_dialog_over_another(cx: &mut TestAppContext) {
  let window = Window::open(cx, repo_host(), Some("/repo"));
  window
    .controller
    .update(cx, |c, cx| c.shortcut(Shortcut::NewWorkspace, cx));
  assert!(window.last().screen.unwrap().modal.is_some());
  for shortcut in [
    Shortcut::AgentPrompt,
    Shortcut::Palette,
    Shortcut::FilePicker,
  ] {
    window
      .controller
      .update(cx, |c, cx| c.shortcut(shortcut, cx));
  }
  let shown = window.last();
  assert!(shown.modal.is_none() && shown.overlay.is_none() && shown.file_picker.is_none());
  assert!(
    shown.screen.unwrap().modal.is_some(),
    "still the only dialog"
  );
}

#[gpui::test]
fn the_agent_prompt_starts_on_the_open_workspace(cx: &mut TestAppContext) {
  use crate::gpui_shell::screen::Modal;
  let window = Window::open(cx, repo_host(), Some("/repo"));
  let initial = |window: &Window| match window.last().modal {
    Some(Modal::AgentPrompt(route)) => route.initial_workspace_id,
    other => panic!("not the prompt dialog: {other:?}"),
  };
  window
    .controller
    .update(cx, |c, cx| c.shortcut(Shortcut::AgentPrompt, cx));
  assert_eq!(initial(&window), None, "the repository home targets main");
  window.press(cx, "agentPrompt.close");

  select(&window, cx, 2, false);
  cx.run_until_parked();
  window
    .controller
    .update(cx, |c, cx| c.shortcut(Shortcut::AgentPrompt, cx));
  assert_eq!(initial(&window), Some(2), "Cmd+I");
  window.press(cx, "agentPrompt.close");

  window.controller.update(cx, |c, cx| {
    c.overlay_action(
      &ScreenAction::Press {
        id: "palette.start-agent-with-prompt".into(),
      },
      cx,
    )
  });
  cx.run_until_parked();
  assert_eq!(initial(&window), Some(2), "the palette's command");
}

#[gpui::test]
fn factory_reset_asks_then_returns_to_onboarding(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::FactoryReset, cx));
  cx.run_until_parked();
  assert_eq!(
    window.asked().as_deref(),
    Some("Reset Treq to factory settings?")
  );
  assert_eq!(host.count("factory reset"), 0, "not before the answer");
  window.click_in_confirm(cx, "Reset");
  let calls = host.calls();
  assert!(calls.contains(&"factory reset".to_string()));
  assert!(calls.contains(&"toast Factory reset complete".to_string()));
  assert!(matches!(
    window.last().screen.unwrap().view,
    View::Onboarding(_)
  ));
  assert!(window.last().sidebar.is_none());
}

#[gpui::test]
fn a_declined_factory_reset_changes_nothing(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::on("/repo", vec![workspace(1, "a", Some("main"))]));
  let window = Window::open(cx, host.clone(), Some("/repo"));
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::FactoryReset, cx));
  cx.run_until_parked();
  window.click_in_confirm(cx, "Cancel");
  assert!(!host.calls().contains(&"factory reset".to_string()));
  assert!(window.last().sidebar.is_some());
  assert_eq!(window.asked(), None);
}

#[gpui::test]
fn a_confirmation_takes_the_keys_and_escape_cancels_it(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::FactoryReset, cx));
  // Asked again while open: still one question.
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::FactoryReset, cx));
  window
    .controller
    .update(cx, |c, cx| c.shortcut(Shortcut::Palette, cx));
  cx.run_until_parked();
  assert!(window.last().overlay.is_none(), "no palette over it");
  window
    .controller
    .update(cx, |c, cx| c.shortcut(Shortcut::Escape, cx));
  cx.run_until_parked();
  assert_eq!(window.asked(), None);
  assert_eq!(host.count("factory reset"), 0);
}

#[gpui::test]
fn adds_and_removes_supporting_repositories(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    folder: Some("/other".into()),
    ..FakeHost::on("/repo", vec![workspace(1, "a", Some("main"))])
  });
  let window = Window::open(cx, host.clone(), Some("/repo"));
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::AddRepository, cx));
  cx.run_until_parked();
  assert!(host.calls().contains(&"toast Added other".to_string()));
  let repos = window.last().sidebar.unwrap().repos;
  assert_eq!(
    repos.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
    ["/repo", "/other"]
  );
  window.controller.update(cx, |c, cx| {
    c.native_event(
      &NativeEvent::Open {
        what: crate::gpui_shell::native::OpenRequest::RemoveRepo {
          repo_path: "/other".into(),
        },
      },
      cx,
    )
  });
  cx.run_until_parked();
  assert_eq!(window.asked().as_deref(), Some("Remove other from Treq?"));
  assert_eq!(window.last().sidebar.unwrap().repos.len(), 2, "not yet");
  window.click_in_confirm(cx, "Remove repository");
  assert_eq!(window.last().sidebar.unwrap().repos.len(), 1);
}

#[gpui::test]
fn force_rebases_the_selected_workspace(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::ForceRebase, cx));
  cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"toast No workspace selected".to_string()));
  window.sidebar(
    cx,
    SidebarAction::SelectWorkspace {
      id: 2,
      shift: false,
      toggle: false,
    },
  );
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::ForceRebase, cx));
  cx.run_until_parked();
  let calls = host.calls();
  assert!(calls.contains(&"rebase 2 a".to_string()));
  assert!(calls.contains(&"toast Force rebase complete".to_string()));
}

#[gpui::test]
fn starts_an_agent_from_the_sidebar(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  window.sidebar(cx, SidebarAction::StartAgent { id: Some(1) });
  assert!(host
    .calls()
    .contains(&"launch /repo Some(1) false".to_string()));
}

#[gpui::test]
fn opens_a_picked_folder_in_this_window(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    folder: Some("/picked".into()),
    ..FakeHost::default()
  });
  let window = Window::open(cx, host.clone(), None);
  window.screen(cx, ScreenAction::OpenRepo);
  assert!(host.calls().contains(&"open /picked".to_string()));
  window.controller.update(cx, |c, cx| {
    c.native_event(
      &NativeEvent::RepoOpened {
        path: "/picked".into(),
      },
      cx,
    )
  });
  cx.run_until_parked();
  assert!(host.calls().contains(&"prepare /picked".to_string()));
  assert!(window.last().sidebar.is_some());
}

#[gpui::test]
fn opens_the_remote_setup_itself(cx: &mut TestAppContext) {
  let window = Window::open(cx, repo_host(), Some("/repo"));
  assert!(window.press(cx, "machines.addSsh"));
  assert!(window.screen_snapshot().remote.setup.is_some());
  assert!(window.press(cx, "remote.setup.close"));
  assert!(window.screen_snapshot().remote.setup.is_none());
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::OpenSsh, cx));
  cx.run_until_parked();
  assert!(window.screen_snapshot().remote.setup.is_some());
}

#[gpui::test]
fn escape_closes_the_remote_setup(cx: &mut TestAppContext) {
  let window = Window::open(cx, repo_host(), Some("/repo"));
  assert!(window.press(cx, "machines.addSsh"));
  window
    .controller
    .update(cx, |c, cx| c.shortcut(Shortcut::Escape, cx));
  cx.run_until_parked();
  assert!(window.screen_snapshot().remote.setup.is_none());
}

/// A window that registered `endpoint()` and opened `/srv/app` on it.
fn remote_window(cx: &mut TestAppContext, host: Arc<FakeHost>) -> Window {
  let window = Window::open(cx, host, None);
  window.press(cx, "machines.addSsh");
  window.field(
    cx,
    "remote.registerUserManaged",
    serde_json::json!({
      "display_name": "Dev box", "hostname": "devbox.example", "port": 22,
      "username": "me", "host_key_fingerprint": "SHA256:x",
      "auth_identity_reference": "/k",
    }),
  );
  window.field(cx, "remoteRepo.browse", "/srv/app".into());
  window.press(cx, "remoteRepo.open");
  window
}

#[gpui::test]
fn registers_an_endpoint_and_opens_a_repository_on_it(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::on(
    "/srv/app",
    vec![workspace(1, "a", Some("main"))],
  ));
  let window = Window::open(cx, host.clone(), None);
  window.press(cx, "machines.addSsh");
  window.field(
    cx,
    "remote.registerUserManaged",
    serde_json::json!({
      "display_name": "Dev box", "hostname": "devbox.example", "port": 22,
      "username": "me", "host_key_fingerprint": "SHA256:x",
      "auth_identity_reference": "/k",
    }),
  );
  let snapshot = window.screen_snapshot();
  assert!(snapshot.remote.setup.is_none(), "registering closes setup");
  let View::Onboarding(route) = snapshot.view else {
    panic!("not onboarding");
  };
  let form = route.remote_repo.expect("no repository form");
  assert_eq!(form.saved.len(), 1, "the endpoint's saved repositories");
  assert_eq!(form.path, "/home/me", "browsing starts at the host's home");
  assert!(!form.show_hidden);
  assert_eq!(form.listing.map(|l| l.entries.len()), Some(2));
  window.field(cx, "remoteRepo.browse", "/srv/app".into());
  window.press(cx, "remoteRepo.open");
  let calls = host.calls();
  for call in [
    "register devbox.example",
    "saved ep-1 0",
    "read ListDirectories",
    "read ProbeRepo",
    "read InspectRepository",
    "save remote /srv/app",
    "show /srv/app",
    "workspaces /srv/app",
  ] {
    assert!(calls.contains(&call.to_string()), "{call} in {calls:?}");
  }
  let snapshot = window.screen_snapshot();
  assert_eq!(
    snapshot.repository.map(|r| r.canonical_path).as_deref(),
    Some("/srv/app")
  );
  let View::Workspace(route) = snapshot.view else {
    panic!("not the workspace view");
  };
  assert_eq!(route.remote_chrome.unwrap().display_name, "app");
  assert_eq!(window.sidebar_ids(), [1]);
}

#[gpui::test]
fn a_path_without_a_repository_offers_clone_and_init(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    no_repo: true,
    ..FakeHost::default()
  });
  let window = remote_window(cx, host.clone());
  let View::Onboarding(route) = window.screen_snapshot().view else {
    panic!("not onboarding");
  };
  let form = route.remote_repo.unwrap();
  assert!(form.probe.unwrap().needs_clone);
  assert!(form.error.is_some());
  window.field(cx, "remoteRepo.confirmInit", true.into());
  window.press(cx, "remoteRepo.init");
  assert!(host.calls().contains(&"change InitRepo".to_string()));
  assert!(window.screen_snapshot().repository.is_some());
}

#[gpui::test]
fn the_folder_browser_moves_up_shows_hidden_and_uses_a_plain_folder(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    no_repo: true,
    ..FakeHost::default()
  });
  let window = Window::open(cx, host, None);
  window.press(cx, "machines.addSsh");
  window.field(
    cx,
    "remote.registerUserManaged",
    serde_json::json!({
      "display_name": "Dev box", "hostname": "devbox.example", "port": 22,
      "username": "me", "host_key_fingerprint": "SHA256:x",
      "auth_identity_reference": "/k",
    }),
  );
  let form = |window: &Window| {
    let View::Onboarding(route) = window.screen_snapshot().view else {
      panic!("not onboarding");
    };
    route.remote_repo.unwrap()
  };
  window.press(cx, "remoteRepo.up");
  assert_eq!(form(&window).path, "/home");
  window.field(cx, "remoteRepo.showHidden", true.into());
  assert!(form(&window).show_hidden);
  window.press(cx, "remoteRepo.useFolder");
  let probe = form(&window).probe.expect("clone and init offered");
  assert!(!probe.is_repo && probe.needs_clone);
  window.field(cx, "remoteRepo.browse", "/home/me".into());
  assert!(form(&window).probe.is_none(), "a new folder starts over");
}

#[gpui::test]
fn the_cloud_row_opens_setup_on_the_cloud_workspace(cx: &mut TestAppContext) {
  let window = Window::open(cx, Arc::new(FakeHost::default()), None);
  let screen = |window: &Window| {
    let setup = window.screen_snapshot().remote.setup.expect("setup open");
    setup.screen
  };
  assert!(window.press(cx, "machines.cloud"));
  assert_eq!(screen(&window).as_deref(), Some("managed"));
  window.press(cx, "remote.setup.close");
  assert!(window.press(cx, "machines.addSsh"));
  assert_eq!(screen(&window).as_deref(), Some("ownVm"));
  window.press(cx, "remote.setup.close");
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::OpenSsh, cx));
  cx.run_until_parked();
  assert_eq!(screen(&window), None, "Open via SSH starts at the choice");
}

#[gpui::test]
fn restores_the_last_remote_repository_at_launch(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    last_remote: Some(saved("ep-1", 0, "/srv/app")),
    ..FakeHost::default()
  });
  let window = Window::open(cx, host.clone(), None);
  assert!(host.calls().contains(&"restore /srv/app".to_string()));
  assert!(window.screen_snapshot().repository.is_some());
}

#[gpui::test]
fn a_machine_shows_its_repository_form_or_the_setup(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let window = Window::open(cx, host, None);
  assert!(window.press(cx, &format!("machines.connect:{}", endpoint().id)));
  let View::Onboarding(route) = window.screen_snapshot().view else {
    panic!("not onboarding");
  };
  let form = route.remote_repo.expect("no repository form");
  assert_eq!(form.endpoint_id, endpoint().id);
  assert!(window.screen_snapshot().remote.setup.is_none());
  // A machine that cannot be reached opens the setup dialog instead.
  assert!(window.press(cx, "machines.connect:gone"));
  assert!(window.screen_snapshot().remote.setup.is_some());
}

#[gpui::test]
fn opens_a_recent_remote_repository_by_its_saved_id(cx: &mut TestAppContext) {
  let descriptor = saved(&endpoint().id, 0, "/srv/app");
  let host = Arc::new(FakeHost {
    last_remote: Some(descriptor.clone()),
    ..FakeHost::default()
  });
  let window = Window::open(cx, host.clone(), None);
  // Launch restored it; leave it to open it again from the list.
  host.calls.lock().unwrap().clear();
  assert!(window.press(cx, &format!("machines.openSaved:{}", descriptor.id)));
  assert!(host.calls().contains(&"restore /srv/app".to_string()));
  assert!(window.screen_snapshot().repository.is_some());
}

#[gpui::test]
fn a_failed_restore_stays_on_onboarding(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    last_remote: Some(saved("ep-1", 0, "/srv/app")),
    no_repo: true,
    ..FakeHost::default()
  });
  let window = Window::open(cx, host, None);
  let snapshot = window.screen_snapshot();
  assert!(snapshot.repository.is_none());
  assert!(matches!(snapshot.view, View::Onboarding(_)));
}

#[gpui::test]
fn opens_a_remote_terminal_on_the_host(cx: &mut TestAppContext) {
  let window = remote_window(cx, Arc::new(FakeHost::default()));
  window.press(cx, "remote.terminal");
  let terminal = window.screen_snapshot().remote.terminal.expect("no dialog");
  assert_eq!(terminal.workspace_id, "root");
  window.field(cx, "remote.terminal.open", serde_json::json!({}));
  assert!(window.screen_snapshot().remote.terminal.is_none());
  let commands = window
    .controller
    .read_with(cx, |c, _| c.model().commands.clone());
  let open = commands.last().expect("no terminal command");
  assert_eq!(open["kind"], "openRemote");
  assert_eq!(open["remote"]["workspaceId"], "root");
  assert_eq!(open["remote"]["launch"]["type"], "shell");
}

#[gpui::test]
fn a_cutoff_blocks_the_terminal_until_reauthenticated(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let window = remote_window(cx, host.clone());
  window.controller.update(cx, |c, cx| {
    c.cutoff_changed("ep-1", Some("session_ended".into()), cx)
  });
  let View::Workspace(route) = window.screen_snapshot().view else {
    panic!("not the workspace view");
  };
  assert!(route.remote_chrome.unwrap().cut_off);
  window.press(cx, "remote.terminal");
  assert!(window.screen_snapshot().remote.terminal.is_none());
  let reads = host.count("workspaces /srv/app");
  window
    .controller
    .update(cx, |c, cx| c.cutoff_changed("ep-1", None, cx));
  cx.run_until_parked();
  let View::Workspace(route) = window.screen_snapshot().view else {
    panic!("not the workspace view");
  };
  assert!(!route.remote_chrome.unwrap().cut_off);
  assert!(host.count("workspaces /srv/app") > reads, "reads again");
}

#[gpui::test]
fn a_managed_connection_lists_its_repositories(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let window = Window::open(cx, host.clone(), None);
  window.controller.update(cx, |c, cx| {
    c.managed_endpoint(
      ManagedEndpointEvent::Connected {
        endpoint: endpoint(),
        generation: 3,
        open_repositories: true,
      },
      cx,
    )
  });
  cx.run_until_parked();
  assert!(host.calls().contains(&"saved ep-1 3".to_string()));
  let View::Onboarding(route) = window.screen_snapshot().view else {
    panic!("not onboarding");
  };
  assert!(route.remote_repo.is_some());
}

#[gpui::test]
fn runs_the_cloud_workspace_buttons_in_rust(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let window = Window::open(cx, host.clone(), None);
  assert!(window.press(cx, "remote.cloud.wake"));
  assert!(window.press(cx, "remote.cloud.openRepositories"));
  let calls = host.calls();
  assert!(calls.contains(&"cloud Wake".to_string()));
  assert!(calls.contains(&"cloud OpenRepositories".to_string()));
}

#[gpui::test]
fn an_unverified_change_shows_its_notice_until_dismissed(cx: &mut TestAppContext) {
  let window = remote_window(cx, Arc::new(FakeHost::default()));
  window.controller.update(cx, |c, cx| {
    c.native_event(
      &NativeEvent::RemoteMutation {
        status: "ambiguous",
        reason: Some("connection lost".into()),
      },
      cx,
    )
  });
  cx.run_until_parked();
  assert_eq!(
    window.screen_snapshot().remote.ambiguous.as_deref(),
    Some("connection lost")
  );
  window.press(cx, "remote.ambiguous.dismiss");
  assert!(window.screen_snapshot().remote.ambiguous.is_none());
}

#[gpui::test]
fn a_local_repository_takes_the_window_from_a_remote_one(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let window = remote_window(cx, host.clone());
  window.controller.update(cx, |c, cx| {
    c.native_event(
      &NativeEvent::RepoOpened {
        path: "/repo".into(),
      },
      cx,
    )
  });
  cx.run_until_parked();
  assert!(host.calls().contains(&"show none".to_string()));
  assert!(window.screen_snapshot().repository.is_none());
}

fn deep_link(branch: &str, request_id: &str) -> crate::core::agent_launch::AgentDeepLinkRequest {
  crate::core::agent_launch::AgentDeepLinkRequest {
    repo: "/repo".into(),
    branch: branch.into(),
    prompt: "Fix it".into(),
    mode: "acceptEdits".into(),
    agent: "claude".into(),
    request_id: request_id.into(),
  }
}

/// `agent-deeplink.test.tsx` (`handleStartAgentRequest`): a branch with no
/// workspace toasts; one with a workspace starts there and is processed.
#[test]
fn deep_links_start_on_their_branch_or_toast_that_it_is_missing() {
  use crate::{core::agent_launch::deep_links, gpui_shell::launch::start_deep_link};
  let host = repo_host();

  start_deep_link(&*host, &deep_link("feat/gone", "window-test-missing"));
  assert_eq!(
    host.calls(),
    ["workspaces /repo", "toast Workspace not found"]
  );
  assert!(!deep_links()
    .lock()
    .unwrap()
    .is_processed("window-test-missing"));

  start_deep_link(&*host, &deep_link("b", "window-test-found"));
  assert_eq!(host.calls().last().unwrap(), "launch /repo Some(2) true");
  assert!(deep_links()
    .lock()
    .unwrap()
    .is_processed("window-test-found"));
}

/// A remote poll reads every workspace's status in one request, not one
/// per workspace, and falls back to those reads when the host has none.
#[test]
fn a_summaries_read_replaces_the_per_workspace_reads() {
  use super::{data::Read, host::read_repo};
  let host = FakeHost::on(
    "/repo",
    vec![workspace(1, "a", None), workspace(2, "b", None)],
  );
  let summary = |id, has_conflicts, has_changes| WorkspaceSummary {
    workspace: workspace(id, "w", None),
    has_conflicts,
    has_changes,
    commits_ahead: 0,
    terminals: 0,
    last_activity_at: None,
  };
  *host.summaries.lock().unwrap() = Some(vec![summary(1, false, true), summary(2, true, true)]);

  let out = read_repo(&host, "/repo", Read::ALL, &[]);
  assert_eq!(host.calls(), ["summaries /repo", "sessions /repo"]);
  assert_eq!(out.workspaces.unwrap().len(), 2);
  assert_eq!(out.statuses.unwrap().len(), 2);
  // The conflicted workspace is not "open".
  assert_eq!(out.has_changes.unwrap(), HashMap::from([(1, true)]));

  *host.summaries.lock().unwrap() = None;
  read_repo(&host, "/repo", Read::ALL, &[]);
  assert_eq!(host.count("workspaces /repo"), 1);
  assert_eq!(host.count("detail 1"), 1);
}

fn session_started(workspace_id: Option<i64>, select: bool) -> NativeEvent {
  NativeEvent::AgentSessionStarted {
    session: crate::core::agent_launch::SessionStart {
      session_id: 7,
      repo_path: "/repo".into(),
      workspace_id,
      ..Default::default()
    },
    pane_session_id: 7,
    select,
  }
}

fn toasts(host: &FakeHost) -> Vec<String> {
  host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("toast "))
    .collect()
}

fn select(window: &Window, cx: &mut TestAppContext, id: i64, toggle: bool) {
  window.sidebar(
    cx,
    SidebarAction::SelectWorkspace {
      id,
      shift: false,
      toggle,
    },
  );
}

/// `session-started-toast.test.tsx`: `showBackendSession` shows no toast,
/// and a session started without `select` leaves the page and selection.
#[gpui::test]
fn a_started_session_shows_no_toast_and_keeps_the_page(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  select(&window, cx, 1, false);
  // Started in another workspace, from the prompt dialog's picker.
  window.controller.update(cx, |c, cx| {
    c.native_event(&session_started(Some(2), false), cx)
  });
  cx.run_until_parked();
  assert_eq!(window.route_workspace(), Some(1), "the selection stays");
  assert_eq!(
    window
      .controller
      .read_with(cx, |c, _| c.model().active_session),
    Some(7)
  );
  // Started from the Linear view.
  window.sidebar(cx, SidebarAction::OpenLinear);
  window.controller.update(cx, |c, cx| {
    c.native_event(&session_started(Some(2), false), cx)
  });
  cx.run_until_parked();
  assert_eq!(
    window
      .controller
      .read_with(cx, |c, _| c.model().page.clone()),
    super::model::Page::Linear
  );
  // A deep link or review shows its workspace.
  window.controller.update(cx, |c, cx| {
    c.native_event(&session_started(Some(2), true), cx)
  });
  cx.run_until_parked();
  assert_eq!(window.route_workspace(), Some(2));
  assert_eq!(toasts(&host), Vec::<String>::new());
}

fn remove(host: &FakeHost, id: i64) {
  host
    .workspaces
    .lock()
    .unwrap()
    .get_mut("/repo")
    .unwrap()
    .retain(|w| w.id != id);
}

/// What `workspace_ops` reports for an archive: the change, then the ids.
fn archived(window: &Window, cx: &mut TestAppContext, ids: Vec<i64>) {
  window.controller.update(cx, |c, cx| {
    c.native_event(
      &NativeEvent::WorkspaceChanged {
        repo_path: "/repo".into(),
      },
      cx,
    );
    c.native_event(
      &NativeEvent::WorkspacesRemoved {
        repo_path: "/repo".into(),
        ids,
      },
      cx,
    );
  });
  cx.run_until_parked();
}

fn stack_host() -> Arc<FakeHost> {
  Arc::new(FakeHost::on(
    "/repo",
    vec![
      workspace(1, "feat/alpha", Some("main")),
      workspace(2, "feat/beta", Some("main")),
      workspace(3, "feat/child", Some("feat/alpha")),
    ],
  ))
}

/// `workspace-removal-navigation.test.tsx` (`leaveRemovedWorkspaces`).
#[gpui::test]
fn archiving_another_workspace_keeps_the_open_one(cx: &mut TestAppContext) {
  let host = stack_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  select(&window, cx, 2, false);
  remove(&host, 1);
  archived(&window, cx, vec![1]);
  assert!(!window.sidebar_ids().contains(&1));
  assert_eq!(window.route_workspace(), Some(2));
}

#[gpui::test]
fn archiving_the_open_workspace_lands_on_its_stack_parent(cx: &mut TestAppContext) {
  let host = stack_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  select(&window, cx, 3, false);
  remove(&host, 3);
  archived(&window, cx, vec![3]);
  assert_eq!(window.route_workspace(), Some(1));
  // With no parent left, the home view.
  select(&window, cx, 2, false);
  remove(&host, 2);
  archived(&window, cx, vec![2]);
  assert_eq!(window.route_workspace(), None);
}

fn removal(window: &Window, cx: &mut TestAppContext, event: fn(String, Vec<i64>) -> NativeEvent) {
  window.controller.update(cx, |c, cx| {
    c.native_event(&event("/repo".into(), vec![3]), cx)
  });
  cx.run_until_parked();
}

/// The row goes and the view leaves it as the removal starts, while the
/// workspace is still listed, and a read meanwhile does not bring it back.
#[gpui::test]
fn a_removal_hides_the_row_before_it_finishes(cx: &mut TestAppContext) {
  let host = stack_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  select(&window, cx, 3, false);
  removal(&window, cx, |repo_path, ids| {
    NativeEvent::WorkspacesRemoving { repo_path, ids }
  });
  assert!(!window.sidebar_ids().contains(&3));
  assert_eq!(window.route_workspace(), Some(1), "onto its stack parent");
  cx.executor().advance_clock(POLL);
  cx.run_until_parked();
  assert!(
    !window.sidebar_ids().contains(&3),
    "the host still lists it"
  );
  remove(&host, 3);
  removal(&window, cx, |repo_path, ids| {
    NativeEvent::WorkspacesRemoved { repo_path, ids }
  });
  assert!(!window.sidebar_ids().contains(&3));
}

#[gpui::test]
fn a_failed_removal_brings_the_row_back(cx: &mut TestAppContext) {
  let host = stack_host();
  let window = Window::open(cx, host, Some("/repo"));
  removal(&window, cx, |repo_path, ids| {
    NativeEvent::WorkspacesRemoving { repo_path, ids }
  });
  assert!(!window.sidebar_ids().contains(&3));
  removal(&window, cx, |repo_path, ids| {
    NativeEvent::WorkspacesRestored { repo_path, ids }
  });
  assert!(window.sidebar_ids().contains(&3));
}

/// The listed-ids effect: a poll that no longer lists the open workspace.
#[gpui::test]
fn a_workspace_removed_outside_the_app_leaves_the_view(cx: &mut TestAppContext) {
  let host = stack_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  select(&window, cx, 3, false);
  remove(&host, 3);
  cx.executor().advance_clock(POLL);
  cx.run_until_parked();
  assert!(!window.sidebar_ids().contains(&3));
  assert_eq!(window.route_workspace(), None, "home, not the parent");
}

/// Found by the fuzzer: a row removed elsewhere stayed in the
/// multi-selection, and in Bulk Archive's count.
#[gpui::test]
fn a_workspace_removed_elsewhere_leaves_the_multi_selection(cx: &mut TestAppContext) {
  let host = stack_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  select(&window, cx, 1, true);
  select(&window, cx, 2, true);
  assert_eq!(window.last().sidebar.unwrap().bulk_archive, Some(2));
  remove(&host, 1);
  cx.executor().advance_clock(POLL);
  cx.run_until_parked();
  assert!(!window.sidebar_ids().contains(&1));
  assert_eq!(window.last().sidebar.unwrap().bulk_archive, Some(1));
}

/// QA: with a workspace open, Ctrl+clicking a second row highlights both,
/// so Bulk Archive counts both.
#[gpui::test]
fn ctrl_click_adds_to_the_open_workspace(cx: &mut TestAppContext) {
  let host = stack_host();
  let window = Window::open(cx, host, Some("/repo"));
  select(&window, cx, 1, false);
  assert_eq!(window.last().sidebar.unwrap().bulk_archive, None);
  select(&window, cx, 2, true);
  let sidebar = window.last().sidebar.unwrap();
  let mut selected: Vec<_> = sidebar.repos[0]
    .workspaces
    .iter()
    .filter(|r| r.selected)
    .map(|r| r.id)
    .collect();
  selected.sort();
  assert_eq!(selected, [1, 2]);
  assert_eq!(sidebar.bulk_archive, Some(2));
}

/// Bulk Archive with one failure: only the archived row leaves.
#[gpui::test]
fn archiving_drops_the_removed_rows_from_the_multi_selection(cx: &mut TestAppContext) {
  let host = stack_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  select(&window, cx, 1, true);
  select(&window, cx, 2, true);
  assert_eq!(window.last().sidebar.unwrap().bulk_archive, Some(2));
  remove(&host, 1);
  archived(&window, cx, vec![1]);
  assert!(window.sidebar_ids().contains(&2));
  assert_eq!(window.last().sidebar.unwrap().bulk_archive, Some(1));
}

/// `supporting-repos.test.tsx`: a home row and a labelled group per
/// repository, the main one first; folding one leaves the others.
#[gpui::test]
fn draws_a_home_row_and_group_for_each_repository(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::on(
    "/code/main",
    vec![workspace(1, "feat/main-work", Some("main"))],
  ));
  let mut theirs = workspace(7, "feat/supporting-work", Some("main"));
  theirs.repo_path = "/code/svc".into();
  host
    .workspaces
    .lock()
    .unwrap()
    .insert("/code/svc".into(), vec![theirs]);
  host.supporting.lock().unwrap().extend([
    SupportingRepo {
      path: "/code/svc".into(),
      exists: true,
    },
    SupportingRepo {
      path: "/gone/lib".into(),
      exists: false,
    },
  ]);
  let window = Window::open(cx, host.clone(), Some("/code/main"));
  let repos = window.last().sidebar.unwrap().repos;
  let titles = |i: usize| -> Vec<String> {
    repos[i]
      .workspaces
      .iter()
      .map(|w| w.title.clone())
      .collect()
  };
  assert_eq!(
    repos.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
    ["/code/main", "/code/svc", "/gone/lib"]
  );
  assert_eq!(repos[0].home_ref.as_deref(), Some("main"));
  assert!(repos[0].active && !repos[1].active);
  assert_eq!(
    repos.iter().map(|r| r.label.as_deref()).collect::<Vec<_>>(),
    [Some("main"), Some("svc"), Some("lib")]
  );
  assert_eq!(titles(0), ["feat/main-work"]);
  assert_eq!(titles(1), ["feat/supporting-work"]);
  assert!(repos[2].missing && !repos[2].loading && titles(2).is_empty());
  assert_eq!(host.count("workspaces /gone/lib"), 0, "not read");

  window.sidebar(
    cx,
    SidebarAction::ToggleRepo {
      repo_path: "/code/svc".into(),
    },
  );
  let repos = window.last().sidebar.unwrap().repos;
  assert!(repos[1].collapsed && !repos[0].collapsed);
  assert_eq!(
    host.count(r#"save sidebar_collapsed_repos:/code/main=["/code/svc"]"#),
    1
  );

  // A supporting workspace puts its repository on screen; home returns.
  window.sidebar(
    cx,
    SidebarAction::OpenRepoWorkspace {
      repo_path: "/code/svc".into(),
      id: 7,
    },
  );
  assert_eq!(window.route_workspace(), Some(7));
  let repos = window.last().sidebar.unwrap().repos;
  assert!(repos[1].active && repos[1].workspaces[0].selected);
  window.sidebar(
    cx,
    SidebarAction::SelectHome {
      repo_path: "/code/main".into(),
    },
  );
  assert_eq!(window.route_workspace(), None);
  assert!(window.last().sidebar.unwrap().repos[0].home_selected);
}

/// What the model costs per action and per poll with many workspaces
/// (ignored): `cargo test --features gpui-shell --lib window_model_perf --
/// --ignored --nocapture --test-threads=1`.
#[gpui::test]
#[ignore = "perf report"]
fn window_model_perf(cx: &mut TestAppContext) {
  fn report(name: &str, samples: &mut [std::time::Duration]) {
    samples.sort();
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.;
    let mean = samples.iter().map(|d| ms(*d)).sum::<f64>() / samples.len() as f64;
    println!(
      "{name:<34} n={:<3} mean={mean:>8.3}ms  p50={:>8.3}ms  max={:>8.3}ms",
      samples.len(),
      ms(samples[samples.len() / 2]),
      ms(samples[samples.len() - 1]),
    );
  }
  for count in [50, 200] {
    // Stacks of three, as agents stack work.
    let workspaces: Vec<Workspace> = (1..=count)
      .map(|i| {
        let target = (i % 3 != 1).then(|| format!("feat/ws-{}", i - 1));
        let mut w = workspace(i, &format!("feat/ws-{i}"), target.as_deref());
        w.created_at = format!("2026-10-01T00:00:{:02}Z", i % 60);
        w
      })
      .collect();
    let host = Arc::new(FakeHost::on("/repo", workspaces));
    let window = Window::open(cx, host, Some("/repo"));
    let mut select: Vec<_> = (1..=30)
      .map(|i| {
        let start = std::time::Instant::now();
        window.controller.update(cx, |c, cx| {
          c.sidebar_action(
            &SidebarAction::SelectWorkspace {
              id: i,
              shift: false,
              toggle: false,
            },
            cx,
          )
        });
        start.elapsed()
      })
      .collect();
    report(&format!("{count} workspaces: select"), &mut select);
    let mut poll: Vec<_> = (0..10)
      .map(|_| {
        let start = std::time::Instant::now();
        cx.executor().advance_clock(POLL);
        cx.run_until_parked();
        start.elapsed()
      })
      .collect();
    report(&format!("{count} workspaces: unchanged poll"), &mut poll);
  }
}
