//! The window controller against a fake backend: what it reads, when, and
//! what it hands the shell to draw.

use std::{
  cell::RefCell,
  collections::HashMap,
  rc::Rc,
  sync::{Arc, Mutex},
};

use gpui::{prelude::*, Entity, TestAppContext};
use tauri_plugin_gpui::gpui;

use super::{
  actions::{Menu, Shortcut},
  controller::{Presentation, WindowController},
  data::{Flags, POLL, SESSIONS_POLL},
  host::WindowHost,
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
    SingleRebaseResult, WorkspaceSidebarStatus,
  },
  github::{GitRemoteInfo, PrInfo},
  gpui_shell::{
    native::{NativeEvent, ToastTone},
    remote_repo::{Mutation, RemoteRepository},
    screen::{account_data::CloudAction, ScreenAction, View},
    sidebar::SidebarAction,
  },
  local_db::{Session, Workspace},
};

#[derive(Default)]
struct FakeHost {
  workspaces: Mutex<HashMap<String, Vec<Workspace>>>,
  supporting: Mutex<Vec<SupportingRepo>>,
  settings: Mutex<HashMap<String, String>>,
  /// The folder `pick_folder` answers with.
  folder: Option<String>,
  decline: bool,
  /// The remote repository `last_remote` restores.
  last_remote: Option<SavedRepository>,
  /// What a probe finds: a repository, unless set.
  no_repo: bool,
  /// The next workspaces read panics, as a backend read before the app's
  /// state is managed did.
  panic_once: std::sync::atomic::AtomicBool,
  /// Every call, in order.
  calls: Mutex<Vec<String>>,
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

  fn statuses(&self, _: &str) -> Result<Vec<WorkspaceSidebarStatus>, String> {
    Ok(Vec::new())
  }

  fn has_changes(&self, _: &str, id: i64) -> Option<bool> {
    self.record(format!("detail {id}"));
    Some(false)
  }

  fn sessions(&self, repo: &str) -> Vec<Session> {
    self.record(format!("sessions {repo}"));
    Vec::new()
  }

  fn branch(&self, _: &str) -> Option<RepoBranch> {
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

  fn ask(&self, title: &str, _: &str) -> bool {
    self.record(format!("ask {title}"));
    !self.decline
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
        WindowController::new(host, repo, Rc::new(move |p, _| s.borrow_mut().push(p)), cx)
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
fn shows_onboarding_without_a_repository(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let window = Window::open(cx, host.clone(), None);
  let shown = window.last();
  assert!(shown.sidebar.is_none());
  assert!(matches!(shown.screen.unwrap().view, View::Onboarding(_)));
  assert_eq!(host.count("workspaces /repo"), 0);
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
  assert!(window.sidebar_ids().is_empty(), "the first read failed");
  cx.executor().advance_clock(POLL);
  cx.run_until_parked();
  assert_eq!(window.sidebar_ids(), [1, 2]);
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
fn the_window_regaining_focus_reads_the_changes_again(cx: &mut TestAppContext) {
  let window = Window::open(cx, repo_host(), Some("/repo"));
  let before = window
    .controller
    .read_with(cx, |c, _| c.model().view.changes_revision);
  window.controller.update(cx, |c, cx| c.focused(cx));
  window.controller.update(cx, |c, cx| c.files_changed(9, cx));
  let after = window
    .controller
    .read_with(cx, |c, _| c.model().view.changes_revision);
  assert_eq!(after, before + 1, "another workspace's files do not count");
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
fn factory_reset_asks_then_returns_to_onboarding(cx: &mut TestAppContext) {
  let host = repo_host();
  let window = Window::open(cx, host.clone(), Some("/repo"));
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::FactoryReset, cx));
  cx.run_until_parked();
  let calls = host.calls();
  assert!(calls.contains(&"ask Factory Reset".to_string()));
  assert!(calls.contains(&"factory reset".to_string()));
  assert!(calls.contains(&"toast Factory Reset Complete".to_string()));
  assert!(matches!(
    window.last().screen.unwrap().view,
    View::Onboarding(_)
  ));
  assert!(window.last().sidebar.is_none());
}

#[gpui::test]
fn a_declined_factory_reset_changes_nothing(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    decline: true,
    ..FakeHost::on("/repo", vec![workspace(1, "a", Some("main"))])
  });
  let window = Window::open(cx, host.clone(), Some("/repo"));
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::FactoryReset, cx));
  cx.run_until_parked();
  assert!(!host.calls().contains(&"factory reset".to_string()));
  assert!(window.last().sidebar.is_some());
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
  assert!(host.calls().contains(&"toast Repository Added".to_string()));
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
  assert!(host.calls().contains(&"ask Remove Repository".to_string()));
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
  assert!(window.screen(cx, ScreenAction::OpenRemote));
  assert!(window.screen_snapshot().remote.setup.is_some());
  assert!(window.press(cx, "remote.setup.close"));
  assert!(window.screen_snapshot().remote.setup.is_none());
  window
    .controller
    .update(cx, |c, cx| c.menu(Menu::OpenSsh, cx));
  cx.run_until_parked();
  assert!(window.screen_snapshot().remote.setup.is_some());
}

/// A window that registered `endpoint()` and opened `/srv/app` on it.
fn remote_window(cx: &mut TestAppContext, host: Arc<FakeHost>) -> Window {
  let window = Window::open(cx, host, None);
  window.screen(cx, ScreenAction::OpenRemote);
  window.field(
    cx,
    "remote.registerUserManaged",
    serde_json::json!({
      "display_name": "Dev box", "hostname": "devbox.example", "port": 22,
      "username": "me", "host_key_fingerprint": "SHA256:x",
      "auth_identity_reference": "/k",
    }),
  );
  window.field(cx, "remoteRepo.path", "/srv/app".into());
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
  window.screen(cx, ScreenAction::OpenRemote);
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
  window.field(cx, "remoteRepo.path", "/srv/app".into());
  window.press(cx, "remoteRepo.open");
  let calls = host.calls();
  for call in [
    "register devbox.example",
    "saved ep-1 0",
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
