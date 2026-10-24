//! What a window reads from and asks of the backend, faked in tests.
//! Everything blocks, so the window calls it off the main thread.
//!
//! For a remote repository the backend reads over SSH what it reads from
//! disk for a local one, and has nothing of what only a local checkout
//! has (pull requests, supporting repositories).

use std::{
  collections::HashMap,
  path::{Path, PathBuf},
  time::Duration,
};

use futures::channel::mpsc::{unbounded, UnboundedReceiver};

use tauri::Manager;

use serde_json::Value;

use super::{
  data::{Flags, Read, RepoRead},
  remote::{RemoteHost, UserManagedValues},
};
use crate::{
  core::{
    self,
    agent_launch::LaunchRequest,
    feature_preview::{self, PreviewFeature},
    remote::{RepositoryInspection, TreqCommandRequest},
    remote_control_plane::{SshEndpoint, SshEndpointSource},
    remote_records::{self, RestoreFailure, RestoreSteps, SavedRepository, UserManagedEndpoint},
    repo::RepoBranch,
    supporting_repos::{self, SupportingRepo},
    workspaces::WorkspaceSummary,
    SingleRebaseResult, WorkspaceSidebarStatus,
  },
  github::{GitRemoteInfo, PrInfo},
  gpui_shell::{
    native::{Backend, ToastTone},
    remote_repo::{self, Cutoff, Mutation, RemoteRepository, Transport},
    screen::{account_data::CloudAction, launch_data::LaunchHost},
  },
  local_db::{Session, Workspace},
  lock_ext::LockExt,
  AppState,
};

/// A running watch: edits arrive on `changes` while `_watcher` lives.
pub struct FileWatch {
  pub changes: UnboundedReceiver<()>,
  pub _watcher: Box<dyn Send>,
}

pub trait WindowHost: RemoteHost + Send + Sync {
  fn workspaces(&self, repo: &str) -> Result<Vec<Workspace>, String>;
  fn statuses(&self, repo: &str) -> Result<Vec<WorkspaceSidebarStatus>, String>;
  /// `has_changes` of `useWorkspaceGitDetail`.
  fn has_changes(&self, repo: &str, workspace_id: i64) -> Option<bool>;
  /// Every workspace's sidebar status and uncommitted changes in one read,
  /// for a repository where that is one round trip. `None` falls back to
  /// `workspaces`, `statuses` and `has_changes`.
  fn summaries(&self, _repo: &str) -> Option<Vec<WorkspaceSummary>> {
    None
  }
  /// The workspaces the local database lists, with no jj or network read,
  /// so a window draws its rows before `statuses` and `has_changes` finish.
  /// `None` where there is no such read (a repository over SSH).
  fn listed_workspaces(&self, _repo: &str) -> Option<Vec<Workspace>> {
    None
  }
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
  /// Whether `path` is a Git or jj repository, without writing to it.
  fn is_repo(&self, path: &str) -> bool;
  /// Initializes a Git repository in the folder at `path`.
  fn init_repo(&self, path: &str) -> Result<(), String>;
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
  fn launch(&self, request: &LaunchRequest, select: bool) -> Result<(), String>;
  fn toast(&self, tone: ToastTone, title: &str, description: &str);
  fn open_url(&self, url: &str);
  fn open_log_dir(&self);
  /// Watches a local checkout for edits on disk (an agent's, an editor's):
  /// each settled burst sends once. `None` when there is nothing to watch.
  fn watch_files(&self, path: &str) -> Option<FileWatch>;
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
    let summaries = host.summaries(repo);
    let (workspaces, statuses) = match &summaries {
      Some(summaries) => (
        Some(summaries.iter().map(|s| s.workspace.clone()).collect()),
        Some(
          summaries
            .iter()
            .map(|s| WorkspaceSidebarStatus {
              current: s.workspace.clone(),
              has_conflicts: s.has_conflicts,
              last_activity_at: s.last_activity_at.clone(),
            })
            .collect::<Vec<_>>(),
        ),
      ),
      None => (host.workspaces(repo).ok(), host.statuses(repo).ok()),
    };
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
          .filter_map(|id| {
            match &summaries {
              Some(summaries) => summaries
                .iter()
                .find(|s| s.workspace.id == *id)
                .map(|s| s.has_changes),
              None => host.has_changes(repo, *id),
            }
            .map(|c| (*id, c))
          })
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
    read_detail(host, repo, &mut out);
  }
  out
}

/// The repository's branch, default branch and remote.
pub fn read_detail(host: &dyn WindowHost, repo: &str, out: &mut RepoRead) {
  out.branch = Some(host.branch(repo));
  out.default_branch = Some(host.default_branch(repo));
  out.remote = Some(host.remote(repo));
}

/// Repositories `open_in_new_window` initialized for the window it opens,
/// so that window's `prepare_repo` doesn't initialize them again.
static JUST_INITIALIZED: std::sync::Mutex<Vec<(String, std::time::Instant)>> =
  std::sync::Mutex::new(Vec::new());

/// How long a window may take to open after its repository was initialized
/// for it.
const JUST_INITIALIZED_FOR: Duration = Duration::from_secs(30);

pub(crate) fn note_just_initialized(repo: &str) {
  let mut noted = JUST_INITIALIZED.lock_or_recover();
  noted.retain(|(_, at)| at.elapsed() < JUST_INITIALIZED_FOR);
  noted.push((repo.to_string(), std::time::Instant::now()));
}

/// Whether `repo` was initialized for the window being prepared; a note
/// serves one window.
fn take_just_initialized(repo: &str) -> bool {
  let mut noted = JUST_INITIALIZED.lock_or_recover();
  noted.retain(|(_, at)| at.elapsed() < JUST_INITIALIZED_FOR);
  match noted.iter().position(|(noted, _)| noted == repo) {
    Some(index) => {
      noted.remove(index);
      true
    }
    None => false,
  }
}

impl Backend {
  /// `InspectRepository` of the window's remote repository `repo`.
  pub(crate) fn inspect_remote(&self, repo: &str) -> Option<RepositoryInspection> {
    let remote = self.remote(repo)?;
    remote
      .read(TreqCommandRequest::InspectRepository {
        repo: remote.repo.canonical_path.clone(),
      })
      .ok()
  }
}

impl WindowHost for Backend {
  fn workspaces(&self, repo: &str) -> Result<Vec<Workspace>, String> {
    if let Some(remote) = self.remote(repo) {
      return remote.workspaces();
    }
    core::list_workspaces(repo)
  }

  fn statuses(&self, repo: &str) -> Result<Vec<WorkspaceSidebarStatus>, String> {
    let Some(remote) = self.remote(repo) else {
      return core::list_workspace_statuses(repo);
    };
    // The host has no sidebar listing: each workspace's own inspection.
    Ok(
      remote
        .workspaces()?
        .into_iter()
        .map(|workspace| {
          let has_conflicts = remote
            .status(Some(workspace.id))
            .is_ok_and(|s| !s.conflicted_files.is_empty());
          WorkspaceSidebarStatus {
            current: workspace,
            has_conflicts,
            last_activity_at: None,
          }
        })
        .collect(),
    )
  }

  fn has_changes(&self, repo: &str, workspace_id: i64) -> Option<bool> {
    match self.remote(repo) {
      Some(remote) => remote
        .status(Some(workspace_id))
        .ok()
        .map(|status| status.partial.has_changes),
      None => core::workspace_has_changes(repo, workspace_id).ok(),
    }
  }

  fn listed_workspaces(&self, repo: &str) -> Option<Vec<Workspace>> {
    match self.remote(repo) {
      Some(_) => None,
      None => core::list_workspaces(repo).ok(),
    }
  }

  fn summaries(&self, repo: &str) -> Option<Vec<WorkspaceSummary>> {
    let remote = self.remote(repo)?;
    remote.workspace_summaries_if_supported()
  }

  fn sessions(&self, repo: &str) -> Vec<Session> {
    crate::core::sessions::list_sessions(&self.state_scope(repo)).unwrap_or_default()
  }

  fn branch(&self, repo: &str) -> Option<RepoBranch> {
    if self.remote(repo).is_some() {
      let inspection = self.inspect_remote(repo)?;
      return Some(RepoBranch {
        display_ref: inspection
          .current_branch
          .clone()
          .unwrap_or_else(|| inspection.current_change_id.chars().take(8).collect()),
        is_detached: inspection.current_branch.is_none(),
        current_branch: inspection.current_branch,
      });
    }
    core::repo::get_repo_current_branch(repo).ok()
  }

  fn default_branch(&self, repo: &str) -> Option<String> {
    if self.remote(repo).is_some() {
      return self.inspect_remote(repo).map(|i| i.default_branch);
    }
    core::repo::get_repo_default_branch(repo).ok()
  }

  fn remote(&self, repo: &str) -> Option<GitRemoteInfo> {
    if let Some(remote) = Backend::remote(self, repo) {
      return remote.git_remote().ok().flatten();
    }
    crate::core::github_cli::remote(repo).ok().flatten()
  }

  fn prs(&self, repo: &str) -> HashMap<String, Option<PrInfo>> {
    // The poller watches local checkouts only.
    if Backend::remote(self, repo).is_some() {
      return HashMap::new();
    }
    crate::pr_status::global().list_cached(repo)
  }

  fn supporting(&self, main_repo: &str) -> Vec<SupportingRepo> {
    if Backend::remote(self, main_repo).is_some() {
      return Vec::new();
    }
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

  fn is_repo(&self, path: &str) -> bool {
    core::is_repository(path)
  }

  fn init_repo(&self, path: &str) -> Result<(), String> {
    core::init_git_repository(path)
  }

  fn prepare_repo(&self, main_repo: &str) {
    if !take_just_initialized(main_repo) {
      if let Err(err) = core::init_deferring_rebase(main_repo) {
        log::error!("Failed to init repo: {err}");
      }
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
    if !core::init_deferring_rebase(path).unwrap_or(false) {
      self.toast(
        ToastTone::Error,
        "Not a Git Repository",
        "Please select a folder that contains a git repository.",
      );
      return;
    }
    note_just_initialized(path);
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

  fn launch(&self, request: &LaunchRequest, select: bool) -> Result<(), String> {
    self.launch_session(request, select).map(drop)
  }

  fn toast(&self, tone: ToastTone, title: &str, description: &str) {
    Backend::toast(self, tone, title, description);
  }

  fn open_url(&self, url: &str) {
    Backend::open_url(self, url);
  }

  fn watch_files(&self, path: &str) -> Option<FileWatch> {
    if self.remote(path).is_some() {
      return None;
    }
    let (tx, changes) = unbounded();
    let mut gitignore = GitignoreFilter::new(Path::new(path));
    let mut watcher = notify_debouncer_full::new_debouncer(
      WATCH_SETTLE,
      None,
      move |result: notify_debouncer_full::DebounceEventResult| match result {
        Ok(events) => {
          if events
            .iter()
            .flat_map(|e| e.paths.iter())
            .any(|p| !is_ignored(p) && !gitignore.ignores(p))
          {
            let _ = tx.unbounded_send(());
          }
        }
        Err(errors) => log::warn!("File watch errors: {errors:?}"),
      },
    )
    .map_err(|err| log::warn!("Failed to create a file watcher: {err}"))
    .ok()?;
    watcher
      .watch(Path::new(path), notify::RecursiveMode::Recursive)
      .map_err(|err| log::warn!("Failed to watch {path}: {err}"))
      .ok()?;
    Some(FileWatch {
      changes,
      _watcher: Box::new(watcher),
    })
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
    self.with_db(remote_records::clear_last_opened);
    self.set_window_repo_path("");
    remote_repo::set_window(self.label(), None);
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

/// `restoreSavedRemoteRepository`'s steps on one endpoint: a probe is the
/// reconnect, and a pinned host key is required.
struct Steps<'a> {
  host: &'a Backend,
  endpoint: &'a SshEndpoint,
  path: &'a str,
}

impl RestoreSteps for Steps<'_> {
  fn reconnect(&self) -> bool {
    self
      .host
      .remote_read(
        self.endpoint,
        TreqCommandRequest::ProbeRepo {
          repo: self.path.to_string(),
        },
      )
      .is_ok()
  }

  fn validate_host_key(&self) -> bool {
    self
      .endpoint
      .host_keys
      .iter()
      .any(|key| !key.fingerprint_sha256.is_empty())
  }

  fn inspect(&self, canonical_path: &str) -> Result<RepositoryInspection, String> {
    let value = self.host.remote_read(
      self.endpoint,
      TreqCommandRequest::InspectRepository {
        repo: canonical_path.to_string(),
      },
    )?;
    serde_json::from_value(value).map_err(|e| format!("invalid_remote_json: {e}"))
  }
}

fn now_iso() -> String {
  chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl RemoteHost for Backend {
  fn saved_repositories(&self, endpoint_id: &str, generation: u64) -> Vec<SavedRepository> {
    self.with_db(|db| remote_records::saved_repositories(db, endpoint_id, generation))
  }

  fn register_user_managed(&self, values: &UserManagedValues) -> SshEndpoint {
    let record = UserManagedEndpoint {
      id: format!("user-managed-{}", super::remote::now_ms()),
      display_name: values.display_name.clone(),
      hostname: values.hostname.clone(),
      port: values.port,
      username: values.username.clone(),
      host_key_fingerprint: values.host_key_fingerprint.clone(),
      auth_identity_reference: values.auth_identity_reference.clone(),
      alias: values.alias.clone(),
      created_at: now_iso(),
    };
    let endpoint = record.to_endpoint();
    self.with_db(|db| remote_records::save_user_managed_endpoint(db, record));
    endpoint
  }

  fn remote_read(
    &self,
    endpoint: &SshEndpoint,
    request: TreqCommandRequest,
  ) -> Result<Value, String> {
    Transport::read(self, endpoint, request).inspect_err(|error| {
      if let Some(cutoff) = Cutoff::from_error(error) {
        self.cut_off(&endpoint.id, cutoff);
      }
    })
  }

  fn remote_change(
    &self,
    endpoint: &SshEndpoint,
    request: TreqCommandRequest,
  ) -> Result<Mutation, String> {
    remote_repo::keyed_endpoint_mutation(self, endpoint, &request).inspect_err(|error| {
      if let Some(cutoff) = Cutoff::from_error(error) {
        self.cut_off(&endpoint.id, cutoff);
      }
    })
  }

  fn save_remote_repository(
    &self,
    endpoint_id: &str,
    generation: u64,
    path: &str,
  ) -> Result<SavedRepository, String> {
    self.with_db(|db| {
      let record = remote_records::upsert(
        db,
        endpoint_id,
        generation,
        path,
        None,
        Some(Some(now_iso())),
      )?;
      remote_records::remember_last_opened(db, &record.id);
      Ok(record)
    })
  }

  fn restore_remote(
    &self,
    descriptor: &SavedRepository,
    endpoint: &SshEndpoint,
    generation: u64,
    transition: bool,
  ) -> Result<(SavedRepository, RepositoryInspection), RestoreFailure> {
    let state = self.app().state::<AppState>();
    let steps = Steps {
      host: self,
      endpoint,
      path: &descriptor.canonical_remote_path,
    };
    remote_records::restore_saved_repository(
      || state.db.lock_or_recover(),
      descriptor,
      generation,
      transition,
      &steps,
      &now_iso(),
    )
  }

  fn last_remote(&self) -> Option<(SavedRepository, SshEndpoint, u64)> {
    let id = self.with_db(remote_records::last_opened_id)?;
    self.saved_remote(&id)
  }

  fn saved_remote(&self, id: &str) -> Option<(SavedRepository, SshEndpoint, u64)> {
    let descriptor = self.with_db(|db| remote_records::saved_repository(db, id))?;
    let (endpoint, generation) = self.machine_endpoint(&descriptor.endpoint_id)?;
    // A user-managed endpoint reconnects on the generation it was saved on.
    let generation = match endpoint.source {
      SshEndpointSource::UserManaged => descriptor.endpoint_generation,
      _ => generation,
    };
    Some((descriptor, endpoint, generation))
  }

  fn machine_endpoint(&self, id: &str) -> Option<(SshEndpoint, u64)> {
    let (user_managed, remote_ssh) = self.with_db(|db| {
      let user_managed = remote_records::user_managed_endpoints(db)
        .into_iter()
        .find(|e| e.id == id);
      let remote_ssh = feature_preview::is_enabled(db, PreviewFeature::RemoteSsh);
      (user_managed, remote_ssh)
    });
    // User-managed endpoints connect only while the preview is on; the
    // cloud workspace regardless.
    if let Some(record) = user_managed.filter(|_| remote_ssh) {
      return Some((record.to_endpoint(), 0));
    }
    let cloud = crate::commands::cloud::owned_cloud(self.app())?;
    let status = cloud.snapshot().cloud.instance_status?;
    let endpoint: SshEndpoint = serde_json::from_value(status.endpoint.clone()?).ok()?;
    if endpoint.id != id {
      return None;
    }
    let generation = match endpoint.source {
      SshEndpointSource::Managed { generation, .. } => generation,
      _ => status.instance.as_ref().map_or(0, |i| i.generation),
    };
    Some((endpoint, generation))
  }

  fn cloud_action(&self, action: CloudAction) {
    crate::gpui_shell::screen::account_data::AccountHost::cloud_action(self, action);
  }

  fn show_remote(&self, repository: Option<RemoteRepository>) {
    remote_repo::set_window(self.label(), repository);
  }
}

/// How long edits must settle before the Changes tab reads again.
const WATCH_SETTLE: Duration = Duration::from_millis(500);

/// Writes nobody reviews: jj's and git's own state (jj writes `.jj` on
/// every read, so watching it would loop), dependencies and build output.
fn is_ignored(path: &Path) -> bool {
  path.components().any(|c| {
    matches!(
      c.as_os_str().to_str(),
      Some(".jj" | ".git" | "node_modules" | "target")
    )
  }) || path
    .to_str()
    .is_some_and(|p| p.ends_with(".swp") || p.ends_with('~'))
}

/// Applies the checkout's gitignore rules, so `dist/`, `.next/` and other
/// ignored output stop triggering reloads of the Changes tab. Rules come from
/// every `.gitignore` between the changed path and the root, plus
/// `.git/info/exclude` and the user's global ignore file, with git's
/// precedence (deeper files win).
struct GitignoreFilter {
  root: PathBuf,
  /// Lowest precedence first: global ignore file, then `.git/info/exclude`.
  base: Vec<ignore::gitignore::Gitignore>,
  /// Each directory's own `.gitignore`, read on first use.
  dirs: HashMap<PathBuf, Option<ignore::gitignore::Gitignore>>,
}

impl GitignoreFilter {
  fn new(root: &Path) -> Self {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut exclude = ignore::gitignore::GitignoreBuilder::new(&root);
    exclude.add(root.join(".git").join("info").join("exclude"));
    let base = [
      Some(ignore::gitignore::Gitignore::global().0),
      exclude.build().ok(),
    ]
    .into_iter()
    .flatten()
    .collect();
    Self {
      root,
      base,
      dirs: HashMap::new(),
    }
  }

  fn ignores(&mut self, path: &Path) -> bool {
    if path.file_name().is_some_and(|name| name == ".gitignore") {
      // The rules themselves changed: drop what was read and let it reload.
      self.dirs.clear();
      return false;
    }
    let Ok(relative) = path.strip_prefix(&self.root) else {
      return false;
    };
    let is_dir = path.is_dir();
    // Deepest directory first: its rules override its ancestors'.
    let dirs: Vec<PathBuf> = relative
      .ancestors()
      .skip(1)
      .map(|dir| self.root.join(dir))
      .collect();
    for dir in dirs {
      let matcher = self.dirs.entry(dir.clone()).or_insert_with(|| {
        let mut builder = ignore::gitignore::GitignoreBuilder::new(&dir);
        builder.add(dir.join(".gitignore"));
        builder.build().ok()
      });
      if let Some(matcher) = matcher {
        match matcher.matched_path_or_any_parents(path, is_dir) {
          ignore::Match::Ignore(_) => return true,
          ignore::Match::Whitelist(_) => return false,
          ignore::Match::None => {}
        }
      }
    }
    for matcher in self.base.iter().rev() {
      match matcher.matched_path_or_any_parents(path, is_dir) {
        ignore::Match::Ignore(_) => return true,
        ignore::Match::Whitelist(_) => return false,
        ignore::Match::None => {}
      }
    }
    false
  }
}

#[cfg(test)]
mod watch_tests {
  use super::*;

  #[test]
  fn gitignore_filter_follows_nested_rules() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("sub/dist")).unwrap();
    std::fs::write(root.join(".gitignore"), "dist/\n*.log\n").unwrap();
    std::fs::write(root.join("sub/.gitignore"), "!keep.log\n").unwrap();
    let mut filter = GitignoreFilter::new(&root);
    assert!(filter.ignores(&root.join("dist/app.js")));
    assert!(filter.ignores(&root.join("sub/dist/app.js")));
    assert!(filter.ignores(&root.join("debug.log")));
    assert!(!filter.ignores(&root.join("sub/keep.log")));
    assert!(!filter.ignores(&root.join("src/main.rs")));
    // Editing a `.gitignore` is a change worth reloading for.
    assert!(!filter.ignores(&root.join(".gitignore")));
    // Paths outside the root are never filtered out.
    assert!(!filter.ignores(Path::new("/elsewhere/dist/app.js")));
  }

  #[test]
  fn ignores_vcs_state_and_build_output() {
    assert!(is_ignored(Path::new("/w/.jj/working_copy/checkout")));
    assert!(is_ignored(Path::new("/w/.git/index")));
    assert!(is_ignored(Path::new("/w/web/node_modules/a/index.js")));
    assert!(is_ignored(Path::new("/w/src-tauri/target/debug/treq")));
    assert!(is_ignored(Path::new("/w/src/.main.rs.swp")));
    assert!(!is_ignored(Path::new("/w/src/main.rs")));
    assert!(!is_ignored(Path::new("/w/src/targets.rs")));
  }
}
