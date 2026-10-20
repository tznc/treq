//! What a window reads from and asks of the backend, faked in tests.
//! Everything blocks, so the window calls it off the main thread.

use std::collections::HashMap;

use tauri::Manager;

use super::data::{Flags, Read, RepoRead};
use crate::{
  core::{
    self,
    agent_launch::LaunchRequest,
    feature_preview::{self, PreviewFeature},
    repo::RepoBranch,
    supporting_repos::{self, SupportingRepo},
    SingleRebaseResult, WorkspaceSidebarStatus,
  },
  github::{GitRemoteInfo, PrInfo},
  gpui_shell::{
    native::{Backend, ToastTone},
    screen::launch_data::LaunchHost,
  },
  local_db::{Session, Workspace},
  lock_ext::LockExt,
  AppState,
};

pub trait WindowHost: Send + Sync {
  fn workspaces(&self, repo: &str) -> Result<Vec<Workspace>, String>;
  fn statuses(&self, repo: &str) -> Result<Vec<WorkspaceSidebarStatus>, String>;
  /// `has_changes` of `useWorkspaceGitDetail`.
  fn has_changes(&self, repo: &str, workspace_id: i64) -> Option<bool>;
  fn sessions(&self, repo: &str) -> Vec<Session>;
  fn branch(&self, repo: &str) -> Option<RepoBranch>;
  fn default_branch(&self, repo: &str) -> Option<String>;
  fn remote(&self, repo: &str) -> Option<GitRemoteInfo>;
  /// The pull request poller's cache, by branch.
  fn prs(&self, repo: &str) -> HashMap<String, Option<PrInfo>>;
  fn supporting(&self, main_repo: &str) -> Vec<SupportingRepo>;
  fn setting(&self, key: &str) -> Option<String>;
  fn flags(&self) -> Flags;

  fn save_setting(&self, key: &str, value: &str);
  /// `init-repo`: sets the repository up, records it as the window's and
  /// starts polling its pull requests.
  fn prepare_repo(&self, main_repo: &str);
  /// `setWindowSupportingRepoPaths`.
  fn set_supporting_paths(&self, paths: Vec<String>);
  /// A folder the user picks, or `None`.
  fn pick_folder(&self) -> Option<String>;
  /// `openRepositoryAtPath`: opens it in this window and reports it
  /// (`NativeEvent::RepoOpened`).
  fn open_repo_at(&self, path: &str);
  fn open_in_new_window(&self, path: &str);
  fn add_supporting(&self, main_repo: &str, path: &str) -> Result<String, String>;
  fn remove_supporting(&self, main_repo: &str, path: &str) -> Result<(), String>;
  /// A Yes/No question in a native dialog.
  fn ask(&self, title: &str, message: &str) -> bool;
  fn launch(&self, request: &LaunchRequest, select: bool) -> Result<(), String>;
  fn toast(&self, tone: ToastTone, title: &str, description: &str);
  fn open_url(&self, url: &str);
  fn open_log_dir(&self);
  fn force_rebase(
    &self,
    repo: &str,
    workspace_id: i64,
    branch: &str,
  ) -> Result<SingleRebaseResult, String>;
  /// Forgets the window's repository (Factory Reset).
  fn factory_reset(&self);
  fn dismiss_send_asset(&self, id: &str);
  /// `detect-editor-apps`, for the "Open in…" menus.
  fn detect_editors(&self);
}

/// Reads what `read` asks of repository `repo`. `known` are the workspaces
/// whose git detail was read last: new ones are read too, as
/// `useWorkspaceGitDetail`'s key changes with them.
pub fn read_repo(host: &dyn WindowHost, repo: &str, read: Read, known: &[i64]) -> RepoRead {
  let mut out = RepoRead {
    prs: host.prs(repo),
    ..RepoRead::default()
  };
  if read.workspaces {
    let workspaces = host.workspaces(repo).ok();
    let statuses = host.statuses(repo).ok();
    let open: Vec<i64> = match (&workspaces, &statuses) {
      (Some(workspaces), statuses) => workspaces
        .iter()
        .filter(|w| {
          !statuses
            .iter()
            .flatten()
            .any(|s| s.current.id == w.id && s.has_conflicts)
        })
        .map(|w| w.id)
        .collect(),
      _ => Vec::new(),
    };
    let mut sorted = open.clone();
    sorted.sort_unstable();
    let mut was = known.to_vec();
    was.sort_unstable();
    if read.detail || sorted != was {
      out.has_changes = Some(
        open
          .iter()
          .filter_map(|id| host.has_changes(repo, *id).map(|c| (*id, c)))
          .collect(),
      );
    }
    out.workspaces = workspaces;
    out.statuses = statuses;
  }
  if read.sessions {
    out.sessions = Some(host.sessions(repo));
  }
  if read.detail {
    out.branch = Some(host.branch(repo));
    out.default_branch = Some(host.default_branch(repo));
    out.remote = Some(host.remote(repo));
  }
  out
}

impl WindowHost for Backend {
  fn workspaces(&self, repo: &str) -> Result<Vec<Workspace>, String> {
    core::list_workspaces(repo)
  }

  fn statuses(&self, repo: &str) -> Result<Vec<WorkspaceSidebarStatus>, String> {
    core::list_workspace_statuses(repo)
  }

  fn has_changes(&self, repo: &str, workspace_id: i64) -> Option<bool> {
    core::workspace_status(repo, Some(workspace_id))
      .ok()
      .map(|status| status.partial.has_changes)
  }

  fn sessions(&self, repo: &str) -> Vec<Session> {
    crate::local_db::get_sessions(&self.state_scope(repo)).unwrap_or_default()
  }

  fn branch(&self, repo: &str) -> Option<RepoBranch> {
    core::repo::get_repo_current_branch(repo).ok()
  }

  fn default_branch(&self, repo: &str) -> Option<String> {
    core::repo::get_repo_default_branch(repo).ok()
  }

  fn remote(&self, repo: &str) -> Option<GitRemoteInfo> {
    crate::github::get_git_remote_url_impl(repo).ok().flatten()
  }

  fn prs(&self, repo: &str) -> HashMap<String, Option<PrInfo>> {
    crate::pr_status::global().list_cached(repo)
  }

  fn supporting(&self, main_repo: &str) -> Vec<SupportingRepo> {
    supporting_repos::list(main_repo).unwrap_or_default()
  }

  fn setting(&self, key: &str) -> Option<String> {
    self.with_db(|db| db.get_setting(key).ok().flatten())
  }

  fn flags(&self) -> Flags {
    self.with_db(|db| Flags {
      linear: feature_preview::is_enabled(db, PreviewFeature::LinearIntegration),
      logs: feature_preview::is_enabled(db, PreviewFeature::Logs),
      remote_ssh: feature_preview::is_enabled(db, PreviewFeature::RemoteSsh),
    })
  }

  fn save_setting(&self, key: &str, value: &str) {
    if let Err(err) = self.with_db(|db| db.set_setting(key, value)) {
      log::warn!("failed to save setting {key}: {err}");
    }
  }

  fn prepare_repo(&self, main_repo: &str) {
    if let Err(err) = core::init(main_repo) {
      log::error!("Failed to init repo: {err}");
    }
    self.set_window_repo_path(main_repo);
    crate::pr_status::global().watch_repo(main_repo);
  }

  fn set_supporting_paths(&self, paths: Vec<String>) {
    let state = self.app().state::<AppState>();
    let mut map = state.window_supporting_repo_paths.lock_or_recover();
    if paths.is_empty() {
      map.remove(self.label());
    } else {
      for path in &paths {
        crate::pr_status::global().watch_repo(path);
      }
      map.insert(self.label().to_string(), paths);
    }
  }

  fn pick_folder(&self) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    self
      .app()
      .dialog()
      .file()
      .set_title("Select Folder")
      .blocking_pick_folder()
      .and_then(|folder| folder.into_path().ok())
      .map(|path| path.to_string_lossy().into_owned())
  }

  fn open_repo_at(&self, path: &str) {
    crate::gpui_shell::screen::open_repo_at(self, path);
  }

  fn open_in_new_window(&self, path: &str) {
    if !core::init(path).unwrap_or(false) {
      self.toast(
        ToastTone::Error,
        "Not a Git Repository",
        "Please select a folder that contains a git repository.",
      );
      return;
    }
    let label = crate::open_new_window::new_repo_window_label();
    let title = crate::open_new_window::new_repo_window_title(path);
    let (app, path) = (self.app().clone(), path.to_string());
    let _ = self.app().run_on_main_thread(move || {
      if let Err(err) = crate::gpui_shell::open_window(&app, &label, Some(path), &title) {
        log::error!("failed to open repo window: {err}");
      }
    });
  }

  fn add_supporting(&self, main_repo: &str, path: &str) -> Result<String, String> {
    let added = supporting_repos::add(main_repo, path)?;
    core::init(&added).map_err(|err| err.to_string())?;
    Ok(added)
  }

  fn remove_supporting(&self, main_repo: &str, path: &str) -> Result<(), String> {
    supporting_repos::remove(main_repo, path)
  }

  fn ask(&self, title: &str, message: &str) -> bool {
    crate::gpui_shell::screen::workspace_ops::WorkspaceActionsHost::ask(self, title, message)
  }

  fn launch(&self, request: &LaunchRequest, select: bool) -> Result<(), String> {
    self.launch_session(request, select).map(drop)
  }

  fn toast(&self, tone: ToastTone, title: &str, description: &str) {
    Backend::toast(self, tone, title, description);
  }

  fn open_url(&self, url: &str) {
    Backend::open_url(self, url);
  }

  fn open_log_dir(&self) {
    use tauri_plugin_opener::OpenerExt;
    let dir = self.app().state::<AppState>().telemetry.log_dir.clone();
    if let Err(err) = self
      .app()
      .opener()
      .open_path(dir.to_string_lossy(), None::<&str>)
    {
      log::warn!("failed to open the log folder: {err}");
    }
  }

  fn force_rebase(
    &self,
    repo: &str,
    workspace_id: i64,
    branch: &str,
  ) -> Result<SingleRebaseResult, String> {
    let style = self.with_db(core::resolve_conflict_marker_style_from_db);
    core::check_and_rebase_workspaces(
      repo,
      Some(workspace_id),
      Some(branch.to_string()),
      Some(true),
      &style,
    )
  }

  fn factory_reset(&self) {
    self.save_setting("last_opened_repo_path", "");
    self.set_window_repo_path("");
  }

  fn dismiss_send_asset(&self, id: &str) {
    LaunchHost::dismiss_send_asset(self, id);
  }

  fn detect_editors(&self) {
    self.with_db(|db| {
      core::editor_apps::detect(db);
    });
  }
}
