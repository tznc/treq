#![expect(
  clippy::string_slice,
  reason = "slices hex commit/change ids and git ref names at ASCII separators"
)]

use crate::result_ext::ResultExt;
use chrono::TimeZone;
use futures::StreamExt as _;
use futures::TryStreamExt as _;

/// Drive a future to completion on the calling thread using thread park/unpark.
///
/// Unlike `futures::executor::block_on`, this implementation does **not** set any
/// thread-local "executor entered" flag, so it is safe to call nested — including
/// from within jj-lib callbacks that internally also call `block_on`.  This avoids
/// the `LocalPool EnterError` panic that occurs when `futures::executor::block_on`
/// is re-entered on the same thread.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
  use std::pin::pin;
  use std::sync::Arc;
  use std::task::{Context, Poll, Wake};
  use std::thread;

  struct ThreadWaker(thread::Thread);
  impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
      self.0.unpark();
    }
  }

  let waker = Arc::new(ThreadWaker(thread::current())).into();
  let mut cx = Context::from_waker(&waker);
  let mut future = pin!(future);
  loop {
    match future.as_mut().poll(&mut cx) {
      Poll::Ready(val) => return val,
      Poll::Pending => thread::park(),
    }
  }
}

/// Lock identity for a workspace path: canonical when it exists, otherwise the
/// path with `.` components and trailing separators dropped, so `foo`, `foo/`
/// and `./foo` share one lock.
fn working_copy_lock_key(workspace_path: &str) -> std::path::PathBuf {
  use std::path::Component;
  let path = std::path::Path::new(workspace_path);
  std::fs::canonicalize(path).unwrap_or_else(|_| {
    path
      .components()
      .filter(|c| *c != Component::CurDir)
      .collect()
  })
}

/// Returns the lock shared by every caller using the same working copy. The map
/// holds only `Weak` handles, so a lock is freed once its last user is done and
/// dead entries are pruned on each lookup.
fn working_copy_lock(workspace_path: &str) -> std::sync::Arc<std::sync::Mutex<()>> {
  use std::collections::HashMap;
  use std::path::PathBuf;
  use std::sync::{Arc, Mutex, OnceLock, Weak};

  static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();
  let locks = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));

  let mut map = locks
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
  map.retain(|_, lock| lock.strong_count() > 0);
  let key = working_copy_lock_key(workspace_path);
  if let Some(lock) = map.get(&key).and_then(Weak::upgrade) {
    return lock;
  }
  let lock = Arc::new(Mutex::new(()));
  map.insert(key, Arc::downgrade(&lock));
  lock
}

/// Serializes working-copy mutations for a single workspace, keyed by its
/// normalised path, without blocking unrelated workspaces on each other.
fn with_working_copy_lock<T>(workspace_path: &str, f: impl FnOnce() -> T) -> T {
  let workspace_lock = working_copy_lock(workspace_path);
  let _guard = workspace_lock
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
  f()
}

/// A read that callers asking at the same time share. A caller that arrives
/// while the read runs waits for the next run, never the current one, so it
/// still sees state from after it asked; any number of callers that pile up
/// behind one run cost one more run, not one each.
struct SharedRead<T> {
  state: std::sync::Mutex<SharedReadState<T>>,
  done: std::sync::Condvar,
}

struct SharedReadState<T> {
  running: bool,
  started: u64,
  finished: u64,
  result: Option<T>,
}

impl<T: Clone> SharedRead<T> {
  fn new() -> Self {
    Self {
      state: std::sync::Mutex::new(SharedReadState {
        running: false,
        started: 0,
        finished: 0,
        result: None,
      }),
      done: std::sync::Condvar::new(),
    }
  }

  fn run(&self, read: impl FnOnce() -> T) -> T {
    /// Lets waiters run the read themselves if this run panics.
    struct Running<'a, T>(&'a SharedRead<T>);
    impl<T> Drop for Running<'_, T> {
      fn drop(&mut self) {
        let mut state = self
          .0
          .state
          .lock()
          .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.running = false;
        drop(state);
        self.0.done.notify_all();
      }
    }

    let lock = || {
      self
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    let mut state = lock();
    let wanted = state.started + 1;
    loop {
      if state.finished >= wanted {
        if let Some(result) = &state.result {
          return result.clone();
        }
      }
      if !state.running {
        state.running = true;
        state.started += 1;
        let run = state.started;
        drop(state);
        let running = Running(self);
        let result = read();
        let mut state = lock();
        state.result = Some(result.clone());
        state.finished = run;
        drop(state);
        drop(running);
        return result;
      }
      state = self
        .done
        .wait(state)
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
  }
}

use gix::refs::transaction::{Change, PreviousValue, RefEdit};
use gix::refs::Target;
use imara_diff::intern::InternedInput;
use imara_diff::sink::Counter;
use imara_diff::{diff, Algorithm, UnifiedDiffBuilder};
use jj_lib::backend::{TreeId, TreeValue};
use jj_lib::commit::Commit;
use jj_lib::config::{ConfigLayer, ConfigSource, StackedConfig};
use jj_lib::conflict_labels::ConflictLabels;
use jj_lib::conflicts::{
  materialize_merge_result_to_bytes, materialized_diff_stream, ConflictMarkerStyle,
  ConflictMaterializeOptions, MaterializedTreeValue,
};
use jj_lib::copies::CopyRecords;
use jj_lib::fileset::FilesetAliasesMap;
use jj_lib::git;
use jj_lib::gitignore::GitIgnoreFile;
use jj_lib::lock::FileLock;
use jj_lib::matchers::{Matcher, NothingMatcher, PrefixMatcher};
use jj_lib::merge::{Diff, Merge, MergedTreeValue};
use jj_lib::merged_tree::MergedTree;
use jj_lib::merged_tree_builder::MergedTreeBuilder;
use jj_lib::object_id::{HexPrefix, ObjectId};
use jj_lib::op_store::{OperationId, RefTarget, RemoteRef};
use jj_lib::operation::Operation;
use jj_lib::ref_name::{RefName, RemoteName, RemoteRefSymbol, WorkspaceNameBuf};
use jj_lib::refs::{classify_ref_push_action, LocalAndRemoteRef, RefPushAction};
use jj_lib::repo::{MutableRepo, ReadonlyRepo, Repo as _, StoreFactories};
use jj_lib::repo_path::{RepoPath, RepoPathBuf, RepoPathUiConverter};
use jj_lib::revset::{
  self, Revset, RevsetAliasesMap, RevsetDiagnostics, RevsetIteratorExt as _, RevsetParseContext,
  RevsetWorkspaceContext, SymbolResolver,
};
use jj_lib::rewrite::{
  self, merge_commit_trees, EmptyBehavior, MoveCommitsLocation, MoveCommitsTarget, RebaseOptions,
  RewriteRefsOptions,
};
use jj_lib::settings::UserSettings;
use jj_lib::store::Store;
use jj_lib::str_util::StringExpression;
use jj_lib::tree_merge::MergeOptions;
use jj_lib::working_copy::{SnapshotOptions, WorkingCopyFreshness};
use jj_lib::workspace::{
  default_working_copy_factories, default_working_copy_factory, LockedWorkspace, Workspace,
};
use jj_lib::workspace_store::SimpleWorkspaceStore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

pub mod oplog;
use std::sync::Arc;

use crate::binary_paths;
use crate::conflict_markers::{self, ConflictRegionView, ConflictStyle};
use crate::local_db;

fn chain_ignore_file(base: &Arc<GitIgnoreFile>, prefix: &str, path: PathBuf) -> Arc<GitIgnoreFile> {
  base
    .chain_with_file(prefix, path)
    .unwrap_or_else(|_| base.clone())
}

fn repo_root_matcher() -> PrefixMatcher {
  PrefixMatcher::new([RepoPath::root()])
}

fn git_global_excludes_path(repo_path: &str) -> Option<PathBuf> {
  if let Ok(repo) = gix::open(repo_path) {
    if let Some(Ok(path)) = repo.config_snapshot().trusted_path("core.excludesFile") {
      let path = path.into_owned();
      if !path.as_os_str().is_empty() {
        return Some(path);
      }
    }
  }

  let config_home = std::env::var_os("XDG_CONFIG_HOME")
    .filter(|value| !value.is_empty())
    .map(PathBuf::from)
    .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
  config_home.map(|dir| dir.join("git").join("ignore"))
}

fn build_snapshot_base_ignores(ignore_root: &str) -> Arc<GitIgnoreFile> {
  let mut base_ignores = GitIgnoreFile::empty()
    .chain(
      "",
      Path::new("<treq builtins>"),
      b".jj/\n.treq/*\n!.treq/config.yaml\n!.treq/workflows/\n.jj*/\nnode_modules/\n.agents/skills/treq*/\n.claude/skills/treq*/\n",
    )
    .unwrap_or_else(|_| GitIgnoreFile::empty());

  if let Some(global_excludes) = git_global_excludes_path(ignore_root) {
    base_ignores = chain_ignore_file(&base_ignores, "", global_excludes);
  }
  base_ignores = chain_ignore_file(&base_ignores, "", Path::new(ignore_root).join(".gitignore"));
  chain_ignore_file(
    &base_ignores,
    "",
    Path::new(ignore_root)
      .join(".git")
      .join("info")
      .join("exclude"),
  )
}

struct LoadedWorkspaceRepo {
  settings: UserSettings,
  workspace: Workspace,
  repo: Arc<ReadonlyRepo>,
  path_converter: RepoPathUiConverter,
  /// Branch Git HEAD was attached to when a colocated home repo was loaded.
  /// Exporting a moved bookmark detaches HEAD, so this is the only record of
  /// which branch to put it back on after a rewrite.
  home_git_head_branch: Option<String>,
}

/// Description of an operation that only exports jj bookmarks to Git refs.
const GIT_REFS_EXPORT_OPERATION: &str = "export git refs";

struct SilentGitCallback;

impl git::GitSubprocessCallback for SilentGitCallback {
  fn needs_progress(&self) -> bool {
    false
  }

  fn progress(&mut self, _progress: &git::GitProgress) -> std::io::Result<()> {
    Ok(())
  }

  fn local_sideband(
    &mut self,
    _message: &[u8],
    _term: Option<git::GitSidebandLineTerminator>,
  ) -> std::io::Result<()> {
    Ok(())
  }

  fn remote_sideband(
    &mut self,
    _message: &[u8],
    _term: Option<git::GitSidebandLineTerminator>,
  ) -> std::io::Result<()> {
    Ok(())
  }
}

/// Returns the hex id of the workspace's current head JJ operation.
///
/// This is a cheap, pollable change marker: the operation log advances on
/// every mutation to the repository (from this workspace or any other
/// process/client touching the same repo), and loading it does not walk
/// commits, diffs, or file contents. A client compares this value against
/// the last one it observed to detect that VM-side repository state moved
/// underneath it (PRD "Change propagation across concurrent clients") and
/// decide whether to refresh, without any conflict-resolution logic here.
pub fn jj_head_operation_id(workspace_path: &str) -> Result<String, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  Ok(loaded.repo.op_id().hex())
}

/// Snapshots the workspace's working copy, then returns the head operation id.
///
/// [`jj_head_operation_id`] alone only moves when something runs jj. An
/// editor, terminal, or agent that writes files directly leaves the operation
/// log untouched, so a client polling that id never sees the edit. This does
/// what the jj CLI does before every command: record disk contents into the
/// working-copy commit first. A snapshot commits an operation only when the
/// tree actually changed, so polling an idle workspace returns the same id
/// every time and adds nothing to the operation log. Unchanged files cost a
/// `stat` each, because jj's tree state caches mtime and size.
///
/// The home workspace goes through the same Git HEAD/refs reconciliation that
/// [`jj_get_changed_files`] runs, so a `git commit` or `git switch` in a
/// terminal is picked up as well. Other workspaces use the repo root for
/// ignore rules, matching [`jj_get_changed_files`], so this read and the
/// changed-files read never disagree about which new files to track.
///
/// A stale working copy (another workspace rewrote its commit) is left alone:
/// snapshotting it would record old disk contents over the newer commit.
pub fn jj_snapshot_head_operation_id(workspace_path: &str) -> Result<String, JjError> {
  if !Path::new(workspace_path).join(".jj").exists() {
    return jj_head_operation_id(workspace_path);
  }
  let Some(repo_path) = derive_repo_path_from_workspace(workspace_path) else {
    reconcile_colocated_home_repo(workspace_path)?;
    return jj_head_operation_id(workspace_path);
  };
  with_working_copy_lock(workspace_path, || {
    let mut loaded = load_workspace_repo(workspace_path)?;
    snapshot_fresh_working_copy(&mut loaded, &repo_path)?;
    Ok(loaded.repo.op_id().hex())
  })
}

fn snapshot_fresh_working_copy(
  loaded: &mut LoadedWorkspaceRepo,
  ignore_root: &str,
) -> Result<(), JjError> {
  let Some(wc_commit) = get_workspace_wc_commit(loaded)? else {
    return Ok(());
  };
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let mut locked_ws = loaded
    .workspace
    .start_working_copy_mutation()
    .map_err(|e| JjError::IoError(format!("Failed to lock working copy: {e}")))?;
  let freshness = block_on(WorkingCopyFreshness::check_stale(
    locked_ws.locked_wc(),
    &wc_commit,
    &loaded.repo,
  ))
  .map_err(|e| JjError::IoError(format!("Failed to check staleness: {e}")))?;
  if !matches!(freshness, WorkingCopyFreshness::Fresh) {
    // Dropping the lock without `finish` leaves the working-copy state as it
    // was; the stale-workspace recovery flow owns updating it.
    return Ok(());
  }
  snapshot_locked_working_copy(
    &mut loaded.repo,
    locked_ws,
    workspace_name,
    &wc_commit,
    ignore_root,
  )?;
  Ok(())
}

/// Snapshots a fresh, locked working copy into `wc_commit`, finishes the
/// lock and returns the snapshotted tree. `repo` moves to the snapshot
/// operation when the tree changed.
fn snapshot_locked_working_copy(
  repo: &mut Arc<ReadonlyRepo>,
  mut locked_ws: LockedWorkspace<'_>,
  workspace_name: WorkspaceNameBuf,
  wc_commit: &Commit,
  ignore_root: &str,
) -> Result<MergedTree, JjError> {
  let matcher = repo_root_matcher();
  let opts = snapshot_options_for_all_paths(ignore_root, &matcher);
  let (new_tree, _) = block_on(locked_ws.locked_wc().snapshot(&opts))
    .map_err(|e| JjError::IoError(format!("Failed to snapshot working copy: {e}")))?;
  if new_tree.tree_ids() != wc_commit.tree_ids() {
    let mut tx = repo.start_transaction();
    let rewritten = block_on(
      tx.repo_mut()
        .rewrite_commit(wc_commit)
        .set_tree(new_tree.clone())
        .write(),
    )
    .map_err(|e| JjError::IoError(format!("Failed to write working-copy snapshot: {e}")))?;
    tx.repo_mut()
      .set_wc_commit(workspace_name, rewritten.id().clone())
      .map_err(|e| JjError::IoError(format!("Failed to set working-copy commit: {e}")))?;
    block_on(tx.repo_mut().rebase_descendants())
      .map_err(|e| JjError::IoError(format!("Failed to rebase after snapshot: {e}")))?;
    *repo = block_on(tx.commit("snapshot working copy"))
      .map_err(|e| JjError::IoError(format!("Failed to commit working-copy snapshot: {e}")))?;
  }
  block_on(locked_ws.finish(repo.op_id().clone()))
    .map_err(|e| JjError::IoError(format!("Failed to finish working-copy snapshot: {e}")))?;
  Ok(new_tree)
}

/// Snapshots the working copy as the jj CLI does before a command. When
/// another workspace's rewrite left the disk behind its working-copy commit,
/// it first checks that commit out (`jj workspace update-stale`): a snapshot
/// of the older disk would record it as a revert of the newer commit.
/// Returns the working-copy commit from before the snapshot and the
/// snapshotted tree. The caller holds [`with_working_copy_lock`].
fn snapshot_up_to_date_working_copy(
  loaded: &mut LoadedWorkspaceRepo,
  ignore_root: &str,
) -> Result<Option<(Commit, MergedTree)>, JjError> {
  // Each pass resolves one staleness or snapshots; a third means another
  // process keeps moving the working copy.
  for _ in 0..3 {
    let Some(wc_commit) = get_workspace_wc_commit(loaded)? else {
      return Ok(None);
    };
    let workspace_name = loaded.workspace.workspace_name().to_owned();
    let mut locked_ws = loaded
      .workspace
      .start_working_copy_mutation()
      .map_err(|e| JjError::IoError(format!("Failed to lock working copy: {e}")))?;
    let freshness = block_on(WorkingCopyFreshness::check_stale(
      locked_ws.locked_wc(),
      &wc_commit,
      &loaded.repo,
    ))
    .map_err(|e| JjError::IoError(format!("Failed to check staleness: {e}")))?;
    match freshness {
      WorkingCopyFreshness::Fresh => {
        let tree = snapshot_locked_working_copy(
          &mut loaded.repo,
          locked_ws,
          workspace_name,
          &wc_commit,
          ignore_root,
        )?;
        return Ok(Some((wc_commit, tree)));
      }
      // Another process moved the working copy after `loaded` was read:
      // keep its operation and read the repo again.
      WorkingCopyFreshness::Updated(_) => {
        let wc_op_id = locked_ws.locked_wc().old_operation_id().clone();
        block_on(locked_ws.finish(wc_op_id))
          .map_err(|e| JjError::IoError(format!("Failed to finish wc: {e}")))?;
        loaded.repo = block_on(loaded.workspace.repo_loader().load_at_head())
          .map_err(|e| JjError::IoError(format!("Failed to load repo: {e}")))?;
      }
      WorkingCopyFreshness::WorkingCopyStale | WorkingCopyFreshness::SiblingOperation => {
        block_on(locked_ws.locked_wc().check_out(&wc_commit))
          .map_err(|e| JjError::IoError(format!("Failed to check out: {e}")))?;
        block_on(locked_ws.finish(loaded.repo.op_id().clone()))
          .map_err(|e| JjError::IoError(format!("Failed to finish wc: {e}")))?;
      }
    }
  }
  Err(JjError::IoError(
    "Working copy kept moving during the snapshot".to_string(),
  ))
}

/// One load of a workspace's repository, for callers that make several
/// read-only queries in a row. Each `jj_*` function loads the repository
/// itself, which dominates their cost; the methods here share one load.
/// Queries see the repository as of `load`, so snapshot first if needed.
pub struct WorkspaceRepo {
  path: String,
  loaded: LoadedWorkspaceRepo,
}

impl WorkspaceRepo {
  pub fn load(workspace_path: &str) -> Result<Self, JjError> {
    Ok(Self {
      path: workspace_path.to_string(),
      loaded: load_workspace_repo(workspace_path)?,
    })
  }

  /// `jj_is_bookmark_conflicted`.
  pub fn is_bookmark_conflicted(&self, branch_name: &str) -> bool {
    is_bookmark_conflicted_in(&self.loaded, branch_name)
  }

  /// `jj_get_commit_id`.
  pub fn commit_id(&self, revision: &str) -> Result<String, JjError> {
    commit_id_in(&self.loaded, &self.path, revision)
  }

  /// Whether `name` is a listed local bookmark (`get_branches`), and
  /// whether `remote` has it (`check_remote_branch_exists`).
  pub fn bookmark_presence(&self, name: &str, remote: &str) -> (bool, bool) {
    let view = self.loaded.repo.view();
    let local = !name.starts_with(STASH_BOOKMARK_PREFIX)
      && view.local_bookmarks().any(|(n, _)| n.as_str() == name);
    let remote_ref = view.get_remote_bookmark(RemoteRefSymbol {
      name: RefName::new(name),
      remote: RemoteName::new(remote),
    });
    (local, !remote_ref.target.is_absent())
  }

  /// `jj_get_commits_ahead`.
  pub fn commits_ahead(&self, target_branch: &str) -> Result<JjCommitsAhead, JjError> {
    commits_ahead_in(&self.loaded, &self.path, target_branch)
  }

  /// Loads for a whole Changes reload: brings a stale working copy up to
  /// date, snapshots it, and reads its changed files and conflicts under one
  /// working-copy lock. Queries on the result see the snapshot.
  pub fn load_working_copy(workspace_path: &str) -> Result<(Self, WorkingCopyChanges), JjError> {
    let ignore_root = match derive_repo_path_from_workspace(workspace_path) {
      Some(repo_path) => repo_path,
      None => {
        // Takes the working-copy lock itself.
        reconcile_colocated_home_repo(workspace_path)?;
        workspace_path.to_string()
      }
    };
    with_working_copy_lock(workspace_path, || {
      let mut loaded = load_workspace_repo(workspace_path)?;
      // As `jj_get_changed_files`, a failed snapshot lists no changes.
      let files = match snapshot_up_to_date_working_copy(&mut loaded, &ignore_root) {
        Ok(Some((wc_commit, tree))) => tree_changes(&loaded.repo, &wc_commit, &tree),
        Ok(None) => Vec::new(),
        Err(err) => {
          tracing::warn!("Failed to snapshot {workspace_path}: {err}");
          Vec::new()
        }
      };
      let conflicted = conflicted_files_in(&mut loaded, workspace_path, false)?;
      let token = get_workspace_wc_commit(&loaded)
        .ok()
        .flatten()
        .and_then(|wc_commit| {
          let parent_tree = block_on(wc_commit.parent_tree(loaded.repo.as_ref())).ok()?;
          Some(working_copy_token(&parent_tree, &wc_commit.tree()))
        });
      let repo = Self {
        path: workspace_path.to_string(),
        loaded,
      };
      let changes = WorkingCopyChanges {
        files,
        conflicted,
        token,
      };
      Ok((repo, changes))
    })
  }

  /// `jj_get_merge_diff_between_revisions`.
  pub fn merge_diff_between_revisions(
    &self,
    from_revision: &str,
    to_revision: &str,
    conflict_marker_style: &str,
  ) -> Result<JjRevisionDiff, JjError> {
    merge_diff_between_revisions_in(
      &self.loaded,
      from_revision,
      to_revision,
      conflict_marker_style,
    )
  }
}

/// What a Changes reload reads from the working copy, from
/// [`WorkspaceRepo::load_working_copy`].
pub struct WorkingCopyChanges {
  /// As [`jj_get_changed_files`] lists them.
  pub files: Vec<JjFileChange>,
  /// As [`get_conflicted_files`] lists them.
  pub conflicted: Vec<String>,
  /// The [`working_copy_token`] that [`jj_get_file_hunks_batch`] would
  /// return now.
  pub token: Option<String>,
}

fn load_workspace_repo(workspace_path: &str) -> Result<LoadedWorkspaceRepo, JjError> {
  let repo_path_opt = derive_repo_path_from_workspace(workspace_path);
  let settings_path = repo_path_opt.as_deref().unwrap_or(workspace_path);
  let settings = create_user_settings(settings_path)?;
  let workspace_root = Path::new(workspace_path);
  let workspace = Workspace::load(
    &settings,
    workspace_root,
    &StoreFactories::default(),
    &default_working_copy_factories(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to load workspace: {}", e)))?;
  let repo = block_on(workspace.repo_loader().load_at_head())
    .map_err(|e| JjError::IoError(format!("Failed to load repo: {}", e)))?;
  let path_converter = RepoPathUiConverter::Fs {
    cwd: workspace_root.to_path_buf(),
    base: workspace_root.to_path_buf(),
  };

  let home_git_head_branch = if repo_path_opt.is_none() {
    read_git_head_branch(workspace_path)
      .ok()
      .filter(|branch| branch != "HEAD")
  } else {
    None
  };

  Ok(LoadedWorkspaceRepo {
    settings,
    workspace,
    repo,
    path_converter,
    home_git_head_branch,
  })
}

/// Reconciles a colocated home working copy using jj-cli's snapshot ordering.
///
/// The Git import/export lock covers the complete HEAD import, metadata reset,
/// filesystem snapshot, and refs import cycle. This also serializes concurrent
/// startup reads in this process and in other jj/Treq processes.
fn reconcile_colocated_home_repo(repo_path: &str) -> Result<(), JjError> {
  let initial = load_workspace_repo(repo_path)?;
  let _git_lock = FileLock::lock(initial.workspace.repo_path().join("git_import_export.lock"))
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to lock Git import/export: {e}")))?;

  // Another process may have published an operation while this caller waited
  // for the lock, so always reload the latest operation under the lock.
  let mut loaded = load_workspace_repo(repo_path)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();

  let mut head_tx = loaded.repo.start_transaction();
  block_on(git::import_head(head_tx.repo_mut()))
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to import Git HEAD: {e}")))?;
  let new_git_head = head_tx.repo().view().git_head().clone();
  let mut locked_ws = None;
  if let Some(head_id) = new_git_head.as_normal() {
    let wc_is_based_on_git_head = head_tx
      .repo()
      .view()
      .get_wc_commit_id(&workspace_name)
      .and_then(|wc_id| head_tx.repo().store().get_commit(wc_id).ok())
      .is_some_and(|wc_commit| {
        wc_commit.parent_ids().len() == 1 && wc_commit.parent_ids().first() == Some(head_id)
      });
    if !wc_is_based_on_git_head {
      let head_commit = head_tx
        .repo()
        .store()
        .get_commit(head_id)
        .map_err(|e| JjError::GitWorkspaceError(format!("Failed to load Git HEAD: {e}")))?;
      let wc_commit = block_on(head_tx.repo_mut().check_out(workspace_name, &head_commit))
        .map_err(|e| JjError::GitWorkspaceError(format!("Failed to check out Git HEAD: {e}")))?;
      let mut working_copy = loaded
        .workspace
        .start_working_copy_mutation()
        .map_err(|e| JjError::GitWorkspaceError(format!("Failed to lock working copy: {e}")))?;
      block_on(working_copy.locked_wc().reset(&wc_commit)).map_err(|e| {
        JjError::GitWorkspaceError(format!("Failed to reset working-copy metadata: {e}"))
      })?;
      block_on(head_tx.repo_mut().rebase_descendants()).map_err(|e| {
        JjError::GitWorkspaceError(format!("Failed to rebase after Git HEAD import: {e}"))
      })?;
      locked_ws = Some(working_copy);
    }
  }
  if head_tx.repo().has_changes() {
    loaded.repo = block_on(head_tx.commit("import git head"))
      .map_err(|e| JjError::GitWorkspaceError(format!("Failed to commit Git HEAD import: {e}")))?;
    if let Some(working_copy) = locked_ws {
      block_on(working_copy.finish(loaded.repo.op_id().clone())).map_err(|e| {
        JjError::GitWorkspaceError(format!("Failed to finish working-copy reset: {e}"))
      })?;
    }
  }

  snapshot_loaded_working_copy(&mut loaded, repo_path)?;
  import_remaining_git_refs(&mut loaded)?;
  Ok(())
}

// Non-home workspaces retain Treq's existing lightweight import behavior.
fn import_git_head_if_needed(
  loaded: &mut LoadedWorkspaceRepo,
  repo_path: &str,
) -> Result<(), JjError> {
  let git_head_branch = read_git_head_branch(repo_path)
    .ok()
    .filter(|branch| !branch.is_empty() && branch != "HEAD");
  let mut tx = loaded.repo.start_transaction();
  block_on(git::import_head(tx.repo_mut()))
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to import Git HEAD: {e}")))?;
  let head_id = tx.repo().view().git_head().added_ids().next().cloned();
  if let (Some(branch), Some(head_id)) = (git_head_branch.as_ref(), head_id) {
    if tx
      .repo()
      .view()
      .get_local_bookmark(RefName::new(branch))
      .is_absent()
    {
      tx.repo_mut()
        .set_local_bookmark_target(RefName::new(branch), RefTarget::normal(head_id));
    }
  }
  if tx.repo().has_changes() {
    loaded.repo = block_on(tx.commit("import git head"))
      .map_err(|e| JjError::GitWorkspaceError(format!("Failed to commit Git HEAD import: {e}")))?;
  }
  Ok(())
}

fn import_colocated_git_state(
  loaded: &mut LoadedWorkspaceRepo,
  repo_path: &str,
) -> Result<(), JjError> {
  import_git_head_if_needed(loaded, repo_path)?;
  import_remaining_git_refs(loaded)
}

fn snapshot_loaded_working_copy(
  loaded: &mut LoadedWorkspaceRepo,
  workspace_path: &str,
) -> Result<(), JjError> {
  with_working_copy_lock(workspace_path, || {
    snapshot_loaded_working_copy_inner(loaded, workspace_path)
  })
}

fn snapshot_loaded_working_copy_inner(
  loaded: &mut LoadedWorkspaceRepo,
  workspace_path: &str,
) -> Result<(), JjError> {
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let Some(wc_id) = loaded
    .repo
    .view()
    .get_wc_commit_id(&workspace_name)
    .cloned()
  else {
    return Ok(());
  };
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_id)
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {e}")))?;
  let matcher = repo_root_matcher();
  let opts = snapshot_options_for_all_paths(workspace_path, &matcher);
  let mut locked_ws = loaded
    .workspace
    .start_working_copy_mutation()
    .map_err(|e| JjError::IoError(format!("Failed to lock working copy: {e}")))?;
  let (new_tree, _) = block_on(locked_ws.locked_wc().snapshot(&opts))
    .map_err(|e| JjError::IoError(format!("Failed to snapshot working copy: {e}")))?;

  if new_tree.tree_ids_and_labels() != wc_commit.tree().tree_ids_and_labels() {
    let mut tx = loaded.repo.start_transaction();
    let rewritten = block_on(
      tx.repo_mut()
        .rewrite_commit(&wc_commit)
        .set_tree(new_tree)
        .write(),
    )
    .map_err(|e| JjError::IoError(format!("Failed to write working-copy snapshot: {e}")))?;
    tx.repo_mut()
      .set_wc_commit(workspace_name, rewritten.id().clone())
      .map_err(|e| JjError::IoError(format!("Failed to set working-copy commit: {e}")))?;
    block_on(tx.repo_mut().rebase_descendants())
      .map_err(|e| JjError::IoError(format!("Failed to rebase after snapshot: {e}")))?;
    loaded.repo = block_on(tx.commit("snapshot working copy"))
      .map_err(|e| JjError::IoError(format!("Failed to commit working-copy snapshot: {e}")))?;
  }
  block_on(locked_ws.finish(loaded.repo.op_id().clone()))
    .map_err(|e| JjError::IoError(format!("Failed to finish working-copy snapshot: {e}")))?;
  Ok(())
}

/// jj-lib's built-in revset functions omit `mutable()` (CLI may inject it via config).
/// Strip it so revsets intended for CLI parity parse under library evaluation.
fn strip_mutable_revset_filter(revset: &str) -> String {
  revset
    .replace("mutable() & ", "")
    .replace(" & mutable()", "")
    .replace("mutable()", "")
}

fn evaluate_revset<'a>(
  loaded: &'a LoadedWorkspaceRepo,
  revset_str: &str,
) -> Result<Box<dyn Revset + 'a>, JjError> {
  let aliases_map = RevsetAliasesMap::new();
  let fileset_aliases_map = FilesetAliasesMap::new();
  let revset_extensions = jj_lib::revset::RevsetExtensions::default();
  let now = loaded
    .settings
    .commit_timestamp()
    .map(|timestamp| {
      chrono::Local
        .timestamp_millis_opt(timestamp.timestamp.0)
        .single()
        .unwrap_or_else(chrono::Local::now)
    })
    .unwrap_or_else(chrono::Local::now);
  let context = RevsetParseContext {
    aliases_map: &aliases_map,
    local_variables: std::collections::HashMap::new(),
    user_email: loaded.settings.user_email(),
    date_pattern_context: now.into(),
    default_ignored_remote: None,
    fileset_aliases_map: &fileset_aliases_map,
    use_glob_by_default: false,
    extensions: &revset_extensions,
    workspace: Some(RevsetWorkspaceContext {
      path_converter: &loaded.path_converter,
      workspace_name: loaded.workspace.workspace_name(),
    }),
  };
  let mut diagnostics = RevsetDiagnostics::new();
  let expression = revset::parse(&mut diagnostics, revset_str, &context)
    .map_err(|e| JjError::IoError(format!("Failed to parse revset '{}': {}", revset_str, e)))?;
  let extensions: Vec<Box<dyn jj_lib::revset::SymbolResolverExtension>> = Vec::new();
  let symbol_resolver = SymbolResolver::new(loaded.repo.as_ref(), &extensions);
  let resolved = expression
    .resolve_user_expression(loaded.repo.as_ref(), &symbol_resolver)
    .map_err(|e| JjError::IoError(format!("Failed to resolve revset '{}': {}", revset_str, e)))?;
  resolved
    .evaluate(loaded.repo.as_ref())
    .map_err(|e| JjError::IoError(format!("Failed to evaluate revset '{}': {}", revset_str, e)))
}

/// Snapshots the working copy into its working-copy commit, as the jj CLI
/// does before every command, and returns that commit and its tree.
/// `loaded.repo` moves to the snapshot operation when the tree changed.
///
/// The saved working-copy state and the commit must advance together. Saving
/// the snapshotted state while the commit keeps its older tree makes the
/// working copy look stale after the next operation that does not touch it
/// (a HEAD or refs import), and stale recovery then checks out the older tree
/// and deletes every file only the snapshot had seen.
fn snapshot_working_copy_tree(
  loaded: &mut LoadedWorkspaceRepo,
  workspace_path: &str,
) -> Result<Option<(jj_lib::backend::CommitId, MergedTree)>, JjError> {
  let ignore_root =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  snapshot_loaded_working_copy_inner(loaded, &ignore_root)?;

  let Some(wc_commit_id) = loaded
    .repo
    .view()
    .get_wc_commit_id(loaded.workspace.workspace_name())
    .cloned()
  else {
    return Ok(None);
  };
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {e}")))?;
  Ok(Some((wc_commit_id, wc_commit.tree())))
}

fn count_lines(content: &[u8]) -> u32 {
  if content.is_empty() {
    0
  } else {
    content.split_inclusive(|byte| *byte == b'\n').count() as u32
  }
}

fn bytes_to_text(bytes: Vec<u8>) -> Option<Vec<u8>> {
  if bytes.contains(&0) {
    return None;
  }
  String::from_utf8(bytes).ok().map(String::into_bytes)
}

async fn materialized_value_to_bytes(
  path: &RepoPath,
  value: MaterializedTreeValue,
  conflict_marker_style: ConflictMarkerStyle,
  merge_options: Option<&MergeOptions>,
) -> Option<Vec<u8>> {
  match value {
    MaterializedTreeValue::Absent => None,
    MaterializedTreeValue::File(mut file) => file.read_all(path).await.ok().and_then(bytes_to_text),
    MaterializedTreeValue::FileConflict(file) => {
      let merge_options = merge_options?;
      let options = ConflictMaterializeOptions {
        marker_style: conflict_marker_style,
        marker_len: None,
        merge: merge_options.clone(),
      };
      let bytes =
        materialize_merge_result_to_bytes(&file.contents, &file.labels, &options).to_vec();
      bytes_to_text(bytes)
    }
    MaterializedTreeValue::Symlink { target, .. } => Some(target.into_bytes()),
    MaterializedTreeValue::AccessDenied(_)
    | MaterializedTreeValue::OtherConflict { .. }
    | MaterializedTreeValue::GitSubmodule(_)
    | MaterializedTreeValue::Tree(_) => None,
  }
}

fn diff_line_counts(before: &str, after: &str) -> (u32, u32) {
  let input = InternedInput::new(before, after);
  let counts = diff(Algorithm::Histogram, &input, Counter::default());
  (counts.insertions, counts.removals)
}

fn build_text_hunks(before: &str, after: &str) -> Vec<JjDiffHunk> {
  let input = InternedInput::new(before, after);
  let patch = diff(
    Algorithm::Histogram,
    &input,
    UnifiedDiffBuilder::new(&input),
  );
  if patch.is_empty() {
    Vec::new()
  } else {
    parse_git_diff_hunks(&patch).unwrap_or_default()
  }
}

fn compute_commit_stats(
  repo: &Arc<ReadonlyRepo>,
  commit: &jj_lib::commit::Commit,
  tree_override: Option<&MergedTree>,
) -> (u32, u32) {
  block_on(async {
    let parent_tree = commit
      .parent_tree(repo.as_ref())
      .await
      .unwrap_or_else(|_| repo.store().root_commit().tree());
    let commit_tree = tree_override.cloned().unwrap_or_else(|| commit.tree());
    let conflict_labels = ConflictLabels::unlabeled();
    let copy_records = CopyRecords::default();
    let matcher = repo_root_matcher();
    let diff_stream = materialized_diff_stream(
      repo.store(),
      parent_tree.diff_stream_with_copies(&commit_tree, &matcher, &copy_records),
      Diff::new(&conflict_labels, &conflict_labels),
    );

    let mut insertions = 0;
    let mut deletions = 0;
    futures::pin_mut!(diff_stream);
    while let Some(entry) = diff_stream.next().await {
      let Ok(values) = entry.values else { continue };
      let before = materialized_value_to_bytes(
        entry.path.source(),
        values.before,
        ConflictMarkerStyle::Git,
        None,
      )
      .await;
      let after = materialized_value_to_bytes(
        entry.path.target(),
        values.after,
        ConflictMarkerStyle::Git,
        None,
      )
      .await;

      match (before, after) {
        (None, Some(after)) => {
          insertions += count_lines(&after);
        }
        (Some(before), None) => {
          deletions += count_lines(&before);
        }
        (Some(before), Some(after)) => {
          let Ok(before) = String::from_utf8(before) else {
            continue;
          };
          let Ok(after) = String::from_utf8(after) else {
            continue;
          };
          let (added, removed) = diff_line_counts(&before, &after);
          insertions += added;
          deletions += removed;
        }
        (None, None) => {}
      }
    }

    (insertions, deletions)
  })
}

fn is_empty_commit(repo: &Arc<ReadonlyRepo>, commit: &jj_lib::commit::Commit) -> bool {
  let parent_tree = block_on(commit.parent_tree(repo.as_ref()))
    .unwrap_or_else(|_| repo.store().root_commit().tree());
  parent_tree.tree_ids() == commit.tree().tree_ids()
}

pub const AUTOSAVE_DESCRIPTION_PREFIX: &str = "treq-autosave:";

pub fn is_autosave_description(description: &str) -> bool {
  description
    .lines()
    .next()
    .unwrap_or("")
    .trim()
    .starts_with(AUTOSAVE_DESCRIPTION_PREFIX)
}

pub fn autosave_commit_message(paths: &[String]) -> String {
  let mut paths: Vec<&str> = paths.iter().map(String::as_str).collect();
  paths.sort_unstable();
  match paths.as_slice() {
    [] => AUTOSAVE_DESCRIPTION_PREFIX.to_string(),
    [one] => format!("{AUTOSAVE_DESCRIPTION_PREFIX} {one}"),
    [first, second] => format!("{AUTOSAVE_DESCRIPTION_PREFIX} {first}, {second}"),
    [first, second, rest @ ..] => format!(
      "{AUTOSAVE_DESCRIPTION_PREFIX} {first}, {second}, … N{} more",
      rest.len()
    ),
  }
}

fn commit_description_first_line(commit: &jj_lib::commit::Commit) -> String {
  commit
    .description()
    .lines()
    .next()
    .filter(|line| !line.is_empty())
    .unwrap_or("(no description)")
    .to_string()
}

fn build_log_commits(
  cache_repo_path: &str,
  repo: &Arc<ReadonlyRepo>,
  commits: Vec<jj_lib::commit::Commit>,
  wc_commit_ids: &HashSet<jj_lib::backend::CommitId>,
  wc_tree_override: Option<(&jj_lib::backend::CommitId, &MergedTree)>,
  is_immutable: &impl Fn(
    &jj_lib::backend::CommitId,
  ) -> Result<bool, jj_lib::revset::RevsetEvaluationError>,
) -> Vec<JjLogCommit> {
  let full_commit_ids: Vec<String> = commits.iter().map(|c| c.id().hex()).collect();
  let cached_map = local_db::get_cached_commit_diff_stats_batch(cache_repo_path, &full_commit_ids)
    .unwrap_or_default();

  let mut misses = Vec::new();
  let built = commits
    .into_iter()
    .map(|commit| {
      build_log_commit(
        repo,
        &commit,
        wc_commit_ids,
        wc_tree_override,
        is_immutable,
        &cached_map,
        &mut misses,
      )
    })
    .collect();
  local_db::cache_commit_rows(cache_repo_path, &misses).warn_on_err("cache commit rows");
  built
}

/// Short ids of conflicted `commits` whose conflict no longer reaches the tip
/// of the stack. jj never rewrites a conflicted commit when a later commit
/// resolves it, so its own tree stays conflicted forever. A conflict counts as
/// resolved when every head descending from the commit (among `commits` and
/// the working copy, read from its snapshot when there is one) has a resolved
/// value at each of the commit's conflicted paths.
fn conflicts_resolved_by_descendants(
  repo: &Arc<ReadonlyRepo>,
  commits: &[Commit],
  wc_commit: Option<&Commit>,
  wc_tree_override: Option<(&jj_lib::backend::CommitId, &MergedTree)>,
) -> HashSet<String> {
  if !commits.iter().any(|commit| commit.tree().has_conflict()) {
    return HashSet::new();
  }
  let index = repo.index();
  let is_ancestor =
    |a: &Commit, b: &Commit| a.id() != b.id() && index.is_ancestor(a.id(), b.id()).unwrap_or(false);
  let candidates: Vec<(&Commit, MergedTree)> = commits
    .iter()
    .map(|commit| (commit, commit.tree()))
    .chain(wc_commit.map(|wc| {
      let tree = wc_tree_override
        .filter(|(id, _)| *id == wc.id())
        .map(|(_, tree)| tree.clone());
      (wc, tree.unwrap_or_else(|| wc.tree()))
    }))
    .collect();
  let heads: Vec<&(&Commit, MergedTree)> = candidates
    .iter()
    .filter(|(c, _)| !candidates.iter().any(|(d, _)| is_ancestor(c, d)))
    .collect();

  commits
    .iter()
    .filter(|commit| commit.tree().has_conflict())
    .filter(|commit| {
      let paths: Vec<RepoPathBuf> = commit.tree().conflicts().map(|(path, _)| path).collect();
      let mut descendants = heads
        .iter()
        .filter(|(head, _)| is_ancestor(commit, head))
        .peekable();
      descendants.peek().is_some()
        && descendants.all(|(_, tree)| {
          paths
            .iter()
            .all(|path| block_on(tree.path_value(path)).is_ok_and(|value| value.is_resolved()))
        })
    })
    .map(|commit| commit.id().hex()[..12].to_string())
    .collect()
}

/// Pushes a commit missing from `cached_map` to `misses`, for the caller to
/// cache in one transaction.
fn build_log_commit(
  repo: &Arc<ReadonlyRepo>,
  commit: &jj_lib::commit::Commit,
  wc_commit_ids: &HashSet<jj_lib::backend::CommitId>,
  wc_tree_override: Option<(&jj_lib::backend::CommitId, &MergedTree)>,
  is_immutable: &impl Fn(
    &jj_lib::backend::CommitId,
  ) -> Result<bool, jj_lib::revset::RevsetEvaluationError>,
  cached_map: &std::collections::HashMap<String, local_db::CachedCommitDiffStat>,
  misses: &mut Vec<local_db::CommitRow>,
) -> JjLogCommit {
  let tree_override = wc_tree_override
    .filter(|(wc_commit_id, _)| **wc_commit_id == *commit.id())
    .map(|(_, tree)| tree);
  let full_commit_id = commit.id().hex();

  // Volatile fields always computed from live repo state.
  let is_working_copy = wc_commit_ids.contains(commit.id());
  let bookmarks: Vec<String> = repo
    .view()
    .local_bookmarks_for_commit(commit.id())
    .filter(|&(_, target)| target.as_normal() == Some(commit.id()))
    .map(|(name, _)| name.as_str().to_string())
    .collect();
  let is_immutable_val = is_immutable(commit.id()).unwrap_or(false);
  let has_conflicts = tree_override
    .map(|tree| tree.has_conflict())
    .unwrap_or_else(|| commit.tree().has_conflict());
  let parent_ids: Vec<String> = commit
    .parent_ids()
    .iter()
    .map(|id| id.hex()[..12].to_string())
    .collect();

  let cached = cached_map.get(&full_commit_id);
  match cached {
    Some(cached)
      if cached.change_id.is_some()
        && cached.description.is_some()
        && cached.author_name.is_some()
        && cached.timestamp.is_some() =>
    {
      JjLogCommit {
        commit_id: cached.commit_id[..12.min(cached.commit_id.len())].to_string(),
        short_id: cached.commit_id[..12.min(cached.commit_id.len())].to_string(),
        change_id: cached.change_id.clone().unwrap_or_default(),
        description: cached.description.clone().unwrap_or_default(),
        author_name: cached.author_name.clone().unwrap_or_default(),
        timestamp: cached.timestamp.clone().unwrap_or_default(),
        parent_ids,
        is_working_copy,
        workspace_label: None,
        bookmarks,
        is_immutable: is_immutable_val,
        insertions: cached.insertions,
        deletions: cached.deletions,
        on_target_only: false,
        has_conflicts,
      }
    }
    _ => {
      let short_id = full_commit_id[..12].to_string();
      let change_id = HexPrefix::from_id(commit.change_id()).reverse_hex()[..12].to_string();
      let description = commit_description_first_line(commit);
      let author_name = commit.author().name.clone();
      let timestamp = commit
        .author()
        .timestamp
        .to_datetime()
        .map(|ts| ts.to_rfc3339())
        .unwrap_or_else(|_| commit.author().timestamp.timestamp.0.to_string());

      let (insertions, deletions) = compute_commit_stats(repo, commit, tree_override);

      misses.push(local_db::CommitRow {
        commit_id: full_commit_id,
        insertions,
        deletions,
        change_id: change_id.clone(),
        description: description.clone(),
        author_name: author_name.clone(),
        timestamp: timestamp.clone(),
      });

      JjLogCommit {
        commit_id: short_id.clone(),
        short_id,
        change_id,
        description,
        author_name,
        timestamp,
        parent_ids,
        is_working_copy,
        workspace_label: None,
        bookmarks,
        is_immutable: is_immutable_val,
        insertions,
        deletions,
        on_target_only: false,
        has_conflicts,
      }
    }
  }
}

fn snapshot_options_for_all_paths<'a>(
  ignore_root: &str,
  start_tracking_matcher: &'a dyn jj_lib::matchers::Matcher,
) -> SnapshotOptions<'a> {
  SnapshotOptions {
    base_ignores: build_snapshot_base_ignores(ignore_root),
    progress: None,
    start_tracking_matcher,
    force_tracking_matcher: &NothingMatcher,
    max_new_file_size: 1024 * 1024,
  }
}

/// Convert git remote branch format to jj bookmark format
/// Examples: "origin/main" -> "main@origin" (if origin is a remote)
///           "treq/test" -> "treq/test" (if treq is not a remote)
fn convert_git_branch_to_jj_format(branch: &str, repo_path: &str) -> String {
  if let Some(slash_pos) = branch.find('/') {
    let prefix = &branch[..slash_pos];
    let suffix = &branch[slash_pos + 1..];

    let remotes = get_git_remotes(repo_path);

    if remotes.contains(prefix) {
      // This is a remote reference, convert to jj format
      format!("{}@{}", suffix, prefix)
    } else {
      // This is a local bookmark with namespace pattern
      branch.to_string()
    }
  } else {
    branch.to_string()
  }
}

/// Public wrapper for use by auto_rebase and other modules
pub fn convert_git_branch_to_jj_format_public(branch: &str, repo_path: &str) -> String {
  convert_git_branch_to_jj_format(branch, repo_path)
}

/// Error type for jj operations
#[derive(Debug)]
pub enum JjError {
  AlreadyInitialized,
  NotGitRepository,
  InitFailed(String),
  ConfigError(String),
  WorkspaceNotFound(String),
  GitWorkspaceError(String),
  IoError(String),
  BookmarkConflict(BookmarkConflictInfo),
}

/// Information about a jj workspace
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceInfo {
  pub name: String,
  pub path: String,
  pub branch: String,
  pub is_colocated: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiscoveredWorkspace {
  pub workspace_name: String,
  pub workspace_path: String,
  pub branch_name: String,
  pub has_conflicts: bool,
  /// Author timestamp of the workspace working-copy tip (RFC3339), when known.
  pub last_activity_at: Option<String>,
  /// The local `branch_name` bookmark is conflicted, as
  /// [`jj_is_bookmark_conflicted`] reports it.
  #[serde(default)]
  pub bookmark_conflicted: bool,
}

/// A diff hunk from jj diff output
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct JjDiffHunk {
  pub id: String,
  pub header: String,
  pub lines: Vec<String>,
  /// Always empty now: the header and lines are the hunk's text. Kept so
  /// that a peer that still requires the field can read it.
  #[serde(default)]
  pub patch: String,
  #[serde(default)]
  pub conflict_style: ConflictStyle,
  #[serde(default)]
  pub conflict_regions: Vec<ConflictRegionView>,
}

/// File change status in JJ working copy
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct JjFileChange {
  pub path: String,
  pub status: String,
  pub previous_path: Option<String>,
  pub changed_line_count: usize,
  pub diff_deferred: bool,
  /// Names the file's two sides in a working-copy diff (with the marker
  /// style, from `workspace_diff`): equal ids mean equal hunks, so a reread
  /// keeps the hunks of files whose id held. `None` when unknown.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub content_id: Option<String>,
}

/// File content lines for context expansion
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JjFileLines {
  pub lines: Vec<String>,
  pub start_line: usize,
  pub end_line: usize,
}

/// Result of a rebase operation
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JjRebaseResult {
  pub success: bool,
  pub message: String,
}

/// A single commit in the log
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct JjLogCommit {
  pub commit_id: String,
  pub short_id: String,
  pub change_id: String,
  pub description: String,
  pub author_name: String,
  pub timestamp: String,
  pub parent_ids: Vec<String>,
  pub is_working_copy: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub workspace_label: Option<String>,
  pub bookmarks: Vec<String>,
  pub is_immutable: bool,
  pub insertions: u32,
  pub deletions: u32,
  /// True when this commit is on the target branch but not the current branch (target is ahead).
  #[serde(default)]
  pub on_target_only: bool,
  /// True when this commit's tree contains jj conflicts that no later commit
  /// in the log (or the working copy) has resolved.
  #[serde(default)]
  pub has_conflicts: bool,
}

/// The full log response including metadata
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct JjLogResult {
  /// Commit history: workspace/current-branch commits first, then target-only commits (`on_target_only = true`) when target-branch history is included.
  pub commits: Vec<JjLogCommit>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub tentative_working_copy: Option<JjTentativeWorkingCopy>,
  pub target_branch: String,
  pub workspace_branch: String,
  /// Deprecated: target-only commits are included in `commits` with `on_target_only = true`.
  #[serde(default)]
  pub target_branch_commits: Vec<JjLogCommit>,
  /// Merge-base commit ID between current and target branch (home-repo non-default views only).
  #[serde(default)]
  pub merge_base_id: Option<String>,
}

/// A tentative working-copy entry shown above the normal commit history when dirty.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct JjTentativeWorkingCopy {
  pub workspace_label: String,
  pub commit: JjLogCommit,
}

/// Result of a home-repo rebase dry run
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HomeRebaseDryRunResult {
  pub would_conflict: bool,
  pub conflicted_files: Vec<String>,
}

/// Commits ahead of target branch
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JjCommitsAhead {
  pub commits: Vec<JjLogCommit>,
  pub total_count: usize,
}

/// Detailed metadata about a conflicted bookmark revision
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BookmarkConflictCommit {
  pub commit_id: String,
  pub short_commit_id: String,
  pub change_id: String,
  pub description: String,
  pub author_name: String,
  pub timestamp: String,
  pub diff_summary: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BookmarkConflictInfo {
  pub bookmark: String,
  pub commits: Vec<BookmarkConflictCommit>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BookmarkConflictResolutionResult {
  pub success: bool,
  pub message: String,
  pub preserved_change_ids: Vec<String>,
}

/// Result of merge operation
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JjMergeResult {
  pub success: bool,
  pub message: String,
  pub has_conflicts: bool,
  pub conflicted_files: Vec<String>,
  pub merge_commit_id: Option<String>,
}

/// Diff hunks for a single file
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JjFileDiff {
  pub path: String,
  pub hunks: Vec<JjDiffHunk>,
  #[serde(default)]
  pub conflict_style: ConflictStyle,
  #[serde(default)]
  pub conflict_regions: Vec<ConflictRegionView>,
}

/// Combined diff between two revisions
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JjRevisionDiff {
  pub committed_files: Vec<JjFileChange>,
  pub hunks_by_file: Vec<JjFileDiff>,
  #[serde(default)]
  pub uncommitted_files: Vec<JjFileChange>,
  #[serde(default)]
  pub conflicted_files: Vec<String>,
  pub too_large_to_render: bool,
  #[serde(default)]
  pub render_block_reason: Option<String>,
  /// Names the working-copy diff `uncommitted_files` came from, as
  /// `WorkspaceFileHunksBatch::snapshot_token` does; `None` when unknown.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub working_copy_token: Option<String>,
}

const LARGE_COMMIT_FILE_DIFF_THRESHOLD: usize = 500;
const TOO_LARGE_COMMIT_DIFF_THRESHOLD: usize = 10_000;
const TOO_LARGE_COMMIT_DIFF_MESSAGE: &str =
  "This commit changes more than 10,000 lines and is too large to render.";

impl std::fmt::Display for JjError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      JjError::AlreadyInitialized => write!(f, "Jujutsu workspace already exists"),
      JjError::NotGitRepository => write!(f, "Not a git repository"),
      JjError::InitFailed(e) => write!(f, "Failed to initialize jj: {}", e),
      JjError::ConfigError(e) => write!(f, "Configuration error: {}", e),
      JjError::WorkspaceNotFound(name) => write!(f, "Workspace '{}' not found", name),
      JjError::GitWorkspaceError(e) => write!(f, "Git workspace error: {}", e),
      JjError::IoError(e) => write!(f, "IO error: {}", e),
      JjError::BookmarkConflict(info) => write!(
        f,
        "Conflicted bookmark '{}' with {} revision(s)",
        info.bookmark,
        info.commits.len()
      ),
    }
  }
}

/// Check if a jj workspace already exists at the given path
pub fn is_jj_workspace(repo_path: &str) -> bool {
  Path::new(repo_path).join(".jj").exists()
}

/// Get git user.name and user.email by shelling out to git config.
/// Used once during repo init to cache values into the local db.
fn read_git_user_config_from_git(repo_path: &str) -> (String, String) {
  let (name, email) = if let Ok(repo) = gix::open(repo_path) {
    let config = repo.config_snapshot();
    let read_value = |key: &str| {
      config
        .string(key)
        .map(|v| String::from_utf8_lossy(v.as_ref()).trim().to_string())
        .filter(|v| !v.is_empty())
    };
    (read_value("user.name"), read_value("user.email"))
  } else {
    (None, None)
  };

  let name = name.unwrap_or_else(|| "Treq User".to_string());
  let email = email.unwrap_or_else(|| "treq@localhost".to_string());

  (name, email)
}

/// Persist git user config to the local db so that subsequent calls don't
/// need to shell out to git. Safe to call multiple times (idempotent).
pub fn cache_git_user_config(
  db: &crate::db::Database,
  repo_path: &str,
) -> Result<(String, String), JjError> {
  let (name, email) = read_git_user_config_from_git(repo_path);
  forget_user_settings(repo_path);
  db.set_repo_setting(repo_path, "git_user_name", &name)
    .map_err(|e| JjError::ConfigError(format!("Failed to cache user.name: {}", e)))?;
  db.set_repo_setting(repo_path, "git_user_email", &email)
    .map_err(|e| JjError::ConfigError(format!("Failed to cache user.email: {}", e)))?;
  Ok((name, email))
}

/// Read cached git user.name / user.email from the local db. Falls back to
/// reading from git (and caching) when the db has no entry yet.
fn get_git_user_config(repo_path: &str) -> (String, String) {
  let db_path = local_db::get_local_db_path(repo_path);
  if let Ok(db) = crate::db::Database::new(db_path) {
    let cached_name = db
      .get_repo_setting(repo_path, "git_user_name")
      .ok()
      .flatten();
    let cached_email = db
      .get_repo_setting(repo_path, "git_user_email")
      .ok()
      .flatten();
    if let (Some(n), Some(e)) = (cached_name, cached_email) {
      return (n, e);
    }
    if let Ok((n, e)) = cache_git_user_config(&db, repo_path) {
      return (n, e);
    }
  }
  read_git_user_config_from_git(repo_path)
}

/// `UserSettings` built per repo path. Every jj operation needs them, and
/// building them opens the local db and parses a config layer. The git
/// identity behind them only changes through `cache_git_user_config`, which
/// forgets the entry.
fn user_settings_cache() -> &'static std::sync::Mutex<HashMap<String, UserSettings>> {
  static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, UserSettings>>> =
    std::sync::OnceLock::new();
  CACHE.get_or_init(Default::default)
}

fn forget_user_settings(repo_path: &str) {
  if let Ok(mut cache) = user_settings_cache().lock() {
    cache.remove(repo_path);
  }
}

fn create_user_settings(repo_path: &str) -> Result<UserSettings, JjError> {
  if let Some(settings) = user_settings_cache()
    .lock()
    .ok()
    .and_then(|cache| cache.get(repo_path).cloned())
  {
    return Ok(settings);
  }
  let settings = build_user_settings(repo_path)?;
  if let Ok(mut cache) = user_settings_cache().lock() {
    cache.insert(repo_path.to_string(), settings.clone());
  }
  Ok(settings)
}

/// Create UserSettings with reasonable defaults for Treq
/// Uses git config values if available, otherwise uses defaults
fn build_user_settings(repo_path: &str) -> Result<UserSettings, JjError> {
  // Get user info from git config
  let (user_name, user_email) = get_git_user_config(repo_path);

  // Get system hostname and username
  let hostname = std::env::var("HOSTNAME")
    .or_else(|_| std::env::var("COMPUTERNAME")) // Windows fallback
    .unwrap_or_else(|_| "treq".to_string());
  let username = std::env::var("USER")
    .or_else(|_| std::env::var("USERNAME")) // Windows fallback
    .unwrap_or_else(|_| "treq".to_string());

  // Build configuration with required fields
  let config_text = format!(
    r#"
[user]
name = "{}"
email = "{}"

[operation]
hostname = "{}"
username = "{}"
"#,
    user_name, user_email, hostname, username
  );

  // Create StackedConfig with defaults and our layer
  let mut config = StackedConfig::with_defaults();
  let layer = ConfigLayer::parse(ConfigSource::User, &config_text)
    .map_err(|e| JjError::ConfigError(e.to_string()))?;
  config.add_layer(layer);

  UserSettings::from_config(config).map_err(|e| JjError::ConfigError(e.to_string()))
}

/// Abandon a specific commit by change-id.
/// Runs: jj abandon <change_id>
// home-head-test: abandon_tip, undo_commit
pub fn jj_abandon(workspace_path: &str, change_id: &str) -> Result<String, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, change_id)?;
  reject_root_commit(&loaded, &commit)?;
  let mut tx = loaded.repo.start_transaction();
  tx.repo_mut().record_abandoned_commit(&commit);
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::InitFailed(format!("Failed to rebase descendants: {}", e)))?;
  let new_repo = block_on(tx.commit("abandon commit"))
    .map_err(|e| JjError::InitFailed(format!("Failed to abandon commit: {}", e)))?;
  let op_hex = new_repo.op_id().hex();

  // rebase_descendants() may have rewritten WC commits of every workspace; reconcile all.
  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  reconcile_all_workspaces_after_rewrite(&repo_path, None)
    .warn_on_err("reconcile all workspaces after rewrite");

  Ok(op_hex)
}

/// Restore the repository view to the parent of `operation_id`.
///
/// Fails if `operation_id` is not the current operation, so a toast Undo cannot
/// clobber later work.
// home-head-test: undo_operation
pub fn jj_undo_operation(workspace_path: &str, operation_id: &str) -> Result<String, JjError> {
  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let requested = OperationId::try_from_hex(operation_id)
    .ok_or_else(|| JjError::IoError("Invalid operation id".to_string()))?;
  let operation = undo_target_operation(&loaded.repo, &requested)?;
  let parents = block_on(operation.parents())
    .map_err(|e| JjError::IoError(format!("Failed to load operation parents: {}", e)))?;
  let Ok([parent]) = <[_; 1]>::try_from(parents) else {
    return Err(JjError::IoError(
      "Cannot undo a merge operation".to_string(),
    ));
  };
  let parent_repo = block_on(loaded.repo.loader().load_at(&parent))
    .map_err(|e| JjError::IoError(format!("Failed to load parent operation: {}", e)))?;
  let mut restored_view = parent_repo.view().store_view().clone();
  // Keep the current record of Git's refs, as `jj op restore` does. With the
  // old record, the export below would see nothing to change and leave Git's
  // branch refs where the undone operation put them; the next import would
  // then undo this restore.
  let current = loaded.repo.view().store_view();
  restored_view.git_refs = current.git_refs.clone();
  restored_view.git_head = current.git_head.clone();

  let mut tx = loaded.repo.start_transaction();
  tx.repo_mut().set_view(restored_view);
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");
  let new_repo = block_on(tx.commit(format!("undo: restore to operation {}", parent.id().hex())))
    .map_err(|e| JjError::IoError(format!("Failed to commit undo: {}", e)))?;
  update_workspace_after_history_edit(&mut loaded, &new_repo, None, CheckoutMode::Immediate)?;
  Ok(new_repo.op_id().hex())
}

/// The operation `requested` names, when nothing but Git ref exports (which
/// [`sync_home_git_head_to_wc_parent`] records after a home rewrite) follow it.
fn undo_target_operation(
  repo: &Arc<ReadonlyRepo>,
  requested: &OperationId,
) -> Result<Operation, JjError> {
  let changed =
    || JjError::IoError("Cannot undo: the repository has changed since this action".to_string());
  let mut operation = repo.operation().clone();
  while operation.id() != requested {
    if operation.metadata().description != GIT_REFS_EXPORT_OPERATION {
      return Err(changed());
    }
    let parents = block_on(operation.parents())
      .map_err(|e| JjError::IoError(format!("Failed to load operation parents: {}", e)))?;
    let [parent] = <[Operation; 1]>::try_from(parents).map_err(|_| changed())?;
    operation = parent;
  }
  Ok(operation)
}

/// Undo the latest commit in a workspace's own lineage.
///
/// Only allowed when `change_id` resolves to the workspace's current tip commit
/// (`@-`) — i.e. not the working-copy commit itself, and not a commit inherited
/// from the target branch. Callers must undo commits sequentially, from the tip
/// backwards: once the tip is undone, its parent becomes the new tip and can then
/// be undone in turn.
///
/// The commit is removed from history and its changes are folded back into the
/// working copy, exactly reversing the act of committing. Returns the operation
/// id.
// home-head-test: undo_commit
pub fn jj_undo_commit(
  workspace_path: &str,
  target_branch: &str,
  change_id: &str,
) -> Result<String, JjError> {
  validate_branch_name(target_branch, "target")?;
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, change_id)?;
  let tip = resolve_commit_by_revision(&loaded, "@-")?;
  if commit.id() != tip.id() {
    return Err(JjError::IoError(
            "Only the latest commit in the workspace can be undone. Undo commits sequentially, starting from the most recent.".to_string(),
        ));
  }

  let target_symbol = resolve_target_branch_symbol(&loaded, workspace_path, target_branch)?;
  let target_commit = resolve_commit_by_revision(&loaded, &target_symbol)?;
  let tip_is_on_target = loaded
    .repo
    .index()
    .is_ancestor(tip.id(), target_commit.id())
    .map_err(|e| JjError::IoError(format!("Failed ancestry check: {}", e)))?;
  if tip_is_on_target {
    return Err(JjError::IoError(
      "Cannot undo a commit that belongs to the target branch".to_string(),
    ));
  }

  // The working copy keeps its tree and moves onto the tip's parents, so the
  // tip's changes become uncommitted (`jj abandon --restore-descendants`).
  let wc = resolve_commit_by_revision(&loaded, "@")?;
  let mut tx = loaded.repo.start_transaction();
  block_on(
    tx.repo_mut()
      .rewrite_commit(&wc)
      .set_parents(tip.parent_ids().to_vec())
      .write(),
  )
  .map_err(|e| JjError::InitFailed(format!("Failed to move the working copy: {}", e)))?;
  tx.repo_mut().record_abandoned_commit(&tip);
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::InitFailed(format!("Failed to rebase descendants: {}", e)))?;
  let new_repo = block_on(tx.commit("undo commit"))
    .map_err(|e| JjError::InitFailed(format!("Failed to undo commit: {}", e)))?;
  let op_hex = new_repo.op_id().hex();

  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  reconcile_all_workspaces_after_rewrite(&repo_path, None)
    .warn_on_err("reconcile all workspaces after rewrite");

  Ok(op_hex)
}

/// Create a new commit that reverses the changes of `change_id`, applied on top of
/// the workspace's current tip commit. Can target any real commit reachable from
/// the workspace (including immutable or target-branch commits) except the
/// working-copy commit itself.
// home-head-test: revert_commit
pub fn jj_revert_commit(workspace_path: &str, change_id: &str) -> Result<String, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, change_id)?;

  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let wc_commit_id = loaded
    .repo
    .view()
    .get_wc_commit_id(&workspace_name)
    .cloned()
    .ok_or_else(|| JjError::IoError("Workspace has no working-copy commit".to_string()))?;
  if commit.id() == &wc_commit_id {
    return Err(JjError::IoError(
      "Cannot revert the working copy".to_string(),
    ));
  }

  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {}", e)))?;
  let wc_parents = block_on(wc_commit.parents())
    .map_err(|e| JjError::InitFailed(format!("Failed to load working-copy parents: {}", e)))?;
  let head_commit = wc_parents
    .into_iter()
    .next()
    .ok_or_else(|| JjError::IoError("Workspace working copy has no parent commit".to_string()))?;

  let parents = block_on(commit.parents())
    .map_err(|e| JjError::InitFailed(format!("Failed to load commit parents: {}", e)))?;
  let parent_tree = block_on(merge_commit_trees(loaded.repo.as_ref(), &parents))
    .map_err(|e| JjError::InitFailed(format!("Failed to merge parent trees: {}", e)))?;

  // Diff from the commit's own tree back to its parent tree is exactly the
  // reverse of the commit; applying it onto the current tip's tree (3-way
  // merge, using the commit's own tree as the merge base) is a backout.
  let reverted_diff = Diff::new(
    (commit.tree(), commit.conflict_label()),
    (
      parent_tree,
      format!("{} (before revert)", commit.conflict_label()),
    ),
  );
  let new_tree = block_on(MergedTree::merge(Merge::from_diffs(
    (head_commit.tree(), head_commit.conflict_label()),
    vec![reverted_diff],
  )))
  .map_err(|e| JjError::IoError(format!("Failed to compute reverted tree: {}", e)))?;

  let original_first_line = commit
    .description()
    .lines()
    .next()
    .unwrap_or("")
    .to_string();
  let description = format!(
    "Revert \"{}\"\n\nThis reverts commit {}.",
    original_first_line,
    commit.id().hex()
  );

  let mut tx = loaded.repo.start_transaction();

  let new_commit = block_on(
    tx.repo_mut()
      .new_commit(vec![head_commit.id().clone()], new_tree)
      .set_description(description)
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to create revert commit: {}", e)))?;

  let rebased_wc = block_on(rewrite::rebase_commit(
    tx.repo_mut(),
    wc_commit,
    vec![new_commit.id().clone()],
  ))
  .map_err(|e| JjError::IoError(format!("Failed to rebase working copy: {}", e)))?;
  block_on(tx.repo_mut().edit(workspace_name, &rebased_wc))
    .map_err(|e| JjError::IoError(format!("Failed to update working-copy pointer: {}", e)))?;

  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::InitFailed(format!("Failed to rebase descendants: {}", e)))?;
  block_on(tx.commit("revert commit"))
    .map_err(|e| JjError::InitFailed(format!("Failed to revert commit: {}", e)))?;

  // rebase_descendants() may have rewritten WC commits of every workspace; reconcile all.
  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  reconcile_all_workspaces_after_rewrite(&repo_path, None)
    .warn_on_err("reconcile all workspaces after rewrite");

  Ok(String::new())
}

/// Get the full (multi-line) description of a specific commit.
pub fn jj_get_commit_description(workspace_path: &str, change_id: &str) -> Result<String, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, change_id)?;
  Ok(commit.description().to_string())
}

/// Set (rewrite) the description of a specific commit by change-id.
/// Runs: jj describe <change_id> -m <message>
// home-head-test: describe_tip, describe_ancestor, sibling_workspace_rewrites_home_ancestor, sibling_rewrite_keeps_unread_external_checkout
pub fn jj_describe(
  workspace_path: &str,
  change_id: &str,
  message: &str,
) -> Result<String, JjError> {
  validate_commit_message(message)?;
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, change_id)?;
  reject_root_commit(&loaded, &commit)?;
  let mut tx = loaded.repo.start_transaction();

  let mut builder = tx.repo_mut().rewrite_commit(&commit).detach();
  builder.set_description(message);
  block_on(builder.write(tx.repo_mut()))
    .map_err(|e| JjError::IoError(format!("Failed to write commit: {}", e)))?;
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::InitFailed(format!("Failed to rebase descendants: {}", e)))?;
  block_on(tx.commit("describe commit"))
    .map_err(|e| JjError::InitFailed(format!("Failed to describe commit: {}", e)))?;

  // rebase_descendants() may have rewritten WC commits of every workspace; reconcile all.
  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  reconcile_all_workspaces_after_rewrite(&repo_path, None)
    .warn_on_err("reconcile all workspaces after rewrite");

  Ok(String::new())
}

fn reject_root_commit(loaded: &LoadedWorkspaceRepo, commit: &Commit) -> Result<(), JjError> {
  if commit.id() == loaded.repo.store().root_commit_id() {
    return Err(JjError::IoError(
      "Cannot rewrite the root commit".to_string(),
    ));
  }
  Ok(())
}

/// Checks that `revision` names one commit this workspace may rewrite: not the
/// root commit, an ancestor of (or) the workspace's working copy, and not on
/// `default_branch` or `parent_branch` (the bookmark a stacked workspace
/// targets; its commits belong to the parent workspace). Revisions arrive as
/// revset strings from the CLI, so without this `main` or another workspace's
/// commit could be rewritten. A branch that no longer resolves (e.g. a deleted
/// parent bookmark) protects nothing; the `::@` check still applies.
pub fn ensure_commit_rewritable(
  workspace_path: &str,
  revision: &str,
  default_branch: Option<&str>,
  parent_branch: Option<&str>,
) -> Result<(), JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, revision)?;
  reject_root_commit(&loaded, &commit)?;

  let lineage = evaluate_revset(&loaded, "::@")?;
  if !lineage.containing_fn()(commit.id()).unwrap_or(false) {
    return Err(JjError::IoError(format!(
      "Revision '{revision}' is not in this workspace's history"
    )));
  }

  let protected = [
    (default_branch, "is on the default branch"),
    (parent_branch, "belongs to parent branch"),
  ];
  for (branch, reason) in protected {
    let Some(branch) = branch else { continue };
    let Ok(symbol) = resolve_target_branch_symbol(&loaded, workspace_path, branch) else {
      continue;
    };
    let protected = evaluate_revset(&loaded, &format!("::{}", format_revset_symbol(&symbol)))?;
    if protected.containing_fn()(commit.id()).unwrap_or(false) {
      return Err(JjError::IoError(format!(
        "Revision '{revision}' {reason} '{branch}' and cannot be rewritten from this workspace"
      )));
    }
  }
  Ok(())
}

/// Rewrite author timestamps for a mutable commit and descendants on the
/// current workspace first-parent lineage toward `@`.
pub fn jj_shift_commit_timestamps(
  workspace_path: &str,
  change_id: &str,
  target_branch: &str,
  shift: &crate::commit_timestamps::TimestampShift,
) -> Result<(), JjError> {
  if !Path::new(workspace_path).exists() {
    return Ok(());
  }
  if change_id.starts_with('-') || change_id.contains('\0') || change_id.is_empty() {
    return Err(JjError::IoError("Invalid change id".to_string()));
  }
  validate_branch_name(target_branch, "target")?;

  let loaded = load_workspace_repo(workspace_path)?;
  let target = resolve_commit_by_revision(&loaded, change_id)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let Some(wc_commit_id) = loaded
    .repo
    .view()
    .get_wc_commit_id(&workspace_name)
    .cloned()
  else {
    return Err(JjError::IoError(
      "Workspace has no working-copy commit".to_string(),
    ));
  };
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {}", e)))?;

  let lineage = first_parent_lineage_from_target_to_wc(&target, &wc_commit)?;
  let immutable_ref = resolve_target_branch_symbol(&loaded, workspace_path, target_branch)
    .unwrap_or_else(|_| target_branch.to_string());
  let immutable_revset = evaluate_revset(
    &loaded,
    &format!("::{}", format_revset_symbol(&immutable_ref)),
  )?;
  let is_immutable = immutable_revset.containing_fn();

  if is_immutable(target.id()).unwrap_or(false) {
    return Err(JjError::IoError(
      "Cannot edit timestamps of an immutable commit".to_string(),
    ));
  }

  let mut mutable_lineage = Vec::new();
  for commit in &lineage {
    if is_immutable(commit.id()).unwrap_or(false) {
      break;
    }
    mutable_lineage.push(commit.clone());
  }
  if mutable_lineage.is_empty() {
    return Err(JjError::IoError(
      "No mutable commits found on this lineage".to_string(),
    ));
  }

  let original_millis: Vec<i64> = mutable_lineage
    .iter()
    .map(|commit| commit.author().timestamp.timestamp.0)
    .collect();
  let parent_millis = {
    let parents = block_on(target.parents())
      .map_err(|e| JjError::IoError(format!("Failed to load commit parents: {}", e)))?;
    parents
      .first()
      .map(|parent| parent.author().timestamp.timestamp.0)
  };
  let new_target = crate::commit_timestamps::apply_shift_to_millis(
    original_millis[0],
    target.author().timestamp.tz_offset,
    shift,
  )
  .map_err(JjError::IoError)?;
  if let Some(parent) = parent_millis {
    if new_target <= parent {
      return Err(JjError::IoError(
        "New timestamp would be at or before the parent commit. Choose a later time.".to_string(),
      ));
    }
  }
  let planned =
    crate::commit_timestamps::plan_lineage_timestamps(&original_millis, new_target, parent_millis);

  if let Some(next_immutable) = lineage.get(mutable_lineage.len()) {
    let next_millis = next_immutable.author().timestamp.timestamp.0;
    if let Some(last_planned) = planned.last() {
      if *last_planned >= next_millis {
        return Err(JjError::IoError(
          "Shifting this commit would place it after an immutable descendant".to_string(),
        ));
      }
    }
  }

  drop(is_immutable);
  drop(immutable_revset);

  apply_lineage_timestamp_plan(
    loaded,
    workspace_path,
    &mutable_lineage,
    &planned,
    "shift commit timestamps",
  )
}

/// Uniformly shift mutable first-parent commits so the newest real commit is now.
pub fn jj_shift_mutable_lineage_to_now(
  workspace_path: &str,
  target_branch: &str,
) -> Result<(), JjError> {
  if !Path::new(workspace_path).exists() {
    return Ok(());
  }
  validate_branch_name(target_branch, "target")?;

  let loaded = load_workspace_repo(workspace_path)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let Some(wc_commit_id) = loaded
    .repo
    .view()
    .get_wc_commit_id(&workspace_name)
    .cloned()
  else {
    return Err(JjError::IoError(
      "Workspace has no working-copy commit".to_string(),
    ));
  };
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {}", e)))?;

  let immutable_ref = resolve_target_branch_symbol(&loaded, workspace_path, target_branch)
    .unwrap_or_else(|_| target_branch.to_string());
  let immutable_revset = evaluate_revset(
    &loaded,
    &format!("::{}", format_revset_symbol(&immutable_ref)),
  )?;
  let is_immutable = immutable_revset.containing_fn();

  let mut newest_first = Vec::new();
  let mut current = wc_commit;
  loop {
    if is_immutable(current.id()).unwrap_or(false) {
      break;
    }
    newest_first.push(current.clone());
    let parents = block_on(current.parents())
      .map_err(|e| JjError::IoError(format!("Failed to load commit parents: {}", e)))?;
    let Some(parent) = parents.first() else {
      break;
    };
    current = parent.clone();
    if newest_first.len() > 10_000 {
      return Err(JjError::IoError(
        "Mutable lineage is unexpectedly long".to_string(),
      ));
    }
  }
  if newest_first.is_empty() {
    return Err(JjError::IoError(
      "No mutable commits found on this workspace".to_string(),
    ));
  }
  newest_first.reverse();
  let mutable_lineage = newest_first;

  let original_millis: Vec<i64> = mutable_lineage
    .iter()
    .map(|commit| commit.author().timestamp.timestamp.0)
    .collect();
  let empty_flags: Vec<bool> = mutable_lineage
    .iter()
    .map(|commit| is_empty_commit(&loaded.repo, commit))
    .collect();
  let Some(tip_index) = crate::commit_timestamps::newest_real_commit_index(&empty_flags) else {
    return Err(JjError::IoError(
      "No mutable commits found on this workspace".to_string(),
    ));
  };

  let parent_millis = {
    let parents = block_on(mutable_lineage[0].parents())
      .map_err(|e| JjError::IoError(format!("Failed to load commit parents: {}", e)))?;
    parents
      .first()
      .map(|parent| parent.author().timestamp.timestamp.0)
  };

  let now = chrono::Utc::now().timestamp_millis();
  let mut delta = now.saturating_sub(original_millis[tip_index]);
  if let Some(parent) = parent_millis {
    let min_oldest = parent.saturating_add(crate::commit_timestamps::MIN_TIMESTAMP_GAP_MS);
    let oldest_after = original_millis[0].saturating_add(delta);
    if oldest_after < min_oldest {
      delta = min_oldest.saturating_sub(original_millis[0]);
    }
  }
  if delta.abs() < crate::commit_timestamps::MIN_TIMESTAMP_GAP_MS {
    return Ok(());
  }

  let planned = crate::commit_timestamps::plan_uniform_lineage_shift(&original_millis, delta);

  drop(is_immutable);
  drop(immutable_revset);

  apply_lineage_timestamp_plan(
    loaded,
    workspace_path,
    &mutable_lineage,
    &planned,
    "shift commits to now",
  )
}

// home-head-test: shift_lineage_timestamps
fn apply_lineage_timestamp_plan(
  loaded: LoadedWorkspaceRepo,
  workspace_path: &str,
  mutable_lineage: &[Commit],
  planned: &[i64],
  op_description: &str,
) -> Result<(), JjError> {
  let mut tx = loaded.repo.start_transaction();
  for (commit, new_millis) in mutable_lineage.iter().zip(planned.iter()).rev() {
    if *new_millis == commit.author().timestamp.timestamp.0
      && *new_millis == commit.committer().timestamp.timestamp.0
    {
      continue;
    }
    let mut author = commit.author().clone();
    author.timestamp.timestamp = jj_lib::backend::MillisSinceEpoch(*new_millis);
    let mut committer = commit.committer().clone();
    committer.timestamp.timestamp = jj_lib::backend::MillisSinceEpoch(*new_millis);
    block_on(
      tx.repo_mut()
        .rewrite_commit(commit)
        .set_author(author)
        .set_committer(committer)
        .write(),
    )
    .map_err(|e| JjError::IoError(format!("Failed to write commit: {}", e)))?;
  }
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::InitFailed(format!("Failed to rebase descendants: {}", e)))?;
  block_on(tx.commit(op_description))
    .map_err(|e| JjError::InitFailed(format!("Failed to shift commit timestamps: {}", e)))?;

  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  reconcile_all_workspaces_after_rewrite(&repo_path, None)
    .warn_on_err("reconcile all workspaces after rewrite");
  Ok(())
}

fn first_parent_lineage_from_target_to_wc(
  target: &Commit,
  wc_commit: &Commit,
) -> Result<Vec<Commit>, JjError> {
  let mut chain = Vec::new();
  let mut current = wc_commit.clone();
  loop {
    chain.push(current.clone());
    if current.id() == target.id() {
      chain.reverse();
      return Ok(chain);
    }
    let parents = block_on(current.parents())
      .map_err(|e| JjError::IoError(format!("Failed to load commit parents: {}", e)))?;
    let Some(parent) = parents.first() else {
      return Err(JjError::IoError(
        "Commit is not on the current workspace lineage".to_string(),
      ));
    };
    current = parent.clone();
    if chain.len() > 10_000 {
      return Err(JjError::IoError(
        "Commit is not on the current workspace lineage".to_string(),
      ));
    }
  }
}

/// Get the list of files changed in a specific commit.
/// Runs: jj diff --summary -r <change_id>
/// Returns file paths (added/modified/removed) from the commit.
pub fn jj_diff_summary(workspace_path: &str, change_id: &str) -> Result<Vec<String>, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, change_id)?;
  let parents = block_on(commit.parents())
    .map_err(|e| JjError::InitFailed(format!("Failed to load commit parents: {}", e)))?;
  let parent_tree = if let Some(p) = parents.first() {
    p.tree()
  } else {
    loaded.repo.store().root_commit().tree()
  };
  let tree = commit.tree();
  let matcher = repo_root_matcher();
  let mut diff_stream = parent_tree.diff_stream(&tree, &matcher);
  let mut files = Vec::new();
  while let Some(entry) = block_on(diff_stream.next()) {
    files.push(entry.path.as_internal_file_string().to_string());
  }
  Ok(files)
}

/// Core staleness reconciliation for a single workspace path.
///
/// Loads the workspace at `workspace_path` fresh from disk (so it always sees the latest
/// committed operation), checks whether its on-disk tree matches its recorded WC commit,
/// and checks out the WC commit if not fresh.  This is the single authoritative
/// recovery path — both `update_stale_workspace` and `reconcile_all_workspaces_after_rewrite`
/// delegate here.
fn reconcile_single_workspace(workspace_path: &str) -> Result<(), JjError> {
  let path = Path::new(workspace_path);
  if !path.join(".jj").exists() {
    return Ok(());
  }

  let repo_path_opt = derive_repo_path_from_workspace(workspace_path);
  let settings_path = repo_path_opt.as_deref().unwrap_or(workspace_path);
  let settings = create_user_settings(settings_path)?;

  let mut workspace = Workspace::load(
    &settings,
    path,
    &StoreFactories::default(),
    &default_working_copy_factories(),
  )
  .map_err(|e| JjError::InitFailed(format!("Failed to load workspace: {}", e)))?;

  let repo = block_on(workspace.repo_loader().load_at_head())
    .map_err(|e| JjError::InitFailed(format!("Failed to load repo: {}", e)))?;

  reconcile_loaded_workspace(&mut workspace, &repo)
}

/// Checks out `repo`'s WC commit if the working copy is behind it.
///
/// `repo` is loaded before the working-copy lock is taken, so a concurrent
/// command (e.g. a snapshot) can move the working copy to a newer operation in
/// between. jj reports that as `WorkingCopyFreshness::Updated`: the working
/// copy is current and `repo` is the stale side. Checking out `repo`'s older WC
/// commit there would revert the disk to the older tree and delete files the
/// snapshot had just recorded, so that case keeps the working copy as it is.
fn reconcile_loaded_workspace(
  workspace: &mut Workspace,
  repo: &Arc<ReadonlyRepo>,
) -> Result<(), JjError> {
  let workspace_name = workspace.workspace_name().to_owned();

  let wc_commit_id = match repo.view().get_wc_commit_id(&workspace_name) {
    Some(id) => id.clone(),
    None => return Ok(()),
  };
  let wc_commit = repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::InitFailed(format!("Failed to get wc commit: {}", e)))?;

  let mut locked_ws = workspace
    .start_working_copy_mutation()
    .map_err(|e| JjError::InitFailed(format!("Failed to lock working copy: {}", e)))?;

  let freshness = block_on(WorkingCopyFreshness::check_stale(
    locked_ws.locked_wc(),
    &wc_commit,
    repo,
  ))
  .map_err(|e| JjError::InitFailed(format!("Failed to check staleness: {}", e)))?;

  match freshness {
    // Already at `repo`'s operation, or behind it with the same tree: the
    // files are right. Finishing would only record the newer operation, an
    // fsynced write for every workspace after every rewrite, so leave the
    // state as it is (dropping the lock without `finish`); jj compares trees
    // again next time.
    WorkingCopyFreshness::Fresh => {}
    WorkingCopyFreshness::Updated(_) => {
      // `repo` is the stale side here; keep the working copy's newer operation.
      let wc_op_id = locked_ws.locked_wc().old_operation_id().clone();
      block_on(locked_ws.finish(wc_op_id))
        .map_err(|e| JjError::InitFailed(format!("Failed to finish wc: {}", e)))?;
    }
    WorkingCopyFreshness::WorkingCopyStale | WorkingCopyFreshness::SiblingOperation => {
      block_on(locked_ws.locked_wc().check_out(&wc_commit))
        .map_err(|e| JjError::InitFailed(format!("Failed to check out: {}", e)))?;
      block_on(locked_ws.finish(repo.op_id().clone()))
        .map_err(|e| JjError::InitFailed(format!("Failed to finish wc: {}", e)))?;
    }
  }

  Ok(())
}

/// After any history-rewriting transaction, reconcile **all** workspace working copies
/// except the one already checked out by the caller (identified by `skip_workspace_name`).
///
/// `rebase_descendants()` rewrites the WC commit of every workspace that descends from the
/// rewritten commits, but only the *loaded* workspace is checked out in-process.  Without
/// this call, sibling/child workspaces have a mismatch between their on-disk tree and their
/// new WC-commit pointer — the next `jj` command in those workspaces snapshots the stale tree
/// and records an inverse diff of every rewritten commit.
///
/// Errors from individual workspaces are logged but do not abort the loop — one bad
/// workspace must not block reconciliation of the others.
pub fn reconcile_all_workspaces_after_rewrite(
  repo_path: &str,
  skip_workspace_name: Option<&str>,
) -> Result<(), JjError> {
  // Load the home workspace to enumerate all registered workspaces and their paths.
  let Ok((home_workspace, repo, _workspace_store)) = load_home_repo(repo_path) else {
    return Ok(());
  };
  let Ok(workspace_paths) = WorkspacePaths::load(home_workspace.repo_path()) else {
    return Ok(());
  };

  let home_ws_name = home_workspace.workspace_name().as_str().to_string();
  let home_ws_path = home_workspace
    .workspace_root()
    .to_string_lossy()
    .into_owned();

  for workspace_name in repo.view().wc_commit_ids().keys() {
    let ws_name_str = workspace_name.as_str();

    // Skip the workspace whose checkout was already handled by the caller.
    if let Some(skip) = skip_workspace_name {
      if ws_name_str == skip {
        continue;
      }
    }

    // Resolve the on-disk path for this workspace.
    let ws_path: String = if ws_name_str == home_ws_name {
      home_ws_path.clone()
    } else {
      match workspace_paths.get(workspace_name) {
        Some(stored_path) => {
          let full = if stored_path.is_absolute() {
            stored_path
          } else {
            Path::new(repo_path)
              .join(".jj")
              .join("repo")
              .join(stored_path)
          };
          // Prefer the .treq/workspaces/<name> path that treq uses.
          let treq_path = Path::new(repo_path)
            .join(".treq")
            .join("workspaces")
            .join(ws_name_str);
          if treq_path.exists() {
            treq_path.to_string_lossy().into_owned()
          } else {
            full.to_string_lossy().into_owned()
          }
        }
        None => continue, // can't resolve path; skip gracefully
      }
    };

    let result = reconcile_single_workspace(&ws_path);
    if ws_name_str == home_ws_name {
      sync_home_git_head_to_wc_parent(&ws_path, None);
    }
    if let Err(e) = result {
      // Log but continue — one bad workspace must not block the rest.
      tracing::warn!(
        "[treq] Warning: failed to reconcile workspace '{}' at '{}': {}",
        ws_name_str,
        ws_path,
        e
      );
    }
  }

  Ok(())
}

/// Update a stale workspace working copy
/// Runs: jj workspace update-stale in the workspace directory
pub fn update_stale_workspace(workspace_path: &str) -> Result<(), JjError> {
  reconcile_single_workspace(workspace_path)
}

// Stale Working Copy Detection and Recovery

/// Check if a workspace has a stale working copy
/// Returns true if the workspace is stale
pub fn is_workspace_stale(workspace_path: &str) -> Result<bool, JjError> {
  let path = Path::new(workspace_path);
  if !path.join(".jj").exists() {
    return Ok(false);
  }
  let repo_path_opt = derive_repo_path_from_workspace(workspace_path);
  let settings_path = repo_path_opt.as_deref().unwrap_or(workspace_path);
  let settings = create_user_settings(settings_path)?;
  let mut workspace = Workspace::load(
    &settings,
    path,
    &StoreFactories::default(),
    &default_working_copy_factories(),
  )
  .map_err(|e| JjError::InitFailed(format!("Failed to load workspace: {}", e)))?;
  let repo = block_on(workspace.repo_loader().load_at_head())
    .map_err(|e| JjError::InitFailed(format!("Failed to load repo: {}", e)))?;
  let workspace_name = workspace.workspace_name().to_owned();
  let wc_commit_id = match repo.view().get_wc_commit_id(&workspace_name) {
    Some(id) => id.clone(),
    None => return Ok(false),
  };
  let wc_commit = repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::InitFailed(format!("Failed to get wc commit: {}", e)))?;
  let mut locked_ws = workspace
    .start_working_copy_mutation()
    .map_err(|e| JjError::InitFailed(format!("Failed to lock working copy: {}", e)))?;
  let freshness = block_on(WorkingCopyFreshness::check_stale(
    locked_ws.locked_wc(),
    &wc_commit,
    &repo,
  ))
  .map_err(|e| JjError::InitFailed(format!("Failed to check staleness: {}", e)))?;
  // `Updated`: `repo` is stale, not the working copy (see `reconcile_loaded_workspace`).
  let finish_op_id = match freshness {
    WorkingCopyFreshness::Updated(_) => locked_ws.locked_wc().old_operation_id().clone(),
    _ => repo.op_id().clone(),
  };
  block_on(locked_ws.finish(finish_op_id))
    .map_err(|e| JjError::InitFailed(format!("Failed to finish wc: {}", e)))?;
  Ok(matches!(
    freshness,
    WorkingCopyFreshness::WorkingCopyStale | WorkingCopyFreshness::SiblingOperation
  ))
}

/// Update a stale working copy using jj workspace update-stale
pub fn jj_workspace_update_stale(workspace_path: &str) -> Result<String, JjError> {
  update_stale_workspace(workspace_path)?;
  Ok(String::new())
}

// Diff Operations using hybrid CLI approach
// Uses jj CLI for file listing (faster) and git CLI for diffs (reliable)

/// Derive repo_path from workspace_path
/// Workspace paths are: {repo_path}/.treq/workspaces/{workspace_name}
pub fn derive_repo_path_from_workspace(workspace_path: &str) -> Option<String> {
  let path = Path::new(workspace_path);

  // Look for .treq/workspaces pattern in the path
  let mut current = path;
  while let Some(parent) = current.parent() {
    if current.file_name() == Some(std::ffi::OsStr::new("workspaces")) {
      if let Some(grandparent) = parent.parent() {
        if parent.file_name() == Some(std::ffi::OsStr::new(".treq")) {
          // Found the pattern - grandparent is repo_path
          return Some(grandparent.to_string_lossy().to_string());
        }
      }
    }
    current = parent;
  }

  None
}

/// Hex id of the workspace working-copy commit, if the workspace can be loaded.
pub fn jj_working_copy_commit_hex(workspace_path: &str) -> Option<String> {
  let loaded = load_workspace_repo(workspace_path).ok()?;
  let commit = get_workspace_wc_commit(&loaded).ok().flatten()?;
  Some(commit.id().hex())
}

/// Commit with message and create new working copy using jj-lib (no subprocess)
///
/// Home-repo caveat: `jj workspace add` uses `check_out(.., root_commit())`. Later edits can leave
/// the home workspace WC under the jj root while the branch bookmark still tracks the real tip;
/// rewriting that WC can yield a root-only parent and truncate history in the log. We stack the
/// home WC on the branch tip before snapshotting when needed.
// home-head-test: commit, commit_drops_autosave_ancestors, commit_before_first_read
pub fn jj_commit(workspace_path: &str, message: &str) -> Result<String, JjError> {
  let repo_path_opt = derive_repo_path_from_workspace(workspace_path);

  // Resolve branch: workspace DB for sub-workspaces, .git/HEAD file for main repo
  let branch = if let Some(ref rp) = repo_path_opt {
    let ws_name = Path::new(workspace_path)
      .file_name()
      .and_then(|n| n.to_str())
      .ok_or_else(|| JjError::IoError("Invalid workspace path".to_string()))?;
    let ws = local_db::get_workspace_by_path(rp, ws_name)
      .map_err(|e| JjError::IoError(format!("Failed to query workspace: {}", e)))?
      .ok_or_else(|| JjError::WorkspaceNotFound(workspace_path.to_string()))?;
    ws.branch_name
  } else {
    resolve_home_repo_branch(workspace_path)?
  };
  // Import git refs first, as jj does at the start of every command in a colocated
  // repo. If jj's record of a git ref is stale (never imported, or moved by git),
  // the export after this commit is refused and the next import conflicts the bookmark.
  let mut loaded = load_workspace_repo(workspace_path)?;
  import_remaining_git_refs(&mut loaded)?;
  let LoadedWorkspaceRepo {
    settings,
    mut workspace,
    mut repo,
    ..
  } = loaded;

  let workspace_name: WorkspaceNameBuf = workspace.workspace_name().to_owned();

  if repo_path_opt.is_none() {
    let root_id = repo.store().root_commit_id();
    if let Some(wc_id) = repo.view().get_wc_commit_id(&workspace_name).cloned() {
      let wc_pre = repo
        .store()
        .get_commit(&wc_id)
        .map_err(|e| JjError::IoError(format!("Failed to load wc commit: {}", e)))?;
      let branch_bookmark_target = repo
        .view()
        .get_local_bookmark(RefName::new(&branch))
        .as_normal()
        .cloned();
      let wc_only_parent_root =
        wc_pre.parent_ids().len() == 1 && wc_pre.parent_ids().first() == Some(root_id);
      if wc_only_parent_root {
        if let Some(branch_tip_id) = branch_bookmark_target {
          if branch_tip_id != *wc_pre.id() {
            let branch_tip_commit = repo
              .store()
              .get_commit(&branch_tip_id)
              .map_err(|e| JjError::IoError(format!("Failed to load branch tip commit: {}", e)))?;
            let mut rtx = repo.start_transaction();
            block_on(
              rtx
                .repo_mut()
                .check_out(workspace_name.clone(), &branch_tip_commit),
            )
            .map_err(|e| JjError::IoError(format!("Failed to repair home wc checkout: {}", e)))?;
            block_on(rtx.repo_mut().rebase_descendants())
              .map_err(|e| JjError::IoError(format!("Failed to rebase after wc repair: {}", e)))?;
            git::export_refs(rtx.repo_mut()).warn_on_err("export refs");
            let _ = block_on(rtx.commit("repair home wc"))
              .map_err(|e| JjError::IoError(format!("Failed to commit wc repair: {}", e)))?;
            workspace = Workspace::load(
              &settings,
              Path::new(workspace_path),
              &StoreFactories::default(),
              &default_working_copy_factories(),
            )
            .map_err(|e| JjError::IoError(format!("Failed to reload workspace: {}", e)))?;
            repo = block_on(workspace.repo_loader().load_at_head())
              .map_err(|e| JjError::IoError(format!("Failed to reload repo: {}", e)))?;
          }
        }
      }
    }
  }

  // Snapshot working copy and load main-repo ignore rules to avoid expensive noise.
  let ignore_repo_root = repo_path_opt.as_deref().unwrap_or(workspace_path);
  let matcher = repo_root_matcher();
  let opts = snapshot_options_for_all_paths(ignore_repo_root, &matcher);
  let mut locked_ws = workspace
    .start_working_copy_mutation()
    .map_err(|e| JjError::IoError(format!("Failed to lock working copy: {}", e)))?;
  let (new_tree, _stats) = block_on(locked_ws.locked_wc().snapshot(&opts))
    .map_err(|e| JjError::IoError(format!("Snapshot failed: {}", e)))?;

  // Start transaction
  let mut tx = repo.start_transaction();

  let wc_commit_id = tx
    .repo()
    .view()
    .get_wc_commit_id(&workspace_name)
    .cloned()
    .ok_or_else(|| JjError::IoError("No working-copy commit found".to_string()))?;
  let wc_commit = tx
    .repo()
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load wc commit: {}", e)))?;
  let mut workspace_names: Vec<_> = tx
    .repo()
    .view()
    .workspaces_for_wc_commit_id(wc_commit.id())
    .into_iter()
    .collect();
  let bookmark_names = tx
    .repo()
    .view()
    .local_bookmarks_for_commit(wc_commit.id())
    .filter(|&(_, target)| target.as_normal() == Some(wc_commit.id()))
    .map(|(name, _)| name.to_owned())
    .collect::<Vec<_>>();

  if repo_path_opt.is_none()
    && tx
      .repo()
      .view()
      .get_local_bookmark(RefName::new(&branch))
      .as_normal()
      == Some(wc_commit.id())
  {
    // Move secondary workspaces off this wc commit before rewriting in place; sharing the wc commit with another workspace would strand or distort history.
    let others: Vec<_> = workspace_names
      .iter()
      .filter(|name| name.as_str() != workspace_name.as_str())
      .cloned()
      .collect();
    if !others.is_empty() {
      let detached_wc = block_on(
        tx.repo_mut()
          .new_commit(vec![wc_commit.id().clone()], wc_commit.tree())
          .write(),
      )
      .map_err(|e| {
        JjError::IoError(format!(
          "Failed to detach secondary workspace working copies: {}",
          e
        ))
      })?;
      for name in others {
        block_on(tx.repo_mut().edit(name, &detached_wc))
          .map_err(|e| JjError::IoError(format!("Failed to detach workspace wc pointer: {}", e)))?;
      }
    }
    workspace_names = tx
      .repo()
      .view()
      .workspaces_for_wc_commit_id(wc_commit.id())
      .into_iter()
      .collect();
  }

  // Rewrite @ with new tree + description, using detached builder to avoid borrow conflict
  let mut builder = tx.repo_mut().rewrite_commit(&wc_commit).detach();
  builder.set_tree(new_tree);
  builder.set_description(message);
  let committed = block_on(builder.write(tx.repo_mut()))
    .map_err(|e| JjError::IoError(format!("Failed to write commit: {}", e)))?;
  // Before fresh WC child / bookmark updates: `rebase_descendants` after those steps can reparent the authored commit incorrectly when another jj workspace exists (sibling wc commits off the same bookmark parent).
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;

  let ref_name = RefName::new(&branch);
  tx.repo_mut()
    .set_local_bookmark_target(ref_name, RefTarget::normal(committed.id().clone()));

  if !workspace_names.is_empty() {
    let new_wc = block_on(
      tx.repo_mut()
        .new_commit(vec![committed.id().clone()], committed.tree())
        .write(),
    )
    .map_err(|e| JjError::IoError(format!("Failed to create wc commit: {}", e)))?;

    for bookmark_name in bookmark_names {
      tx.repo_mut()
        .set_local_bookmark_target(&bookmark_name, RefTarget::normal(committed.id().clone()));
    }

    for name in workspace_names {
      block_on(tx.repo_mut().edit(name, &new_wc))
        .map_err(|e| JjError::IoError(format!("Failed to update wc pointer: {}", e)))?;
    }
  }

  // Keep colocated git refs aligned with jj refs to avoid stale/conflicted bookmark revsets.
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");

  // Commit the transaction and finalize working copy
  let new_repo = block_on(tx.commit("commit_workspace"))
    .map_err(|e| JjError::IoError(format!("Failed to commit transaction: {}", e)))?;
  block_on(locked_ws.finish(new_repo.op_id().clone()))
    .map_err(|e| JjError::IoError(format!("Failed to finish wc mutation: {}", e)))?;

  // Reconcile sibling workspaces after rebase_descendants rewrites WCs (mirrors update_workspace_after_history_edit).
  let reconcile_repo_path = repo_path_opt.as_deref().unwrap_or(workspace_path);
  reconcile_all_workspaces_after_rewrite(reconcile_repo_path, Some(workspace_name.as_str()))
    .warn_on_err("reconcile all workspaces after rewrite");

  // For home repos, keep Git HEAD symbolic to the active branch (avoid detached HEAD after jj commit).
  if repo_path_opt.is_none() {
    sync_home_git_head_to_wc_parent(
      workspace_path,
      Some(branch.as_str()).filter(|b| *b != "HEAD"),
    );
  }

  Ok(format!("Committed successfully to branch '{}'", branch))
}

/// Split selected files from working copy into a new parent commit
/// Uses: jj split -r @ -m <message> <file_paths...>
// home-head-test: split
pub fn jj_split(
  workspace_path: &str,
  message: &str,
  file_paths: Vec<String>,
) -> Result<String, JjError> {
  let repo_path = derive_repo_path_from_workspace(workspace_path);

  // Get branch name - different logic for workspaces vs main repo
  let branch = if let Some(ref rp) = repo_path {
    // For workspaces: get branch_name from the workspace record in db
    let workspace_name = Path::new(workspace_path)
      .file_name()
      .and_then(|n| n.to_str())
      .ok_or_else(|| JjError::IoError("Invalid workspace path".to_string()))?;
    let workspace = local_db::get_workspace_by_path(rp, workspace_name)
      .map_err(|e| JjError::IoError(format!("Failed to query workspace: {}", e)))?
      .ok_or_else(|| JjError::WorkspaceNotFound(workspace_path.to_string()))?;
    workspace.branch_name
  } else {
    let git_branch = get_workspace_branch(workspace_path)
      .map_err(|e| JjError::IoError(format!("Failed to determine current git branch: {}", e)))?;

    if git_branch.is_empty() || git_branch == "HEAD" {
      return Err(JjError::IoError(
        "Git is not checked out to a branch. Please checkout a branch before committing."
          .to_string(),
      ));
    }
    git_branch
  };
  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();

  let (wc_commit_id, current_tree) =
    if let Some((wc_id, tree)) = snapshot_working_copy_tree(&mut loaded, workspace_path)? {
      (wc_id, tree)
    } else {
      let wc_id = loaded
        .repo
        .view()
        .get_wc_commit_id(&workspace_name)
        .cloned()
        .ok_or_else(|| JjError::IoError("No working-copy commit found".to_string()))?;
      let wc = loaded
        .repo
        .store()
        .get_commit(&wc_id)
        .map_err(|e| JjError::IoError(format!("Failed to load wc commit: {}", e)))?;
      (wc_id, wc.tree())
    };
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load wc commit: {}", e)))?;
  let parent_tree = block_on(wc_commit.parent_tree(loaded.repo.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to load parent tree: {}", e)))?;

  let mut selected_tree_builder = MergedTreeBuilder::new(parent_tree.clone());
  let diff_matcher = repo_root_matcher();
  let diff_entries = block_on(
    parent_tree
      .diff_stream(&current_tree, &diff_matcher)
      .collect::<Vec<_>>(),
  );
  let mut matched = vec![false; file_paths.len()];
  for entry in diff_entries {
    let path_str = entry.path.as_internal_file_string().to_string();
    let mut selected = false;
    for (p, hit) in file_paths.iter().zip(matched.iter_mut()) {
      if path_str == *p
        || path_str.starts_with(&format!("{}/", p.trim_end_matches('/')))
        || p.starts_with(&format!("{}/", path_str))
      {
        *hit = true;
        selected = true;
      }
    }
    if !selected {
      continue;
    }
    let values = entry
      .values
      .map_err(|e| JjError::IoError(format!("Failed to read split diff entry: {}", e)))?;
    selected_tree_builder.set_or_remove(entry.path, values.after);
  }
  if let Some((missing, _)) = file_paths.iter().zip(&matched).find(|(_, hit)| !**hit) {
    return Err(JjError::IoError(format!(
      "invalid_arguments: '{}' has no changes in the working copy",
      missing
    )));
  }
  let selected_tree = block_on(selected_tree_builder.write_tree())
    .map_err(|e| JjError::IoError(format!("Failed to write selected split tree: {}", e)))?;

  let mut tx = loaded.repo.start_transaction();
  let mut first_builder = tx.repo_mut().rewrite_commit(&wc_commit).detach();
  first_builder.set_tree(selected_tree);
  first_builder.set_description(message);
  let first_commit = block_on(first_builder.write(tx.repo_mut()))
    .map_err(|e| JjError::IoError(format!("Failed to write selected split commit: {}", e)))?;
  // Like `jj split`: the first half keeps the change id; the remainder gets a new one.
  let mut second_builder = tx.repo_mut().rewrite_commit(&wc_commit).detach();
  second_builder.set_parents(vec![first_commit.id().clone()]);
  second_builder.set_tree(current_tree);
  second_builder.generate_new_change_id();
  let second_commit = block_on(second_builder.write(tx.repo_mut()))
    .map_err(|e| JjError::IoError(format!("Failed to write remaining split commit: {}", e)))?;

  tx.repo_mut()
    .set_rewritten_commit(wc_commit.id().clone(), second_commit.id().clone());
  block_on(tx.repo_mut().transform_descendants(
    vec![wc_commit.id().clone()],
    async |mut rewriter| {
      rewriter.replace_parent(first_commit.id(), [second_commit.id()]);
      rewriter.rebase().await?.write().await?;
      Ok(())
    },
  ))
  .map_err(|e| JjError::IoError(format!("Failed to rewrite split descendants: {}", e)))?;
  for (name, working_copy_commit) in loaded.repo.view().wc_commit_ids() {
    if working_copy_commit == wc_commit.id() {
      block_on(tx.repo_mut().edit(name.clone(), &second_commit)).map_err(|e| {
        JjError::IoError(format!(
          "Failed to move working copy to remaining commit: {}",
          e
        ))
      })?;
    }
  }
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");
  let new_repo = block_on(tx.commit("split working copy"))
    .map_err(|e| JjError::IoError(format!("Failed to commit split transaction: {}", e)))?;
  // No old tree: the snapshot already moved tree_state, so a check would hit ConcurrentCheckout.
  update_workspace_after_history_edit(&mut loaded, &new_repo, None, CheckoutMode::Immediate)?;

  // Set the bookmark to point at @- (critical - same as jj_commit)
  jj_set_bookmark(workspace_path, &branch, "@-")
    .map_err(|e| JjError::IoError(format!("Failed to advance bookmark '{}': {}", branch, e)))?;

  // Home repo only: move the git branch to the new commit and keep HEAD on it.
  if repo_path.is_none() {
    sync_home_git_head_to_wc_parent(workspace_path, Some(&branch));
  }

  Ok(format!("Committed successfully to branch '{}'", branch))
}

/// Rebase a workspace bookmark's commit lineage onto `target_branch` using jj-lib.
///
/// Unlike `jj rebase -d <target>`, this anchors the move at the workspace bookmark tip,
/// so success is not satisfied by rebasing only a detached/empty working-copy commit chain.
pub fn jj_rebase_workspace_bookmark_onto(
  workspace_path: &str,
  workspace_branch: &str,
  target_branch: &str,
) -> Result<JjRebaseResult, JjError> {
  jj_retarget_workspace_bookmark(workspace_path, workspace_branch, target_branch, None)
}

/// Like [`jj_rebase_workspace_bookmark_onto`], but commits of `old_target_branch`
/// (the branch the workspace stacks on now) stay put, so a child lifted off its
/// parent does not drag the parent's commits along.
pub fn jj_retarget_workspace_bookmark(
  workspace_path: &str,
  workspace_branch: &str,
  target_branch: &str,
  old_target_branch: Option<&str>,
) -> Result<JjRebaseResult, JjError> {
  jj_rebase_workspace_bookmark_onto_with_checkout_mode(
    workspace_path,
    workspace_branch,
    target_branch,
    old_target_branch,
    CheckoutMode::Immediate,
  )
}

pub fn jj_rebase_workspace_bookmark_onto_deferred_checkout(
  workspace_path: &str,
  workspace_branch: &str,
  target_branch: &str,
) -> Result<JjRebaseResult, JjError> {
  jj_rebase_workspace_bookmark_onto_with_checkout_mode(
    workspace_path,
    workspace_branch,
    target_branch,
    None,
    CheckoutMode::Deferred,
  )
}

// home-head-test: workspace_bookmark_rebase, stacked_workspace_rebase_keeps_home_on_branch, stacked_workspace_rebase_moves_chained_working_copies
fn jj_rebase_workspace_bookmark_onto_with_checkout_mode(
  workspace_path: &str,
  workspace_branch: &str,
  target_branch: &str,
  old_target_branch: Option<&str>,
  checkout_mode: CheckoutMode,
) -> Result<JjRebaseResult, JjError> {
  validate_branch_name(workspace_branch, "workspace")?;
  validate_branch_name(target_branch, "target")?;

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let old_wc_commit = get_workspace_wc_commit(&loaded)?;
  let workspace_revision =
    resolve_local_target_branch_revision(&loaded, workspace_path, workspace_branch)?;
  let target_revision =
    resolve_local_target_branch_revision(&loaded, workspace_path, target_branch)?;
  let workspace_tip_commit = resolve_commit_by_revision(&loaded, &workspace_revision)?;
  let target_commit = resolve_commit_by_revision(&loaded, &target_revision)?;
  let mut lineage_base = target_commit.id().hex();
  if let Some(old_target) = old_target_branch.filter(|old| *old != target_branch) {
    // Without the old parent's tip the rebase would drag its commits along.
    let old_revision = resolve_local_target_branch_revision(&loaded, workspace_path, old_target)?;
    let old_commit = resolve_commit_by_revision(&loaded, &old_revision)?;
    lineage_base = format!("({lineage_base} | {})", old_commit.id().hex());
  }

  // Select the complete ancestry that belongs to this bookmark, stopping at
  // the target's ancestry. Workspace creation can put the bookmark on a new
  // empty working-copy commit above an existing remote branch tip; selecting
  // only that tip would detach the authored remote commits during the move.
  // An explicit commit set (rather than Roots) also prevents descendants on
  // sibling bookmarks from becoming part of the move target.
  let branch_only_commits = evaluate_revset(
    &loaded,
    &format!("{}..{}", lineage_base, workspace_tip_commit.id().hex()),
  )?
  .iter()
  .commits(loaded.repo.store())
  .collect::<Result<Vec<_>, _>>()
  .map_err(|e| {
    JjError::IoError(format!(
      "Failed to collect workspace bookmark lineage: {}",
      e
    ))
  })?;

  let rebase_options = RebaseOptions {
    empty: EmptyBehavior::Keep,
    rewrite_refs: RewriteRefsOptions {
      delete_abandoned_bookmarks: false,
    },
    simplify_ancestor_merge: false,
  };

  let mut commits_to_move: Vec<_> = branch_only_commits
    .iter()
    .map(|commit| commit.id().clone())
    .collect();
  // Treat this workspace's working-copy commit as part of the explicit move.
  // Leaving it for the descendant pass can replay its stale empty-commit delta
  // over a conflicted bookmark rebase and incorrectly resolve the conflict as
  // a deletion. Keeping it explicit preserves both the conflict and the
  // sibling-workspace isolation provided by `MoveCommitsTarget::Commits`.
  if let Some(wc_commit) = old_wc_commit.as_ref() {
    if !commits_to_move.iter().any(|id| id == wc_commit.id()) {
      commits_to_move.insert(0, wc_commit.id().clone());
    }
  }
  // Other working copies sitting on moved commits move with them: the home
  // repo on the branch this workspace is stacked on, and stacked workspaces
  // with no commits yet, which chain on each other's working copies. Left
  // out, the move reparents them onto the lineage's old base, which takes
  // home off its branch and splits the stack. A working copy with commits on
  // top stays, since moving it alone would split those commits off it.
  let wc_commits = revset::ResolvedRevsetExpression::commits(
    loaded
      .repo
      .view()
      .wc_commit_ids()
      .values()
      .cloned()
      .collect(),
  );
  let mut moved: HashSet<_> = commits_to_move.iter().cloned().collect();
  let mut pending = wc_commits
    .minus(&wc_commits.children().minus(&wc_commits).ancestors())
    .evaluate(loaded.repo.as_ref())
    .map_err(|e| JjError::IoError(format!("Failed to find working copies to move: {}", e)))?
    .iter()
    .commits(loaded.repo.store())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {}", e)))?;
  pending.retain(|wc_commit| !moved.contains(wc_commit.id()));
  // Repeat until nothing joins: a working copy can sit on another one that
  // only joins in this loop. Each joins after its parents, so inserting it at
  // the front keeps the list children-first, the order `compute_move_commits`
  // expects.
  while let Some(index) = pending
    .iter()
    .position(|wc_commit| wc_commit.parent_ids().iter().all(|id| moved.contains(id)))
  {
    let wc_commit = pending.swap_remove(index);
    moved.insert(wc_commit.id().clone());
    commits_to_move.insert(0, wc_commit.id().clone());
  }

  let mut tx = loaded.repo.start_transaction();
  let location = MoveCommitsLocation {
    new_parent_ids: vec![target_commit.id().clone()],
    new_child_ids: Vec::new(),
    target: MoveCommitsTarget::Commits(commits_to_move),
  };
  let stats = block_on(async {
    rewrite::compute_move_commits(tx.repo_mut(), &location)
      .await?
      .apply(tx.repo_mut(), &rebase_options)
      .await
  })
  .map_err(|e| {
    JjError::IoError(format!(
      "Failed to rebase workspace bookmark lineage: {}",
      e
    ))
  })?;

  block_on(tx.repo_mut().rebase_descendants()).map_err(|e| {
    JjError::IoError(format!(
      "Failed to rebase descendants after bookmark lineage rebase: {}",
      e
    ))
  })?;

  git::export_refs(tx.repo_mut()).warn_on_err("export refs");

  let new_repo = block_on(tx.commit("rebase workspace bookmark lineage"))
    .map_err(|e| JjError::IoError(format!("Failed to commit rebase transaction: {}", e)))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    old_wc_commit.as_ref(),
    checkout_mode,
  )?;

  Ok(JjRebaseResult {
    success: true,
    message: summarize_rebase_stats(&stats),
  })
}

/// Get list of conflicted files in the workspace
pub fn get_conflicted_files(
  workspace_path: &str,
  target_branch: Option<&str>,
) -> Result<Vec<String>, JjError> {
  // Validate workspace path
  if workspace_path.is_empty() {
    return Err(JjError::IoError("Workspace path is empty".to_string()));
  }

  let path = std::path::Path::new(workspace_path);
  if !path.exists() {
    return Err(JjError::IoError(format!(
      "Workspace path does not exist: {}",
      workspace_path
    )));
  }

  if !path.is_dir() {
    return Err(JjError::IoError(format!(
      "Workspace path is not a directory: {}",
      workspace_path
    )));
  }

  // 1. Proactively check and update if stale
  if let Ok(true) = is_workspace_stale(workspace_path) {
    if let Err(update_err) = jj_workspace_update_stale(workspace_path) {
      tracing::warn!(
        "Failed to update stale workspace in get_conflicted_files for {}: {}",
        workspace_path,
        update_err
      );
    }
  }

  let effective_target = match target_branch {
    Some(branch) => branch.to_string(),
    None => get_default_branch(&workspace_repo_path(workspace_path))?,
  };
  get_conflicted_files_from_branch_diff(workspace_path, &effective_target)
}

fn get_conflicted_files_from_branch_diff(
  workspace_path: &str,
  _target_branch: &str,
) -> Result<Vec<String>, JjError> {
  let mut loaded = load_workspace_repo(workspace_path)?;
  conflicted_files_in(&mut loaded, workspace_path, true)
}

/// Conflicted paths after importing Git state. `snapshot` false trusts a
/// snapshot the caller just took, unless the import moved the operation.
fn conflicted_files_in(
  loaded: &mut LoadedWorkspaceRepo,
  workspace_path: &str,
  snapshot: bool,
) -> Result<Vec<String>, JjError> {
  let loaded_op_id = loaded.repo.op_id().clone();
  import_colocated_git_state(loaded, workspace_path)?;

  // A Git import can move the working-copy commit, so the caller's snapshot
  // no longer covers it.
  let snapshotted_tree_override = if snapshot || loaded.repo.op_id() != &loaded_op_id {
    snapshot_working_copy_tree(loaded, workspace_path)?.map(|(_, tree)| tree)
  } else {
    None
  };

  let wc_commit_id = loaded
    .repo
    .view()
    .get_wc_commit_id(loaded.workspace.workspace_name())
    .cloned()
    .ok_or_else(|| JjError::IoError("No working-copy commit found".to_string()))?;
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load wc commit: {}", e)))?;

  // jj-lib MergedTree::conflicts() is the only source of truth — never scan
  // working-copy file text for markers (false positives on literal <<<<<<<).
  // Empty WC children inherit a conflicted parent tree, so committed-tip
  // conflicts still appear here.
  let mut conflicts = collect_conflict_paths_from_tree(&wc_commit.tree());
  if let Some(snapshotted_tree) = snapshotted_tree_override.as_ref() {
    conflicts.extend(collect_conflict_paths_from_tree(snapshotted_tree));
  }

  conflicts.sort();
  conflicts.dedup();
  Ok(conflicts)
}

/// List files changed by already-committed ancestors of the working-copy commit,
/// relative to `target_branch` — i.e. files in the stack that are *not* part of
/// the mutable working-copy change itself. Returns an empty list when the
/// working-copy commit has no parent (nothing has been committed yet).
pub fn jj_get_committed_files(
  workspace_path: &str,
  target_branch: Option<&str>,
) -> Result<Vec<String>, JjError> {
  let effective_target = match target_branch {
    Some(branch) => branch.to_string(),
    None => get_default_branch(&workspace_repo_path(workspace_path))?,
  };

  let mut loaded = load_workspace_repo(workspace_path)?;
  import_colocated_git_state(&mut loaded, workspace_path)?;

  let wc_commit_id = loaded
    .repo
    .view()
    .get_wc_commit_id(loaded.workspace.workspace_name())
    .cloned()
    .ok_or_else(|| JjError::IoError("No working-copy commit found".to_string()))?;
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load wc commit: {}", e)))?;

  let Some(parent_id) = wc_commit.parent_ids().first().cloned() else {
    return Ok(Vec::new());
  };
  let parent_commit = loaded
    .repo
    .store()
    .get_commit(&parent_id)
    .map_err(|e| JjError::IoError(format!("Failed to load parent commit: {}", e)))?;

  let target_sym = format_revset_symbol(&effective_target);
  let merge_base_revset = format!("latest(::{target_sym} & ::{})", parent_id.hex());
  let merge_base_commit = {
    let revset = evaluate_revset(&loaded, &merge_base_revset)
      .map_err(|e| JjError::IoError(format!("Failed to find merge base: {}", e)))?;
    match revset
      .iter()
      .commits(loaded.repo.store())
      .next()
      .transpose()
      .map_err(|e| JjError::IoError(format!("Failed to load merge base commit: {}", e)))?
    {
      Some(commit) => commit,
      // No common ancestor (e.g. target branch doesn't exist yet) — nothing
      // to report as "already committed" relative to it.
      None => return Ok(Vec::new()),
    }
  };

  let diff_matcher = repo_root_matcher();
  let committed: Vec<String> = block_on(
    merge_base_commit
      .tree()
      .diff_stream(&parent_commit.tree(), &diff_matcher)
      .collect::<Vec<_>>(),
  )
  .into_iter()
  .filter_map(|entry| {
    entry
      .values
      .ok()
      .map(|_| entry.path.as_internal_file_string().to_string())
  })
  .collect();

  Ok(committed)
}

/// Paths with unresolved conflicts inside a single tree (jj-lib MergedTree::conflicts).
fn collect_conflict_paths_from_tree(tree: &MergedTree) -> Vec<String> {
  tree
    .conflicts()
    .map(|(path, _value)| path.as_internal_file_string().to_string())
    .collect()
}

/// Collect detailed information about all revisions for a conflicted bookmark
fn collect_bookmark_conflict_info(
  repo_path: &str,
  revision: &str,
) -> Result<BookmarkConflictInfo, JjError> {
  let bookmark_name = revision.split('@').next().unwrap_or(revision).to_string();
  let loaded = load_workspace_repo(repo_path)?;
  let target = loaded
    .repo
    .view()
    .get_local_bookmark(RefName::new(&bookmark_name));
  let mut commits = Vec::new();
  if target.has_conflict() {
    for commit_id in target.added_ids() {
      let Ok(commit) = loaded.repo.store().get_commit(commit_id) else {
        continue;
      };
      let timestamp = commit
        .author()
        .timestamp
        .to_datetime()
        .map(|ts| ts.to_rfc3339())
        .unwrap_or_else(|_| commit.author().timestamp.timestamp.0.to_string());
      let (insertions, deletions) = compute_commit_stats(&loaded.repo, &commit, None);
      let diff_summary = if insertions == 0 && deletions == 0 {
        String::new()
      } else {
        format!("{insertions} insertions, {deletions} deletions")
      };
      commits.push(BookmarkConflictCommit {
        commit_id: commit.id().hex(),
        short_commit_id: short_commit_id(&commit),
        change_id: HexPrefix::from_id(commit.change_id()).reverse_hex()[..12].to_string(),
        description: commit_description_first_line(&commit),
        author_name: commit.author().name.clone(),
        timestamp,
        diff_summary,
      });
    }
    commits.sort_by(|a, b| {
      b.timestamp
        .cmp(&a.timestamp)
        .then_with(|| a.commit_id.cmp(&b.commit_id))
    });
    if commits.is_empty() {
      return Err(JjError::IoError(format!(
        "Failed to gather conflicted bookmark info for '{}'",
        bookmark_name
      )));
    }
  }

  Ok(BookmarkConflictInfo {
    bookmark: bookmark_name,
    commits,
  })
}

/// Get the current commit ID for a branch/revision
/// Uses: jj log -r <revision> --no-graph -T 'commit_id.short(12)'
/// Returns error if the bookmark is conflicted (with details about all conflicting commits)
pub fn jj_get_commit_id(repo_path: &str, revision: &str) -> Result<String, JjError> {
  WorkspaceRepo::load(repo_path)?.commit_id(revision)
}

fn commit_id_in(
  loaded: &LoadedWorkspaceRepo,
  repo_path: &str,
  revision: &str,
) -> Result<String, JjError> {
  match resolve_commit_by_revision(loaded, revision) {
    Ok(commit) => {
      let commit_id = short_commit_id(&commit);
      if commit_id.is_empty() {
        return Err(JjError::IoError(format!(
          "No commit found for revision '{}'",
          revision
        )));
      }
      Ok(commit_id)
    }
    Err(err) => {
      if !revision.starts_with("bookmarks(") {
        let bookmark_name = revision.split('@').next().unwrap_or(revision);
        let target = loaded
          .repo
          .view()
          .get_local_bookmark(RefName::new(bookmark_name));
        if target.has_conflict() {
          if let Ok(info) = collect_bookmark_conflict_info(repo_path, revision) {
            if !info.commits.is_empty() {
              return Err(JjError::BookmarkConflict(info));
            }
          }
        }
      }
      Err(JjError::IoError(format!(
        "Failed to get commit ID for '{}': {}",
        revision, err
      )))
    }
  }
}

/// Get the short (12-char) change ID for a revision.
///
/// Unlike a commit ID, a change ID keeps pointing at the same logical change after
/// it is rewritten (rebased, amended), so it is the stable way to refer back to a
/// commit across later history edits.
pub fn jj_get_change_id(workspace_path: &str, revision: &str) -> Result<String, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, revision)?;
  Ok(HexPrefix::from_id(commit.change_id()).reverse_hex()[..12].to_string())
}

/// List every file path tracked in `revision`'s tree (equivalent to
/// `jj file list -r <revision>`), as internal repo-path strings.
pub fn jj_list_files_at_revision(
  workspace_path: &str,
  revision: &str,
) -> Result<Vec<String>, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, revision)?;
  let tree = commit.tree();
  let mut files: Vec<String> = tree
    .entries()
    .filter_map(|(path, value): (RepoPathBuf, _)| {
      value
        .ok()
        .map(|_| path.as_internal_file_string().to_string())
    })
    .collect();
  files.sort();
  Ok(files)
}

/// Get commit IDs matching a revset expression.
/// Returns short (12-char) commit IDs, one per matching commit.
/// Returns empty vec if the revset matches nothing (not an error).
pub fn jj_log_revset_commit_ids(
  workspace_path: &str,
  revset: &str,
) -> Result<Vec<String>, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let normalized_revset = strip_mutable_revset_filter(revset);
  let revset_expr = match evaluate_revset(&loaded, &normalized_revset) {
    Ok(expr) => expr,
    Err(e) => {
      let msg = e.to_string();
      if msg.contains("Empty revision set") {
        return Ok(Vec::new());
      }
      return Err(JjError::IoError(format!(
        "Failed to log revset '{}': {}",
        revset, msg
      )));
    }
  };

  let ids: Vec<String> = revset_expr
    .iter()
    .commits(loaded.repo.store())
    .map(|c| c.map_err(|e| JjError::IoError(format!("Failed to log revset '{}': {}", revset, e))))
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .map(|c| short_commit_id(&c))
    .filter(|id| !id.is_empty())
    .collect();

  Ok(ids)
}

/// Get change IDs matching a revset expression, one per matching commit, in the
/// revset's own order. A change id repeated across two entries means those
/// commits diverged (jj CLI's log output marks this with a trailing `??`).
pub fn jj_log_revset_change_ids(
  workspace_path: &str,
  revset: &str,
) -> Result<Vec<String>, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let revset_expr = evaluate_revset(&loaded, revset)
    .map_err(|e| JjError::IoError(format!("Failed to log revset '{}': {}", revset, e)))?;
  let ids: Vec<String> = revset_expr
    .iter()
    .commits(loaded.repo.store())
    .map(|c| {
      c.map(|commit| HexPrefix::from_id(commit.change_id()).reverse_hex()[..12].to_string())
        .map_err(|e| JjError::IoError(format!("Failed to log revset '{}': {}", revset, e)))
    })
    .collect::<Result<Vec<_>, _>>()?;
  Ok(ids)
}

/// Get (description first line, short commit id) pairs for commits matching a
/// revset expression, in the revset's own order, capped at `limit` if given.
pub fn jj_log_descriptions(
  workspace_path: &str,
  revset: &str,
  limit: Option<usize>,
) -> Result<Vec<(String, String)>, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let revset_expr = evaluate_revset(&loaded, revset)
    .map_err(|e| JjError::IoError(format!("Failed to log revset '{}': {}", revset, e)))?;
  let entries: Vec<(String, String)> = revset_expr
    .iter()
    .commits(loaded.repo.store())
    .take(limit.unwrap_or(usize::MAX))
    .map(|c| {
      c.map(|commit| {
        let first_line = commit
          .description()
          .lines()
          .next()
          .unwrap_or("")
          .to_string();
        (first_line, short_commit_id(&commit))
      })
      .map_err(|e| JjError::IoError(format!("Failed to log revset '{}': {}", revset, e)))
    })
    .collect::<Result<Vec<_>, _>>()?;
  Ok(entries)
}

/// A commit in a jj log, richer than [`jj_log_descriptions`]'s pairs: carries the
/// full description and whether the commit is empty (its tree equals its parents'
/// merged tree) — jj CLI's default log template marks this with `(empty)`.
#[derive(Debug, Clone)]
pub struct JjLogEntry {
  pub description: String,
  pub commit_id: String,
  pub is_empty: bool,
}

/// Get full log entries (description, short commit id, emptiness) for commits
/// matching a revset expression, in the revset's own order, capped at `limit`.
pub fn jj_log_entries(
  workspace_path: &str,
  revset: &str,
  limit: Option<usize>,
) -> Result<Vec<JjLogEntry>, JjError> {
  use futures::StreamExt as _;

  let loaded = load_workspace_repo(workspace_path)?;
  let revset_expr = evaluate_revset(&loaded, revset)
    .map_err(|e| JjError::IoError(format!("Failed to log revset '{}': {}", revset, e)))?;
  let matcher = repo_root_matcher();
  let entries: Vec<JjLogEntry> = revset_expr
    .iter()
    .commits(loaded.repo.store())
    .take(limit.unwrap_or(usize::MAX))
    .map(|c| {
      c.map_err(|e| JjError::IoError(format!("Failed to log revset '{}': {}", revset, e)))
        .and_then(|commit| {
          let parent_tree = block_on(commit.parent_tree(loaded.repo.as_ref()))
            .map_err(|e| JjError::IoError(format!("Failed to read parent tree: {}", e)))?;
          let tree = commit.tree();
          let is_empty = block_on(parent_tree.diff_stream(&tree, &matcher).next()).is_none();
          Ok(JjLogEntry {
            description: commit.description().to_string(),
            commit_id: short_commit_id(&commit),
            is_empty,
          })
        })
    })
    .collect::<Result<Vec<_>, _>>()?;
  Ok(entries)
}

/// Rebase using a revset expression
/// Runs from specified directory to ensure correct commit resolution
/// Sets jj bookmark after successful rebase
// home-head-test: rebase_home_branch
pub fn jj_rebase_with_revset(
  working_dir: &str,
  revset: &str,
  target_branch: &str,
  _branch_name: &str,
  _conflict_marker_style: &str,
) -> Result<JjRebaseResult, JjError> {
  let mut loaded = load_workspace_repo_for_history_edit(working_dir)?;
  let old_wc_commit = get_workspace_wc_commit(&loaded)?;
  let commits = {
    let normalized_revset = strip_mutable_revset_filter(revset);
    let revset_expr = evaluate_revset(&loaded, &normalized_revset)?;
    revset_expr
      .iter()
      .commits(loaded.repo.store())
      .collect::<Result<Vec<_>, _>>()
      .map_err(|e| JjError::IoError(format!("Failed to resolve revset '{}': {}", revset, e)))?
  };
  if commits.is_empty() {
    // Nothing to rebase — still reconcile on-disk WC state so sibling workspaces stay consistent.
    let current_repo = Arc::clone(&loaded.repo);
    update_workspace_after_history_edit(
      &mut loaded,
      &current_repo,
      old_wc_commit.as_ref(),
      CheckoutMode::Immediate,
    )
    .warn_on_err("update workspace after history edit");
    return Ok(JjRebaseResult {
      success: true,
      message: "Nothing to rebase (empty revision set)".to_string(),
    });
  }
  let target_revision = resolve_local_target_branch_revision(&loaded, working_dir, target_branch)?;
  let target_commit = resolve_commit_by_revision(&loaded, &target_revision)?;

  let rebase_options = RebaseOptions {
    empty: EmptyBehavior::Keep,
    rewrite_refs: RewriteRefsOptions {
      delete_abandoned_bookmarks: false,
    },
    simplify_ancestor_merge: false,
  };

  let mut tx = loaded.repo.start_transaction();
  let location = MoveCommitsLocation {
    new_parent_ids: vec![target_commit.id().clone()],
    new_child_ids: Vec::new(),
    target: MoveCommitsTarget::Roots(commits.into_iter().map(|c| c.id().clone()).collect()),
  };
  let stats = block_on(async {
    rewrite::compute_move_commits(tx.repo_mut(), &location)
      .await?
      .apply(tx.repo_mut(), &rebase_options)
      .await
  })
  .map_err(|e| JjError::IoError(format!("Failed to rebase revset roots: {}", e)))?;
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");
  let new_repo = block_on(tx.commit("rebase revset roots"))
    .map_err(|e| JjError::IoError(format!("Failed to commit rebase transaction: {}", e)))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    old_wc_commit.as_ref(),
    CheckoutMode::Immediate,
  )?;

  Ok(JjRebaseResult {
    success: true,
    message: summarize_rebase_stats(&stats),
  })
}

/// Resolve a diverged bookmark without discarding local work. All local-only
/// mutable changes are captured before the ref is changed, rebased in one jj
/// transaction, and checked for reachability before that transaction is committed.
// home-head-test: resolve_bookmark_conflict_onto_target
pub fn jj_resolve_bookmark_conflict_losslessly(
  workspace_path: &str,
  bookmark_name: &str,
  target_revision: &str,
) -> Result<BookmarkConflictResolutionResult, JjError> {
  if !Path::new(workspace_path).exists() {
    return Err(JjError::IoError(
      "Workspace path does not exist".to_string(),
    ));
  }
  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let old_wc_commit = get_workspace_wc_commit(&loaded)?;
  let target = resolve_commit_by_revision(&loaded, target_revision)?;
  let local_revset = format!("({}..@-) & mutable()", target.id().hex());
  let local_commits = evaluate_revset(&loaded, &strip_mutable_revset_filter(&local_revset))?
    .iter()
    .commits(loaded.repo.store())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| JjError::IoError(format!("Failed to capture local commits: {e}")))?;
  let preserved_change_ids: Vec<String> = local_commits
    .iter()
    .map(|commit| HexPrefix::from_id(commit.change_id()).reverse_hex()[..12].to_string())
    .collect();
  let root_commits = evaluate_revset(&loaded, &format!("roots({}..@-)", target.id().hex()))?
    .iter()
    .commits(loaded.repo.store())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| JjError::IoError(format!("Failed to find local roots: {e}")))?;

  let mut tx = loaded.repo.start_transaction();
  if !local_commits.is_empty() {
    let options = RebaseOptions {
      empty: EmptyBehavior::Keep,
      rewrite_refs: RewriteRefsOptions {
        delete_abandoned_bookmarks: false,
      },
      simplify_ancestor_merge: false,
    };
    let location = MoveCommitsLocation {
      new_parent_ids: vec![target.id().clone()],
      new_child_ids: Vec::new(),
      target: MoveCommitsTarget::Roots(root_commits.iter().map(|c| c.id().clone()).collect()),
    };
    block_on(async {
      rewrite::compute_move_commits(tx.repo_mut(), &location)
        .await?
        .apply(tx.repo_mut(), &options)
        .await
    })
    .map_err(|e| JjError::IoError(format!("Failed to rebase local commits: {e}")))?;
    block_on(tx.repo_mut().rebase_descendants())
      .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {e}")))?;
  } else {
    let workspace_name = loaded.workspace.workspace_name().to_owned();
    block_on(tx.repo_mut().edit(workspace_name, &target))
      .map_err(|e| JjError::IoError(format!("Failed to move working copy to target: {e}")))?;
  }

  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let wc_id = tx
    .repo_mut()
    .view()
    .get_wc_commit_id(&workspace_name)
    .cloned()
    .ok_or_else(|| JjError::IoError("Workspace commit is missing".to_string()))?;
  let wc = tx
    .repo_mut()
    .store()
    .get_commit(&wc_id)
    .map_err(|e| JjError::IoError(format!("Failed to load rewritten working copy: {e}")))?;
  let tip_id = if local_commits.is_empty() {
    target.id().clone()
  } else {
    wc.parent_ids()
      .first()
      .cloned()
      .unwrap_or_else(|| target.id().clone())
  };

  // Verify the stable change IDs from the pre-operation snapshot are all
  // ancestors of the new bookmark tip before making the operation visible.
  let mut pending = vec![tip_id.clone()];
  let mut seen = HashSet::new();
  let mut reachable_changes = HashSet::new();
  while let Some(id) = pending.pop() {
    if !seen.insert(id.clone()) {
      continue;
    }
    let commit = tx
      .repo_mut()
      .store()
      .get_commit(&id)
      .map_err(|e| JjError::IoError(format!("Failed to verify rewritten history: {e}")))?;
    reachable_changes
      .insert(HexPrefix::from_id(commit.change_id()).reverse_hex()[..12].to_string());
    pending.extend(commit.parent_ids().iter().cloned());
  }
  if let Some(missing) = preserved_change_ids
    .iter()
    .find(|change_id| !reachable_changes.contains(*change_id))
  {
    return Err(JjError::IoError(format!(
      "Refusing bookmark resolution: captured change {missing} is not reachable"
    )));
  }

  tx.repo_mut()
    .set_local_bookmark_target(RefName::new(bookmark_name), RefTarget::normal(tip_id));
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");
  let new_repo = block_on(tx.commit("resolve bookmark conflict losslessly"))
    .map_err(|e| JjError::IoError(format!("Failed to commit bookmark resolution: {e}")))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    old_wc_commit.as_ref(),
    CheckoutMode::Immediate,
  )?;

  Ok(BookmarkConflictResolutionResult {
    success: true,
    message: format!(
      "Preserved and rebased {} local commit(s) onto {}",
      preserved_change_ids.len(),
      target_revision
    ),
    preserved_change_ids,
  })
}

/// Abandon every commit matched by `revset_expr` (by change id) in `workspace_path`.
/// Returns the list of abandoned change IDs.
fn abandon_commits_matching_revset(
  loaded: &LoadedWorkspaceRepo,
  workspace_path: &str,
  revset_expr: &str,
) -> Result<Vec<String>, JjError> {
  let revset = evaluate_revset(loaded, revset_expr).map_err(|e| {
    JjError::IoError(format!(
      "Failed to evaluate revset '{}': {}",
      revset_expr, e
    ))
  })?;
  // Never abandon a commit that is currently the live working-copy commit of *any*
  // workspace (not just this one) — e.g. a stacked workspace's target may be another
  // workspace's still-empty `@`, which must be exempt from the empty-commit policy.
  let wc_commit_ids: HashSet<_> = loaded
    .repo
    .view()
    .wc_commit_ids()
    .values()
    .cloned()
    .collect();
  let change_ids: Vec<String> = revset
    .iter()
    .commits(loaded.repo.store())
    .filter_map(|commit| {
      let commit = match commit {
        Ok(commit) => commit,
        Err(e) => return Some(Err(JjError::IoError(e.to_string()))),
      };
      if wc_commit_ids.contains(commit.id()) {
        return None;
      }
      Some(Ok(
        HexPrefix::from_id(commit.change_id()).reverse_hex()[..12].to_string(),
      ))
    })
    .collect::<Result<Vec<_>, JjError>>()?;

  for change_id in &change_ids {
    jj_abandon(workspace_path, change_id)?;
  }

  Ok(change_ids)
}

/// Abandon empty commits between target branch and working copy parent.
/// Returns list of abandoned change IDs.
pub fn jj_abandon_empty_commits(
  workspace_path: &str,
  target_branch: &str,
) -> Result<Vec<String>, JjError> {
  if target_branch.starts_with('-') || target_branch.contains('\0') || target_branch.is_empty() {
    return Err(JjError::IoError("Invalid target branch name".to_string()));
  }

  let loaded = load_workspace_repo(workspace_path)?;
  let selected_target_ref =
    resolve_local_target_branch_revision(&loaded, workspace_path, target_branch)?;
  let revset_expr = format!(
    "({}..@-) & empty()",
    format_revset_symbol(&selected_target_ref)
  );
  abandon_commits_matching_revset(&loaded, workspace_path, &revset_expr)
}

/// Collapse an undescribed merge at the workspace bookmark when its tree is
/// identical to one of its parents.
///
/// `jj abandon` preserves every parent of a merge commit. If the bookmark points
/// at that commit, abandoning it therefore leaves a conflicted bookmark, which
/// cannot be pushed. Sync-created ancestry-only merges need to move the bookmark
/// to the tree-equivalent (normally first) parent before the generic empty-commit
/// cleanup runs.
pub fn jj_collapse_empty_merge_tip(
  workspace_path: &str,
  branch_name: &str,
) -> Result<bool, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let tip = resolve_commit_by_revision(&loaded, branch_name)?;
  if !tip.description().trim().is_empty() || tip.parent_ids().len() < 2 {
    return Ok(false);
  }

  let tip_tree = tip.tree_ids();
  let equivalent_parent = block_on(tip.parents())
    .map_err(|e| JjError::IoError(format!("Failed to load merge parents: {e}")))?
    .into_iter()
    .find(|parent| parent.tree_ids() == tip_tree);
  let Some(parent) = equivalent_parent else {
    return Ok(false);
  };

  jj_set_bookmark(workspace_path, branch_name, &parent.id().hex())?;
  Ok(true)
}

/// After a manual commit, drop consecutive autosave ancestors of `@-`.
///
/// Reparents the new commit onto the last non-autosave ancestor, keeping the
/// new commit's tree, then abandons the autosave commits. Does nothing when
/// `@-` itself is still an autosave (no manual commit yet).
// home-head-test: commit_drops_autosave_ancestors
pub fn jj_drop_autosave_ancestors(workspace_path: &str) -> Result<Vec<String>, JjError> {
  if !Path::new(workspace_path).exists() {
    return Ok(Vec::new());
  }

  let loaded = load_workspace_repo(workspace_path)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let Some(wc_id) = loaded
    .repo
    .view()
    .get_wc_commit_id(&workspace_name)
    .cloned()
  else {
    return Ok(Vec::new());
  };
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_id)
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {e}")))?;
  let wc_parents = block_on(wc_commit.parents())
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy parents: {e}")))?;
  let Some(tip) = wc_parents.into_iter().next() else {
    return Ok(Vec::new());
  };
  if is_autosave_description(tip.description()) {
    return Ok(Vec::new());
  }

  let mut autosaves = Vec::new();
  let mut current = tip.clone();
  loop {
    let parents = block_on(current.parents())
      .map_err(|e| JjError::IoError(format!("Failed to load commit parents: {e}")))?;
    let Ok([parent]) = <[_; 1]>::try_from(parents) else {
      break;
    };
    if !is_autosave_description(parent.description()) {
      break;
    }
    autosaves.push(parent.clone());
    current = parent;
  }
  let Some(oldest) = autosaves.last() else {
    return Ok(Vec::new());
  };

  let oldest_parents = block_on(oldest.parents())
    .map_err(|e| JjError::IoError(format!("Failed to load autosave parents: {e}")))?;
  let Ok([base]) = <[_; 1]>::try_from(oldest_parents) else {
    return Ok(Vec::new());
  };

  let dropped_ids: Vec<String> = autosaves
    .iter()
    .map(|commit| HexPrefix::from_id(commit.change_id()).reverse_hex()[..12].to_string())
    .collect();

  let mut tx = loaded.repo.start_transaction();
  let mut builder = tx.repo_mut().rewrite_commit(&tip).detach();
  builder.set_parents(vec![base.id().clone()]);
  block_on(builder.write(tx.repo_mut()))
    .map_err(|e| JjError::IoError(format!("Failed to reparent manual commit: {e}")))?;
  for autosave in &autosaves {
    tx.repo_mut().record_abandoned_commit(autosave);
  }
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase after dropping autosaves: {e}")))?;
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");
  block_on(tx.commit("drop autosave commits"))
    .map_err(|e| JjError::IoError(format!("Failed to commit autosave drop: {e}")))?;

  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  reconcile_all_workspaces_after_rewrite(&repo_path, None)
    .warn_on_err("reconcile all workspaces after rewrite");

  Ok(dropped_ids)
}

fn too_large_revision_diff() -> JjRevisionDiff {
  JjRevisionDiff {
    committed_files: Vec::new(),
    hunks_by_file: Vec::new(),
    uncommitted_files: Vec::new(),
    conflicted_files: Vec::new(),
    too_large_to_render: true,
    render_block_reason: Some(TOO_LARGE_COMMIT_DIFF_MESSAGE.to_string()),
    working_copy_token: None,
  }
}

pub fn empty_revision_diff() -> JjRevisionDiff {
  JjRevisionDiff {
    committed_files: Vec::new(),
    hunks_by_file: Vec::new(),
    uncommitted_files: Vec::new(),
    conflicted_files: Vec::new(),
    too_large_to_render: false,
    render_block_reason: None,
    working_copy_token: None,
  }
}

/// None if the workspace dir exists; empty diff if it is already gone (test teardown).
pub fn revision_diff_if_workspace_missing(
  workspace_dir: &std::path::Path,
) -> Option<JjRevisionDiff> {
  if workspace_dir.exists() {
    None
  } else {
    Some(empty_revision_diff())
  }
}

fn resolve_commit_by_revision(
  loaded: &LoadedWorkspaceRepo,
  revision: &str,
) -> Result<Commit, JjError> {
  let revset = evaluate_revset(loaded, revision)?;
  let commits = revset
    .iter()
    .commits(loaded.repo.store())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| JjError::IoError(format!("Failed to resolve revision '{}': {}", revision, e)))?;
  match commits.len() {
    1 => commits.into_iter().next().ok_or_else(|| {
      JjError::IoError(format!(
        "Revision '{}' did not resolve to a commit",
        revision
      ))
    }),
    0 => Err(JjError::IoError(format!(
      "Revision '{}' did not resolve to a commit",
      revision
    ))),
    _ => Err(JjError::IoError(format!(
      "Revision '{}' resolved to multiple commits",
      revision
    ))),
  }
}

fn validate_branch_name(branch: &str, kind: &str) -> Result<(), JjError> {
  if branch.starts_with('-') || branch.contains('\0') || branch.is_empty() {
    return Err(JjError::IoError(format!("Invalid {kind} branch name")));
  }
  Ok(())
}

fn validate_commit_message(message: &str) -> Result<(), JjError> {
  if message.contains('\0') {
    return Err(JjError::IoError("Invalid commit message".to_string()));
  }
  if message.len() > 10000 {
    return Err(JjError::IoError(
      "Commit message too long (max 10000 characters)".to_string(),
    ));
  }
  Ok(())
}

fn workspace_repo_path(workspace_path: &str) -> String {
  derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string())
}

fn resolve_target_branch_symbol(
  loaded: &LoadedWorkspaceRepo,
  workspace_path: &str,
  target_branch: &str,
) -> Result<String, JjError> {
  resolve_local_target_branch_revision(loaded, workspace_path, target_branch).map_err(|_| {
    JjError::IoError(format!(
      "Target branch '{}' could not be resolved",
      target_branch
    ))
  })
}

/// Returns whether workspace parent commit (`@-`) descends from the target branch tip.
/// If target branch cannot be resolved to a commit, returns an error.
pub fn jj_workspace_parent_descends_from_target(
  workspace_path: &str,
  workspace_branch: &str,
  target_branch: &str,
) -> Result<bool, JjError> {
  validate_branch_name(workspace_branch, "workspace")?;
  validate_branch_name(target_branch, "target")?;
  let loaded = load_workspace_repo(workspace_path)?;
  let target_symbol = resolve_target_branch_symbol(&loaded, workspace_path, target_branch)?;
  let workspace_parent = resolve_commit_by_revision(&loaded, workspace_branch)?;
  let target_commit = resolve_commit_by_revision(&loaded, &target_symbol)?;
  loaded
    .repo
    .index()
    .is_ancestor(target_commit.id(), workspace_parent.id())
    .map_err(|e| JjError::IoError(format!("Failed ancestry check: {}", e)))
}

/// Result of [`rebase_snapshot`].
pub struct RebaseSnapshot {
  operation_id: String,
  in_sync: Vec<bool>,
}

impl RebaseSnapshot {
  /// The operation the snapshot was taken at.
  pub fn operation_id(&self) -> &str {
    &self.operation_id
  }

  /// Whether the pair at `index` is certainly not behind its target.
  pub fn is_in_sync(&self, index: usize) -> bool {
    self.in_sync.get(index).copied().unwrap_or(false)
  }
}

/// Checks many `(workspace branch, target branch)` pairs against one repo
/// load. A pair is in sync only when both are local bookmarks with a single
/// target and the target tip is an ancestor of the workspace bookmark; any
/// other outcome (missing, conflicted, behind, error) is reported as not in
/// sync so the caller falls back to the per-workspace path that handles it.
pub fn rebase_snapshot(repo_path: &str, pairs: &[(&str, &str)]) -> Result<RebaseSnapshot, JjError> {
  let loaded = load_workspace_repo(repo_path)?;
  let operation_id = loaded.repo.operation().id().hex();
  let in_sync = pairs
    .iter()
    .map(|(workspace_branch, target_branch)| {
      if validate_branch_name(workspace_branch, "workspace").is_err()
        || validate_branch_name(target_branch, "target").is_err()
      {
        return false;
      }
      let view = loaded.repo.view();
      let (Some(workspace_tip), Some(target_tip)) = (
        view
          .get_local_bookmark(RefName::new(workspace_branch))
          .as_normal(),
        view
          .get_local_bookmark(RefName::new(target_branch))
          .as_normal(),
      ) else {
        return false;
      };
      loaded
        .repo
        .index()
        .is_ancestor(target_tip, workspace_tip)
        .unwrap_or(false)
    })
    .collect();
  Ok(RebaseSnapshot {
    operation_id,
    in_sync,
  })
}

/// Imports git branch refs into jj once, so a following [`rebase_snapshot`]
/// sees branches moved by git outside jj. Writes an operation only when
/// something changed. Git HEAD is deliberately not imported: recording it in
/// the view without moving the home working copy (as
/// `reconcile_colocated_home_repo` does) would hide a git-only commit from the
/// next home read.
pub fn import_git_refs(repo_path: &str) -> Result<(), JjError> {
  let mut loaded = load_workspace_repo(repo_path)?;
  import_remaining_git_refs(&mut loaded)
}

/// The id of the repo's current operation (op head).
pub fn current_operation_id(repo_path: &str) -> Result<String, JjError> {
  Ok(load_workspace_repo(repo_path)?.repo.operation().id().hex())
}

fn resolve_merge_target_parent(
  loaded: &LoadedWorkspaceRepo,
  workspace_path: &str,
  target_branch: &str,
) -> Result<Commit, JjError> {
  let target_symbol = resolve_target_branch_symbol(loaded, workspace_path, target_branch)?;
  if let Ok(commit) = resolve_commit_by_revision(loaded, &target_symbol) {
    return Ok(commit);
  }

  // Keep compatibility with historical `<target>+` behavior, but do not require it.
  let target_plus_symbol = format!("{}+", format_revset_symbol(&target_symbol));
  if let Ok(commit) = resolve_commit_by_revision(loaded, &target_plus_symbol) {
    return Ok(commit);
  }

  Err(JjError::IoError(format!(
    "Target branch '{}' could not be resolved (tried '{}' and '{}')",
    target_branch, target_symbol, target_plus_symbol
  )))
}

fn load_workspace_repo_for_history_edit(
  workspace_path: &str,
) -> Result<LoadedWorkspaceRepo, JjError> {
  let mut loaded = load_workspace_repo(workspace_path)?;
  import_colocated_git_state(&mut loaded, workspace_path)?;
  Ok(loaded)
}

pub fn workspace_has_unresolved_conflicts(workspace_path: &str) -> Result<bool, JjError> {
  if !Path::new(workspace_path).exists() {
    return Ok(false);
  }
  let loaded = load_workspace_repo(workspace_path)?;
  let Some(wc_commit_id) = loaded
    .repo
    .view()
    .get_wc_commit_id(loaded.workspace.workspace_name())
    .cloned()
  else {
    return Ok(false);
  };
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {}", e)))?;
  // Walk the WC tree itself — tree_ids().is_resolved() is equivalent to
  // has_conflict() for the root, but pairing this with get_conflicted_files
  // (which uses tree.conflicts()) keeps the boolean and path list in sync.
  Ok(wc_commit.tree().has_conflict())
}

fn get_workspace_wc_commit(loaded: &LoadedWorkspaceRepo) -> Result<Option<Commit>, JjError> {
  let workspace_name = loaded.workspace.workspace_name();
  let Some(wc_commit_id) = loaded.repo.view().get_wc_commit_id(workspace_name).cloned() else {
    return Ok(None);
  };
  loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map(Some)
    .map_err(|e| JjError::IoError(format!("Failed to load wc commit: {}", e)))
}

/// A log row for a working-copy commit. Its id changes whenever its tree
/// does, so a cached diff stat for that id is current: reading it skips
/// diffing the working copy, and the cache is written only on a miss.
fn build_wc_log_commit(
  cache_repo_path: &str,
  repo: &Arc<ReadonlyRepo>,
  commit: &Commit,
  wc_commit_ids: &HashSet<jj_lib::backend::CommitId>,
  wc_tree_override: Option<(&jj_lib::backend::CommitId, &MergedTree)>,
  is_immutable: &impl Fn(
    &jj_lib::backend::CommitId,
  ) -> Result<bool, jj_lib::revset::RevsetEvaluationError>,
) -> JjLogCommit {
  let cached = local_db::get_cached_commit_diff_stats_batch(cache_repo_path, &[commit.id().hex()])
    .unwrap_or_default();
  let mut misses = Vec::new();
  let row = build_log_commit(
    repo,
    commit,
    wc_commit_ids,
    wc_tree_override,
    is_immutable,
    &cached,
    &mut misses,
  );
  local_db::cache_commit_rows(cache_repo_path, &misses).warn_on_err("cache commit rows");
  row
}

/// A stacked workspace's working-copy commit, for the Commits list of a
/// workspace above it; `None` when the working copy has no changes.
///
/// One load and one snapshot serve both the emptiness check and the row,
/// under the lock [`jj_get_changed_files`] takes. A working copy that cannot
/// be read or snapshotted counts as empty, as it does there.
///
/// Each row of a stack reads the rows below it, and a workspace's tabs and
/// header read at once, so many callers ask for the same row together; they
/// share runs (see [`SharedRead`]).
pub fn jj_get_tentative_working_copy(
  workspace_path: &str,
  workspace_label: &str,
) -> Result<Option<JjTentativeWorkingCopy>, JjError> {
  use std::collections::HashMap;
  use std::sync::{Arc, Mutex, OnceLock, Weak};

  type Shared = SharedRead<Result<Option<JjTentativeWorkingCopy>, String>>;
  type Reads = HashMap<(std::path::PathBuf, String), Weak<Shared>>;
  static READS: OnceLock<Mutex<Reads>> = OnceLock::new();
  let shared = {
    let mut reads = READS
      .get_or_init(|| Mutex::new(HashMap::new()))
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner);
    reads.retain(|_, read| read.strong_count() > 0);
    let key = (
      working_copy_lock_key(workspace_path),
      workspace_label.to_string(),
    );
    match reads.get(&key).and_then(Weak::upgrade) {
      Some(read) => read,
      None => {
        let read = Arc::new(Shared::new());
        reads.insert(key, Arc::downgrade(&read));
        read
      }
    }
  };
  shared
    .run(|| {
      read_tentative_working_copy(workspace_path, workspace_label).map_err(|e| match e {
        JjError::IoError(message) => message,
        other => other.to_string(),
      })
    })
    .map_err(JjError::IoError)
}

fn read_tentative_working_copy(
  workspace_path: &str,
  workspace_label: &str,
) -> Result<Option<JjTentativeWorkingCopy>, JjError> {
  if !Path::new(workspace_path).join(".jj").exists() {
    return Ok(None);
  }
  let cache_repo_path = match derive_repo_path_from_workspace(workspace_path) {
    Some(repo_path) => repo_path,
    None => {
      reconcile_colocated_home_repo(workspace_path)?;
      workspace_path.to_string()
    }
  };
  with_working_copy_lock(workspace_path, || {
    let Ok(mut loaded) = load_workspace_repo(workspace_path) else {
      return Ok(None);
    };
    let Ok(Some(snapshot)) = snapshot_working_copy_tree(&mut loaded, workspace_path) else {
      return Ok(None);
    };
    if snapshot_is_empty(&loaded, workspace_path, Some(&snapshot))? {
      return Ok(None);
    }
    let immutable_revset = evaluate_revset(
      &loaded,
      &format!("::{}", format_revset_symbol(workspace_label)),
    )?;
    let is_immutable = immutable_revset.containing_fn();
    let wc_commit_ids: HashSet<_> = loaded
      .repo
      .view()
      .wc_commit_ids()
      .values()
      .cloned()
      .collect();
    let (wc_commit_id, tree) = &snapshot;
    let commit = loaded
      .repo
      .store()
      .get_commit(wc_commit_id)
      .map_err(|e| JjError::IoError(format!("Failed to load wc commit: {}", e)))?;
    let mut row = build_wc_log_commit(
      &cache_repo_path,
      &loaded.repo,
      &commit,
      &wc_commit_ids,
      Some((wc_commit_id, tree)),
      &is_immutable,
    );
    row.workspace_label = Some(workspace_label.to_string());
    Ok(Some(JjTentativeWorkingCopy {
      workspace_label: workspace_label.to_string(),
      commit: row,
    }))
  })
}

#[derive(Clone, Copy)]
enum CheckoutMode {
  Immediate,
  Deferred,
}

fn update_workspace_after_history_edit(
  loaded: &mut LoadedWorkspaceRepo,
  new_repo: &Arc<ReadonlyRepo>,
  old_wc_commit: Option<&Commit>,
  checkout_mode: CheckoutMode,
) -> Result<(), JjError> {
  // Derive the home repo path (needed to enumerate sibling workspaces below).
  let workspace_root = loaded
    .workspace
    .workspace_root()
    .to_string_lossy()
    .into_owned();
  let repo_path =
    derive_repo_path_from_workspace(&workspace_root).unwrap_or_else(|| workspace_root.clone());

  let primary_workspace_name = loaded.workspace.workspace_name().as_str().to_string();

  if matches!(checkout_mode, CheckoutMode::Deferred) {
    loaded.repo = Arc::clone(new_repo);
    // In deferred mode still reconcile sibling workspaces; skip the loaded workspace (caller owns checkout).
    reconcile_all_workspaces_after_rewrite(&repo_path, Some(&primary_workspace_name))
      .warn_on_err("reconcile all workspaces after rewrite");
    return Ok(());
  }

  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let Some(new_wc_commit_id) = new_repo.view().get_wc_commit_id(&workspace_name).cloned() else {
    // Primary workspace has no WC commit — still reconcile siblings.
    reconcile_all_workspaces_after_rewrite(&repo_path, Some(&primary_workspace_name))
      .warn_on_err("reconcile all workspaces after rewrite");
    return Ok(());
  };
  let new_wc_commit = new_repo
    .store()
    .get_commit(&new_wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load updated wc commit: {}", e)))?;
  let old_tree = old_wc_commit.map(|commit| commit.tree());
  block_on(
    loaded
      .workspace
      .check_out(new_repo.op_id().clone(), old_tree.as_ref(), &new_wc_commit),
  )
  .map_err(|e| JjError::IoError(format!("Failed to update working copy: {}", e)))?;
  loaded.repo = Arc::clone(new_repo);
  if repo_path == workspace_root {
    sync_home_git_head_to_wc_parent(&workspace_root, loaded.home_git_head_branch.as_deref());
  }

  // Reconcile every other workspace whose WC commit was rewritten by rebase_descendants.
  reconcile_all_workspaces_after_rewrite(&repo_path, Some(&primary_workspace_name))
    .warn_on_err("reconcile all workspaces after rewrite");

  Ok(())
}

fn set_local_bookmark_target(
  repo_mut: &mut MutableRepo,
  bookmark_name: &str,
  commit_id: jj_lib::backend::CommitId,
) {
  repo_mut.set_local_bookmark_target(RefName::new(bookmark_name), RefTarget::normal(commit_id));
}

fn short_commit_id(commit: &Commit) -> String {
  commit.id().hex().chars().take(12).collect()
}

fn summarize_rebase_stats(stats: &jj_lib::rewrite::MoveCommitsStats) -> String {
  let mut parts = Vec::new();
  if stats.num_rebased_targets > 0 {
    parts.push(format!(
      "Rebased {} commits to destination",
      stats.num_rebased_targets
    ));
  }
  if stats.num_rebased_descendants > 0 {
    parts.push(format!(
      "Rebased {} descendant commits",
      stats.num_rebased_descendants
    ));
  }
  if stats.num_skipped_rebases > 0 {
    parts.push(format!(
      "Skipped rebase of {} commits already in place",
      stats.num_skipped_rebases
    ));
  }
  if stats.num_abandoned_empty > 0 {
    parts.push(format!(
      "Abandoned {} newly emptied commits",
      stats.num_abandoned_empty
    ));
  }
  if parts.is_empty() {
    "No revisions to rebase.".to_string()
  } else {
    parts.join("\n")
  }
}

/// Imports colocated git state into jj (`import_git_head_if_needed`, `git::import_refs`) without
/// snapshotting the working copy.
///
/// Aligns with the import portion of [jj `util snapshot`](https://github.com/jj-vcs/jj/blob/main/cli/src/commands/util/snapshot.rs).
///
/// Does not shell out to `jj util snapshot`.
pub fn jj_util_import_git_refs(repo_path: &str) -> Result<(), JjError> {
  reconcile_colocated_home_repo(repo_path)
}

/// Import git branch refs into jj bookmarks (e.g. a branch set purely via git
/// config, never touched by jj) without the git-HEAD/working-copy reconciliation
/// [`jj_util_import_git_refs`] also does — safe to call before resolving a
/// revision that might be a bookmark name, without risking a working-copy reset.
pub fn jj_import_remaining_git_refs(repo_path: &str) -> Result<(), JjError> {
  let mut loaded = load_workspace_repo(repo_path)?;
  import_remaining_git_refs(&mut loaded)
}

fn import_remaining_git_refs(loaded: &mut LoadedWorkspaceRepo) -> Result<(), JjError> {
  let import_options = git::GitImportOptions {
    auto_local_bookmark: false,
    abandon_unreachable_commits: true,
    remote_auto_track_bookmarks: HashMap::new(),
  };
  let mut tx = loaded.repo.start_transaction();
  block_on(git::import_refs(tx.repo_mut(), &import_options))
    .map_err(|e| JjError::GitWorkspaceError(format!("git import_refs: {}", e)))?;
  if tx.repo().has_changes() {
    block_on(tx.repo_mut().rebase_descendants()).map_err(|e| {
      JjError::GitWorkspaceError(format!(
        "Failed to rebase descendants after git import_refs: {}",
        e
      ))
    })?;
    loaded.repo = block_on(tx.commit("import git refs")).map_err(|e| {
      JjError::GitWorkspaceError(format!("Failed to commit git import_refs: {}", e))
    })?;
  }

  Ok(())
}

mod workspaces;
pub use workspaces::*;
mod files;
pub use files::*;
mod stash;
pub use stash::*;
mod bookmarks;
pub use bookmarks::*;
mod git_head;
pub use git_head::*;
mod remote;
pub use remote::*;
mod log;
pub use log::*;
mod commit_diff;
pub use commit_diff::*;
mod merge_commits;
pub use merge_commits::*;

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_cli::git;
  use std::ffi::OsStr;
  use std::fs;
  use std::process::Command;
  use tempfile::TempDir;

  #[test]
  fn text_hunks_keep_lines_that_look_like_file_headers() {
    let hunks = build_text_hunks("-- users\nkeep\n---\n", "++ accounts\nkeep\n");
    let lines: Vec<&str> = hunks
      .iter()
      .flat_map(|h| h.lines.iter().map(String::as_str))
      .collect();
    assert!(lines.contains(&"--- users"), "{lines:?}");
    assert!(lines.contains(&"----"), "{lines:?}");
    assert!(lines.contains(&"+++ accounts"), "{lines:?}");
    assert_eq!(count_changed_lines(&hunks), 3);
  }

  #[test]
  fn git_subprocess_environment_turns_off_auto_maintenance() {
    let mut environment = HashMap::new();
    disable_auto_maintenance(&mut environment);
    let get = |key: &str| environment.get(OsStr::new(key)).and_then(|v| v.to_str());
    let count: usize = get("GIT_CONFIG_COUNT").unwrap().parse().unwrap();
    // Entries below an inherited `GIT_CONFIG_COUNT` belong to the caller.
    let configured: Vec<_> = (0..count)
      .filter_map(|i| {
        Some((
          get(&format!("GIT_CONFIG_KEY_{i}"))?,
          get(&format!("GIT_CONFIG_VALUE_{i}"))?,
        ))
      })
      .collect();
    assert!(configured.contains(&("maintenance.auto", "false")));
    assert!(configured.contains(&("gc.auto", "0")));
  }

  #[test]
  fn shared_read_serves_callers_that_pile_up_with_one_more_run() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    let shared = SharedRead::new();
    let runs = AtomicUsize::new(0);
    let arrived = AtomicUsize::new(0);
    thread::scope(|scope| {
      let first = scope.spawn(|| {
        shared.run(|| {
          let run = runs.fetch_add(1, Ordering::SeqCst) + 1;
          // Hold run 1 until every waiter has asked.
          while arrived.load(Ordering::SeqCst) < 6 {
            thread::yield_now();
          }
          thread::sleep(std::time::Duration::from_millis(50));
          run
        })
      });
      while runs.load(Ordering::SeqCst) == 0 {
        thread::yield_now();
      }
      let waiters: Vec<_> = (0..6)
        .map(|_| {
          scope.spawn(|| {
            arrived.fetch_add(1, Ordering::SeqCst);
            shared.run(|| runs.fetch_add(1, Ordering::SeqCst) + 1)
          })
        })
        .collect();
      assert_eq!(first.join().unwrap(), 1);
      for waiter in waiters {
        assert_eq!(
          waiter.join().unwrap(),
          2,
          "waiters share the run after theirs began"
        );
      }
    });
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    assert_eq!(
      shared.run(|| runs.fetch_add(1, Ordering::SeqCst) + 1),
      3,
      "a later caller runs again"
    );
  }

  #[test]
  fn shared_read_recovers_when_a_run_panics() {
    let shared = SharedRead::new();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      shared.run(|| -> u32 { panic!("read failed") })
    }));
    assert!(panicked.is_err());
    assert_eq!(shared.run(|| 7), 7);
  }

  #[test]
  fn working_copy_lock_is_shared_across_equivalent_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let plain = working_copy_lock(path);
    assert!(Arc::ptr_eq(&plain, &working_copy_lock(&format!("{path}/"))));
    assert!(Arc::ptr_eq(
      &plain,
      &working_copy_lock(&format!("{path}/."))
    ));
    let missing = working_copy_lock("/tmp/treq-lock-missing/foo");
    assert!(Arc::ptr_eq(
      &missing,
      &working_copy_lock("/tmp/treq-lock-missing/foo/")
    ));
    assert!(Arc::ptr_eq(
      &missing,
      &working_copy_lock("/tmp/treq-lock-missing/./foo")
    ));
    assert!(!Arc::ptr_eq(&plain, &missing));
  }

  #[test]
  fn working_copy_lock_serializes_threads_for_the_same_workspace() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    static CONCURRENT: AtomicUsize = AtomicUsize::new(0);
    static MAX: AtomicUsize = AtomicUsize::new(0);
    let threads: Vec<_> = (0..4)
      .map(|_| {
        thread::spawn(|| {
          with_working_copy_lock("/tmp/shared-workspace", || {
            let now = CONCURRENT.fetch_add(1, Ordering::SeqCst) + 1;
            MAX.fetch_max(now, Ordering::SeqCst);
            thread::sleep(std::time::Duration::from_millis(15));
            CONCURRENT.fetch_sub(1, Ordering::SeqCst);
          })
        })
      })
      .collect();
    for t in threads {
      t.join().unwrap();
    }
    assert_eq!(MAX.load(Ordering::SeqCst), 1);
  }

  #[test]
  fn working_copy_lock_does_not_serialize_different_workspaces() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    static CONCURRENT: AtomicUsize = AtomicUsize::new(0);
    static MAX: AtomicUsize = AtomicUsize::new(0);
    let threads: Vec<_> = (0..4)
      .map(|i| {
        thread::spawn(move || {
          with_working_copy_lock(&format!("/tmp/distinct-workspace-{i}"), || {
            let now = CONCURRENT.fetch_add(1, Ordering::SeqCst) + 1;
            MAX.fetch_max(now, Ordering::SeqCst);
            thread::sleep(std::time::Duration::from_millis(15));
            CONCURRENT.fetch_sub(1, Ordering::SeqCst);
          })
        })
      })
      .collect();
    for t in threads {
      t.join().unwrap();
    }
    assert!(MAX.load(Ordering::SeqCst) > 1);
  }

  #[test]
  fn is_autosave_description_matches_first_line_prefix() {
    assert!(is_autosave_description("treq-autosave: good.txt"));
    assert!(is_autosave_description(
      "treq-autosave: a.txt, b.txt, … N1 more\nmore"
    ));
    assert!(!is_autosave_description("ship the good changes"));
    assert!(!is_autosave_description(""));
  }

  #[test]
  fn autosave_commit_message_lists_up_to_two_files() {
    assert_eq!(
      autosave_commit_message(&["good.txt".to_string()]),
      "treq-autosave: good.txt"
    );
    assert_eq!(
      autosave_commit_message(&["b.txt".to_string(), "a.txt".to_string()]),
      "treq-autosave: a.txt, b.txt"
    );
  }

  #[test]
  fn autosave_commit_message_ellipsizes_after_two_files() {
    let paths: Vec<String> = (1..=57).map(|i| format!("f{i:02}.txt")).collect();
    assert_eq!(
      autosave_commit_message(&paths),
      "treq-autosave: f01.txt, f02.txt, … N55 more"
    );
    assert_eq!(
      autosave_commit_message(&[
        "c.txt".to_string(),
        "a.txt".to_string(),
        "b.txt".to_string()
      ]),
      "treq-autosave: a.txt, b.txt, … N1 more"
    );
  }

  fn init_git_repo(temp: &TempDir) {
    git(temp.path(), &["init"]);
  }

  #[test]
  fn interrupted_workspace_check_fails_closed_on_unreadable_dir() {
    let temp = TempDir::new().expect("tempdir");
    init_jj_repo(&temp);
    let not_a_dir = temp.path().join(".git").join("HEAD");
    let repo = temp.path().to_str().expect("utf8");
    assert!(is_interrupted_workspace_init(repo, "x", &not_a_dir).is_err());
  }

  fn init_jj_repo(temp: &TempDir) {
    crate::test_cli::jj(temp.path(), &["git", "init", "--colocate", "."]);
  }

  // A pushed origin plus a colocated home clone that tracks `main@origin`.
  fn origin_and_home_clone() -> (TempDir, TempDir) {
    let origin = TempDir::new().expect("origin");
    git(origin.path(), &["init", "--bare", "-b", "main", "."]);
    let home = TempDir::new().expect("home");
    git(home.path(), &["init", "-b", "main", "."]);
    git(home.path(), &["config", "user.name", "Test User"]);
    git(home.path(), &["config", "user.email", "test@example.com"]);
    git(
      home.path(),
      &["remote", "add", "origin", origin.path().to_str().unwrap()],
    );
    fs::write(home.path().join("base.txt"), "base\n").expect("write");
    git(home.path(), &["add", "."]);
    git(home.path(), &["commit", "-m", "base"]);
    git(home.path(), &["push", "origin", "main"]);
    let home_path = home.path().to_str().unwrap();
    init_jj_for_git_repo(home_path).expect("init jj");
    jj_git_fetch(home_path).expect("fetch");
    jj_bookmark_track(home_path, "main", "origin").expect("track");
    (origin, home)
  }

  // Lands one commit on origin's `main` from a second clone; returns its id.
  fn advance_origin_main(origin: &TempDir) -> String {
    let other = TempDir::new().expect("other");
    git(
      other.path(),
      &["clone", origin.path().to_str().unwrap(), "."],
    );
    git(other.path(), &["config", "user.name", "Other"]);
    git(other.path(), &["config", "user.email", "other@example.com"]);
    let count = git(other.path(), &["rev-list", "--count", "HEAD"]);
    fs::write(other.path().join(format!("next-{count}.txt")), "next\n").expect("write");
    git(other.path(), &["add", "."]);
    git(other.path(), &["commit", "-m", "next"]);
    git(other.path(), &["push", "origin", "main"]);
    git(other.path(), &["rev-parse", "HEAD"])
  }

  // Two operations started from the same op head each move `main`: the race
  // a background fetch and another jj op hit, leaving `main` conflicted.
  fn race_main_into_conflict(repo_path: &str, start: &str, a: &str, b: &str) {
    let set_main = |repo: &Arc<ReadonlyRepo>, hex: &str, name: &str| {
      let id = jj_lib::backend::CommitId::try_from_hex(hex).expect("hex");
      let mut tx = repo.start_transaction();
      tx.repo_mut()
        .set_local_bookmark_target(RefName::new("main"), RefTarget::normal(id));
      block_on(tx.commit(name)).expect("commit race op")
    };
    let loaded = load_workspace_repo(repo_path).expect("load");
    let started = set_main(&loaded.repo, start, "race start");
    set_main(&started, a, "race a");
    set_main(&started, b, "race b");
    assert!(jj_is_bookmark_conflicted(repo_path, "main"));
  }

  #[test]
  fn fetch_waits_for_an_op_holding_the_repo_lock_from_a_workspace() {
    let (origin, home) = origin_and_home_clone();
    let home_path = home.path().to_str().unwrap().to_string();
    advance_origin_main(&origin);
    // A workspace path shares the home repo's lock: one operation log.
    let workspace_path = home.path().join(".treq").join("workspaces").join("ws");
    let lock = repo_op_lock(workspace_path.to_str().unwrap());
    let held = lock.lock().unwrap();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let fetching = std::thread::spawn(move || {
      jj_git_fetch(&home_path).expect("fetch");
      done_tx.send(()).unwrap();
    });
    let finished_while_held = done_rx
      .recv_timeout(std::time::Duration::from_secs(3))
      .is_ok();
    drop(held);
    fetching.join().unwrap();
    assert!(
      !finished_while_held,
      "fetch ran while another op held the lock"
    );
  }

  #[test]
  fn fetch_exports_moved_trunk_to_git() {
    let (origin, home) = origin_and_home_clone();
    let tip = advance_origin_main(&origin);
    jj_git_fetch(home.path().to_str().unwrap()).expect("fetch");
    assert_eq!(git(home.path(), &["rev-parse", "refs/heads/main"]), tip);
  }

  #[test]
  fn fetch_resets_conflicted_trunk_when_origin_has_every_side() {
    let (origin, home) = origin_and_home_clone();
    let home_path = home.path().to_str().unwrap();
    let base = git(home.path(), &["rev-parse", "main"]);
    // Origin merges two sibling commits, so both race sides are on origin
    // and neither descends from the other.
    let other = TempDir::new().expect("other");
    git(
      other.path(),
      &["clone", origin.path().to_str().unwrap(), "."],
    );
    git(other.path(), &["config", "user.name", "Other"]);
    git(other.path(), &["config", "user.email", "other@example.com"]);
    let mut sides = Vec::new();
    for side in ["left", "right"] {
      git(other.path(), &["switch", "-C", side, &base]);
      fs::write(other.path().join(format!("{side}.txt")), "x\n").expect("write");
      git(other.path(), &["add", "."]);
      git(other.path(), &["commit", "-m", side]);
      sides.push(git(other.path(), &["rev-parse", "HEAD"]));
    }
    git(other.path(), &["switch", "-C", "main", &sides[0]]);
    git(other.path(), &["merge", "--no-edit", "right"]);
    git(other.path(), &["push", "origin", "main"]);
    let tip = git(other.path(), &["rev-parse", "HEAD"]);
    jj_git_fetch(home_path).expect("fetch");
    race_main_into_conflict(home_path, &base, &sides[0], &sides[1]);
    jj_git_fetch(home_path).expect("fetch resolves");
    assert!(!jj_is_bookmark_conflicted(home_path, "main"));
    assert_eq!(git(home.path(), &["rev-parse", "refs/heads/main"]), tip);
  }

  #[test]
  fn fetch_keeps_conflicted_trunk_holding_a_local_only_commit() {
    let (origin, home) = origin_and_home_clone();
    let home_path = home.path().to_str().unwrap();
    git(home.path(), &["switch", "-c", "local-work"]);
    fs::write(home.path().join("mine.txt"), "mine\n").expect("write");
    git(home.path(), &["add", "mine.txt"]);
    git(home.path(), &["commit", "-m", "local only"]);
    let local = git(home.path(), &["rev-parse", "HEAD"]);
    let base = git(home.path(), &["rev-parse", "main"]);
    let tip = advance_origin_main(&origin);
    jj_git_fetch(home_path).expect("fetch");
    race_main_into_conflict(home_path, &base, &local, &tip);
    jj_git_fetch(home_path).expect("fetch");
    assert!(jj_is_bookmark_conflicted(home_path, "main"));
  }
  fn init_colocated_repo_with_two_branches() -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    git(temp.path(), &["config", "user.name", "Test User"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(temp.path(), &["add", "base.txt"]);
    git(temp.path(), &["commit", "-m", "base"]);
    git(temp.path(), &["branch", "branch-a"]);
    git(temp.path(), &["switch", "-c", "branch-b"]);
    fs::write(temp.path().join("branch.txt"), "branch b\n").expect("write branch file");
    git(temp.path(), &["add", "branch.txt"]);
    git(temp.path(), &["commit", "-m", "branch b"]);
    git(temp.path(), &["switch", "branch-a"]);
    init_jj_repo(&temp);
    temp
  }

  fn home_wc_parent_id(repo_path: &str) -> String {
    let loaded = load_workspace_repo(repo_path).expect("load workspace");
    let wc_id = loaded
      .repo
      .view()
      .get_wc_commit_id(loaded.workspace.workspace_name())
      .expect("working-copy commit");
    let wc = loaded
      .repo
      .store()
      .get_commit(wc_id)
      .expect("load wc commit");
    wc.parent_ids().first().expect("wc parent").hex()
  }

  #[test]
  fn rebase_snapshot_marks_only_workspaces_that_are_certainly_in_sync() {
    let temp = init_colocated_repo_with_two_branches();
    let repo_path = temp.path().to_str().expect("utf8 path");

    let snapshot = rebase_snapshot(
      repo_path,
      &[
        ("branch-b", "branch-a"),
        ("branch-a", "branch-a"),
        ("branch-a", "branch-b"),
        ("missing", "branch-a"),
        ("branch-b", "missing"),
      ],
    )
    .expect("snapshot");

    assert!(snapshot.is_in_sync(0), "descendant of its target");
    assert!(snapshot.is_in_sync(1), "branch is its own target");
    assert!(!snapshot.is_in_sync(2), "behind its target");
    assert!(!snapshot.is_in_sync(3), "missing workspace bookmark");
    assert!(!snapshot.is_in_sync(4), "missing target bookmark");
    assert!(!snapshot.is_in_sync(9), "out of range is not in sync");
    assert_eq!(
      snapshot.operation_id(),
      current_operation_id(repo_path).expect("op id")
    );
  }

  #[test]
  fn reconcile_external_checkout_reparents_home_without_false_changes() {
    let temp = init_colocated_repo_with_two_branches();
    git(temp.path(), &["switch", "branch-b"]);
    let repo_path = temp.path().to_str().expect("utf8 path");

    let changes = jj_get_changed_files(repo_path).expect("reconcile changes");

    assert!(
      changes.is_empty(),
      "branch delta must not appear as local changes"
    );
    assert_eq!(
      home_wc_parent_id(repo_path),
      git(temp.path(), &["rev-parse", "HEAD"])
    );
  }

  #[test]
  fn reconcile_reparents_when_git_head_was_already_imported() {
    let temp = init_colocated_repo_with_two_branches();
    git(temp.path(), &["switch", "branch-b"]);
    let repo_path = temp.path().to_str().expect("utf8 path");
    let mut loaded = load_workspace_repo(repo_path).expect("load workspace");
    import_git_head_if_needed(&mut loaded, repo_path).expect("lightweight import");
    assert_ne!(
      home_wc_parent_id(repo_path),
      git(temp.path(), &["rev-parse", "HEAD"])
    );

    let changes = jj_get_changed_files(repo_path).expect("reconcile changes");

    assert!(changes.is_empty());
    assert_eq!(
      home_wc_parent_id(repo_path),
      git(temp.path(), &["rev-parse", "HEAD"])
    );
  }

  #[test]
  fn reconcile_external_checkout_preserves_real_local_edit() {
    let temp = init_colocated_repo_with_two_branches();
    fs::write(temp.path().join("local.txt"), "local edit\n").expect("write local edit");
    git(temp.path(), &["switch", "branch-b"]);
    let repo_path = temp.path().to_str().expect("utf8 path");

    let changes = jj_get_changed_files(repo_path).expect("reconcile changes");

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "local.txt");
    assert_eq!(
      fs::read_to_string(temp.path().join("local.txt")).unwrap(),
      "local edit\n"
    );
    assert_eq!(
      home_wc_parent_id(repo_path),
      git(temp.path(), &["rev-parse", "HEAD"])
    );
  }

  #[test]
  fn reconcile_external_commit_reparents_without_false_changes() {
    let temp = init_colocated_repo_with_two_branches();
    let repo_path = temp.path().to_str().expect("utf8 path");
    fs::write(temp.path().join("pulled.txt"), "new upstream content\n").expect("write file");
    git(temp.path(), &["add", "pulled.txt"]);
    git(temp.path(), &["commit", "-m", "advance checked-out branch"]);

    let changes = jj_get_changed_files(repo_path).expect("reconcile changes");

    assert!(
      changes.is_empty(),
      "new committed files must not appear as local changes"
    );
    assert_eq!(
      home_wc_parent_id(repo_path),
      git(temp.path(), &["rev-parse", "HEAD"])
    );
  }

  #[test]
  fn reconcile_detached_external_checkout_reparents_home() {
    let temp = init_colocated_repo_with_two_branches();
    git(temp.path(), &["checkout", "--detach", "branch-b"]);
    let repo_path = temp.path().to_str().expect("utf8 path");

    let changes = jj_get_changed_files(repo_path).expect("reconcile changes");

    assert!(changes.is_empty());
    assert_eq!(
      home_wc_parent_id(repo_path),
      git(temp.path(), &["rev-parse", "HEAD"])
    );
  }

  #[test]
  fn repeated_reconcile_when_aligned_does_not_advance_operation() {
    let temp = init_colocated_repo_with_two_branches();
    let repo_path = temp.path().to_str().expect("utf8 path");
    reconcile_colocated_home_repo(repo_path).expect("initial reconciliation");
    let before = jj_head_operation_id(repo_path).expect("operation before");

    reconcile_colocated_home_repo(repo_path).expect("repeated reconciliation");

    assert_eq!(
      jj_head_operation_id(repo_path).expect("operation after"),
      before
    );
  }

  #[test]
  fn snapshot_head_operation_id_sees_child_workspace_file_edits() {
    let temp = init_colocated_repo_with_two_branches();
    let repo_path = temp.path().to_str().expect("utf8 path");
    let child_name = create_workspace(
      repo_path,
      "child",
      "child",
      true,
      None,
      Some("branch-a"),
      None,
    )
    .expect("create child workspace");
    let child_dir = temp.path().join(".treq/workspaces").join(child_name);
    let child_path = child_dir.to_string_lossy().into_owned();

    let idle = jj_snapshot_head_operation_id(&child_path).expect("idle marker");
    assert_eq!(
      jj_snapshot_head_operation_id(&child_path).expect("repeat marker"),
      idle
    );
    assert_eq!(jj_head_operation_id(&child_path).expect("head op"), idle);

    fs::write(child_dir.join("base.txt"), "edited outside jj\n").expect("edit file");
    let edited = jj_snapshot_head_operation_id(&child_path).expect("edited marker");
    assert_ne!(edited, idle);
    let changes = jj_get_changed_files(&child_path).expect("changed files");
    assert!(changes.iter().any(|change| change.path == "base.txt"));
    assert_eq!(
      jj_head_operation_id(&child_path).expect("head op after read"),
      edited,
      "the changed-files read must agree with the marker snapshot"
    );
  }

  #[test]
  fn create_workspace_from_home_branch_skips_home_uncommitted_changes() {
    let temp = init_colocated_repo_with_two_branches();
    let repo_path = temp.path().to_str().expect("utf8 path");
    // Home sits on branch-a with an uncommitted edit; snapshot it into the home wc commit.
    fs::write(temp.path().join("home-dirty.txt"), "wip\n").expect("write dirty file");
    jj_get_changed_files(repo_path).expect("snapshot home");

    let child_name = create_workspace(
      repo_path,
      "child",
      "child",
      true,
      Some("branch-a"),
      Some("branch-a"),
      None,
    )
    .expect("create child workspace");

    let child_dir = temp.path().join(".treq/workspaces").join(child_name);
    assert!(child_dir.join("base.txt").exists());
    assert!(
      !child_dir.join("home-dirty.txt").exists(),
      "new workspace must not inherit the home repo's uncommitted changes"
    );
  }

  #[test]
  fn reconcile_home_does_not_reparent_child_workspace() {
    let temp = init_colocated_repo_with_two_branches();
    let repo_path = temp.path().to_str().expect("utf8 path");
    let child_name = create_workspace(
      repo_path,
      "child",
      "child",
      true,
      None,
      Some("branch-a"),
      None,
    )
    .expect("create child workspace");
    let child_path = temp
      .path()
      .join(".treq/workspaces")
      .join(child_name)
      .to_string_lossy()
      .into_owned();
    let child_parent_before = home_wc_parent_id(&child_path);
    git(temp.path(), &["switch", "branch-b"]);

    reconcile_colocated_home_repo(repo_path).expect("reconcile home");

    assert_eq!(home_wc_parent_id(&child_path), child_parent_before);
    assert_eq!(
      home_wc_parent_id(repo_path),
      git(temp.path(), &["rev-parse", "HEAD"])
    );
  }

  #[test]
  fn repair_detached_home_head_reattaches_to_non_default_branch() {
    let temp = TempDir::new().expect("tempdir");
    let status = Command::new("git")
      .current_dir(temp.path())
      .args(["init", "-b", "main"])
      .status()
      .expect("git init should run");
    assert!(status.success(), "git init should succeed");
    git(temp.path(), &["config", "user.name", "Test User"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(temp.path(), &["add", "base.txt"]);
    git(temp.path(), &["commit", "-m", "base"]);
    git(temp.path(), &["switch", "-c", "feature"]);
    fs::write(temp.path().join("feature.txt"), "feature\n").expect("write feature");
    git(temp.path(), &["add", "feature.txt"]);
    git(temp.path(), &["commit", "-m", "feature work"]);
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");
    let feature_tip = git(temp.path(), &["rev-parse", "feature"]);
    git(temp.path(), &["checkout", "--detach", &feature_tip]);
    assert_eq!(
      read_git_head_branch(repo_path).expect("read HEAD"),
      "HEAD",
      "precondition: HEAD should be detached before repair"
    );

    let repaired =
      repair_detached_home_head_at_default_branch(repo_path).expect("repair should not error");

    assert_eq!(
      repaired,
      Some("feature".to_string()),
      "repair should reattach HEAD to the branch matching its tip, not just the default branch"
    );
    assert_eq!(
      read_git_head_branch(repo_path).expect("read HEAD"),
      "feature"
    );
  }

  #[test]
  fn snapshot_read_keeps_new_files_when_a_later_operation_marks_the_working_copy_stale() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");
    jj_git_init_colocated(repo_path).expect("init colocated repo");
    fs::write(temp.path().join(".gitignore"), ".jj/\n.treq/\n").expect("write gitignore");

    // Reads such as jj_get_log and get_conflicted_files snapshot this way.
    let mut loaded = load_workspace_repo(repo_path).expect("load repo");
    snapshot_working_copy_tree(&mut loaded, repo_path).expect("snapshot");

    // A later operation that leaves the working copy alone, like the HEAD and
    // refs imports those reads run.
    let loaded = load_workspace_repo(repo_path).expect("reload repo");
    let root_id = loaded.repo.store().root_commit_id().clone();
    let mut tx = loaded.repo.start_transaction();
    tx.repo_mut()
      .set_local_bookmark_target(RefName::new("marker"), RefTarget::normal(root_id));
    block_on(tx.commit("unrelated operation")).expect("commit operation");

    // The stale-working-copy recovery get_conflicted_files runs.
    update_stale_workspace(repo_path).expect("update stale working copy");

    assert!(
      temp.path().join(".gitignore").exists(),
      "stale recovery after a snapshot read must not delete a new file"
    );
  }

  #[test]
  fn init_keeps_treq_and_jj_dirs_out_of_git_without_a_root_gitignore() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8");
    init_jj_for_git_repo(repo_path).expect("init jj");
    fs::write(temp.path().join(".treq/local.db"), "db").expect("write db");
    fs::create_dir_all(temp.path().join(".treq/workspaces/feat")).expect("mkdir workspace");
    fs::write(temp.path().join(".treq/workspaces/feat/a.txt"), "a").expect("write file");
    fs::write(temp.path().join(".treq/config.yaml"), "setup: x\n").expect("write config");
    fs::create_dir_all(temp.path().join(".treq/workflows/nested")).expect("mkdir workflows");
    fs::write(
      temp.path().join(".treq/workflows/nested/ci.yml"),
      "on: push\n",
    )
    .expect("write workflow");

    assert!(!temp.path().join(".gitignore").exists());
    let status = Command::new("git")
      .current_dir(temp.path())
      .args(["status", "--porcelain", "--untracked-files=all"])
      .output()
      .expect("git status");
    assert!(status.status.success(), "git status should succeed");
    assert_eq!(
      String::from_utf8_lossy(&status.stdout),
      "?? .treq/config.yaml\n?? .treq/workflows/nested/ci.yml\n"
    );

    let mut changed: Vec<String> = jj_get_changed_files(repo_path)
      .expect("snapshot")
      .into_iter()
      .map(|change| change.path)
      .collect();
    changed.sort();
    assert_eq!(
      changed,
      [".treq/config.yaml", ".treq/workflows/nested/ci.yml"]
    );
  }

  #[test]
  fn ensure_treq_dirs_ignored_upgrades_generated_ignores_and_keeps_user_ones() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8");
    fs::create_dir_all(temp.path().join(".treq")).expect("mkdir .treq");
    fs::write(
      temp.path().join(".treq/.gitignore"),
      "# Generated by Treq\n*\n",
    )
    .expect("write");
    fs::create_dir_all(temp.path().join(".jj")).expect("mkdir .jj");
    fs::write(temp.path().join(".jj/.gitignore"), "/*\n").expect("write");

    ensure_treq_dirs_ignored(repo_path).expect("ignore");

    assert_eq!(
      fs::read_to_string(temp.path().join(".treq/.gitignore")).expect("read"),
      "# Generated by Treq\n*\n!config.yaml\n!workflows/\n!workflows/**\n"
    );
    assert_eq!(
      fs::read_to_string(temp.path().join(".jj/.gitignore")).expect("read"),
      "/*\n"
    );
  }

  #[test]
  fn parse_sparse_patterns_normalizes_and_dedups() {
    let patterns = vec![
      "src".to_string(),
      "./docs/".to_string(),
      " src ".to_string(),
      "a/b/file.txt".to_string(),
    ];
    let parsed = parse_sparse_patterns(&patterns).expect("valid patterns should parse");
    let as_strings: Vec<String> = parsed
      .iter()
      .map(|p| p.as_internal_file_string().to_string())
      .collect();
    assert_eq!(as_strings, vec!["src", "docs", "a/b/file.txt"]);
  }

  #[test]
  fn parse_sparse_patterns_rejects_invalid() {
    for bad in ["", "  ", "/abs/path", "../escape", "a/../b"] {
      let err = parse_sparse_patterns(&[bad.to_string()])
        .expect_err(&format!("pattern '{}' should be rejected", bad));
      let msg = format!("{:?}", err);
      assert!(
        msg.contains("Invalid sparse pattern"),
        "error for '{}' should mention the invalid pattern, got: {}",
        bad,
        msg
      );
    }
  }

  #[test]
  fn create_workspace_retry_rejects_registered_workspace_without_deleting_files() {
    let temp = TempDir::new().expect("tempdir");
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");
    jj_set_bookmark(repo_path, "main", "@").expect("set main bookmark");
    create_workspace(
      repo_path,
      "fix/example",
      "fix/example",
      true,
      None,
      Some("main"),
      None,
    )
    .expect("first creation");
    let workspace_dir = temp.path().join(".treq/workspaces/fix-example");
    let marker = workspace_dir.join("keep-me.txt");
    fs::write(&marker, "existing work").expect("write marker");
    let before = jj_get_commit_id(repo_path, "fix/example").expect("bookmark target");

    let err = create_workspace(
      repo_path,
      "fix/example",
      "fix/example",
      true,
      None,
      Some("main"),
      None,
    )
    .expect_err("retry must be rejected");

    assert!(format!("{err}").contains("already registered"));
    assert_eq!(
      fs::read_to_string(marker).expect("marker retained"),
      "existing work"
    );
    assert_eq!(
      jj_get_commit_id(repo_path, "fix/example").expect("bookmark remains normal"),
      before
    );
  }

  #[test]
  fn create_workspace_recovers_unregistered_partial_directory() {
    let temp = TempDir::new().expect("tempdir");
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");
    jj_set_bookmark(repo_path, "main", "@").expect("set main bookmark");
    let workspace_dir = temp.path().join(".treq/workspaces/fix-partial");
    fs::create_dir_all(workspace_dir.join(".jj")).expect("create partial directory");
    fs::write(workspace_dir.join("partial"), "incomplete").expect("write partial marker");

    create_workspace(
      repo_path,
      "fix/partial",
      "fix/partial",
      true,
      None,
      Some("main"),
      None,
    )
    .expect("orphaned partial directory should be recreated");

    assert!(workspace_dir.join(".jj").exists());
    assert!(!workspace_dir.join("partial").exists());
    assert!(list_jj_workspaces(repo_path)
      .expect("list workspaces")
      .contains(&"fix-partial".to_string()));
  }

  #[test]
  fn creation_verification_resolves_stale_operation_conflict_to_registered_wc() {
    let temp = TempDir::new().expect("tempdir");
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");
    jj_set_bookmark(repo_path, "main", "@").expect("set main bookmark");

    let stale = load_workspace_repo(repo_path).expect("load stale repo");
    let stale_target = stale
      .repo
      .view()
      .get_wc_commit_id(stale.workspace.workspace_name())
      .expect("default wc")
      .clone();
    let mut stale_tx = stale.repo.start_transaction();
    stale_tx.repo_mut().set_local_bookmark_target(
      RefName::new("fix/concurrent"),
      RefTarget::normal(stale_target),
    );

    create_workspace(
      repo_path,
      "fix/concurrent",
      "fix/concurrent",
      true,
      None,
      Some("main"),
      None,
    )
    .expect("create workspace");
    block_on(stale_tx.commit("stale concurrent workspace creation"))
      .expect("commit stale operation");

    let workspace_path = temp.path().join(".treq/workspaces/fix-concurrent");
    let loaded = load_workspace_repo(workspace_path.to_str().expect("utf8 path"))
      .expect("load created workspace");
    let workspace_name: WorkspaceNameBuf = "fix-concurrent".into();
    let expected = loaded
      .repo
      .view()
      .get_wc_commit_id(&workspace_name)
      .expect("registered wc")
      .clone();
    let resolved = verify_created_workspace_bookmark(
      &loaded.workspace,
      "fix/concurrent",
      &workspace_name,
      &expected,
    )
    .expect("registered target should resolve conflict");

    assert_eq!(
      resolved
        .view()
        .get_local_bookmark(RefName::new("fix/concurrent"))
        .as_normal(),
      Some(&expected)
    );
  }

  #[test]
  fn creation_verification_reports_unrelated_ambiguous_bookmark_without_resolving() {
    let temp = TempDir::new().expect("tempdir");
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");
    jj_set_bookmark(repo_path, "main", "@").expect("set main bookmark");
    create_workspace(
      repo_path,
      "fix/ambiguous",
      "fix/ambiguous",
      true,
      None,
      Some("main"),
      None,
    )
    .expect("create workspace");

    let workspace_path = temp.path().join(".treq/workspaces/fix-ambiguous");
    let base =
      load_workspace_repo(workspace_path.to_str().expect("utf8 path")).expect("load workspace");
    let workspace_name: WorkspaceNameBuf = "fix-ambiguous".into();
    let expected = base
      .repo
      .view()
      .get_wc_commit_id(&workspace_name)
      .expect("registered wc")
      .clone();
    let main_id = base
      .repo
      .view()
      .get_local_bookmark(RefName::new("main"))
      .as_normal()
      .expect("main target")
      .clone();
    let root_id = base.repo.store().root_commit_id().clone();
    let mut left = base.repo.start_transaction();
    left
      .repo_mut()
      .set_local_bookmark_target(RefName::new("fix/ambiguous"), RefTarget::normal(main_id));
    let mut right = base.repo.start_transaction();
    right
      .repo_mut()
      .set_local_bookmark_target(RefName::new("fix/ambiguous"), RefTarget::normal(root_id));
    block_on(left.commit("unrelated left target")).expect("commit left");
    block_on(right.commit("unrelated right target")).expect("commit right");

    let err = verify_created_workspace_bookmark(
      &base.workspace,
      "fix/ambiguous",
      &workspace_name,
      &expected,
    )
    .expect_err("unrelated conflict must not be guessed");
    assert!(format!("{err}").contains("manual recovery is required"));
    let latest = block_on(base.workspace.repo_loader().load_at_head()).expect("reload repo");
    assert!(
      latest
        .view()
        .get_local_bookmark(RefName::new("fix/ambiguous"))
        .has_conflict(),
      "unrelated conflict must remain intact"
    );
  }

  #[test]
  fn bookmark_set_get_delete_round_trip() {
    let temp = TempDir::new().expect("tempdir");
    init_jj_repo(&temp);
    let workspace_path = temp.path().to_str().expect("utf8 path");

    jj_set_bookmark(workspace_path, "feat/round-trip", "@").expect("set bookmark");
    let on_head =
      get_bookmarks_on_revision(workspace_path, "@").expect("list bookmarks on revision");
    assert!(
      on_head.iter().any(|name| name == "feat/round-trip"),
      "bookmark should be attached to @"
    );

    jj_delete_bookmark(workspace_path, "feat/round-trip").expect("delete bookmark");
    let on_head_after =
      get_bookmarks_on_revision(workspace_path, "@").expect("list bookmarks on revision");
    assert!(
      !on_head_after.iter().any(|name| name == "feat/round-trip"),
      "deleted bookmark should not be attached to @"
    );
  }

  #[test]
  fn bookmark_track_rejects_remote_that_does_not_exist() {
    let temp = TempDir::new().expect("tempdir");
    init_jj_repo(&temp);
    let workspace_path = temp.path().to_str().expect("utf8 path");
    jj_set_bookmark(workspace_path, "main", "@").expect("set main bookmark");

    let err = jj_bookmark_track(workspace_path, "main", "origin").expect_err("no origin remote");
    assert!(
      err.to_string().contains("Remote 'origin' not found"),
      "{err}"
    );
    assert!(
      !is_bookmark_tracked(workspace_path, "main", "origin").expect("tracked check"),
      "main@origin should stay untracked"
    );
  }

  #[test]
  fn reconcile_keeps_files_a_concurrent_snapshot_recorded_after_repo_load() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    let workspace_path = temp.path().to_str().expect("utf8 path");
    init_jj_for_git_repo(workspace_path).expect("init jj for git repo");
    jj_get_changed_files(workspace_path).expect("initial snapshot");

    // Load the repo the way reconcile does, before its working-copy lock.
    let settings = create_user_settings(workspace_path).expect("settings");
    let mut workspace = Workspace::load(
      &settings,
      temp.path(),
      &StoreFactories::default(),
      &default_working_copy_factories(),
    )
    .expect("load workspace");
    let repo_before_snapshot = block_on(workspace.repo_loader().load_at_head()).expect("load repo");

    // A concurrent snapshot moves the working copy past the loaded repo.
    fs::write(temp.path().join("new.txt"), "hello\n").expect("write file");
    let changed = jj_get_changed_files(workspace_path).expect("snapshot new file");
    assert!(changed.iter().any(|change| change.path == "new.txt"));

    reconcile_loaded_workspace(&mut workspace, &repo_before_snapshot).expect("reconcile");

    assert!(
      temp.path().join("new.txt").exists(),
      "reconcile must not check out the older WC commit over a newer snapshot"
    );
    let changed = jj_get_changed_files(workspace_path).expect("snapshot after reconcile");
    assert!(changed.iter().any(|change| change.path == "new.txt"));
  }

  #[test]
  fn jj_get_change_id_returns_short_reverse_hex_for_at_revision() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    let workspace_path = temp.path().to_str().expect("utf8 path");
    init_jj_for_git_repo(workspace_path).expect("init jj for git repo");

    let change_id = jj_get_change_id(workspace_path, "@").expect("resolve @");
    assert_eq!(change_id.len(), 12, "change id should be 12 chars");
    assert!(
      change_id.chars().all(|c| c.is_ascii_alphabetic()),
      "change id should be reverse hex, got {}",
      change_id
    );
  }

  #[test]
  fn jj_new_with_parents_checks_out_child_of_requested_parent() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    let workspace_path = temp.path().to_str().expect("utf8 path");
    init_jj_for_git_repo(workspace_path).expect("init jj for git repo");

    let parent = jj_get_commit_id(workspace_path, "@").expect("resolve @");
    let created =
      jj_new_with_parents(workspace_path, &["@".to_string()]).expect("create new commit");

    assert_eq!(
      jj_get_commit_id(workspace_path, "@").expect("resolve new @"),
      created,
      "new commit should become the working-copy commit"
    );
    assert_eq!(
      jj_get_commit_id(workspace_path, "@-").expect("resolve new @-"),
      parent,
      "previous working-copy commit should become the parent"
    );
  }

  #[test]
  fn jj_new_with_parents_rejects_empty_parent_list() {
    let temp = TempDir::new().expect("tempdir");
    let workspace_path = temp.path().to_str().expect("utf8 path");

    assert!(
      jj_new_with_parents(workspace_path, &[]).is_err(),
      "at least one parent revision should be required"
    );
  }

  #[test]
  fn jj_log_revset_commit_ids_returns_empty_for_none_revset() {
    let temp = TempDir::new().expect("tempdir");
    init_jj_repo(&temp);
    let workspace_path = temp.path().to_str().expect("utf8 path");
    let ids = jj_log_revset_commit_ids(workspace_path, "none()").expect("none() should resolve");
    assert!(ids.is_empty(), "none() should return no commits");
  }

  #[test]
  fn resolve_branch_revision_for_rebase_returns_current_tip() {
    let temp = TempDir::new().expect("tempdir");
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");

    let branch = "feat/conflict-rebase";
    jj_set_bookmark(repo_path, branch, "@").expect("set initial bookmark");
    let loaded = load_workspace_repo(repo_path).expect("load repo");
    let expected = resolve_commit_by_revision(&loaded, "@").expect("resolve wc tip");
    let resolved =
      resolve_branch_revision_for_rebase(repo_path, branch).expect("branch should resolve");
    let resolved_commit =
      resolve_commit_by_revision(&loaded, &resolved).expect("resolved branch should map to commit");
    assert_eq!(
      resolved_commit.id().hex(),
      expected.id().hex(),
      "helper should resolve to the bookmark tip commit"
    );
  }

  #[test]
  fn retarget_fails_when_old_target_bookmark_is_missing() {
    let temp = TempDir::new().expect("tempdir");
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");
    jj_set_bookmark(repo_path, "feat/target", "@").expect("set target bookmark");
    jj_set_bookmark(repo_path, "feat/ws", "@").expect("set workspace bookmark");

    let result =
      jj_retarget_workspace_bookmark(repo_path, "feat/ws", "feat/target", Some("feat/gone"));

    assert!(
      result.is_err(),
      "unresolvable old target must fail: {result:?}"
    );
  }

  #[test]
  fn prefers_remote_tip_when_conflicted_bookmark_trees_match_after_squash() {
    let local_id = "unsquashed".to_string();
    let remote_id = "squashed".to_string();
    let candidates = vec![
      (local_id, "same-tree".to_string()),
      (remote_id.clone(), "same-tree".to_string()),
    ];

    assert_eq!(
      select_equivalent_remote_tip(&candidates, Some(&remote_id)),
      Some(remote_id)
    );
  }

  #[test]
  fn keeps_normal_conflict_resolution_when_remote_tip_changes_content() {
    let local_id = "local".to_string();
    let remote_id = "remote".to_string();
    let candidates = vec![
      (local_id, "local-tree".to_string()),
      (remote_id.clone(), "remote-tree".to_string()),
    ];

    assert_eq!(
      select_equivalent_remote_tip(&candidates, Some(&remote_id)),
      None
    );
  }

  #[test]
  fn strip_mutable_revset_filter_removes_leading_intersection() {
    assert_eq!(
      strip_mutable_revset_filter("roots(mutable() & (main..feat) ~ @)"),
      "roots((main..feat) ~ @)"
    );
  }

  #[test]
  fn strip_mutable_revset_filter_removes_trailing_intersection() {
    assert_eq!(
      strip_mutable_revset_filter("(origin/main..@-) & mutable()"),
      "(origin/main..@-)"
    );
  }

  #[test]
  fn git_global_excludes_path_reads_repo_config_value() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    let excludes = temp.path().join("global_ignore");
    fs::write(&excludes, "*.tmp\n").expect("write excludes");

    let status = Command::new("git")
      .current_dir(temp.path())
      .args([
        "config",
        "core.excludesFile",
        excludes.to_str().expect("utf8 path"),
      ])
      .status()
      .expect("git config should run");
    assert!(status.success(), "git config should succeed");

    let actual = git_global_excludes_path(temp.path().to_str().expect("utf8 path"))
      .expect("excludes path should be set");
    assert_eq!(actual, excludes);
  }

  #[test]
  fn read_git_user_config_from_git_reads_repo_local_values() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    let repo_path = temp.path();

    let status = Command::new("git")
      .current_dir(repo_path)
      .args(["config", "user.name", "Local User"])
      .status()
      .expect("git config user.name");
    assert!(status.success());
    let status = Command::new("git")
      .current_dir(repo_path)
      .args(["config", "user.email", "local@example.com"])
      .status()
      .expect("git config user.email");
    assert!(status.success());

    let (name, email) = read_git_user_config_from_git(repo_path.to_str().expect("utf8 path"));
    assert_eq!(name, "Local User");
    assert_eq!(email, "local@example.com");
  }

  #[test]
  fn read_git_user_config_from_git_uses_defaults_for_missing_repo() {
    let temp = TempDir::new().expect("tempdir");
    let missing_repo = temp.path().join("missing");

    let (name, email) = read_git_user_config_from_git(missing_repo.to_str().expect("utf8 path"));
    assert_eq!(name, "Treq User");
    assert_eq!(email, "treq@localhost");
  }

  #[test]
  fn jj_get_file_lines_from_parent_reads_head_blob_content() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    let repo_path = temp.path();
    let file_name = "hello.txt";
    let file_path = repo_path.join(file_name);
    fs::write(&file_path, "one\ntwo\nthree\n").expect("write file");

    let add_status = Command::new("git")
      .current_dir(repo_path)
      .args(["add", file_name])
      .status()
      .expect("git add");
    assert!(add_status.success(), "git add should succeed");

    let commit_status = Command::new("git")
      .current_dir(repo_path)
      .args([
        "-c",
        "user.name=Test User",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-m",
        "add file",
      ])
      .status()
      .expect("git commit");
    assert!(commit_status.success(), "git commit should succeed");

    fs::write(&file_path, "changed\n").expect("overwrite file");

    let lines = jj_get_file_lines(
      repo_path.to_str().expect("utf8 path"),
      file_name,
      true,
      2,
      3,
    )
    .expect("read from parent");

    assert_eq!(lines.lines, vec!["two".to_string(), "three".to_string()]);
    assert_eq!(lines.start_line, 2);
    assert_eq!(lines.end_line, 3);
  }

  #[test]
  fn jj_get_file_lines_rejects_paths_outside_the_workspace() {
    let root = TempDir::new().expect("tempdir");
    let workspace = root.path().join("ws");
    fs::create_dir(&workspace).expect("mkdir");
    fs::write(root.path().join("secret.txt"), "secret\n").expect("write secret");
    let workspace = workspace.to_str().expect("utf8 path");
    let absolute = root.path().join("secret.txt");

    for path in [
      "../secret.txt",
      "a/../../secret.txt",
      absolute.to_str().unwrap(),
    ] {
      let err = jj_get_file_lines(workspace, path, false, 1, 10).unwrap_err();
      assert!(
        err.to_string().contains("outside the workspace"),
        "{path}: {err}"
      );
    }
  }

  #[test]
  fn jj_get_file_lines_returns_empty_for_a_reversed_range() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("f.txt"), "1\n2\n3\n4\n5\n6\n").expect("write");
    let lines =
      jj_get_file_lines(temp.path().to_str().unwrap(), "f.txt", false, 5, 2).expect("read");
    assert!(lines.lines.is_empty());
  }

  #[test]
  fn jj_git_init_colocated_creates_jj_and_git_dirs() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");

    jj_git_init_colocated(repo_path).expect("colocated init should succeed");

    assert!(temp.path().join(".jj").is_dir(), ".jj should exist");
    assert!(temp.path().join(".git").is_dir(), ".git should exist");
    assert!(
      jj_snapshot_working_copy(repo_path).is_ok(),
      "the resulting repo should be usable"
    );
  }

  #[test]
  fn jj_git_init_bare_creates_working_repo_without_git_dir() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().to_str().expect("utf8 path");

    jj_git_init_bare(repo_path).expect("jj_git_init_bare should succeed");

    assert!(temp.path().join(".jj").exists(), ".jj should exist");
    assert!(
      !temp.path().join(".git").exists(),
      "non-colocated init should not create .git"
    );
    assert!(
      jj_snapshot_working_copy(repo_path).is_ok(),
      "the resulting repo should be usable"
    );
  }

  #[test]
  fn jj_get_sparse_patterns_defaults_to_root_full_checkout() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    git(temp.path(), &["config", "user.name", "Test User"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(temp.path(), &["add", "base.txt"]);
    git(temp.path(), &["commit", "-m", "base"]);
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");

    // Full checkout isn't an empty pattern list — it's a single pattern for the
    // repo root, whose internal-path representation is the empty string.
    let patterns = jj_get_sparse_patterns(repo_path).expect("read sparse patterns");
    assert_eq!(
      patterns,
      vec!["".to_string()],
      "no sparse patterns set means a single root (full checkout) pattern, got: {:?}",
      patterns
    );
  }

  #[test]
  fn list_all_workspace_names_includes_default() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    git(temp.path(), &["config", "user.name", "Test User"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(temp.path(), &["add", "base.txt"]);
    git(temp.path(), &["commit", "-m", "base"]);
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");

    let names = list_all_workspace_names(repo_path).expect("list workspace names");
    assert_eq!(names, vec!["default".to_string()]);
  }

  #[test]
  fn jj_list_files_at_revision_lists_tree_contents() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    git(temp.path(), &["config", "user.name", "Test User"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    fs::create_dir_all(temp.path().join("dir")).expect("mkdir");
    fs::write(temp.path().join("dir/nested.txt"), "nested\n").expect("write nested");
    git(temp.path(), &["add", "base.txt", "dir/nested.txt"]);
    git(temp.path(), &["commit", "-m", "base"]);
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");

    let files = jj_list_files_at_revision(repo_path, "@-").expect("list files at @-");
    assert_eq!(
      files,
      vec!["base.txt".to_string(), "dir/nested.txt".to_string()]
    );
  }

  #[test]
  fn jj_log_revset_change_ids_returns_one_id_for_at_symbol() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    git(temp.path(), &["config", "user.name", "Test User"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(temp.path(), &["add", "base.txt"]);
    git(temp.path(), &["commit", "-m", "base"]);
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");

    let ids = jj_log_revset_change_ids(repo_path, "@").expect("log revset change ids");
    assert_eq!(ids.len(), 1);
    assert_eq!(
      ids[0].len(),
      12,
      "change id should be the 12-char short form"
    );
  }

  #[test]
  fn jj_log_entries_reports_description_and_emptiness() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    git(temp.path(), &["config", "user.name", "Test User"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(temp.path(), &["add", "base.txt"]);
    git(temp.path(), &["commit", "-m", "base"]);
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");

    jj_describe(repo_path, "@", "described wc").expect("describe wc");

    let entries = jj_log_entries(repo_path, "@", Some(1)).expect("log entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].description, "described wc");
    assert!(
      entries[0].is_empty,
      "wc has no file changes relative to its parent"
    );
  }

  #[test]
  fn jj_import_remaining_git_refs_makes_git_only_branch_resolvable() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    git(temp.path(), &["config", "user.name", "Test User"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(temp.path(), &["add", "base.txt"]);
    git(temp.path(), &["commit", "-m", "base"]);
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");

    // A branch created purely via git, never touched by jj, isn't yet a jj bookmark.
    git(temp.path(), &["branch", "git-only-branch"]);
    assert!(
      jj_get_commit_id(repo_path, "git-only-branch").is_err(),
      "branch should not resolve as a jj revision before import"
    );

    jj_import_remaining_git_refs(repo_path).expect("import remaining git refs");

    // `git branch` with no explicit target points at git HEAD, which is still the
    // "base" commit here — `jj git init --colocate` doesn't move git HEAD to jj's
    // new (empty) @ until something reconciles them, so compare against the git
    // commit the branch actually points at, not jj's own working-copy commit.
    let imported = jj_get_commit_id(repo_path, "git-only-branch")
      .expect("branch should resolve as a jj revision after import");
    assert_eq!(
      imported,
      jj_get_commit_id(repo_path, "master").expect("master id")
    );
  }

  #[test]
  fn move_paths_rejects_paths_outside_the_workspace() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    let outside = TempDir::new().expect("outside");
    fs::write(outside.path().join("keep.txt"), "keep").expect("write");
    fs::create_dir(source.path().join(".jj")).expect("mkdir .jj");
    fs::create_dir(target.path().join(".git")).expect("mkdir .git");
    let escape = format!(
      "../{}",
      outside.path().file_name().expect("name").to_string_lossy()
    );
    let absolute = outside.path().to_string_lossy().into_owned();

    for bad in [escape.as_str(), absolute.as_str(), ".jj", ".git", "", "."] {
      let result = move_paths_between_workspaces(source.path(), target.path(), &[bad.to_string()]);
      assert!(result.is_err(), "'{bad}' should be rejected");
    }
    assert!(outside.path().join("keep.txt").exists());
    assert!(source.path().join(".jj").is_dir());
    assert!(target.path().join(".git").is_dir());
  }

  #[test]
  fn move_paths_validates_every_path_before_touching_files() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    fs::write(source.path().join("a.txt"), "a").expect("write");

    let result = move_paths_between_workspaces(
      source.path(),
      target.path(),
      &["a.txt".to_string(), "../escape".to_string()],
    );

    assert!(result.is_err());
    assert!(source.path().join("a.txt").exists());
    assert!(!target.path().join("a.txt").exists());
  }

  #[test]
  fn move_paths_moves_files_and_directories() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    fs::create_dir_all(source.path().join("dir/nested")).expect("mkdir");
    fs::write(source.path().join("dir/nested/b.txt"), "b").expect("write");
    fs::write(source.path().join("a.txt"), "a").expect("write");
    fs::write(target.path().join("gone.txt"), "x").expect("write");

    move_paths_between_workspaces(
      source.path(),
      target.path(),
      &[
        "a.txt".to_string(),
        "dir".to_string(),
        "gone.txt".to_string(),
      ],
    )
    .expect("move");

    assert!(!source.path().join("a.txt").exists());
    assert!(!source.path().join("dir").exists());
    assert_eq!(
      fs::read_to_string(target.path().join("a.txt")).expect("a"),
      "a"
    );
    assert_eq!(
      fs::read_to_string(target.path().join("dir/nested/b.txt")).expect("b"),
      "b"
    );
    // Deleted in the source, so the move deletes it in the target too.
    assert!(!target.path().join("gone.txt").exists());
  }
}
