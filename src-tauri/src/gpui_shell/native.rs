//! What GPUI screens call to read and change the app's state directly.
//!
//! After a screen changes something, it tells its window's model with a
//! [`NativeEvent`], so the model rereads what went stale (it never repeats
//! the change).

use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::remote_repo::{self, Cutoff, Mutation, Reads, Remote, Transport};
use crate::{
  core::{
    feature_preview,
    remote::{self as remote_command, MutationRetryOutcome, TreqCommandRequest},
    remote_control_plane::SshEndpoint,
    remote_ssh_transport::{CancellationToken, ExecLimits},
  },
  db::Database,
  lock_ext::LockExt,
  AppState,
};

/// What a GPUI screen did, for the window model to follow, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NativeEvent {
  /// The window now shows the repository at `path`.
  RepoOpened { path: String },
  /// App settings (`set_setting` keys) changed.
  SettingsChanged,
  /// The settings of the repository at `repo_path` changed.
  #[serde(rename_all = "camelCase")]
  RepoSettingsChanged { repo_path: String },
  /// Something in the repository at `repo_path` changed (a push, sync or
  /// rebase), so everything read from it is stale.
  #[serde(rename_all = "camelCase")]
  WorkspaceChanged { repo_path: String },
  Toast {
    title: String,
    description: String,
    tone: ToastTone,
    /// A button that runs a [`register_toast_action`] closure.
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<ToastAction>,
  },
  /// How a change GPUI sent to the remote repository ended, for the
  /// window's feedback.
  RemoteMutation {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
  },
  /// A remote command found the endpoint cut off.
  #[serde(rename_all = "camelCase")]
  RemoteCutoff { endpoint_id: String, reason: String },
  /// The backend started an agent session (`gpui_shell::launch`). The
  /// model makes it the active session, and with `select` shows its
  /// workspace.
  #[serde(rename_all = "camelCase")]
  AgentSessionStarted {
    session: crate::core::agent_launch::SessionStart,
    pane_session_id: i64,
    select: bool,
  },
  /// GPUI is removing (deleting or archiving) these workspaces of the
  /// repository: the window hides their rows and leaves them at once, while
  /// the removal runs. It ends in [`Self::WorkspacesRemoved`] or
  /// [`Self::WorkspacesRestored`].
  #[serde(rename_all = "camelCase")]
  WorkspacesRemoving { repo_path: String, ids: Vec<i64> },
  /// The removal started by [`Self::WorkspacesRemoving`] failed: the rows
  /// show again.
  #[serde(rename_all = "camelCase")]
  WorkspacesRestored { repo_path: String, ids: Vec<i64> },
  /// GPUI removed (deleted or archived) these workspaces of the repository,
  /// so the window leaves them if they show.
  #[serde(rename_all = "camelCase")]
  WorkspacesRemoved { repo_path: String, ids: Vec<i64> },
  /// A menu item picked in GPUI that opens something the window model
  /// routes: a dialog, a page or a flow.
  Open { what: OpenRequest },
}

/// What a GPUI menu or button asks the window model to open, tagged by
/// `kind`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OpenRequest {
  BranchSwitcher,
  /// The add-supporting-repository flow.
  AddRepository,
  #[serde(rename_all = "camelCase")]
  RepoSettings {
    repo_path: String,
  },
  #[serde(rename_all = "camelCase")]
  LocateRepo {
    repo_path: String,
  },
  #[serde(rename_all = "camelCase")]
  RemoveRepo {
    repo_path: String,
  },
  /// The rename dialog for a workspace.
  #[serde(rename_all = "camelCase")]
  RenameWorkspace {
    repo_path: String,
    workspace_id: i64,
  },
  /// The pull request in Treq's PR view (`onViewPrInApp`).
  PrReview {
    number: u64,
    state: String,
  },
}

/// A toast's button: `label`, running the action registered as `id`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToastAction {
  pub label: String,
  pub id: u64,
}

type PendingAction = Box<dyn FnOnce() + Send>;

/// The toast buttons not yet clicked, by id.
fn toast_actions() -> &'static std::sync::Mutex<(u64, std::collections::HashMap<u64, PendingAction>)>
{
  static ACTIONS: std::sync::OnceLock<
    std::sync::Mutex<(u64, std::collections::HashMap<u64, PendingAction>)>,
  > = std::sync::OnceLock::new();
  ACTIONS.get_or_init(Default::default)
}

/// Keeps `action` until its toast's button is clicked; returns its id for
/// [`ToastAction`].
pub fn register_toast_action(action: impl FnOnce() + Send + 'static) -> u64 {
  let mut actions = toast_actions().lock_or_recover();
  actions.0 += 1;
  let id = actions.0;
  actions.1.insert(id, Box::new(action));
  id
}

/// Runs, once, the action of the toast button `id`. Blocks for as long as
/// the action does.
pub fn run_toast_action(id: u64) {
  let action = toast_actions().lock_or_recover().1.remove(&id);
  if let Some(action) = action {
    action();
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToastTone {
  Success,
  Warning,
  Error,
  Info,
}

fn package_json() -> serde_json::Value {
  const PACKAGE_JSON: &str = include_str!("../../../package.json");
  serde_json::from_str(PACKAGE_JSON).unwrap_or_default()
}

/// The website the docs links open, as React's `WEB_URL`.
pub fn web_url() -> String {
  let env = if cfg!(debug_assertions) {
    "dev"
  } else {
    "prod"
  };
  package_json()["env"][env]["webUrl"]
    .as_str()
    .map_or_else(|| "https://treq.dev".to_string(), str::to_string)
}

/// Whether `flag` is on in React's `FEATURES` (package.json `featureFlags`).
pub fn feature_flag(flag: &str) -> bool {
  package_json()["featureFlags"][flag]
    .as_bool()
    .unwrap_or(false)
}

/// The backend, as seen from window `label`'s screens.
#[derive(Clone)]
pub struct Backend {
  app: AppHandle,
  label: String,
}

impl Backend {
  pub fn new(app: AppHandle, label: String) -> Self {
    Self { app, label }
  }

  /// The window's label.
  pub fn label(&self) -> &str {
    &self.label
  }

  pub fn app(&self) -> &AppHandle {
    &self.app
  }

  /// Runs `f` with the app database. Blocks on its lock, so call it off
  /// the main thread for anything slow.
  pub fn with_db<T>(&self, f: impl FnOnce(&Database) -> T) -> T {
    let state = self.app.state::<AppState>();
    let db = state.db.lock_or_recover();
    f(&db)
  }

  /// Records which repository this window shows, as `set_window_repo_path`.
  pub fn set_window_repo_path(&self, path: &str) {
    let state = self.app.state::<AppState>();
    state
      .window_repo_paths
      .lock_or_recover()
      .insert(self.label.clone(), path.to_string());
  }

  /// Tells this window's model what a screen did, and shows its toasts.
  pub fn notify(&self, event: NativeEvent) {
    if let NativeEvent::Toast {
      title,
      description,
      tone,
      action,
    } = event
    {
      let action = action.map(|action| {
        let run: super::toasts::ToastActionFn = Box::new(move || run_toast_action(action.id));
        (action.label, run)
      });
      super::toasts::push(&self.app, &self.label, tone, &title, &description, action);
      return;
    }
    super::window::native_event(&self.app, &self.label, event);
  }

  pub fn toast(&self, tone: ToastTone, title: impl Into<String>, description: impl Into<String>) {
    self.notify(NativeEvent::Toast {
      title: title.into(),
      description: description.into(),
      tone,
      action: None,
    });
  }

  /// Opens `url` in the default browser, from a task.
  pub fn open_url(&self, url: &str) {
    let (app, url) = (self.app.clone(), url.to_string());
    tauri::async_runtime::spawn(async move {
      use tauri_plugin_opener::OpenerExt;
      if let Err(err) = app.opener().open_url(&url, None::<&str>) {
        log::warn!("failed to open {url}: {err}");
      }
    });
  }
}

impl Backend {
  /// The window's remote repository, when `path` names it (`activeForPath`).
  pub fn remote(&self, path: &str) -> Option<Remote> {
    let repo = remote_repo::for_window(&self.label).filter(|repo| repo.matches(path))?;
    Some(Remote::new(repo, std::sync::Arc::new(self.clone())))
  }

  /// The window's remote repository, when `path` is anything inside it
  /// (`remoteRepositoryContaining`).
  pub fn remote_containing(&self, path: &str) -> Option<Remote> {
    let repo = remote_repo::for_window(&self.label).filter(|repo| repo.contains(path))?;
    Some(Remote::new(repo, std::sync::Arc::new(self.clone())))
  }

  /// `repoStateScope`: where `path`'s review state is kept on this machine.
  /// `assertLocalOperation`: refuses `operation` for a remote repository.
  pub fn local_only(&self, path: &str, operation: &str) -> Result<(), String> {
    match self.remote_containing(path) {
      Some(_) => Err(remote_repo::unsupported(operation)),
      None => Ok(()),
    }
  }

  pub fn state_scope(&self, path: &str) -> String {
    match remote_repo::for_window(&self.label).filter(|repo| repo.contains(path)) {
      Some(repo) => repo.scope(path),
      None => path.to_string(),
    }
  }

  pub(crate) fn exec_pool(
    &self,
  ) -> std::sync::Arc<crate::core::remote_ssh_transport::SshConnectionPool> {
    self
      .app
      .state::<crate::commands::remote_control::RemoteExecState>()
      .0
      .clone()
  }
}

impl Transport for Backend {
  fn read(
    &self,
    endpoint: &SshEndpoint,
    request: TreqCommandRequest,
  ) -> Result<serde_json::Value, String> {
    self.with_db(|db| feature_preview::require_remote_for_source(db, &endpoint.source))?;
    let pool = self.exec_pool();
    // A relay drop or dead network leaves a pooled connection that fails the
    // first read; `execute_remote_read` sends it once more on a fresh one.
    crate::gpui_shell::block_on(remote_command::execute_remote_read::<serde_json::Value>(
      &pool,
      endpoint,
      request,
      ExecLimits::default(),
      &CancellationToken::new(),
    ))
    .map_err(|error| error.to_string())
  }

  fn mutate(
    &self,
    endpoint: &SshEndpoint,
    request: TreqCommandRequest,
  ) -> Result<Mutation, String> {
    self.with_db(|db| feature_preview::require_remote_for_source(db, &endpoint.source))?;
    let pool = self.exec_pool();
    let metrics = pool.metrics.clone();
    let outcome = crate::gpui_shell::block_on(remote_command::retry_after_reconnect::<
      serde_json::Value,
      _,
    >(
      &pool,
      endpoint,
      request,
      ExecLimits::default(),
      &CancellationToken::new(),
      move |verification| metrics.record_post_reconnect_verification(verification),
    ))
    .map_err(|error| error.to_string())?;
    Ok(match outcome {
      MutationRetryOutcome::Applied(value) => Mutation::Applied(value),
      MutationRetryOutcome::AlreadyApplied => Mutation::AlreadyApplied,
      MutationRetryOutcome::Ambiguous { reason } => Mutation::Ambiguous(reason),
    })
  }

  fn report(&self, mutation: &Mutation) {
    self.notify(NativeEvent::RemoteMutation {
      status: mutation.status(),
      reason: match mutation {
        Mutation::Ambiguous(reason) => Some(reason.clone()),
        _ => None,
      },
    });
  }

  fn cut_off(&self, endpoint_id: &str, cutoff: Cutoff) {
    remote_repo::record_cutoff(endpoint_id, cutoff);
    self.notify(NativeEvent::RemoteCutoff {
      endpoint_id: endpoint_id.to_string(),
      reason: cutoff.id().to_string(),
    });
  }

  fn reads(&self) -> &Reads {
    static READS: std::sync::OnceLock<Reads> = std::sync::OnceLock::new();
    READS.get_or_init(Reads::default)
  }
}
