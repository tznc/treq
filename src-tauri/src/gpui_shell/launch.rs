//! Agent sessions a shell window starts without React: the backend side of
//! [`LaunchHost`], automatic reviews, agent deep links and `treq send`.
//!
//! Each start keeps the session's start data for the window's terminal
//! pane (`core::agent_launch::remember_pending`), records its prompt, and
//! tells the window's React app which session to show
//! ([`NativeEvent::AgentSessionStarted`]), since React still routes the
//! selection.

use crate::result_ext::ResultExt;
use tauri::{AppHandle, Manager};

use super::native::{Backend, NativeEvent, ToastTone};
use super::screen::launch_data::LaunchHost;
use super::window::WindowHost;
use crate::{
  core::agent_launch::{
    self as launch, AgentDeepLinkRequest, DeepLinkRoute, IssueSource, LaunchRequest, LaunchStore,
    PendingSession, SendAsset, SessionStart, TaskRequest,
  },
  lock_ext::LockExt,
  AppState,
};

/// The backend as `core::agent_launch` sees it.
struct Store<'a>(&'a Backend);

impl LaunchStore for Store<'_> {
  fn create_session(
    &self,
    repo: &str,
    workspace_id: Option<i64>,
    name: &str,
  ) -> Result<i64, String> {
    let scope = self.0.state_scope(repo);
    self
      .0
      .with_db(|db| crate::core::sessions::create_session(db, &scope, workspace_id, name.into()))
  }

  fn session_count(&self, repo: &str, workspace_id: Option<i64>) -> usize {
    crate::core::sessions::list_sessions(&self.0.state_scope(repo))
      .unwrap_or_default()
      .iter()
      .filter(|s| s.workspace_id == workspace_id)
      .count()
  }

  fn repo_setting(&self, repo: &str, key: &str) -> Option<String> {
    self
      .0
      .with_db(|db| db.get_repo_setting(repo, key).ok().flatten())
  }

  fn app_setting(&self, key: &str) -> Option<String> {
    self.0.with_db(|db| db.get_setting(key).ok().flatten())
  }

  fn set_repo_setting(&self, repo: &str, key: &str, value: &str) -> Result<(), String> {
    self
      .0
      .with_db(|db| db.set_repo_setting(repo, key, value))
      .map_err(|e| e.to_string())?;
    self.0.notify(NativeEvent::RepoSettingsChanged {
      repo_path: repo.into(),
    });
    Ok(())
  }

  fn add_prompt_history(
    &self,
    repo: &str,
    workspace_id: Option<i64>,
    session_id: i64,
    prompt: &str,
    agent: Option<&str>,
  ) -> Result<(), String> {
    crate::core::sessions::add_prompt_history(
      &self.0.state_scope(repo),
      workspace_id,
      session_id,
      prompt,
      agent,
    )
  }

  fn open_issue_workspace(
    &self,
    repo: &str,
    source: IssueSource,
    id: &str,
    title: &str,
    url: &str,
  ) -> Result<(i64, bool), String> {
    self.0.local_only(repo, "Opening an issue's workspace")?;
    let opened = match source {
      IssueSource::Github => {
        let number: u64 = id
          .parse()
          .map_err(|_| format!("Invalid GitHub issue number: {id}"))?;
        crate::core::github_issues::open_or_create_workspace_from_github_issue(
          repo, number, title, url,
        )
        .map(|(workspace, created)| (workspace.id, created))?
      }
      IssueSource::Linear => {
        let client = self.0.with_db(|db| {
          crate::core::feature_preview::require(
            db,
            crate::core::feature_preview::PreviewFeature::LinearIntegration,
          )?;
          crate::linear::resolve_linear_client(repo, db)
        })?;
        let result = crate::gpui_shell::block_on(async {
          let issue = crate::linear::linear_get_issue_impl(&client, id).await?;
          crate::commands::linear::open_or_create_workspace_from_linear_issue(repo, &issue).await
        })?;
        (result.workspace_id, result.created)
      }
    };
    self.0.notify(NativeEvent::WorkspaceChanged {
      repo_path: repo.into(),
    });
    Ok(opened)
  }
}

impl Backend {
  /// The repository this window shows as its main one.
  fn main_repo_path(&self) -> Option<String> {
    self
      .app()
      .state::<AppState>()
      .window_repo_paths
      .lock_or_recover()
      .get(self.label())
      .cloned()
  }
}

impl LaunchHost for Backend {
  fn start_agent_task(&self, task: &TaskRequest) -> Result<bool, String> {
    let Some((start, toasts)) = launch::start_agent_task(&Store(self), task)? else {
      return Ok(false);
    };
    for toast in toasts {
      let tone = if toast.warning {
        ToastTone::Warning
      } else {
        ToastTone::Success
      };
      self.toast(tone, toast.title, toast.description);
    }
    self.session_started(&start, false);
    Ok(true)
  }

  fn launch_session(&self, request: &LaunchRequest, select: bool) -> Result<SessionStart, String> {
    let start = launch::launch_session(&Store(self), request)?;
    self.session_started(&start, select);
    Ok(start)
  }

  fn session_started(&self, start: &SessionStart, select: bool) {
    let main = self
      .main_repo_path()
      .unwrap_or_else(|| start.repo_path.clone());
    let pane_session_id = launch::pane_session_id(&start.repo_path, &main, start.session_id);
    launch::remember_pending(self.label(), pane_session_id, start.pending());
    if let Err(err) = launch::record_prompt(&Store(self), start) {
      log::warn!("failed to record prompt history: {err}");
    }
    self.notify(NativeEvent::AgentSessionStarted {
      session: start.clone(),
      pane_session_id,
      select,
    });
  }

  fn resolve_agent(&self, repo_path: &str, agent: Option<&str>) -> Option<String> {
    launch::resolve_agent(&Store(self), repo_path, agent)
  }

  fn pending_session(&self, pane_id: i64) -> Option<PendingSession> {
    launch::pending_session(self.label(), pane_id)
  }

  fn send_assets(&self) -> Vec<SendAsset> {
    launch::send_assets()
  }

  fn dismiss_send_asset(&self, id: &str) {
    launch::dismiss_send_asset(id);
  }

  fn record_review(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    scope: &str,
    markdown: &str,
    mode: &str,
  ) {
    if let Err(err) = crate::core::review_history::record_sent_review(
      repo_path,
      workspace_id,
      scope,
      markdown,
      mode,
    ) {
      log::warn!("Failed to record review history: {err}");
    }
  }
}

/// The repositories window `label` shows: its main one, then supporting.
fn window_repos(app: &AppHandle, label: &str) -> Vec<String> {
  let state = app.state::<AppState>();
  let mut repos: Vec<String> = state
    .window_repo_paths
    .lock_or_recover()
    .get(label)
    .cloned()
    .into_iter()
    .collect();
  repos.extend(
    state
      .window_supporting_repo_paths
      .lock_or_recover()
      .get(label)
      .cloned()
      .unwrap_or_default(),
  );
  repos
}

/// `handleAutoReviewEvent`: starts the review the backend found due, in
/// one window showing its repository (the focused one, as for deep links), so
/// a repo open in several windows gets one review per operation.
pub fn auto_review(app: &AppHandle, event: crate::auto_review::AutoReviewEvent) {
  if let Some(label) =
    crate::agent_runtime::find_target_window_label_for_repo(app, &event.repo_path)
  {
    let backend = Backend::new(app.clone(), label);
    let event = event.clone();
    std::thread::spawn(move || {
      let workspaces = crate::core::list_all_workspaces(&event.repo_path).unwrap_or_default();
      let Some(workspace) = workspaces.iter().find(|w| w.id == event.workspace_id) else {
        return;
      };
      let summary = launch::auto_review_summary(&event.trigger, Some(&workspace.branch_name));
      let (prompt, agent) = launch::prepare_workspace_review(
        &Store(&backend),
        &event.repo_path,
        workspace.id,
        &summary,
      );
      let request = LaunchRequest {
        repo_path: event.repo_path.clone(),
        workspace_id: Some(workspace.id),
        prompt: Some(prompt),
        mode: Some("acceptEdits".into()),
        agent: launch::agent_kind(agent.as_deref()),
        session_name: Some("AI Review".into()),
      };
      if let Err(err) = backend.launch_session(&request, true) {
        backend.toast(ToastTone::Error, "Failed to start automatic review", err);
      }
    });
  }
}

/// `handleStartAgentRequest`: starts a deep link's agent on its branch's
/// workspace in window `label`.
fn start_request(app: &AppHandle, label: String, request: AgentDeepLinkRequest) {
  let backend = Backend::new(app.clone(), label);
  std::thread::spawn(move || start_deep_link(&backend, &request));
}

/// `handleStartAgentRequest` through `host`: the agent on the workspace of
/// the request's branch, shown once it starts. Blocks.
pub(super) fn start_deep_link(host: &dyn WindowHost, request: &AgentDeepLinkRequest) {
  let workspaces = host.workspaces(&request.repo).unwrap_or_default();
  let Some(workspace) = launch::find_workspace_by_branch(&workspaces, &request.branch) else {
    host.toast(
      ToastTone::Error,
      "Workspace not found",
      &format!(
        "Branch '{}' is not available in this window.",
        request.branch
      ),
    );
    return;
  };
  let launched = LaunchRequest {
    repo_path: request.repo.clone(),
    workspace_id: Some(workspace.id),
    prompt: Some(request.prompt.clone()),
    mode: Some(request.mode.clone()),
    agent: Some(request.agent.clone()),
    session_name: None,
  };
  match host.launch(&launched, true) {
    Ok(()) => launch::deep_links()
      .lock_or_recover()
      .mark_processed(&request.request_id),
    Err(err) => host.toast(ToastTone::Error, "Failed to start agent", &err),
  }
}

/// `processAgentDeepLinkRequests` for a `treq://agent/start` link meant for
/// window `label` (`None`: no window shows its repository, so one opens
/// and starts it once it shows the repository).
pub fn agent_deep_link(app: &AppHandle, label: Option<String>, url: &str) {
  let Some(request) = launch::parse_agent_deep_link_url(url) else {
    log::info!("agent deep link ignored (malformed)");
    return;
  };
  let repos = label.as_deref().map(|label| window_repos(app, label));
  let route = launch::deep_links()
    .lock_or_recover()
    .route(&request, repos.as_deref());
  match (route, label) {
    (DeepLinkRoute::Start, Some(label)) => start_request(app, label, request),
    (DeepLinkRoute::OpenWindow, _) => open_window_for(app, &request.repo),
    _ => {}
  }
}

fn open_window_for(app: &AppHandle, repo: &str) {
  let label = crate::open_new_window::new_repo_window_label();
  let title = crate::open_new_window::new_repo_window_title(repo);
  let (app_for_window, repo) = (app.clone(), repo.to_string());
  app
    .run_on_main_thread(move || {
      if let Err(err) = super::open_window(&app_for_window, &label, Some(repo), &title) {
        log::error!("failed to open a window for an agent deep link: {err}");
      }
    })
    .ignore_closed();
}

/// Starts the deep links queued for `repo` once window `label` shows it.
/// Waits a moment first, so the window's React app listens for the start.
pub fn window_shows_repo(app: &AppHandle, label: &str, repo: &str) {
  let queued = launch::deep_links().lock_or_recover().pop(repo);
  if queued.is_empty() {
    return;
  }
  let (app, label) = (app.clone(), label.to_string());
  tauri::async_runtime::spawn(async move {
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    for request in queued {
      if !launch::deep_links()
        .lock_or_recover()
        .is_processed(&request.request_id)
      {
        start_request(&app, label.clone(), request);
      }
    }
  });
}

/// A `treq send` attachment: kept for GPUI's terminal panes, which draw
/// again to show it.
pub fn treq_send(app: &AppHandle, request: &crate::send_dispatch::SendDispatchRequest) {
  let Some(asset) = launch::send_asset(request) else {
    return;
  };
  if launch::add_send_asset(asset) {
    super::defer_main(app, |_| {
      tauri_plugin_gpui::with_app(|cx| cx.refresh_windows()).ignore_closed();
    });
  }
}
