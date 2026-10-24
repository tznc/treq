//! The remote (SSH) repository a window shows, and how GPUI's screens reach
//! it.
//!
//! React's data layer sends every call for the active remote repository
//! over SSH as a typed command (`repository-adapter.ts`), refuses what only
//! works on a local clone (`assertLocalOperation`), and reads defaults where
//! a remote has no data yet (`localReadOr`). GPUI's hosts do the same: React
//! says which remote repository the window shows (the snapshot's
//! `repository`), and the backend sends each read and change for a path in
//! it through [`Remote`] instead of running it on this machine.

use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use crate::{
  core::{
    remote::{FileRevision, TreqCommandRequest},
    remote_control_plane::SshEndpoint,
    remote_idempotency::{self, Outcome},
    workspaces::WorkspaceSummary,
    PullWorkspaceResult, WorkspaceFileHunksBatch, WorkspaceFileHunksBatchFile, WorkspaceStatus,
  },
  github::GitRemoteInfo,
  jj::{JjBranch, JjDiffHunk, JjFileChange, JjFileDiff, JjFileLines, JjLogResult, JjRevisionDiff},
  local_db::{StashEntry, Workspace},
  lock_ext::LockExt,
};

/// Read by the local DB as a remote repository's state scope (`repoStateScope`).
const STATE_SCOPE_PREFIX: &str = "treq-remote-state:";
const WORKSPACES_DIR: &str = ".treq/workspaces/";
/// What a change reports when it landed before the connection dropped.
pub const ALREADY_APPLIED: &str = "Already applied before the connection dropped";

/// The remote repository React shows (`ActiveRepository` with an SSH
/// location), as the snapshot carries it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRepository {
  /// `None` when the repository has no resolved endpoint: every call fails.
  pub endpoint: Option<SshEndpoint>,
  pub endpoint_id: Option<String>,
  /// The repository's path on the host, which the screens use as its path.
  pub canonical_path: String,
  /// `repositoryCacheKey`, which scopes its review state on this machine.
  pub cache_key: String,
  pub display_name: String,
}

fn trim_slashes(path: &str) -> &str {
  if path.len() > 1 {
    path.trim_end_matches('/')
  } else {
    path
  }
}

impl RemoteRepository {
  /// `matchesActiveCanonicalPath`: `path` names the repository itself.
  pub fn matches(&self, path: &str) -> bool {
    path == self.canonical_path || path == self.cache_key
  }

  /// `remoteRepositoryContaining`: `path` is the repository or anything
  /// inside it, so it exists only on the host.
  pub fn contains(&self, path: &str) -> bool {
    self.matches(path) || path.starts_with(&format!("{}/", trim_slashes(&self.canonical_path)))
  }

  /// `repoStateScope`: the key the review state for `path` is stored under
  /// on this machine.
  pub fn scope(&self, path: &str) -> String {
    let below = if self.matches(path) {
      ""
    } else {
      trim_slashes(path)
        .get(trim_slashes(&self.canonical_path).len()..)
        .unwrap_or("")
    };
    format!("{STATE_SCOPE_PREFIX}{}{below}", self.cache_key)
  }
}

/// How a `RemoteOperationUnsupportedError` message starts.
pub const UNSUPPORTED: &str = "unsupported: ";

/// `RemoteOperationUnsupportedError`'s message.
pub fn unsupported(operation: &str) -> String {
  format!("{UNSUPPORTED}{operation} is not available for remote repositories yet")
}

/// The remote repository each window shows, by window label.
fn windows() -> &'static Mutex<HashMap<String, RemoteRepository>> {
  static WINDOWS: std::sync::OnceLock<Mutex<HashMap<String, RemoteRepository>>> =
    std::sync::OnceLock::new();
  WINDOWS.get_or_init(Mutex::default)
}

/// Records which remote repository window `label` shows; `None` for a
/// local one.
pub fn set_window(label: &str, repository: Option<RemoteRepository>) {
  let mut windows = windows().lock_or_recover();
  match repository {
    Some(repository) => windows.insert(label.to_string(), repository),
    None => windows.remove(label),
  };
}

pub fn for_window(label: &str) -> Option<RemoteRepository> {
  windows().lock_or_recover().get(label).cloned()
}

/// How a remote change ended (`MutationDispatchResult`).
#[derive(Debug, Clone, PartialEq)]
pub enum Mutation {
  Applied(Value),
  /// It landed before the connection dropped; nothing was resent.
  AlreadyApplied,
  /// Whether it landed is unknown; the user decides.
  Ambiguous(String),
}

impl Mutation {
  pub fn status(&self) -> &'static str {
    match self {
      Mutation::Applied(_) => "applied",
      Mutation::AlreadyApplied => "already_applied",
      Mutation::Ambiguous(_) => "ambiguous",
    }
  }
}

/// Why an endpoint is cut off (`CutoffReason` in `remote-cert-lifecycle.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cutoff {
  SessionEnded,
  KeyRevoked,
  InstanceInaccessible,
  CertificateExpired,
}

impl Cutoff {
  pub fn id(self) -> &'static str {
    match self {
      Cutoff::SessionEnded => "session_ended",
      Cutoff::KeyRevoked => "key_revoked",
      Cutoff::InstanceInaccessible => "instance_inaccessible",
      Cutoff::CertificateExpired => "certificate_expired",
    }
  }

  /// `cutoffReasonFromError`: the cutoff a command's error reports, if any.
  pub fn from_error(error: &str) -> Option<Self> {
    if !error.contains("credential_cut_off") && !error.contains("CredentialCutOff") {
      return None;
    }
    Some(if error.contains("(session ended)") {
      Cutoff::SessionEnded
    } else if error.contains("(client key revoked)") {
      Cutoff::KeyRevoked
    } else if error.contains("(instance no longer accessible)") {
      Cutoff::InstanceInaccessible
    } else {
      Cutoff::CertificateExpired
    })
  }

  /// The cutoff an id names (`remote://cutoff`'s `reason`).
  pub fn parse(id: &str) -> Option<Self> {
    [
      Cutoff::SessionEnded,
      Cutoff::KeyRevoked,
      Cutoff::InstanceInaccessible,
      Cutoff::CertificateExpired,
    ]
    .into_iter()
    .find(|c| c.id() == id)
  }
}

/// The endpoints cut off until the user reauthenticates, app-wide
/// (`remoteCutoffStore`): fed by the transport's `remote://cutoff` events
/// and by commands that find an endpoint cut off.
fn cutoffs() -> &'static Mutex<HashMap<String, Cutoff>> {
  static CUTOFFS: std::sync::OnceLock<Mutex<HashMap<String, Cutoff>>> = std::sync::OnceLock::new();
  CUTOFFS.get_or_init(Mutex::default)
}

/// Records `endpoint_id` as cut off; true when that changed anything.
pub fn record_cutoff(endpoint_id: &str, cutoff: Cutoff) -> bool {
  cutoffs()
    .lock_or_recover()
    .insert(endpoint_id.to_string(), cutoff)
    != Some(cutoff)
}

/// Lifts `endpoint_id`'s cutoff (a fresh certificate was issued); true when
/// it was cut off.
pub fn clear_cutoff(endpoint_id: &str) -> bool {
  cutoffs().lock_or_recover().remove(endpoint_id).is_some()
}

/// Why `endpoint_id` is cut off, if it is.
pub fn cutoff(endpoint_id: &str) -> Option<Cutoff> {
  cutoffs().lock_or_recover().get(endpoint_id).copied()
}

/// Runs typed commands on an endpoint and tells React how they went; faked
/// in tests. Blocks.
pub trait Transport: Send + Sync {
  /// `dispatchOverSsh`: a read, safe to retry.
  fn read(&self, endpoint: &SshEndpoint, request: TreqCommandRequest) -> Result<Value, String>;
  /// `dispatchMutationOverSsh`: a change, verified before any retry.
  fn mutate(&self, endpoint: &SshEndpoint, request: TreqCommandRequest)
    -> Result<Mutation, String>;
  /// `applyMutationDispatchResult`: React's feedback for a change.
  fn report(&self, mutation: &Mutation);
  /// `noteCutoffFromError`: React blocks the endpoint behind reauthentication.
  fn cut_off(&self, endpoint_id: &str, cutoff: Cutoff);
}

/// How a change's attempt ended, for its idempotency key.
fn outcome(mutation: &Mutation) -> Outcome {
  match mutation {
    Mutation::Applied(_) => Outcome::Applied,
    Mutation::AlreadyApplied => Outcome::AlreadyApplied,
    Mutation::Ambiguous(_) => Outcome::Ambiguous,
  }
}

/// `dispatchKeyedMutationOverSsh`: sends `request` to `endpoint` with the
/// key of the same unconfirmed change on that endpoint generation (the
/// shared `remoteActionKeys`), or a new one. For changes outside an open
/// repository: clone and init.
pub fn keyed_endpoint_mutation(
  transport: &dyn Transport,
  endpoint: &SshEndpoint,
  request: &TreqCommandRequest,
) -> Result<Mutation, String> {
  let generation = crate::core::remote_records::endpoint_generation(endpoint);
  let scope = format!("{}#{generation}", endpoint.id);
  let unkeyed = without_key(request)?;
  let keys = remote_idempotency::global();
  let key = keys.key_for(request.kind_name(), &[unkeyed], &scope);
  let mutation = transport.mutate(endpoint, with_key(request, &key)?)?;
  keys.settle(&key, outcome(&mutation));
  Ok(mutation)
}

/// `request`'s JSON without its idempotency key: the change's inputs.
fn without_key(request: &TreqCommandRequest) -> Result<Value, String> {
  let mut value = serde_json::to_value(request).map_err(|e| e.to_string())?;
  if let Some(object) = value.as_object_mut() {
    object.remove("idempotency_key");
  }
  Ok(value)
}

/// `request` with its `idempotency_key` set to `key`.
fn with_key(request: &TreqCommandRequest, key: &str) -> Result<TreqCommandRequest, String> {
  let mut value = serde_json::to_value(request).map_err(|e| e.to_string())?;
  if let Some(object) = value.as_object_mut() {
    object.insert("idempotency_key".into(), Value::String(key.into()));
  }
  serde_json::from_value(value).map_err(|e| e.to_string())
}

fn workspace_arg(workspace_id: Option<i64>) -> Option<String> {
  workspace_id.map(|id| id.to_string())
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, String> {
  serde_json::from_value(value).map_err(|e| format!("invalid_remote_json: {e}"))
}

/// A path in a remote repository, as the typed commands address it.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
  /// The workspace id, or `None` for the repository root.
  pub workspace: Option<String>,
  /// The path below the workspace's (or repository's) root.
  pub relative: String,
}

/// The remote repository and the transport that reaches it: what a host
/// calls for a path inside it.
#[derive(Clone)]
pub struct Remote {
  pub repo: RemoteRepository,
  transport: Arc<dyn Transport>,
}

impl Remote {
  pub fn new(repo: RemoteRepository, transport: Arc<dyn Transport>) -> Self {
    Self { repo, transport }
  }

  fn repo_arg(&self) -> String {
    self.repo.canonical_path.clone()
  }

  /// `requireSshTransport`.
  fn endpoint(&self) -> Result<&SshEndpoint, String> {
    self.repo.endpoint.as_ref().ok_or_else(|| {
      format!(
        "endpoint_unresolved: {} has no SSH endpoint; reopen it from the remote setup dialog",
        self.repo.display_name
      )
    })
  }

  fn note_cutoff(&self, error: String) -> String {
    if let (Some(endpoint), Some(cutoff)) = (&self.repo.endpoint_id, Cutoff::from_error(&error)) {
      self.transport.cut_off(endpoint, cutoff);
    }
    error
  }

  /// `remoteDispatch`.
  pub fn read<T: DeserializeOwned>(&self, request: TreqCommandRequest) -> Result<T, String> {
    let endpoint = self.endpoint()?;
    let value = self
      .transport
      .read(endpoint, request)
      .map_err(|e| self.note_cutoff(e))?;
    parse(value)
  }

  fn dispatch_mutation(&self, request: TreqCommandRequest) -> Result<Mutation, String> {
    let endpoint = self.endpoint()?;
    self
      .transport
      .mutate(endpoint, request)
      .map_err(|e| self.note_cutoff(e))
  }

  /// `finishMutation`: the value when the change ran in this call, `None`
  /// when it had already landed; an unknown outcome is an error.
  fn finish<T: DeserializeOwned>(&self, mutation: Mutation) -> Result<Option<T>, String> {
    self.transport.report(&mutation);
    match mutation {
      Mutation::Applied(value) => parse(value).map(Some),
      Mutation::AlreadyApplied => Ok(None),
      Mutation::Ambiguous(reason) => Err(reason),
    }
  }

  /// `remoteMutation`.
  pub fn mutation<T: DeserializeOwned>(
    &self,
    request: TreqCommandRequest,
  ) -> Result<Option<T>, String> {
    let mutation = self.dispatch_mutation(request)?;
    self.finish(mutation)
  }

  /// `remoteKeyedMutation`: `request`'s `idempotency_key` is replaced by the
  /// key of the same unconfirmed change, or a new one.
  pub fn keyed_mutation<T: DeserializeOwned>(
    &self,
    request: TreqCommandRequest,
  ) -> Result<Option<T>, String> {
    let unkeyed = without_key(&request)?;
    let keys = remote_idempotency::global();
    let key = keys.key_for(request.kind_name(), &[unkeyed], &self.repo.cache_key);
    let mutation = self.dispatch_mutation(with_key(&request, &key)?)?;
    keys.settle(&key, outcome(&mutation));
    self.finish(mutation)
  }

  /// `resolveRemoteLocation`: the workspace and relative path of an
  /// absolute path inside the repository.
  pub fn location(&self, path: &str) -> Result<Location, String> {
    let root = trim_slashes(&self.repo.canonical_path).to_string();
    let trimmed = trim_slashes(path);
    if trimmed == root || self.repo.matches(path) {
      return Ok(Location {
        workspace: None,
        relative: String::new(),
      });
    }
    let rest = trimmed.get(root.len() + 1..).unwrap_or("");
    let Some(below) = rest.strip_prefix(WORKSPACES_DIR) else {
      return Ok(Location {
        workspace: None,
        relative: rest.to_string(),
      });
    };
    let (dir, tail) = below.split_once('/').unwrap_or((below, ""));
    let found = self
      .workspaces()?
      .into_iter()
      .find(|w| w.workspace_path == dir)
      .ok_or_else(|| format!("workspace_not_found: no remote workspace at {dir}"))?;
    Ok(Location {
      workspace: Some(found.id.to_string()),
      relative: tail.to_string(),
    })
  }

  // -- Reads (`repository-adapter.ts`) ---------------------------------------

  pub fn workspaces(&self) -> Result<Vec<Workspace>, String> {
    self.read(TreqCommandRequest::ListWorkspaces {
      repo: self.repo_arg(),
    })
  }

  /// `WorkspaceSummaries`: every workspace with its sidebar status and
  /// uncommitted changes, in one round trip.
  pub fn workspace_summaries(&self) -> Result<Vec<WorkspaceSummary>, String> {
    self.read(TreqCommandRequest::WorkspaceSummaries {
      repo: self.repo_arg(),
    })
  }

  /// [`Self::workspace_summaries`], or `None` when the host does not answer
  /// it (an older `treq`), so the caller reads workspace by workspace. A
  /// failure is remembered for a while rather than retried on every poll.
  pub fn workspace_summaries_if_supported(&self) -> Option<Vec<WorkspaceSummary>> {
    static FAILED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
    const RETRY_AFTER: Duration = Duration::from_secs(300);
    let key = self.repo_arg();
    let recently_failed = FAILED
      .lock_or_recover()
      .get_or_insert_with(HashMap::new)
      .get(&key)
      .is_some_and(|at| at.elapsed() < RETRY_AFTER);
    if recently_failed {
      return None;
    }
    let summaries = self.workspace_summaries().ok();
    if summaries.is_none() {
      FAILED
        .lock_or_recover()
        .get_or_insert_with(HashMap::new)
        .insert(key, Instant::now());
    }
    summaries
  }

  // -- Workspace lifecycle (`useWorkspaceDeletion`) ----------------------------

  /// `delete_workspace`: forgets the workspace and removes its directory.
  pub fn delete_workspace(&self, workspace_id: i64) -> Result<(), String> {
    self
      .mutation::<Value>(TreqCommandRequest::DeleteWorkspace {
        repo: self.repo_arg(),
        workspace: workspace_id.to_string(),
      })
      .map(drop)
  }

  /// `archive_workspace`: removes the directory but keeps the record.
  pub fn archive_workspace(&self, workspace_id: i64) -> Result<(), String> {
    self
      .mutation::<Value>(TreqCommandRequest::ArchiveWorkspace {
        repo: self.repo_arg(),
        workspace: workspace_id.to_string(),
      })
      .map(drop)
  }

  pub fn status(&self, workspace_id: Option<i64>) -> Result<WorkspaceStatus, String> {
    self.read(match workspace_id {
      None => TreqCommandRequest::RepositoryStatus {
        repo: self.repo_arg(),
      },
      Some(id) => TreqCommandRequest::InspectWorkspace {
        repo: self.repo_arg(),
        workspace: id.to_string(),
      },
    })
  }

  pub fn git_remote(&self) -> Result<Option<GitRemoteInfo>, String> {
    self.read(TreqCommandRequest::GitRemoteInfo {
      repo: self.repo_arg(),
    })
  }

  pub fn branches(&self) -> Result<Vec<JjBranch>, String> {
    self.read(TreqCommandRequest::ListBranches {
      repo: self.repo_arg(),
    })
  }

  pub fn changed_files(&self, workspace_id: Option<i64>) -> Result<Vec<JjFileChange>, String> {
    self.read(TreqCommandRequest::ListChanges {
      repo: self.repo_arg(),
      workspace: workspace_arg(workspace_id),
    })
  }

  /// `transportListCommits`: the workspace's log; an older host answers
  /// with the bare list.
  pub fn commits(&self, workspace_id: Option<i64>) -> Result<JjLogResult, String> {
    let value: Value = self.read(TreqCommandRequest::ListCommits {
      repo: self.repo_arg(),
      workspace: workspace_arg(workspace_id),
    })?;
    match value {
      Value::Array(commits) => Ok(JjLogResult {
        commits: parse(Value::Array(commits))?,
        tentative_working_copy: None,
        target_branch: String::new(),
        workspace_branch: String::new(),
        target_branch_commits: Vec::new(),
        merge_base_id: None,
      }),
      value => parse(value),
    }
  }

  pub fn workspace_diff(&self, workspace_id: i64) -> Result<JjRevisionDiff, String> {
    self.read(TreqCommandRequest::WorkspaceDiff {
      repo: self.repo_arg(),
      workspace: workspace_id.to_string(),
    })
  }

  pub fn file_hunks(
    &self,
    workspace_id: Option<i64>,
    path: &str,
  ) -> Result<Vec<JjDiffHunk>, String> {
    let hunks: Option<Vec<JjDiffHunk>> = self.read(TreqCommandRequest::DiffFile {
      repo: self.repo_arg(),
      workspace: workspace_arg(workspace_id),
      path: path.to_string(),
    })?;
    Ok(hunks.unwrap_or_default())
  }

  /// `transportGetWorkspaceFileHunksBatch`: one `DiffFile` per path.
  pub fn file_hunks_batch(
    &self,
    workspace_id: Option<i64>,
    paths: Vec<String>,
  ) -> Result<WorkspaceFileHunksBatch, String> {
    let files = paths
      .into_iter()
      .map(|path| {
        let hunks = self.file_hunks(workspace_id, &path)?;
        Ok(WorkspaceFileHunksBatchFile {
          path,
          hunks,
          error: None,
        })
      })
      .collect::<Result<_, String>>()?;
    Ok(WorkspaceFileHunksBatch {
      snapshot_token: "remote".into(),
      files,
    })
  }

  pub fn file_lines(
    &self,
    workspace_id: Option<i64>,
    path: &str,
    from_parent: bool,
    start: usize,
    end: usize,
  ) -> Result<JjFileLines, String> {
    self.read(TreqCommandRequest::ReadFile {
      repo: self.repo_arg(),
      workspace: workspace_arg(workspace_id),
      path: path.to_string(),
      revision: if from_parent {
        FileRevision::Parent
      } else {
        FileRevision::WorkingCopy
      },
      start_line: Some(start),
      end_line: Some(end),
    })
  }

  pub fn commit_diff(
    &self,
    workspace_id: Option<i64>,
    revision: &str,
  ) -> Result<JjRevisionDiff, String> {
    self.read(TreqCommandRequest::CommitDiff {
      repo: self.repo_arg(),
      workspace: workspace_arg(workspace_id),
      revision: revision.to_string(),
    })
  }

  pub fn commit_file_diff(
    &self,
    workspace_id: Option<i64>,
    revision: &str,
    path: &str,
  ) -> Result<JjFileDiff, String> {
    self.read(TreqCommandRequest::CommitFileDiff {
      repo: self.repo_arg(),
      workspace: workspace_arg(workspace_id),
      revision: revision.to_string(),
      path: path.to_string(),
    })
  }

  /// The matching files' paths relative to the workspace.
  pub fn search_files(
    &self,
    workspace_id: Option<i64>,
    query: &str,
    limit: usize,
  ) -> Result<Vec<String>, String> {
    let found: Vec<crate::core::remote::RemoteFileSearchResult> =
      self.read(TreqCommandRequest::SearchFiles {
        repo: self.repo_arg(),
        workspace: workspace_arg(workspace_id),
        query: query.to_string(),
        limit: Some(limit),
      })?;
    Ok(found.into_iter().map(|f| f.relative_path).collect())
  }

  /// `transportGetCommitDescription`: found in the workspace log.
  pub fn commit_description(&self, workspace_id: i64, change_id: &str) -> Result<String, String> {
    let log = self.commits(Some(workspace_id))?;
    log
      .commits
      .into_iter()
      .find(|c| c.change_id.starts_with(change_id) || change_id.starts_with(&c.change_id))
      .map(|c| c.description)
      .ok_or_else(|| format!("commit_not_found: {change_id}"))
  }

  // -- Changes (`repository-adapter-mutations.ts`) ---------------------------

  /// `transportCreateWorkspace`: the new workspace's id.
  pub fn create_workspace(
    &self,
    branch: &str,
    source_branch: Option<&str>,
    metadata: Option<String>,
  ) -> Result<i64, String> {
    let created: Option<Workspace> = self.keyed_mutation(TreqCommandRequest::CreateWorkspace {
      repo: self.repo_arg(),
      branch_name: branch.to_string(),
      source_branch: source_branch.map(str::to_string),
      metadata,
      idempotency_key: String::new(),
    })?;
    if let Some(created) = created {
      return Ok(created.id);
    }
    self
      .workspaces()?
      .into_iter()
      .find(|w| w.branch_name == branch)
      .map(|w| w.id)
      .ok_or_else(|| format!("workspace_not_found: {branch} after reconnect"))
  }

  /// `transportUpdateWorkspace`.
  pub fn update_workspace(
    &self,
    workspace_id: i64,
    target_branch: Option<&str>,
    title: Option<&str>,
    description: Option<&str>,
  ) -> Result<Workspace, String> {
    let updated: Option<Workspace> = self.mutation(TreqCommandRequest::UpdateWorkspace {
      repo: self.repo_arg(),
      workspace: workspace_id.to_string(),
      target_branch: target_branch.map(str::to_string),
      title: title.map(str::to_string),
      description: description.map(str::to_string),
    })?;
    if let Some(updated) = updated {
      return Ok(updated);
    }
    self
      .workspaces()?
      .into_iter()
      .find(|w| w.id == workspace_id)
      .ok_or_else(|| format!("workspace_not_found: {workspace_id} after reconnect"))
  }

  /// `transportPushWorkspace`.
  pub fn push(&self, workspace_id: Option<i64>) -> Result<String, String> {
    let pushed: Option<String> = self.keyed_mutation(TreqCommandRequest::GitPush {
      repo: self.repo_arg(),
      workspace: workspace_arg(workspace_id),
      idempotency_key: String::new(),
    })?;
    Ok(pushed.unwrap_or_else(|| ALREADY_APPLIED.into()))
  }

  /// `transportPullWorkspace`.
  pub fn pull(&self, workspace_id: Option<i64>) -> Result<PullWorkspaceResult, String> {
    let pulled: Option<PullWorkspaceResult> = self.mutation(TreqCommandRequest::PullWorkspace {
      repo: self.repo_arg(),
      workspace: workspace_arg(workspace_id),
    })?;
    Ok(pulled.unwrap_or(PullWorkspaceResult {
      success: true,
      message: ALREADY_APPLIED.into(),
      was_diverged: false,
      commits_rebased: 0,
      has_conflicts: false,
    }))
  }

  /// `transportCreateCommit`.
  pub fn create_commit(&self, workspace_id: Option<i64>, message: &str) -> Result<String, String> {
    let committed: Option<String> = self.keyed_mutation(TreqCommandRequest::CreateCommit {
      repo: self.repo_arg(),
      workspace: workspace_arg(workspace_id),
      message: message.to_string(),
      base_change_id: None,
      idempotency_key: String::new(),
    })?;
    Ok(committed.unwrap_or_else(|| "Commit applied".into()))
  }

  /// `transportSplitWorkingCopy`: whole files of a workspace's working copy.
  pub fn split(
    &self,
    workspace_path: &str,
    message: &str,
    files: Vec<String>,
  ) -> Result<String, String> {
    let Some(workspace) = self.location(workspace_path)?.workspace else {
      return Err(unsupported(
        "Committing selected files in the repository root",
      ));
    };
    let split: Option<String> = self.keyed_mutation(TreqCommandRequest::SplitCommit {
      repo: self.repo_arg(),
      workspace,
      commit: "@".into(),
      files,
      hunks: Vec::new(),
      message: message.to_string(),
      idempotency_key: String::new(),
    })?;
    Ok(split.unwrap_or_else(|| ALREADY_APPLIED.into()))
  }

  /// `transportRestoreFile`.
  pub fn restore_file(&self, workspace_path: &str, path: &str) -> Result<String, String> {
    let workspace = self.location(workspace_path)?.workspace;
    let restored: Option<String> = self.mutation(TreqCommandRequest::RestoreFile {
      repo: self.repo_arg(),
      workspace,
      path: path.to_string(),
    })?;
    Ok(restored.unwrap_or_else(|| ALREADY_APPLIED.into()))
  }

  /// `transportWorkingCopyMutation` for `RestoreAll`.
  pub fn restore_all(&self, workspace_path: &str) -> Result<(), String> {
    let workspace = self.location(workspace_path)?.workspace;
    self
      .mutation::<Value>(TreqCommandRequest::RestoreAll {
        repo: self.repo_arg(),
        workspace,
      })
      .map(drop)
  }

  /// `jjSnapshotWorkingCopy`: the snapshot's id, which a discard's undo needs.
  pub fn snapshot(&self, workspace_path: &str) -> Result<String, String> {
    let workspace = self.location(workspace_path)?.workspace;
    let id: Option<String> = self.mutation(TreqCommandRequest::SnapshotWorkingCopy {
      repo: self.repo_arg(),
      workspace,
    })?;
    id.ok_or_else(|| {
      "ambiguous: the VM did not return a value for this working-copy operation; try again".into()
    })
  }

  pub fn restore_snapshot(&self, workspace_path: &str, snapshot_id: &str) -> Result<(), String> {
    let workspace = self.location(workspace_path)?.workspace;
    self
      .mutation::<Value>(TreqCommandRequest::RestoreSnapshot {
        repo: self.repo_arg(),
        workspace,
        snapshot_id: snapshot_id.to_string(),
      })
      .map(drop)
  }

  /// `transportStashWorkspaceChanges`.
  pub fn stash_changes(&self, workspace_id: Option<i64>) -> Result<StashEntry, String> {
    let entry: Option<StashEntry> =
      self.keyed_mutation(TreqCommandRequest::StashWorkspaceChanges {
        repo: self.repo_arg(),
        workspace: workspace_arg(workspace_id),
        idempotency_key: String::new(),
      })?;
    entry.ok_or_else(|| "stash_ambiguous: stash state unknown".into())
  }

  /// `transportAbandonCommit`: the operation id, for undo.
  pub fn abandon(&self, workspace_id: i64, change_id: &str) -> Result<String, String> {
    let operation: Option<String> = self.keyed_mutation(TreqCommandRequest::AbandonCommit {
      repo: self.repo_arg(),
      workspace: workspace_id.to_string(),
      commit: change_id.to_string(),
      idempotency_key: String::new(),
    })?;
    Ok(operation.unwrap_or_default())
  }

  pub fn undo_operation(
    &self,
    workspace_id: Option<i64>,
    operation_id: &str,
  ) -> Result<(), String> {
    self
      .mutation::<Value>(TreqCommandRequest::UndoOperation {
        repo: self.repo_arg(),
        workspace: workspace_arg(workspace_id),
        operation_id: operation_id.to_string(),
      })
      .map(drop)
  }

  pub fn undo_commit(&self, workspace_id: i64, change_id: &str) -> Result<(), String> {
    self
      .mutation::<Value>(TreqCommandRequest::UndoCommit {
        repo: self.repo_arg(),
        workspace: workspace_id.to_string(),
        commit: change_id.to_string(),
      })
      .map(drop)
  }

  pub fn revert(&self, workspace_id: i64, change_id: &str) -> Result<(), String> {
    self
      .keyed_mutation::<Value>(TreqCommandRequest::RevertCommit {
        repo: self.repo_arg(),
        workspace: workspace_id.to_string(),
        commit: change_id.to_string(),
        idempotency_key: String::new(),
      })
      .map(drop)
  }

  pub fn describe(&self, workspace_id: i64, change_id: &str, message: &str) -> Result<(), String> {
    self
      .mutation::<Value>(TreqCommandRequest::DescribeCommit {
        repo: self.repo_arg(),
        workspace: workspace_id.to_string(),
        commit: change_id.to_string(),
        message: message.to_string(),
      })
      .map(drop)
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;

  pub fn repository() -> RemoteRepository {
    serde_json::from_value(serde_json::json!({
      "endpoint": {
        "id": "ep-1",
        "instance_id": null,
        "source": { "type": "user_managed" },
        "hostname": "devbox.example",
        "port": 22,
        "username": "me",
        "host_keys": [],
        "authentication": { "type": "public_key", "key_reference": "/k.pub" },
      },
      "endpointId": "ep-1",
      "canonicalPath": "/srv/app",
      "cacheKey": "ssh:ep-1#0:/srv/app",
      "displayName": "app",
    }))
    .unwrap()
  }

  /// Answers reads from `answers` by kind, records every request and what
  /// React was told.
  #[derive(Default)]
  pub struct FakeTransport {
    pub answers: Mutex<HashMap<&'static str, Result<Value, String>>>,
    pub mutations: Mutex<HashMap<&'static str, Mutation>>,
    pub sent: Mutex<Vec<TreqCommandRequest>>,
    pub reported: Mutex<Vec<&'static str>>,
    pub cutoffs: Mutex<Vec<(String, Cutoff)>>,
  }

  impl FakeTransport {
    pub fn answer(&self, kind: &'static str, value: Value) {
      self.answers.lock().unwrap().insert(kind, Ok(value));
    }

    pub fn sent(&self) -> Vec<TreqCommandRequest> {
      self.sent.lock().unwrap().clone()
    }
  }

  impl Transport for FakeTransport {
    fn read(&self, _: &SshEndpoint, request: TreqCommandRequest) -> Result<Value, String> {
      let kind = request.kind_name();
      self.sent.lock().unwrap().push(request);
      self
        .answers
        .lock()
        .unwrap()
        .get(kind)
        .cloned()
        .unwrap_or(Ok(Value::Null))
    }

    fn mutate(&self, _: &SshEndpoint, request: TreqCommandRequest) -> Result<Mutation, String> {
      let kind = request.kind_name();
      self.sent.lock().unwrap().push(request);
      Ok(
        self
          .mutations
          .lock()
          .unwrap()
          .get(kind)
          .cloned()
          .unwrap_or(Mutation::Applied(Value::Null)),
      )
    }

    fn report(&self, mutation: &Mutation) {
      self.reported.lock().unwrap().push(mutation.status());
    }

    fn cut_off(&self, endpoint_id: &str, cutoff: Cutoff) {
      self
        .cutoffs
        .lock()
        .unwrap()
        .push((endpoint_id.to_string(), cutoff));
    }
  }

  fn remote() -> (Remote, Arc<FakeTransport>) {
    let transport = Arc::new(FakeTransport::default());
    (Remote::new(repository(), transport.clone()), transport)
  }

  pub fn workspace_json(id: i64, dir: &str, branch: &str) -> Value {
    serde_json::json!({
      "id": id,
      "repo_path": "/srv/app",
      "workspace_name": dir,
      "workspace_path": dir,
      "branch_name": branch,
      "created_at": "2026-01-01T00:00:00Z",
      "metadata": null,
      "target_branch": null,
      "title": "",
      "not_on_remote": false,
    })
  }

  #[test]
  fn paths_inside_the_repository_are_remote() {
    let repo = repository();
    assert!(repo.matches("/srv/app"));
    assert!(repo.matches("ssh:ep-1#0:/srv/app"));
    assert!(repo.contains("/srv/app/.treq/workspaces/feat"));
    assert!(!repo.contains("/srv/application"));
    assert!(!repo.matches("/srv/app/src"));
  }

  #[test]
  fn review_state_is_scoped_to_the_repository_identity() {
    let repo = repository();
    assert_eq!(
      repo.scope("/srv/app"),
      "treq-remote-state:ssh:ep-1#0:/srv/app"
    );
    assert_eq!(
      repo.scope("/srv/app/.treq/workspaces/feat/"),
      "treq-remote-state:ssh:ep-1#0:/srv/app/.treq/workspaces/feat"
    );
  }

  #[test]
  fn windows_remember_their_remote_repository() {
    set_window("remote-test-window", Some(repository()));
    assert_eq!(
      for_window("remote-test-window").map(|r| r.canonical_path),
      Some("/srv/app".to_string())
    );
    set_window("remote-test-window", None);
    assert!(for_window("remote-test-window").is_none());
  }

  #[test]
  fn reads_send_typed_commands_for_the_repository() {
    let (remote, transport) = remote();
    transport.answer(
      "ListWorkspaces",
      serde_json::json!([workspace_json(3, "feat", "feat")]),
    );
    let workspaces = remote.workspaces().unwrap();
    assert_eq!(workspaces[0].id, 3);
    let _ = remote.status(Some(3));
    let _ = remote.status(None);
    assert_eq!(
      transport.sent()[1..],
      [
        TreqCommandRequest::InspectWorkspace {
          repo: "/srv/app".into(),
          workspace: "3".into()
        },
        TreqCommandRequest::RepositoryStatus {
          repo: "/srv/app".into()
        },
      ]
    );
  }

  #[test]
  fn a_bare_commit_list_reads_as_a_log() {
    let (remote, transport) = remote();
    transport.answer("ListCommits", serde_json::json!([]));
    let log = remote.commits(Some(1)).unwrap();
    assert!(log.commits.is_empty());
  }

  #[test]
  fn workspace_paths_resolve_to_their_workspace() {
    let (remote, transport) = remote();
    transport.answer(
      "ListWorkspaces",
      serde_json::json!([workspace_json(7, "feat-x", "feat/x")]),
    );
    assert_eq!(
      remote.location("/srv/app").unwrap(),
      Location {
        workspace: None,
        relative: String::new()
      }
    );
    assert_eq!(
      remote
        .location("/srv/app/.treq/workspaces/feat-x/src/main.rs")
        .unwrap(),
      Location {
        workspace: Some("7".into()),
        relative: "src/main.rs".into()
      }
    );
    assert!(remote
      .location("/srv/app/.treq/workspaces/gone")
      .unwrap_err()
      .starts_with("workspace_not_found"));
  }

  #[test]
  fn a_split_needs_a_workspace() {
    let (remote, _) = remote();
    assert_eq!(
      remote
        .split("/srv/app", "msg", vec!["a".into()])
        .unwrap_err(),
      unsupported("Committing selected files in the repository root")
    );
  }

  #[test]
  fn keyed_changes_reuse_the_key_until_confirmed() {
    let (remote, transport) = remote();
    let kind = "GitPush";
    transport
      .mutations
      .lock()
      .unwrap()
      .insert(kind, Mutation::Ambiguous("lost".into()));
    assert_eq!(remote.push(Some(9)).unwrap_err(), "lost");
    transport
      .mutations
      .lock()
      .unwrap()
      .insert(kind, Mutation::AlreadyApplied);
    assert_eq!(remote.push(Some(9)).unwrap(), ALREADY_APPLIED);
    let _ = remote.push(Some(9));
    let keys: Vec<String> = transport
      .sent()
      .into_iter()
      .map(|request| {
        serde_json::to_value(request).unwrap()["idempotency_key"]
          .as_str()
          .unwrap()
          .to_string()
      })
      .collect();
    assert!(keys[0].starts_with(&format!("{kind}:")));
    assert_eq!(keys[0], keys[1], "a retry keeps the unconfirmed key");
    assert_ne!(keys[1], keys[2], "a confirmed change ends the action");
    assert_eq!(
      *transport.reported.lock().unwrap(),
      vec!["ambiguous", "already_applied", "already_applied"]
    );
  }

  #[test]
  fn a_cut_off_endpoint_is_reported() {
    let (remote, transport) = remote();
    transport.answers.lock().unwrap().insert(
      "ListWorkspaces",
      Err("credential_cut_off: endpoint ep-1 (client key revoked)".into()),
    );
    assert!(remote.workspaces().is_err());
    assert_eq!(
      *transport.cutoffs.lock().unwrap(),
      vec![("ep-1".to_string(), Cutoff::KeyRevoked)]
    );
    assert_eq!(
      Cutoff::from_error("CredentialCutOff"),
      Some(Cutoff::CertificateExpired)
    );
    assert_eq!(Cutoff::from_error("transport_error: x"), None);
  }

  #[test]
  fn an_unresolved_endpoint_fails_closed() {
    let mut repo = repository();
    repo.endpoint = None;
    let remote = Remote::new(repo, Arc::new(FakeTransport::default()));
    assert!(remote
      .workspaces()
      .unwrap_err()
      .starts_with("endpoint_unresolved: app"));
  }

  fn sent_keys(transport: &FakeTransport) -> Vec<String> {
    transport
      .sent()
      .into_iter()
      .map(|request| {
        serde_json::to_value(request).unwrap()["idempotency_key"]
          .as_str()
          .unwrap()
          .to_string()
      })
      .collect()
  }

  #[test]
  fn endpoint_changes_keep_their_key_per_endpoint_generation() {
    let transport = FakeTransport::default();
    let endpoint = repository().endpoint.unwrap();
    let init = TreqCommandRequest::InitRepo {
      repo: "~/keyed-endpoint-test".into(),
      idempotency_key: String::new(),
    };
    transport
      .mutations
      .lock()
      .unwrap()
      .insert("InitRepo", Mutation::Ambiguous("lost".into()));
    keyed_endpoint_mutation(&transport, &endpoint, &init).unwrap();
    transport.mutations.lock().unwrap().clear();
    keyed_endpoint_mutation(&transport, &endpoint, &init).unwrap();
    let mut other = endpoint;
    other.id = "ep-other".into();
    keyed_endpoint_mutation(&transport, &other, &init).unwrap();
    let keys = sent_keys(&transport);
    assert!(keys[0].starts_with("InitRepo:"));
    assert_eq!(keys[0], keys[1], "the retry after an unknown outcome");
    assert_ne!(keys[1], keys[2], "another endpoint never shares a key");
  }

  #[test]
  fn cutoffs_are_kept_until_cleared() {
    let id = "ep-cutoff-registry";
    assert_eq!(cutoff(id), None);
    assert!(record_cutoff(id, Cutoff::KeyRevoked));
    assert!(!record_cutoff(id, Cutoff::KeyRevoked));
    assert_eq!(cutoff(id), Some(Cutoff::KeyRevoked));
    assert!(clear_cutoff(id));
    assert!(!clear_cutoff(id));
    assert_eq!(cutoff(id), None);
    assert_eq!(Cutoff::parse("session_ended"), Some(Cutoff::SessionEnded));
    assert_eq!(Cutoff::parse("nope"), None);
  }
}
