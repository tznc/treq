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

/// Serializes working-copy mutations for a single workspace, keyed by
/// `workspace_path`, without blocking unrelated workspaces on each other.
fn with_working_copy_lock<T>(workspace_path: &str, f: impl FnOnce() -> T) -> T {
  use std::collections::HashMap;
  use std::sync::{Arc, Mutex, OnceLock};

  static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
  let locks = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));

  let workspace_lock = {
    let mut map = locks
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner());
    map
      .entry(workspace_path.to_string())
      .or_insert_with(|| Arc::new(Mutex::new(())))
      .clone()
  };

  let _guard = workspace_lock
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
  f()
}

use gix::refs::transaction::{Change, PreviousValue, RefEdit};
use gix::refs::Target;
use imara_diff::intern::InternedInput;
use imara_diff::sink::Counter;
use imara_diff::{diff, Algorithm, UnifiedDiffBuilder};
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
use jj_lib::merge::{Diff, Merge};
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
use jj_lib::str_util::StringExpression;
use jj_lib::tree_merge::MergeOptions;
use jj_lib::working_copy::{SnapshotOptions, WorkingCopyFreshness};
use jj_lib::workspace::{default_working_copy_factories, default_working_copy_factory, Workspace};
use jj_lib::workspace_store::{SimpleWorkspaceStore, WorkspaceStore as _};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
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
      b".jj/\n.treq/\n.jj*/\nnode_modules/\n.agents/skills/treq*/\n.claude/skills/treq*/\n",
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
  let matcher = repo_root_matcher();
  let opts = snapshot_options_for_all_paths(ignore_root, &matcher);
  let (new_tree, _) = block_on(locked_ws.locked_wc().snapshot(&opts))
    .map_err(|e| JjError::IoError(format!("Failed to snapshot working copy: {e}")))?;
  if new_tree.tree_ids() != wc_commit.tree_ids() {
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
      let wc_commit = block_on(
        head_tx
          .repo_mut()
          .check_out(workspace_name.clone(), &head_commit),
      )
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
      let values = match entry.values {
        Ok(values) => values,
        Err(_) => continue,
      };
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
          let before = match String::from_utf8(before) {
            Ok(content) => content,
            Err(_) => continue,
          };
          let after = match String::from_utf8(after) {
            Ok(content) => content,
            Err(_) => continue,
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

  commits
    .into_iter()
    .map(|commit| {
      build_log_commit(
        cache_repo_path,
        repo,
        &commit,
        wc_commit_ids,
        wc_tree_override,
        is_immutable,
        &cached_map,
      )
    })
    .collect()
}

fn build_log_commit(
  cache_repo_path: &str,
  repo: &Arc<ReadonlyRepo>,
  commit: &jj_lib::commit::Commit,
  wc_commit_ids: &HashSet<jj_lib::backend::CommitId>,
  wc_tree_override: Option<(&jj_lib::backend::CommitId, &MergedTree)>,
  is_immutable: &impl Fn(
    &jj_lib::backend::CommitId,
  ) -> Result<bool, jj_lib::revset::RevsetEvaluationError>,
  cached_map: &std::collections::HashMap<String, local_db::CachedCommitDiffStat>,
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

      let _ = local_db::cache_commit_row(
        cache_repo_path,
        &full_commit_id,
        insertions,
        deletions,
        &change_id,
        &description,
        &author_name,
        &timestamp,
      );

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
}

/// A diff hunk from jj diff output
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JjDiffHunk {
  pub id: String,
  pub header: String,
  pub lines: Vec<String>,
  pub patch: String,
  #[serde(default)]
  pub conflict_style: ConflictStyle,
  #[serde(default)]
  pub conflict_regions: Vec<ConflictRegionView>,
}

/// File change status in JJ working copy
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JjFileChange {
  pub path: String,
  pub status: String,
  pub previous_path: Option<String>,
  pub changed_line_count: usize,
  pub diff_deferred: bool,
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
#[derive(Debug, Serialize, Deserialize, Clone)]
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
  /// True when this commit's tree still contains unresolved jj conflicts.
  #[serde(default)]
  pub has_conflicts: bool,
}

/// The full log response including metadata
#[derive(Debug, Serialize, Deserialize, Clone)]
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
#[derive(Debug, Serialize, Deserialize, Clone)]
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

/// Create UserSettings with reasonable defaults for Treq
/// Uses git config values if available, otherwise uses defaults
fn create_user_settings(repo_path: &str) -> Result<UserSettings, JjError> {
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

/// Ensure Treq's required repository metadata is in `.gitignore`.
/// This is idempotent - entries won't be duplicated.
pub fn ensure_gitignore_entries(repo_path: &str) -> Result<(), JjError> {
  append_gitignore_entries(
    &Path::new(repo_path).join(".gitignore"),
    &[".jj/", ".treq/"],
  )
}

/// Add broader generated paths after an explicit repository-level opt-in.
pub fn ensure_optional_gitignore_entries(repo_path: &str) -> Result<(), JjError> {
  append_gitignore_entries(
    &Path::new(repo_path).join(".gitignore"),
    &[".jj*/", ".agents/skills/treq*/", ".claude/skills/treq*/"],
  )
}

fn append_gitignore_entries(gitignore_path: &Path, entries_to_add: &[&str]) -> Result<(), JjError> {
  // Read existing .gitignore content
  let existing_content = if gitignore_path.exists() {
    fs::read_to_string(gitignore_path)
      .map_err(|e| JjError::InitFailed(format!("Failed to read .gitignore: {}", e)))?
  } else {
    String::new()
  };

  let existing_entries: std::collections::HashSet<&str> =
    existing_content.lines().map(|l| l.trim()).collect();

  // Find entries that need to be added
  let entries_needed: Vec<&str> = entries_to_add
    .iter()
    .filter(|entry| !existing_entries.contains(*entry))
    .copied()
    .collect();

  if entries_needed.is_empty() {
    return Ok(());
  }

  // Append missing entries to .gitignore
  let mut file = OpenOptions::new()
    .create(true)
    .append(true)
    .open(gitignore_path)
    .map_err(|e| JjError::InitFailed(format!("Failed to open .gitignore: {}", e)))?;

  // Add a newline before our entries if file doesn't end with newline
  if !existing_content.is_empty() && !existing_content.ends_with('\n') {
    writeln!(file)
      .map_err(|e| JjError::InitFailed(format!("Failed to write to .gitignore: {}", e)))?;
  }

  for entry in entries_needed {
    writeln!(file, "{}", entry)
      .map_err(|e| JjError::InitFailed(format!("Failed to write to .gitignore: {}", e)))?;
  }

  Ok(())
}

/// Initialize jj for an existing git repository (colocated mode)
/// This creates a .jj/ directory alongside the existing .git/ directory
fn init_jj_for_git_repo(repo_path: &str) -> Result<(), JjError> {
  let path = Path::new(repo_path);

  // Check if .jj already exists
  if is_jj_workspace(repo_path) {
    return Err(JjError::AlreadyInitialized);
  }

  // Check if .git exists
  if !path.join(".git").exists() {
    return Err(JjError::NotGitRepository);
  }

  let settings = create_user_settings(repo_path)?;

  // Use init_external_git to link jj to the existing .git repository.
  let git_repo_path = path.join(".git");

  block_on(Workspace::init_external_git(
    &settings,
    path,
    &git_repo_path,
  ))
  .map_err(|e| JjError::InitFailed(e.to_string()))?;
  // Ensure .gitignore entries
  ensure_gitignore_entries(repo_path)?;

  Ok(())
}

/// Initialize a fresh jj repo with its own internal (non-colocated) Git backend
/// at `.jj/repo/store/git`, equivalent to `jj git init` (no `--colocate`) on an
/// empty directory. Unlike [`ensure_jj_initialized`], this does not require an
/// existing `.git` directory and does not touch the treq database.
pub fn jj_git_init_bare(repo_path: &str) -> Result<(), JjError> {
  let settings = create_user_settings(repo_path)?;
  block_on(Workspace::init_internal_git(
    &settings,
    Path::new(repo_path),
  ))
  .map_err(|e| JjError::InitFailed(e.to_string()))?;
  Ok(())
}

/// Initialize a fresh colocated repo (`.jj` next to a new `.git`), equivalent
/// to `jj git init --colocate` on an empty directory, without running `jj`.
pub fn jj_git_init_colocated(repo_path: &str) -> Result<(), JjError> {
  let settings = create_user_settings(repo_path)?;
  block_on(Workspace::init_colocated_git(
    &settings,
    Path::new(repo_path),
  ))
  .map_err(|e| JjError::InitFailed(e.to_string()))?;
  Ok(())
}

/// Ensure jj is initialized for a repository
/// This is idempotent - safe to call multiple times
/// Returns true on success, false only if initialization failed
pub fn ensure_jj_initialized(db: &crate::db::Database, repo_path: &str) -> Result<bool, JjError> {
  // Check database flag first (avoid filesystem check if already configured)
  let flag_key = "jj_initialized";
  let already_configured = db
    .get_repo_setting(repo_path, flag_key)
    .ok()
    .flatten()
    .map(|v| v == "true")
    .unwrap_or(false);
  let ignore_generated_treq_paths = db
    .get_repo_setting(repo_path, "ignore_generated_treq_paths")
    .ok()
    .flatten()
    .is_some_and(|value| value == "true");

  if already_configured {
    if is_jj_workspace(repo_path) {
      ensure_gitignore_entries(repo_path)?;
      if ignore_generated_treq_paths {
        ensure_optional_gitignore_entries(repo_path)?;
      }
      return Ok(true); // Flag valid, .jj exists
    }
    // Flag stale — .jj was deleted. Clear flag, fall through to reinit
    let _ = db.set_repo_setting(repo_path, flag_key, "false");
  } else if is_jj_workspace(repo_path) {
    // Double-check filesystem in case flag got out of sync
    let _ = db.set_repo_setting(repo_path, flag_key, "true");
    ensure_gitignore_entries(repo_path)?;
    if ignore_generated_treq_paths {
      ensure_optional_gitignore_entries(repo_path)?;
    }
    return Ok(true);
  }

  // Check if it's actually a git repo before trying to initialize
  if !Path::new(repo_path).join(".git").exists() {
    return Err(JjError::NotGitRepository);
  }

  // Cache git user config so subsequent commits don't shell out
  let _ = cache_git_user_config(db, repo_path);

  // Initialize jj
  init_jj_for_git_repo(repo_path)?;
  if ignore_generated_treq_paths {
    ensure_optional_gitignore_entries(repo_path)?;
  }

  // Mark as configured in database
  db.set_repo_setting(repo_path, flag_key, "true")
    .map_err(|e| JjError::ConfigError(format!("Failed to save flag: {}", e)))?;

  Ok(true)
}

/// Sanitize workspace name for filesystem use
pub fn sanitize_workspace_name(name: &str) -> String {
  name
    .replace(['/', '\\'], "-")
    .replace(['*', '?', '<', '>', '|', '"', ':'], "_")
    .trim_matches('.')
    .trim()
    .to_string()
}

/// Read the working copy's current sparse checkout patterns (equivalent to
/// `jj sparse list`), as internal repo-path strings.
pub fn jj_get_sparse_patterns(workspace_path: &str) -> Result<Vec<String>, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let patterns = loaded
    .workspace
    .working_copy()
    .sparse_patterns()
    .map_err(|e| JjError::IoError(format!("Failed to read sparse patterns: {}", e)))?;
  Ok(
    patterns
      .iter()
      .map(|p| p.as_internal_file_string().to_string())
      .collect(),
  )
}

/// Create a colocated jj workspace
///
/// Parse and validate user-supplied sparse checkout patterns into repo paths.
/// Patterns are jj sparse prefix paths: `src` matches `src` and everything
/// under `src/`; a file path matches just that file.
fn parse_sparse_patterns(patterns: &[String]) -> Result<Vec<RepoPathBuf>, JjError> {
  let mut parsed: Vec<RepoPathBuf> = Vec::new();
  for raw in patterns {
    let trimmed = raw.trim();
    let normalized = trimmed
      .strip_prefix("./")
      .unwrap_or(trimmed)
      .trim_end_matches('/');
    if normalized.is_empty() {
      return Err(JjError::GitWorkspaceError(format!(
        "Invalid sparse pattern '{}': pattern is empty",
        raw
      )));
    }
    let repo_path = RepoPathBuf::from_relative_path(normalized).map_err(|e| {
      JjError::GitWorkspaceError(format!("Invalid sparse pattern '{}': {}", raw, e))
    })?;
    if !parsed.contains(&repo_path) {
      parsed.push(repo_path);
    }
  }
  Ok(parsed)
}

/// True when a registered workspace never got past `init_workspace_with_existing_repo`:
/// its working copy is still the empty commit on root and its directory holds only `.jj`
/// (plus the `.git` worktree file that jj >= 0.46 writes in colocated repos).
fn is_interrupted_workspace_init(
  repo_path: &str,
  name: &str,
  workspace_dir: &Path,
) -> Result<bool, JjError> {
  let only_jj = match fs::read_dir(workspace_dir) {
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
    entries => entries
      .and_then(|entries| entries.collect::<std::io::Result<Vec<_>>>())
      .map_err(|e| JjError::IoError(format!("Failed to read workspace dir: {}", e)))?
      .iter()
      .all(|entry| {
        entry.file_name() == ".jj" || (entry.file_name() == ".git" && entry.path().is_file())
      }),
  };
  if !only_jj {
    return Ok(false);
  }
  let repo = load_workspace_repo(repo_path)?.repo;
  let Some(wc_id) = repo
    .view()
    .wc_commit_ids()
    .iter()
    .find(|(wc_name, _)| wc_name.as_str() == name)
    .map(|(_, id)| id.clone())
  else {
    return Ok(false);
  };
  let wc = repo
    .store()
    .get_commit(&wc_id)
    .map_err(|e| JjError::IoError(e.to_string()))?;
  Ok(wc.parent_ids() == [repo.store().root_commit_id().clone()] && is_empty_commit(&repo, &wc))
}

/// This creates:
/// 1. A git workspace at the specified path
/// 2. A jj workspace initialized on top of it
///
/// Returns the workspace path on success
pub fn create_workspace(
  repo_path: &str,
  workspace_name: &str,
  branch_name: &str,
  new_branch: bool,
  source_branch: Option<&str>,
  target_branch: Option<&str>,
  sparse_patterns: Option<&[String]>,
) -> Result<String, JjError> {
  let repo_path_buf = Path::new(repo_path);

  // Validate before any side effect; None and empty both mean a full checkout.
  let parsed_sparse_patterns: Option<Vec<RepoPathBuf>> = match sparse_patterns {
    Some(patterns) if !patterns.is_empty() => Some(parse_sparse_patterns(patterns)?),
    _ => None,
  };

  if !is_jj_workspace(repo_path) {
    return Err(JjError::NotGitRepository);
  }

  let sanitized_name = sanitize_workspace_name(workspace_name);
  let workspace_dir = repo_path_buf
    .join(".treq")
    .join("workspaces")
    .join(&sanitized_name);

  // Never tear down a registered workspace. A retry may arrive after the JJ
  // transaction committed but before the caller persisted its database row.
  // Treat JJ's workspace registry as authoritative in that window.
  if list_jj_workspaces(repo_path)?
    .iter()
    .any(|name| name == &sanitized_name)
  {
    if !is_interrupted_workspace_init(repo_path, &sanitized_name, &workspace_dir)? {
      return Err(JjError::GitWorkspaceError(format!(
        "Workspace '{}' is already registered; refusing to recreate it",
        branch_name
      )));
    }
    // A create killed between registering the workspace and its own transaction.
    forget_workspace(repo_path, &workspace_dir.to_string_lossy())?;
  }

  // A .jj directory which is not present in JJ's registry is demonstrably a
  // partial initialization and is safe to clean up before retrying.
  if workspace_dir.join(".jj").exists() {
    let workspace_dir_str = workspace_dir.to_string_lossy();
    remove_workspace_directory_only(&workspace_dir_str).map_err(|e| {
      JjError::GitWorkspaceError(format!("Failed to remove orphaned workspace dir: {}", e))
    })?;
  }

  let settings = create_user_settings(repo_path)?;

  // Import colocated git refs (HEAD + branches/remotes) so external commits are visible for parent resolution.
  let _ = jj_util_import_git_refs(repo_path);

  // Load parent workspace and repo (re-loaded after the git import so it sees fresh refs).
  let parent_workspace = Workspace::load(
    &settings,
    repo_path_buf,
    &StoreFactories::default(),
    &default_working_copy_factories(),
  )
  .map_err(|e| JjError::GitWorkspaceError(format!("Failed to load parent workspace: {}", e)))?;

  let parent_repo = block_on(parent_workspace.repo_loader().load_at_head())
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to load repo: {}", e)))?;
  let git_head_branch = read_git_head_branch(repo_path)
    .ok()
    .filter(|branch| !branch.is_empty() && branch != "HEAD");

  // Mirror fetch-style bookmark tracking to avoid untracked remote push failures.
  let parent_repo = {
    let mut import_tx = parent_repo.start_transaction();

    let git_head_id = import_tx
      .repo()
      .view()
      .git_head()
      .added_ids()
      .next()
      .cloned();
    if let (Some(branch_name), Some(head_id)) = (git_head_branch.as_ref(), git_head_id) {
      let existing = import_tx
        .repo()
        .view()
        .get_local_bookmark(RefName::new(branch_name));
      if existing.is_absent() {
        import_tx
          .repo_mut()
          .set_local_bookmark_target(RefName::new(branch_name), RefTarget::normal(head_id));
      }
    }

    // Collect (name, remote) pairs that have a local counterpart but are untracked.
    let to_track: Vec<(String, String)> = import_tx
      .repo()
      .view()
      .all_remote_bookmarks()
      .filter_map(|(sym, remote_ref)| {
        if remote_ref.is_tracked() {
          return None;
        }
        let local = import_tx
          .repo()
          .view()
          .get_local_bookmark(sym.name.as_ref());
        if local.is_absent() {
          return None;
        }
        Some((
          sym.name.as_str().to_string(),
          sym.remote.as_str().to_string(),
        ))
      })
      .collect();
    for (name, remote) in to_track {
      let sym = RemoteRefSymbol {
        name: RefName::new(&name),
        remote: RemoteName::new(&remote),
      };
      let _ = import_tx.repo_mut().track_remote_bookmark(sym);
    }

    if import_tx.repo().has_changes() {
      block_on(import_tx.commit("mirror bookmark tracking")).map_err(|e| {
        JjError::GitWorkspaceError(format!("Failed to mirror bookmark tracking: {}", e))
      })?
    } else {
      parent_repo
    }
  };

  // Collect remote names from view (avoids subprocess for branch format conversion)
  let remote_names: HashSet<String> = parent_repo
    .view()
    .all_remote_bookmarks()
    .map(|(sym, _)| sym.remote.as_str().to_string())
    .collect();

  // Resolve the source commit to use as parent of the new wc commit
  let source_commit_id =
    if !new_branch {
      // Existing local bookmark: use the bookmark's current commit
      let target = parent_repo
        .view()
        .get_local_bookmark(RefName::new(branch_name));
      target.added_ids().next().cloned().ok_or_else(|| {
        JjError::GitWorkspaceError(format!("Bookmark '{}' not found", branch_name))
      })?
    } else if let Some(source) = source_branch {
      // Convert git-style prefix (origin/branch) to jj style (branch@origin)
      let jj_ref = if let Some(slash) = source.find('/') {
        let prefix = &source[..slash];
        let suffix = &source[slash + 1..];
        if remote_names.contains(prefix) {
          format!("{}@{}", suffix, prefix)
        } else {
          source.to_string()
        }
      } else {
        source.to_string()
      };

      if let Some((name, remote)) = jj_ref.split_once('@') {
        let sym = RemoteRefSymbol {
          name: RefName::new(name),
          remote: RemoteName::new(remote),
        };
        parent_repo
          .view()
          .get_remote_bookmark(sym)
          .target
          .added_ids()
          .next()
          .cloned()
          .ok_or_else(|| {
            JjError::GitWorkspaceError(format!("Remote bookmark '{}' not found", jj_ref))
          })?
      } else {
        // Prefer the WC commit whose parent is the bookmark so stacked workspaces stay linear (main → bookmark → wc → child).
        let bookmark_id = parent_repo
          .view()
          .get_local_bookmark(RefName::new(&jj_ref))
          .added_ids()
          .next()
          .cloned()
          .ok_or_else(|| JjError::GitWorkspaceError(format!("Bookmark '{}' not found", jj_ref)))?;
        let wc_override = parent_repo
          .view()
          .wc_commit_ids()
          .iter()
          // The home repo's working copy holds the user's uncommitted changes; never base a new workspace on it.
          .filter(|(name, _)| name.as_str() != "default")
          .map(|(_, wc_id)| wc_id)
          .find(|wc_id| {
            if *wc_id == &bookmark_id {
              return false;
            }
            parent_repo
              .store()
              .get_commit(wc_id)
              .map(|wc| wc.parent_ids() == [bookmark_id.clone()])
              .unwrap_or(false)
          })
          .cloned();
        wc_override.unwrap_or(bookmark_id)
      }
    } else {
      // Parent on target branch bookmark tip (post import_colocated_git_state); fall back to git HEAD if absent.
      let resolved_target = target_branch
        .map(|b| b.to_string())
        .or_else(|| get_default_branch(repo_path).ok())
        .unwrap_or_else(|| "main".to_string());
      // Read the live git branch ref first — it reflects external branch moves before jj bookmarks catch up.
      let git_branch_id: Option<jj_lib::backend::CommitId> = (|| {
        let git_repo = gix::open(repo_path).ok()?;
        let ref_name: gix::refs::FullName =
          format!("refs/heads/{resolved_target}").try_into().ok()?;
        let reference = git_repo.find_reference(&ref_name).ok()?;
        Some(jj_lib::backend::CommitId::from_bytes(
          reference.id().as_bytes(),
        ))
      })();
      git_branch_id
        .or_else(|| {
          parent_repo
            .view()
            .get_local_bookmark(RefName::new(&resolved_target))
            .added_ids()
            .next()
            .cloned()
        })
        .or_else(|| parent_repo.view().git_head().added_ids().next().cloned())
        .ok_or_else(|| {
          JjError::GitWorkspaceError(format!(
            "Bookmark '{}' not found and no git HEAD",
            resolved_target
          ))
        })?
    };

  // init_workspace_with_existing_repo needs the dir; made after source resolution so errors leave none.
  if !workspace_dir.exists() {
    fs::create_dir_all(&workspace_dir)
      .map_err(|e| JjError::GitWorkspaceError(format!("Failed to create workspace dir: {}", e)))?;
  }

  let new_ws_name: WorkspaceNameBuf = sanitized_name.clone().into();

  // Initialize the new workspace (creates .jj, registers workspace, checks out root commit)
  let wc_factory = default_working_copy_factory();
  let (mut new_workspace, new_repo) = block_on(Workspace::init_workspace_with_existing_repo(
    &workspace_dir,
    parent_workspace.repo_path(),
    &parent_repo,
    &*wc_factory,
    new_ws_name.clone(),
  ))
  .map_err(|e| JjError::GitWorkspaceError(format!("Failed to init workspace: {}", e)))?;

  // Set sparse patterns while the wc is still empty so check_out below only materializes matching paths (mirrors `jj workspace add --sparse-patterns`).
  if let Some(patterns) = parsed_sparse_patterns {
    let mut locked_ws = new_workspace
      .start_working_copy_mutation()
      .map_err(|e| JjError::GitWorkspaceError(format!("Failed to lock working copy: {}", e)))?;
    block_on(locked_ws.locked_wc().set_sparse_patterns(patterns))
      .map_err(|e| JjError::GitWorkspaceError(format!("Failed to set sparse patterns: {}", e)))?;
    let op_id = locked_ws.locked_wc().old_operation_id().clone();
    block_on(locked_ws.finish(op_id)).map_err(|e| {
      JjError::GitWorkspaceError(format!("Failed to finish sparse patterns mutation: {}", e))
    })?;
  }

  // Start a transaction on the new repo to move wc to the desired source commit
  let source_commit = new_repo
    .store()
    .get_commit(&source_commit_id)
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to get source commit: {}", e)))?;

  let mut tx = new_repo.start_transaction();

  let parent_tree = block_on(merge_commit_trees(
    tx.repo(),
    std::slice::from_ref(&source_commit),
  ))
  .map_err(|e| JjError::GitWorkspaceError(format!("Failed to merge trees: {}", e)))?;

  // Create new wc commit on top of source_commit (empty, inherits source tree)
  let new_wc = block_on(
    tx.repo_mut()
      .new_commit(vec![source_commit_id.clone()], parent_tree)
      .write(),
  )
  .map_err(|e| JjError::GitWorkspaceError(format!("Failed to create wc commit: {}", e)))?;

  // Point the new workspace's @ to the new wc commit
  block_on(tx.repo_mut().edit(new_ws_name.clone(), &new_wc))
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to set wc: {}", e)))?;

  // Set the local bookmark to the new wc commit
  tx.repo_mut().set_local_bookmark_target(
    RefName::new(branch_name),
    RefTarget::normal(new_wc.id().clone()),
  );

  // Track branch@origin if the remote bookmark exists and is not yet tracked
  {
    let sym = RemoteRefSymbol {
      name: RefName::new(branch_name),
      remote: RemoteName::new("origin"),
    };
    let remote_ref: RemoteRef = tx.repo().view().get_remote_bookmark(sym).clone();
    if !remote_ref.target.is_absent() && !remote_ref.is_tracked() {
      let sym = RemoteRefSymbol {
        name: RefName::new(branch_name),
        remote: RemoteName::new("origin"),
      };
      let _ = tx.repo_mut().track_remote_bookmark(sym);
    }
  }

  // Rebase any descendants of rewritten commits (required before tx.commit)
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to rebase descendants: {}", e)))?;

  // Export to git refs (colocated repo)
  let _ = git::export_refs(tx.repo_mut());

  let _committed_repo = block_on(tx.commit("create_workspace"))
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to commit transaction: {}", e)))?;

  let verified_repo =
    verify_created_workspace_bookmark(&new_workspace, branch_name, &new_ws_name, new_wc.id())?;

  // Update the physical working copy to match the new wc commit
  block_on(new_workspace.check_out(verified_repo.op_id().clone(), None, &new_wc))
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to checkout workspace: {}", e)))?;

  Ok(sanitized_name)
}

/// Create a short-lived resolve workspace checked out in *edit* mode on a
/// conflicted commit (so `@` *is* that commit). No bookmark is created.
///
/// Layout: `{repo}/.treq/resolve/{workspace_slug}/{change_id}/`
///
/// Returns `(jj_workspace_name, absolute_workspace_path)`.
pub fn create_resolve_workspace(
  repo_path: &str,
  revision: &str,
  workspace_slug: &str,
) -> Result<(String, String), JjError> {
  if !is_jj_workspace(repo_path) {
    return Err(JjError::NotGitRepository);
  }

  let loaded_home = load_workspace_repo(repo_path)?;
  let target_commit = resolve_commit_by_revision(&loaded_home, revision)?;
  if !target_commit.tree().has_conflict() {
    return Err(JjError::IoError(format!(
      "Revision '{}' has no unresolved conflicts",
      revision
    )));
  }
  let target_commit_id = target_commit.id().clone();
  let change_id = HexPrefix::from_id(target_commit.change_id()).reverse_hex();
  let short_change = &change_id[..12.min(change_id.len())];
  let conflict_paths: Vec<String> = collect_conflict_paths_from_tree(&target_commit.tree());
  drop(loaded_home);

  let slug = sanitize_workspace_name(workspace_slug);
  let slug = if slug.is_empty() {
    "home".to_string()
  } else {
    slug
  };
  // jj workspace names must stay unique and discoverable as resolve sandboxes.
  let mut jj_name = sanitize_workspace_name(&format!("_resolve-{slug}-{short_change}"));
  if jj_name.len() > 80 {
    jj_name = format!("_resolve-{short_change}");
  }
  let workspace_dir = Path::new(repo_path)
    .join(".treq")
    .join("resolve")
    .join(&slug)
    .join(short_change);

  if workspace_dir.exists() && workspace_dir.join(".jj").exists() {
    // Reuse an existing resolve workspace for this change.
    let full = workspace_dir.to_string_lossy().to_string();
    return Ok((jj_name, full));
  }

  if workspace_dir.join(".jj").exists() {
    let workspace_dir_str = workspace_dir.to_string_lossy();
    remove_workspace_directory_only(&workspace_dir_str).map_err(|e| {
      JjError::GitWorkspaceError(format!("Failed to remove orphaned resolve dir: {}", e))
    })?;
  }
  if !workspace_dir.exists() {
    fs::create_dir_all(&workspace_dir).map_err(|e| {
      JjError::GitWorkspaceError(format!("Failed to create resolve workspace dir: {}", e))
    })?;
  }

  let settings = create_user_settings(repo_path)?;
  let parent_workspace = Workspace::load(
    &settings,
    Path::new(repo_path),
    &StoreFactories::default(),
    &default_working_copy_factories(),
  )
  .map_err(|e| JjError::GitWorkspaceError(format!("Failed to load parent workspace: {}", e)))?;
  let parent_repo = block_on(parent_workspace.repo_loader().load_at_head())
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to load repo: {}", e)))?;

  let new_ws_name: WorkspaceNameBuf = jj_name.clone().into();
  let wc_factory = default_working_copy_factory();
  let (mut new_workspace, new_repo) = block_on(Workspace::init_workspace_with_existing_repo(
    &workspace_dir,
    parent_workspace.repo_path(),
    &parent_repo,
    &*wc_factory,
    new_ws_name.clone(),
  ))
  .map_err(|e| JjError::GitWorkspaceError(format!("Failed to init resolve workspace: {}", e)))?;

  // Re-fetch from the new repo's store to keep MergedTree store pointers aligned.
  let target_commit = new_repo
    .store()
    .get_commit(&target_commit_id)
    .map_err(|e| {
      JjError::GitWorkspaceError(format!(
        "Failed to load conflicted commit in resolve workspace: {}",
        e
      ))
    })?;

  // Sparse-checkout only conflicted paths when possible.
  if !conflict_paths.is_empty() {
    if let Ok(patterns) = parse_sparse_patterns(&conflict_paths) {
      let mut locked_ws = new_workspace.start_working_copy_mutation().map_err(|e| {
        JjError::GitWorkspaceError(format!("Failed to lock resolve working copy: {}", e))
      })?;
      block_on(locked_ws.locked_wc().set_sparse_patterns(patterns)).map_err(|e| {
        JjError::GitWorkspaceError(format!("Failed to set resolve sparse patterns: {}", e))
      })?;
      let op_id = locked_ws.locked_wc().old_operation_id().clone();
      block_on(locked_ws.finish(op_id)).map_err(|e| {
        JjError::GitWorkspaceError(format!("Failed to finish resolve sparse patterns: {}", e))
      })?;
    }
  }

  let mut tx = new_repo.start_transaction();
  // Edit the conflicted commit directly — no child WC commit, no bookmark.
  block_on(tx.repo_mut().edit(new_ws_name.clone(), &target_commit)).map_err(|e| {
    JjError::GitWorkspaceError(format!(
      "Failed to edit conflicted commit in resolve WC: {}",
      e
    ))
  })?;
  block_on(tx.repo_mut().rebase_descendants()).map_err(|e| {
    JjError::GitWorkspaceError(format!(
      "Failed to rebase descendants after resolve edit: {}",
      e
    ))
  })?;
  let _ = git::export_refs(tx.repo_mut());
  let committed = block_on(tx.commit("create_resolve_workspace")).map_err(|e| {
    JjError::GitWorkspaceError(format!("Failed to commit resolve workspace tx: {}", e))
  })?;

  block_on(new_workspace.check_out(committed.op_id().clone(), None, &target_commit)).map_err(
    |e| JjError::GitWorkspaceError(format!("Failed to checkout resolve workspace: {}", e)),
  )?;

  Ok((jj_name, workspace_dir.to_string_lossy().to_string()))
}

/// Snapshot the resolve working copy (rewriting the edited conflicted commit in place).
pub fn jj_snapshot_resolve_workspace(workspace_path: &str) -> Result<(), JjError> {
  if !Path::new(workspace_path).exists() {
    return Ok(());
  }
  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  snapshot_loaded_working_copy(&mut loaded, workspace_path)?;
  Ok(())
}

/// Applies a unified diff that changes only `file_path` to that file in the
/// workspace working copy, in-process (no `git apply`), then snapshots the
/// working copy so jj records the edit.
pub fn jj_apply_file_patch(
  workspace_path: &str,
  file_path: &str,
  patch: &str,
) -> Result<(), JjError> {
  let invalid = |message: String| JjError::IoError(format!("Invalid patch: {message}"));
  let repo_path = RepoPathBuf::from_relative_path(file_path)
    .map_err(|e| invalid(format!("path '{file_path}': {e}")))?;
  if repo_path.is_root()
    || matches!(
      repo_path.components().next().map(|c| c.as_internal_str()),
      Some(".jj" | ".git")
    )
  {
    return Err(invalid(format!("'{file_path}' is not a workspace file")));
  }
  let patches = crate::unified_patch::parse(patch).map_err(invalid)?;
  let [file_patch] = patches.as_slice() else {
    return Err(invalid(format!(
      "it changes {} files; expected only '{file_path}'",
      patches.len()
    )));
  };
  let touches_only_target = [&file_patch.old_path, &file_patch.new_path]
    .into_iter()
    .flatten()
    .all(|path| path == repo_path.as_internal_file_string());
  if !touches_only_target {
    return Err(invalid(format!(
      "it changes '{}', not '{file_path}'",
      file_patch.target_path().unwrap_or_default()
    )));
  }
  let fs_path = repo_path
    .to_fs_path(Path::new(workspace_path))
    .map_err(|e| invalid(format!("path '{file_path}': {e}")))?;

  let original = match fs::read_to_string(&fs_path) {
    Ok(content) => Some(content),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
    Err(e) => {
      return Err(JjError::IoError(format!(
        "Failed to read '{file_path}': {e}"
      )))
    }
  };
  match crate::unified_patch::apply(original.as_deref(), file_patch).map_err(invalid)? {
    Some(content) => {
      if let Some(parent) = fs_path.parent() {
        fs::create_dir_all(parent)
          .map_err(|e| JjError::IoError(format!("Failed to create '{}': {e}", parent.display())))?;
      }
      fs::write(&fs_path, content)
        .map_err(|e| JjError::IoError(format!("Failed to write '{file_path}': {e}")))?;
    }
    None => fs::remove_file(&fs_path)
      .map_err(|e| JjError::IoError(format!("Failed to delete '{file_path}': {e}")))?,
  }

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  snapshot_loaded_working_copy(&mut loaded, workspace_path)
}

/// Move a resolve workspace `@` to a new empty child of the current tip so
/// forgetting the workspace cannot hide the resolved change.
// home-head-test: resolve_workspace_conflict
pub fn jj_detach_resolve_workspace_wc(workspace_path: &str) -> Result<(), JjError> {
  if !Path::new(workspace_path).exists() {
    return Ok(());
  }
  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  snapshot_loaded_working_copy(&mut loaded, workspace_path)?;
  let ws_name = loaded.workspace.workspace_name().to_owned();
  let wc_commit = match get_workspace_wc_commit(&loaded)? {
    Some(commit) => commit,
    None => return Ok(()),
  };
  let tree = wc_commit.tree();
  let mut tx = loaded.repo.start_transaction();
  let detached = block_on(
    tx.repo_mut()
      .new_commit(vec![wc_commit.id().clone()], tree)
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to create detach WC for resolve: {}", e)))?;
  block_on(tx.repo_mut().edit(ws_name, &detached))
    .map_err(|e| JjError::IoError(format!("Failed to detach resolve WC: {}", e)))?;
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase after resolve detach: {}", e)))?;
  let new_repo = block_on(tx.commit("detach resolve workspace"))
    .map_err(|e| JjError::IoError(format!("Failed to commit resolve detach: {}", e)))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    Some(&wc_commit),
    CheckoutMode::Immediate,
  )?;
  Ok(())
}

/// Resolve conflicted paths in an edit-mode workspace by taking conflict sides.
///
/// Side indexes: 1 = left/add0, 2 = right/add1, 0 = base/remove0.
/// Multiple sides concatenate textual content in the given order ("both").
// home-head-test: resolve_workspace_conflict
pub fn jj_resolve_conflict_sides(workspace_path: &str, sides: &[u8]) -> Result<(), JjError> {
  if sides.is_empty() {
    return Err(JjError::IoError(
      "At least one conflict side is required".to_string(),
    ));
  }

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  snapshot_loaded_working_copy(&mut loaded, workspace_path)?;
  let ws_name = loaded.workspace.workspace_name().to_owned();
  let wc_commit = get_workspace_wc_commit(&loaded)?
    .ok_or_else(|| JjError::IoError("Resolve workspace has no working-copy commit".to_string()))?;
  let tree = wc_commit.tree();
  if !tree.has_conflict() {
    return Ok(());
  }

  if sides.len() == 1 {
    let side = sides[0];
    let mut builder = MergedTreeBuilder::new(tree.clone());
    for (path, value_res) in tree.conflicts() {
      let conflict = value_res.map_err(|e| {
        JjError::IoError(format!(
          "Failed to read conflict at '{}': {}",
          path.as_internal_file_string(),
          e
        ))
      })?;
      let chosen = match side {
        0 => conflict.get_remove(0).cloned().flatten(),
        n => conflict
          .get_add((n as usize).saturating_sub(1))
          .cloned()
          .flatten(),
      };
      use jj_lib::merge::Merge;
      builder.set_or_remove(
        path,
        match chosen {
          Some(tv) => Merge::normal(tv),
          None => Merge::absent(),
        },
      );
    }
    let resolved_tree = block_on(builder.write_tree())
      .map_err(|e| JjError::IoError(format!("Failed to write resolved tree: {}", e)))?;
    let mut tx = loaded.repo.start_transaction();
    let rewritten = block_on(
      tx.repo_mut()
        .rewrite_commit(&wc_commit)
        .set_tree(resolved_tree)
        .write(),
    )
    .map_err(|e| JjError::IoError(format!("Failed to rewrite resolved commit: {}", e)))?;
    block_on(tx.repo_mut().edit(ws_name, &rewritten))
      .map_err(|e| JjError::IoError(format!("Failed to edit after resolve: {}", e)))?;
    block_on(tx.repo_mut().rebase_descendants())
      .map_err(|e| JjError::IoError(format!("Failed to rebase after resolve: {}", e)))?;
    let new_repo = block_on(tx.commit("resolve conflict sides"))
      .map_err(|e| JjError::IoError(format!("Failed to commit resolve: {}", e)))?;
    update_workspace_after_history_edit(
      &mut loaded,
      &new_repo,
      Some(&wc_commit),
      CheckoutMode::Immediate,
    )?;
    return Ok(());
  }

  // Multi-side: materialize markers, pick textual sides, write files, snapshot.
  let marker_style = ConflictMarkerStyle::Git;
  let merge_options = MergeOptions::from_settings(&loaded.settings)
    .map_err(|e| JjError::IoError(format!("Failed to load merge options: {}", e)))?;
  let labels = ConflictLabels::unlabeled();
  for (path, value_res) in tree.conflicts() {
    let conflict = value_res.map_err(|e| {
      JjError::IoError(format!(
        "Failed to read conflict at '{}': {}",
        path.as_internal_file_string(),
        e
      ))
    })?;
    let materialized = block_on(jj_lib::conflicts::materialize_tree_value(
      loaded.repo.store().as_ref(),
      path.as_ref(),
      conflict,
      &labels,
    ))
    .map_err(|e| {
      JjError::IoError(format!(
        "Failed to materialize conflict at '{}': {}",
        path.as_internal_file_string(),
        e
      ))
    })?;
    let Some(bytes) = block_on(materialized_value_to_bytes(
      path.as_ref(),
      materialized,
      marker_style,
      Some(&merge_options),
    )) else {
      continue;
    };
    let text = String::from_utf8_lossy(&bytes);
    let regions =
      crate::conflict_markers::parse_conflict_markers(&text, path.as_internal_file_string());
    let resolved_text = if regions.is_empty() {
      text.into_owned()
    } else {
      apply_sides_to_conflict_text(&text, &regions, sides)
    };
    let abs = Path::new(workspace_path).join(path.as_internal_file_string());
    if let Some(parent) = abs.parent() {
      let _ = fs::create_dir_all(parent);
    }
    fs::write(&abs, resolved_text.as_bytes()).map_err(|e| {
      JjError::IoError(format!(
        "Failed to write resolved '{}': {}",
        path.as_internal_file_string(),
        e
      ))
    })?;
  }
  snapshot_loaded_working_copy(&mut loaded, workspace_path)?;
  Ok(())
}

fn apply_sides_to_conflict_text(
  content: &str,
  regions: &[crate::conflict_markers::ConflictRegionView],
  sides: &[u8],
) -> String {
  let lines: Vec<&str> = content.split('\n').collect();
  let mut out = String::new();
  let mut cursor = 0usize;

  for region in regions {
    let start = region.start_line.saturating_sub(1);
    let end = region.end_line.min(lines.len());
    if start > cursor {
      out.push_str(&lines[cursor..start].join("\n"));
      if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
      }
    }
    let mut pieces = Vec::new();
    for side in sides {
      let indexes = match side {
        0 => &region.comparison.base_line_indexes,
        1 => &region.comparison.left_line_indexes,
        _ => &region.comparison.right_line_indexes,
      };
      let mut chunk = String::new();
      for &idx in indexes {
        if let Some(line) = region.lines.get(idx) {
          if !chunk.is_empty() {
            chunk.push('\n');
          }
          chunk.push_str(&line.raw);
        }
      }
      if !chunk.is_empty() {
        pieces.push(chunk);
      }
    }
    let joined = pieces.join("\n");
    if !joined.is_empty() {
      out.push_str(&joined);
      out.push('\n');
    }
    cursor = end;
  }
  if cursor < lines.len() {
    out.push_str(&lines[cursor..].join("\n"));
  }
  out
}

/// Write path→content replacements into a resolve workspace and snapshot.
pub fn jj_apply_resolve_replacements(
  workspace_path: &str,
  files: &std::collections::HashMap<String, String>,
) -> Result<(), JjError> {
  for (rel, content) in files {
    let abs = Path::new(workspace_path).join(rel);
    if let Some(parent) = abs.parent() {
      fs::create_dir_all(parent)
        .map_err(|e| JjError::IoError(format!("Failed to create parent for '{}': {}", rel, e)))?;
    }
    fs::write(&abs, content.as_bytes())
      .map_err(|e| JjError::IoError(format!("Failed to write '{}': {}", rel, e)))?;
  }
  jj_snapshot_resolve_workspace(workspace_path)
}

/// Reload the operation head after creation. JJ reconciles concurrent operation
/// heads during this load, so verification must use this repo rather than the
/// transaction's returned snapshot.
fn verify_created_workspace_bookmark(
  workspace: &Workspace,
  branch_name: &str,
  workspace_name: &WorkspaceNameBuf,
  expected_wc_id: &jj_lib::backend::CommitId,
) -> Result<Arc<ReadonlyRepo>, JjError> {
  let latest = block_on(workspace.repo_loader().load_at_head()).map_err(|e| {
    JjError::GitWorkspaceError(format!(
      "Failed to reload repository after workspace creation: {e}"
    ))
  })?;
  let registered_wc = latest
    .view()
    .get_wc_commit_id(workspace_name)
    .ok_or_else(|| {
      JjError::GitWorkspaceError(format!(
        "Workspace '{}' is no longer registered after creation; manual recovery is required",
        branch_name
      ))
    })?;
  if registered_wc != expected_wc_id {
    return Err(JjError::GitWorkspaceError(format!(
      "Workspace '{}' changed during creation; manual recovery is required",
      branch_name
    )));
  }

  let target = latest.view().get_local_bookmark(RefName::new(branch_name));
  if target.as_normal() == Some(registered_wc) {
    return Ok(latest);
  }

  if target.has_conflict() && target.added_ids().any(|id| id == registered_wc) {
    let mut tx = latest.start_transaction();
    tx.repo_mut().set_local_bookmark_target(
      RefName::new(branch_name),
      RefTarget::normal(registered_wc.clone()),
    );
    git::export_refs(tx.repo_mut()).map_err(|e| {
      JjError::GitWorkspaceError(format!(
        "Failed to export recovered bookmark '{}': {e}",
        branch_name
      ))
    })?;
    let resolved =
      block_on(tx.commit("resolve workspace creation bookmark conflict")).map_err(|e| {
        JjError::GitWorkspaceError(format!(
          "Failed to resolve bookmark '{}' after workspace creation: {e}",
          branch_name
        ))
      })?;
    let resolved_target = resolved
      .view()
      .get_local_bookmark(RefName::new(branch_name));
    if resolved_target.as_normal() == Some(registered_wc) {
      return Ok(resolved);
    }
  }

  Err(JjError::GitWorkspaceError(format!(
        "Bookmark '{}' is ambiguous after workspace creation and cannot be resolved without choosing between unrelated changes; manual recovery is required",
        branch_name
    )))
}

/// Result of a workspace recovery operation
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceRecoveryResult {
  pub workspace_name: String,
  pub branch_name: String,
  pub success: bool,
  pub message: String,
}

#[derive(Debug, Clone)]
struct RegisteredWorkspace {
  workspace_name: String,
  workspace_path: String,
  branch_name: String,
  has_conflicts: bool,
  last_activity_at: Option<String>,
}

fn load_home_workspace(repo_path: &str) -> Result<Workspace, JjError> {
  let settings = create_user_settings(repo_path)?;
  Workspace::load(
    &settings,
    Path::new(repo_path),
    &StoreFactories::default(),
    &default_working_copy_factories(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to load workspace: {}", e)))
}

fn load_home_repo(
  repo_path: &str,
) -> Result<(Workspace, Arc<ReadonlyRepo>, SimpleWorkspaceStore), JjError> {
  let workspace = load_home_workspace(repo_path)?;
  let repo = block_on(workspace.repo_loader().load_at_head())
    .map_err(|e| JjError::IoError(format!("Failed to load repo: {}", e)))?;
  let workspace_store = SimpleWorkspaceStore::load(workspace.repo_path())
    .map_err(|e| JjError::IoError(format!("Failed to load workspace store: {}", e)))?;
  Ok((workspace, repo, workspace_store))
}

fn list_registered_workspaces(repo_path: &str) -> Result<Vec<RegisteredWorkspace>, JjError> {
  let (_home_workspace, repo, workspace_store) = load_home_repo(repo_path)?;

  let mut workspaces = Vec::new();
  for (workspace_name, wc_commit_id) in repo.view().wc_commit_ids() {
    if workspace_name.as_str() == "default" {
      continue;
    }
    // Short-lived conflict-resolve sandboxes are not product workspaces.
    if workspace_name.as_str().starts_with("_resolve-") {
      continue;
    }

    let stored_path = workspace_store
      .get_workspace_path(workspace_name)
      .map_err(|e| JjError::IoError(format!("Failed to read workspace path: {}", e)))?
      .ok_or_else(|| {
        JjError::IoError(format!(
          "Workspace has no recorded path: {}",
          workspace_name.as_str()
        ))
      })?;
    let stored_full_path = if stored_path.is_absolute() {
      stored_path
    } else {
      Path::new(repo_path)
        .join(".jj")
        .join("repo")
        .join(stored_path)
    };
    let workspace_path = stored_full_path
      .file_name()
      .and_then(|name| name.to_str())
      .ok_or_else(|| {
        JjError::IoError(format!(
          "Failed to derive workspace directory name for '{}'",
          workspace_name.as_str()
        ))
      })?
      .to_string();

    let wc_commit = repo
      .store()
      .get_commit(wc_commit_id)
      .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {}", e)))?;
    let (branch_name, bookmark_has_conflicts) =
      branch_name_for_workspace_commit(repo.as_ref(), workspace_name.as_str(), &wc_commit);
    let last_activity_at = commit_author_timestamp_rfc3339(&wc_commit);

    workspaces.push(RegisteredWorkspace {
      workspace_name: workspace_name.as_str().to_string(),
      workspace_path,
      branch_name,
      has_conflicts: bookmark_has_conflicts || !wc_commit.tree_ids().is_resolved(),
      last_activity_at,
    });
  }

  workspaces.sort_by(|a, b| a.branch_name.cmp(&b.branch_name));
  Ok(workspaces)
}

/// List every workspace name in the repo's view, including `default` (the home
/// repo's own working copy) — equivalent to `jj workspace list`. Unlike
/// [`list_jj_workspaces`], nothing is filtered out for product purposes.
pub fn list_all_workspace_names(repo_path: &str) -> Result<Vec<String>, JjError> {
  let (_home_workspace, repo, _workspace_store) = load_home_repo(repo_path)?;
  let mut names: Vec<String> = repo
    .view()
    .wc_commit_ids()
    .keys()
    .map(|name| name.as_str().to_string())
    .collect();
  names.sort();
  Ok(names)
}

/// List workspace names registered in JJ without shelling out.
pub fn list_jj_workspaces(repo_path: &str) -> Result<Vec<String>, JjError> {
  Ok(
    list_registered_workspaces(repo_path)?
      .into_iter()
      .map(|workspace| workspace.workspace_name)
      .collect(),
  )
}

fn commit_author_timestamp_rfc3339(commit: &jj_lib::commit::Commit) -> Option<String> {
  commit
    .author()
    .timestamp
    .to_datetime()
    .ok()
    .map(|ts| ts.to_rfc3339())
}

fn branch_name_for_workspace_commit(
  repo: &dyn jj_lib::repo::Repo,
  workspace_name: &str,
  wc_commit: &jj_lib::commit::Commit,
) -> (String, bool) {
  let bookmark_matches_workspace = |bookmark_name: &str| {
    bookmark_name == workspace_name || sanitize_workspace_name(bookmark_name) == workspace_name
  };

  let exact_bookmarks: Vec<String> = repo
    .view()
    .local_bookmarks_for_commit(wc_commit.id())
    .filter(|&(_, target)| target.as_normal() == Some(wc_commit.id()))
    .map(|(name, _)| name.as_str().to_string())
    .collect();
  if let Some(name) = exact_bookmarks
    .iter()
    .find(|name| bookmark_matches_workspace(name.as_str()))
  {
    return (name.clone(), false);
  }

  // A conflicted bookmark does not appear as an exact bookmark because it is
  // not a normal RefTarget. Still retain its canonical (possibly slash-
  // containing) name when the registered working-copy commit is one of the
  // conflict's targets, and surface the ambiguity to callers.
  if let Some((name, _)) = repo.view().local_bookmarks().find(|(name, target)| {
    bookmark_matches_workspace(name.as_str())
      && target.has_conflict()
      && target.added_ids().any(|id| id == wc_commit.id())
  }) {
    return (name.as_str().to_string(), true);
  }

  let parent_bookmarks: Vec<String> = wc_commit
    .parent_ids()
    .iter()
    .flat_map(|parent_id| {
      repo
        .view()
        .local_bookmarks_for_commit(parent_id)
        .filter(move |&(_, target)| target.as_normal() == Some(parent_id))
        .map(|(name, _)| name.as_str().to_string())
    })
    .collect();
  if let Some(name) = parent_bookmarks
    .iter()
    .find(|name| bookmark_matches_workspace(name.as_str()))
  {
    return (name.clone(), false);
  }

  // After a commit the WC tip is an empty commit above the bookmark. A later
  // divergent fetch conflicted-bookmarks the parent tip — not the WC — so the
  // lookups above miss. Prefer any matching bookmark (including conflicted)
  // over inventing a dashed workspace-name fallback that breaks PR / tip
  // revsets (`Revision 'feat-…' doesn't exist`).
  if let Some((name, target)) = repo
    .view()
    .local_bookmarks()
    .find(|(name, _)| bookmark_matches_workspace(name.as_str()))
  {
    return (name.as_str().to_string(), target.has_conflict());
  }

  (workspace_name.to_string(), false)
}

pub fn discover_workspaces_with_conflicts(
  repo_path: &str,
) -> Result<Vec<DiscoveredWorkspace>, JjError> {
  Ok(
    list_registered_workspaces(repo_path)?
      .into_iter()
      .map(|workspace| DiscoveredWorkspace {
        workspace_name: workspace.workspace_name,
        workspace_path: workspace.workspace_path,
        branch_name: workspace.branch_name,
        has_conflicts: workspace.has_conflicts,
        last_activity_at: workspace.last_activity_at,
      })
      .collect(),
  )
}

/// List all workspaces in a repository
/// Returns workspaces found in .treq/workspaces/ directory
pub fn list_workspaces(repo_path: &str) -> Result<Vec<WorkspaceInfo>, JjError> {
  let workspaces_dir = Path::new(repo_path).join(".treq").join("workspaces");

  if !workspaces_dir.exists() {
    return Ok(Vec::new());
  }

  let mut workspaces = Vec::new();

  let entries = fs::read_dir(&workspaces_dir).map_err(|e| JjError::IoError(e.to_string()))?;

  for entry in entries.filter_map(|e| e.ok()) {
    let entry_path = entry.path();

    // Must be a directory
    if !entry_path.is_dir() {
      continue;
    }

    // Must have a .git file/dir (valid git workspace)
    let git_path = entry_path.join(".git");
    if !git_path.exists() {
      continue;
    }

    let name = entry.file_name().to_string_lossy().to_string();

    let path = entry_path.to_string_lossy().to_string();

    // Check if it's colocated (has .jj directory)
    let is_colocated = entry_path.join(".jj").exists();

    // Get branch name from git
    let branch = get_workspace_branch(&path).unwrap_or_default();

    workspaces.push(WorkspaceInfo {
      name,
      path,
      branch,
      is_colocated,
    });
  }

  // Sort by name
  workspaces.sort_by(|a, b| a.name.cmp(&b.name));

  Ok(workspaces)
}

/// Get the current branch of a workspace
pub fn get_workspace_branch(workspace_path: &str) -> Result<String, JjError> {
  if let Some(repo_path) = derive_repo_path_from_workspace(workspace_path) {
    let ws_name = Path::new(workspace_path)
      .file_name()
      .and_then(|n| n.to_str())
      .ok_or_else(|| JjError::IoError("Invalid workspace path".to_string()))?;

    if let Ok(Some(workspace)) = local_db::get_workspace_by_path(&repo_path, ws_name) {
      return Ok(workspace.branch_name);
    }
  }

  resolve_home_repo_branch(workspace_path)
}

/// Remove the workspace directory from disk only (no jj subprocess).
pub fn remove_workspace_directory_only(workspace_path: &str) -> Result<(), JjError> {
  let workspace_dir = Path::new(workspace_path);
  if workspace_dir.exists() {
    fs::remove_dir_all(workspace_dir).map_err(|e| JjError::IoError(e.to_string()))?;
  }
  Ok(())
}

/// Forget a jj workspace without deleting its directory.
// home-head-test: delete_workspace, resolve_workspace_conflict
pub fn forget_workspace(repo_path: &str, workspace_path: &str) -> Result<(), JjError> {
  let workspace_dir = Path::new(workspace_path);

  // Extract workspace name from path (last component)
  let workspace_name = workspace_dir
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("");

  // Always forget the jj workspace first so jj stops tracking even if directory is gone.
  if !workspace_name.is_empty() {
    let loaded = load_workspace_repo(repo_path)?;
    let mut tx = loaded.repo.start_transaction();
    let ws_name = tx
      .repo()
      .view()
      .wc_commit_ids()
      .iter()
      .find(|(name, _)| name.as_str() == workspace_name)
      .map(|(name, _)| (*name).clone());
    if let Some(ws_name) = ws_name {
      block_on(tx.repo_mut().remove_wc_commit(&ws_name))
        .map_err(|e| JjError::IoError(format!("Failed to forget workspace: {}", e)))?;
      block_on(tx.repo_mut().rebase_descendants()).map_err(|e| {
        JjError::IoError(format!(
          "Failed to rebase descendants while forgetting workspace: {}",
          e
        ))
      })?;
      block_on(tx.commit("forget workspace"))
        .map_err(|e| JjError::IoError(format!("Failed to forget workspace: {}", e)))?;

      // Reconcile remaining workspaces — WC commits may have been rewritten by rebase_descendants; skip the workspace being removed.
      let _ = reconcile_all_workspaces_after_rewrite(repo_path, Some(workspace_name));
    }
  }

  Ok(())
}

/// Remove a workspace (jj workspace + files)
pub fn remove_workspace(repo_path: &str, workspace_path: &str) -> Result<(), JjError> {
  forget_workspace(repo_path, workspace_path)?;
  remove_workspace_directory_only(workspace_path)
}

/// Move changes from one workspace to another using jj squash
/// This moves changes from the current workspace (@) to the target workspace's working copy
/// Uses: jj squash --from @ --into <target-workspace-name>@
pub fn squash_to_workspace(
  source_workspace_path: &str,
  target_workspace_name: &str,
  file_paths: Option<Vec<String>>,
) -> Result<String, JjError> {
  let repo_path = derive_repo_path_from_workspace(source_workspace_path)
    .unwrap_or_else(|| source_workspace_path.to_string());
  let target_workspace = local_db::get_workspaces(&repo_path)
    .map_err(JjError::IoError)?
    .into_iter()
    .find(|ws| ws.workspace_name == target_workspace_name)
    .ok_or_else(|| {
      JjError::IoError(format!(
        "Target workspace '{}' not found",
        target_workspace_name
      ))
    })?;
  let target_workspace_path = Path::new(&repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&target_workspace.workspace_path);
  let target_workspace_path_str = target_workspace_path
    .to_str()
    .ok_or_else(|| JjError::IoError("Failed to convert target workspace path".to_string()))?;
  move_paths_between_workspace_paths(source_workspace_path, target_workspace_path_str, file_paths)
}

/// Moves the given (or all changed) paths from a source working-copy directory
/// directly into a destination working-copy directory, resolved by filesystem
/// path rather than by looking up a registered workspace. Works for regular
/// workspace directories as well as the home repo root.
pub fn move_paths_between_workspace_paths(
  source_workspace_path: &str,
  target_workspace_path: &str,
  file_paths: Option<Vec<String>>,
) -> Result<String, JjError> {
  let selected_paths = match file_paths {
    Some(paths) => paths,
    None => jj_get_changed_files(source_workspace_path)?
      .into_iter()
      .map(|c| c.path)
      .collect(),
  };
  move_paths_between_workspaces(
    Path::new(source_workspace_path),
    Path::new(target_workspace_path),
    &selected_paths,
  )?;
  Ok(String::new())
}

/// Squash a commit out of the source history into another workspace's working copy.
/// Equivalent to: jj squash --from <change_id> --into <target workspace>@
// home-head-test: move_commit_to_workspace
pub fn squash_commit_to_workspace(
  workspace_path: &str,
  change_id: &str,
  target_workspace_path: &str,
) -> Result<(), JjError> {
  // Record pending edits in both working copies so the rewrite cannot drop them.
  jj_get_changed_files(workspace_path)?;
  jj_get_changed_files(target_workspace_path)?;
  let loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, change_id)?;
  reject_root_commit(&loaded, &commit)?;
  let target_name = load_workspace_repo(target_workspace_path)?
    .workspace
    .workspace_name()
    .to_owned();
  let target_wc = loaded
    .repo
    .view()
    .get_wc_commit_id(&target_name)
    .map(|id| loaded.repo.store().get_commit(id))
    .transpose()
    .map_err(|e| JjError::IoError(format!("Failed to load target working copy: {}", e)))?
    .ok_or_else(|| JjError::IoError("Target workspace has no working-copy commit".to_string()))?;
  let target_history = evaluate_revset(&loaded, &format!("::{}", target_wc.id().hex()))?;
  if target_history.containing_fn()(commit.id()).unwrap_or(false) {
    return Err(JjError::IoError(
      "Commit is already in the target workspace's history".to_string(),
    ));
  }

  let parent_tree = block_on(commit.parent_tree(loaded.repo.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to load parent tree: {}", e)))?;
  let new_tree = block_on(MergedTree::merge(Merge::from_diffs(
    (target_wc.tree(), target_wc.conflict_label()),
    vec![Diff::new(
      (
        parent_tree,
        format!("{} (parents)", commit.conflict_label()),
      ),
      (commit.tree(), commit.conflict_label()),
    )],
  )))
  .map_err(|e| JjError::IoError(format!("Failed to merge commit into target: {}", e)))?;
  if new_tree.has_conflict() {
    return Err(JjError::IoError("Commit conflicts with target".into()));
  }

  let mut tx = loaded.repo.start_transaction();
  let new_target_wc = block_on(
    tx.repo_mut()
      .rewrite_commit(&target_wc)
      .set_tree(new_tree)
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to rewrite target working copy: {}", e)))?;
  block_on(tx.repo_mut().edit(target_name.clone(), &new_target_wc))
    .map_err(|e| JjError::IoError(format!("Failed to update target working copy: {}", e)))?;
  tx.repo_mut().record_abandoned_commit(&commit);
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;
  let _ = git::export_refs(tx.repo_mut());
  block_on(tx.commit("move commit to workspace"))
    .map_err(|e| JjError::IoError(format!("Failed to commit move: {}", e)))?;

  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  let _ = reconcile_all_workspaces_after_rewrite(&repo_path, Some(target_name.as_str()));
  update_stale_workspace(target_workspace_path)
    .map_err(|e| JjError::IoError(format!("Failed to update target working copy: {e}")))
}

#[derive(Debug, Clone)]
pub struct HunkMoveOutcome {
  pub applied: usize,
  pub skipped: usize,
  pub warnings: Vec<String>,
}

pub fn move_hunks_between_workspaces(
  source_workspace_path: &str,
  destination_workspace_path: &str,
  hunks: &[crate::core::workspaces::HunkSpec],
) -> Result<HunkMoveOutcome, JjError> {
  let mut outcome = HunkMoveOutcome {
    applied: 0,
    skipped: 0,
    warnings: Vec::new(),
  };

  for hunk in hunks {
    let source_file = Path::new(source_workspace_path).join(&hunk.file_path);
    let destination_file = Path::new(destination_workspace_path).join(&hunk.file_path);

    let source_content = match fs::read_to_string(&source_file) {
      Ok(content) => content,
      Err(_) => {
        outcome.skipped += 1;
        outcome.warnings.push(format!(
          "Skipped {}:{}-{}: source file not found",
          hunk.file_path, hunk.start_line, hunk.end_line
        ));
        continue;
      }
    };
    let mut source_lines: Vec<String> = source_content
      .lines()
      .map(|line| line.to_string())
      .collect();
    if hunk.end_line > source_lines.len() || hunk.start_line > source_lines.len() {
      outcome.skipped += 1;
      outcome.warnings.push(format!(
        "Skipped {}:{}-{}: source range out of bounds",
        hunk.file_path, hunk.start_line, hunk.end_line
      ));
      continue;
    }

    let moved_lines: Vec<String> = source_lines[(hunk.start_line - 1)..hunk.end_line].to_vec();
    source_lines.drain((hunk.start_line - 1)..hunk.end_line);
    let new_source_content = if source_lines.is_empty() {
      String::new()
    } else {
      format!("{}\n", source_lines.join("\n"))
    };

    let destination_content = fs::read_to_string(&destination_file).unwrap_or_default();
    let mut destination_lines: Vec<String> = destination_content
      .lines()
      .map(|line| line.to_string())
      .collect();
    destination_lines.extend(moved_lines);
    let new_destination_content = if destination_lines.is_empty() {
      String::new()
    } else {
      format!("{}\n", destination_lines.join("\n"))
    };

    if let Some(parent) = destination_file.parent() {
      fs::create_dir_all(parent).map_err(|e| {
        JjError::IoError(format!(
          "Failed to create directory {}: {}",
          parent.display(),
          e
        ))
      })?;
    }

    fs::write(&source_file, new_source_content).map_err(|e| {
      JjError::IoError(format!(
        "Failed to write source file {}: {}",
        source_file.display(),
        e
      ))
    })?;
    fs::write(&destination_file, new_destination_content).map_err(|e| {
      JjError::IoError(format!(
        "Failed to write destination file {}: {}",
        destination_file.display(),
        e
      ))
    })?;

    outcome.applied += 1;
  }

  Ok(outcome)
}

fn copy_path_recursive(source: &Path, destination: &Path) -> Result<(), JjError> {
  if source.is_dir() {
    fs::create_dir_all(destination).map_err(|e| {
      JjError::IoError(format!(
        "Failed to create directory {}: {}",
        destination.display(),
        e
      ))
    })?;
    for entry in fs::read_dir(source).map_err(|e| {
      JjError::IoError(format!(
        "Failed to read directory {}: {}",
        source.display(),
        e
      ))
    })? {
      let entry =
        entry.map_err(|e| JjError::IoError(format!("Failed to read dir entry: {}", e)))?;
      copy_path_recursive(&entry.path(), &destination.join(entry.file_name()))?;
    }
  } else if source.is_file() {
    if let Some(parent) = destination.parent() {
      fs::create_dir_all(parent).map_err(|e| {
        JjError::IoError(format!(
          "Failed to create directory {}: {}",
          parent.display(),
          e
        ))
      })?;
    }
    fs::copy(source, destination).map_err(|e| {
      JjError::IoError(format!(
        "Failed to copy {} to {}: {}",
        source.display(),
        destination.display(),
        e
      ))
    })?;
  }
  Ok(())
}

fn move_paths_between_workspaces(
  source_workspace_path: &Path,
  target_workspace_path: &Path,
  paths: &[String],
) -> Result<(), JjError> {
  for relative in paths {
    let source = source_workspace_path.join(relative);
    let target = target_workspace_path.join(relative);
    if source.exists() {
      copy_path_recursive(&source, &target)?;
      if source.is_dir() {
        fs::remove_dir_all(&source).map_err(|e| {
          JjError::IoError(format!(
            "Failed to remove directory {}: {}",
            source.display(),
            e
          ))
        })?;
      } else if source.is_file() {
        fs::remove_file(&source).map_err(|e| {
          JjError::IoError(format!("Failed to remove file {}: {}", source.display(), e))
        })?;
      }
    } else if target.exists() {
      if target.is_dir() {
        fs::remove_dir_all(&target).map_err(|e| {
          JjError::IoError(format!(
            "Failed to remove directory {}: {}",
            target.display(),
            e
          ))
        })?;
      } else {
        fs::remove_file(&target).map_err(|e| {
          JjError::IoError(format!("Failed to remove file {}: {}", target.display(), e))
        })?;
      }
    }
  }
  Ok(())
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
  let _ = reconcile_all_workspaces_after_rewrite(&repo_path, None);

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
  if parents.len() != 1 {
    return Err(JjError::IoError(
      "Cannot undo a merge operation".to_string(),
    ));
  }
  let parent = parents.into_iter().next().unwrap();
  let parent_repo = block_on(loaded.repo.loader().load_at(&parent))
    .map_err(|e| JjError::IoError(format!("Failed to load parent operation: {}", e)))?;
  let restored_view = parent_repo.view().store_view().clone();

  let mut tx = loaded.repo.start_transaction();
  tx.repo_mut().set_view(restored_view);
  let _ = git::export_refs(tx.repo_mut());
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
/// Implemented as a validated abandon: the commit is removed from history and its
/// changes are folded back into the (rebased) working copy, exactly reversing the
/// act of committing.
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

  drop(loaded);
  jj_abandon(workspace_path, change_id)
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
  let _ = reconcile_all_workspaces_after_rewrite(&repo_path, None);

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
  let _ = reconcile_all_workspaces_after_rewrite(&repo_path, None);

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
  let _ = reconcile_all_workspaces_after_rewrite(&repo_path, None);
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
    WorkingCopyFreshness::Fresh => {
      block_on(locked_ws.finish(repo.op_id().clone()))
        .map_err(|e| JjError::InitFailed(format!("Failed to finish wc: {}", e)))?;
    }
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
  let (home_workspace, repo, workspace_store) = match load_home_repo(repo_path) {
    Ok(r) => r,
    Err(_) => return Ok(()), // best-effort; don't abort the calling operation
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
      match workspace_store.get_workspace_path(workspace_name) {
        Ok(Some(stored_path)) => {
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
        _ => continue, // can't resolve path; skip gracefully
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

/// Get list of changed files in working copy using jj-lib (no subprocess)
/// Snapshots the working copy and diffs against the parent commit tree.
pub fn jj_get_changed_files(workspace_path: &str) -> Result<Vec<JjFileChange>, JjError> {
  use futures::StreamExt as _;

  let path = Path::new(workspace_path);
  if !path.exists() {
    return Ok(Vec::new());
  }
  if !path.is_dir() {
    return Err(JjError::IoError(format!(
      "Workspace path is not a directory: {}",
      workspace_path
    )));
  }
  if !path.join(".jj").exists() {
    return Ok(Vec::new());
  }

  let repo_path_opt = derive_repo_path_from_workspace(workspace_path);
  if repo_path_opt.is_none() {
    reconcile_colocated_home_repo(workspace_path)?;
  }
  let settings_path = repo_path_opt.as_deref().unwrap_or(workspace_path);
  let settings = match create_user_settings(settings_path) {
    Ok(s) => s,
    Err(_) => return Ok(Vec::new()),
  };
  with_working_copy_lock(workspace_path, || {
    let mut workspace = match Workspace::load(
      &settings,
      path,
      &StoreFactories::default(),
      &default_working_copy_factories(),
    ) {
      Ok(ws) => ws,
      Err(_) => return Ok(Vec::new()),
    };
    let repo = match block_on(workspace.repo_loader().load_at_head()) {
      Ok(r) => r,
      Err(_) => return Ok(Vec::new()),
    };
    let ignore_root = repo_path_opt.as_deref().unwrap_or(workspace_path);
    let matcher = repo_root_matcher();
    let opts = snapshot_options_for_all_paths(ignore_root, &matcher);

    let workspace_name = workspace.workspace_name().to_owned();

    // Load current WC commit before locking (needed for tree comparison and diffing)
    let wc_commit_id = match repo.view().get_wc_commit_id(&workspace_name) {
      Some(id) => id.clone(),
      None => return Ok(Vec::new()),
    };
    let wc_commit = match repo.store().get_commit(&wc_commit_id) {
      Ok(c) => c,
      Err(_) => return Ok(Vec::new()),
    };

    let mut locked_ws = match workspace.start_working_copy_mutation() {
      Ok(lws) => lws,
      Err(_) => return Ok(Vec::new()),
    };
    let new_tree = match block_on(locked_ws.locked_wc().snapshot(&opts)) {
      Ok((tree, _)) => tree,
      Err(_) => return Ok(Vec::new()), // locked_ws dropped here, lock released
    };

    // If tree changed, create a new WC commit and update op-store to expose the snapshotted state.
    if new_tree.tree_ids() != wc_commit.tree_ids() {
      let mut tx = repo.start_transaction();
      let mut builder = tx.repo_mut().rewrite_commit(&wc_commit).detach();
      builder.set_tree(new_tree.clone());
      match block_on(builder.write(tx.repo_mut())) {
        Ok(rewritten_wc) => {
          let _ = block_on(tx.repo_mut().edit(workspace_name.clone(), &rewritten_wc));
          let _ = block_on(tx.repo_mut().rebase_descendants());
          match block_on(tx.commit("snapshot working copy")) {
            Ok(final_repo) => {
              let _ = block_on(locked_ws.finish(final_repo.op_id().clone()));
            }
            Err(_) => {
              let _ = block_on(locked_ws.finish(repo.op_id().clone()));
            }
          }
        }
        Err(_) => {
          let _ = block_on(locked_ws.finish(repo.op_id().clone()));
        }
      }
    } else {
      let _ = block_on(locked_ws.finish(repo.op_id().clone()));
    }

    // Get the wc commit's parent tree to diff against
    let parent_tree = if wc_commit.parent_ids().is_empty() {
      repo.store().root_commit().tree()
    } else {
      match repo.store().get_commit(&wc_commit.parent_ids()[0]) {
        Ok(parent) => parent.tree(),
        Err(_) => return Ok(Vec::new()),
      }
    };

    let diff_matcher = repo_root_matcher();
    let diff_entries = block_on(
      parent_tree
        .diff_stream(&new_tree, &diff_matcher)
        .collect::<Vec<_>>(),
    );

    let mut changes = Vec::new();
    for entry in diff_entries {
      let file_path = entry.path.as_internal_file_string().to_string();
      let values = match entry.values {
        Ok(v) => v,
        Err(_) => continue,
      };
      let status = if !values.before.is_resolved() || !values.after.is_resolved() {
        "C"
      } else if values.before.is_absent() {
        "A"
      } else if values.after.is_absent() {
        "D"
      } else {
        "M"
      };
      changes.push(JjFileChange {
        path: file_path,
        status: status.to_string(),
        previous_path: None,
        changed_line_count: 0,
        diff_deferred: false,
      });
    }

    Ok(changes)
  })
}

/// Checks if the working copy is empty (no uncommitted changes)
pub fn jj_is_working_copy_empty(workspace_path: &str) -> Result<bool, JjError> {
  let changed_files = jj_get_changed_files(workspace_path)?;
  Ok(changed_files.is_empty())
}

/// Checks if working copy needs syncing with bookmark.
///
/// A sync is only needed when `@` has fallen *behind* the bookmark — i.e. the
/// bookmark tip is not an ancestor of `@`. Plain inequality is not enough: right
/// after a commit, `@` is a fresh empty child of the bookmark, which is the
/// normal, desired state. Treating that as "needs sync" would move `@` back onto
/// the bookmark and discard the working-copy commit.
pub fn jj_working_copy_needs_sync(
  workspace_path: &str,
  branch_name: &str,
) -> Result<bool, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let bookmark_commit = resolve_commit_by_revision(&loaded, branch_name)?;
  let working_copy_commit = resolve_commit_by_revision(&loaded, "@")?;
  if bookmark_commit.id() == working_copy_commit.id() {
    return Ok(false);
  }
  // `@` already contains the bookmark tip (e.g. an empty commit on top of it).
  let bookmark_is_ancestor = loaded
    .repo
    .index()
    .is_ancestor(bookmark_commit.id(), working_copy_commit.id())
    .map_err(|e| JjError::IoError(format!("Failed ancestry check: {}", e)))?;
  Ok(!bookmark_is_ancestor)
}

/// Resolves the commit a branch bookmark should point at for this workspace.
///
/// Returns `@` normally, but `@-` when `@` is the usual empty, undescribed
/// working-copy commit — pointing a bookmark at that commit would push an empty
/// commit to the remote and leave the workspace without a working-copy commit.
pub fn jj_resolve_bookmark_tip(workspace_path: &str) -> Result<String, JjError> {
  let wc_commit = {
    let loaded = load_workspace_repo(workspace_path)?;
    resolve_commit_by_revision(&loaded, "@")?
  };

  // Only a plain, single-parent, undescribed commit is a throwaway working copy.
  if wc_commit.parent_ids().len() != 1 || !wc_commit.description().trim().is_empty() {
    return jj_get_commit_id(workspace_path, "@");
  }
  if !jj_is_working_copy_empty(workspace_path)? {
    return jj_get_commit_id(workspace_path, "@");
  }

  jj_get_commit_id(workspace_path, "@-")
}

/// Safely syncs working copy to bookmark (only if empty)
///
/// Returns:
/// - Ok(true) if sync was performed
/// - Ok(false) if sync was skipped (working copy not empty or already synced)
/// - Err if sync failed
// home-head-test: sync_working_copy_to_moved_bookmark
pub fn jj_sync_working_copy_if_safe(
  workspace_path: &str,
  branch_name: &str,
) -> Result<bool, JjError> {
  // Check if sync is needed
  if !jj_working_copy_needs_sync(workspace_path, branch_name)? {
    return Ok(false); // Already synced
  }

  // Check if working copy is empty
  if !jj_is_working_copy_empty(workspace_path)? {
    return Ok(false); // Skip: working copy has uncommitted changes
  }

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  if derive_repo_path_from_workspace(workspace_path).is_none() {
    // Moving the bookmark detached Git HEAD; `@` is joining `branch_name`.
    loaded.home_git_head_branch = Some(branch_name.to_string());
  }
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let destination = resolve_commit_by_revision(&loaded, branch_name)?;
  let old_wc_commit = get_workspace_wc_commit(&loaded)?;

  let mut tx = loaded.repo.start_transaction();
  // Edit bookmark commit directly so @ == bookmark after sync (empty child caused infinite sync loop).
  block_on(tx.repo_mut().edit(workspace_name, &destination))
    .map_err(|e| JjError::IoError(format!("Failed to sync working copy to bookmark: {}", e)))?;
  // jj requires rebase_descendants() after any rewrite before committing the transaction.
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;
  let new_repo = block_on(tx.commit("sync working copy to bookmark"))
    .map_err(|e| JjError::IoError(format!("Failed to commit working copy sync: {}", e)))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    old_wc_commit.as_ref(),
    CheckoutMode::Immediate,
  )?;

  Ok(true) // Sync performed successfully
}

/// Get diff hunks for a specific file using jj-lib materialized tree values.
pub fn jj_get_file_hunks(
  workspace_path: &str,
  file_path: &str,
  conflict_marker_style: &str,
) -> Result<Vec<JjDiffHunk>, JjError> {
  if file_path.contains('\0') || file_path.is_empty() {
    return Err(JjError::IoError("Invalid file path".to_string()));
  }

  let mut loaded = load_workspace_repo(workspace_path)?;
  import_colocated_git_state(&mut loaded, workspace_path)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let wc_commit_id = loaded
    .repo
    .view()
    .get_wc_commit_id(&workspace_name)
    .cloned()
    .ok_or_else(|| JjError::IoError("No working-copy commit found".to_string()))?;
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load wc commit: {}", e)))?;
  let parent_tree = block_on(wc_commit.parent_tree(loaded.repo.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to read parent tree: {}", e)))?;
  let mut tree = wc_commit.tree();
  if let Some((_, snapshotted_tree)) = snapshot_working_copy_tree(&mut loaded, workspace_path)? {
    tree = snapshotted_tree;
  }

  let target_repo_path = RepoPathBuf::from_internal_string(file_path)
    .map_err(|_| JjError::IoError("Invalid file path".to_string()))?;
  let before = block_on(parent_tree.path_value(target_repo_path.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to read parent path value: {}", e)))?;
  let after = block_on(tree.path_value(target_repo_path.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to read working path value: {}", e)))?;
  let merge_options = MergeOptions::from_settings(&loaded.settings)
    .map_err(|e| JjError::IoError(format!("Failed to load merge options: {}", e)))?;
  let labels = ConflictLabels::unlabeled();
  let before_materialized = block_on(jj_lib::conflicts::materialize_tree_value(
    loaded.repo.store(),
    target_repo_path.as_ref(),
    before,
    &labels,
  ))
  .map_err(|e| JjError::IoError(format!("Failed to materialize parent value: {}", e)))?;
  let after_materialized = block_on(jj_lib::conflicts::materialize_tree_value(
    loaded.repo.store(),
    target_repo_path.as_ref(),
    after,
    &labels,
  ))
  .map_err(|e| JjError::IoError(format!("Failed to materialize working value: {}", e)))?;

  let style = parse_conflict_marker_style(conflict_marker_style);
  let before_bytes = block_on(materialized_value_to_bytes(
    target_repo_path.as_ref(),
    before_materialized,
    style,
    Some(&merge_options),
  ));
  let after_bytes = block_on(materialized_value_to_bytes(
    target_repo_path.as_ref(),
    after_materialized,
    style,
    Some(&merge_options),
  ));
  let (_, mut hunks, conflict_style, conflict_regions_raw) =
    build_file_diff_from_bytes(before_bytes, after_bytes);
  let conflict_regions = annotate_conflict_regions(file_path, conflict_regions_raw);
  for hunk in &mut hunks {
    hunk.conflict_style = conflict_style;
    hunk.conflict_regions = conflict_regions.clone();
  }
  Ok(hunks)
}

fn parse_conflict_marker_style(conflict_marker_style: &str) -> ConflictMarkerStyle {
  match conflict_marker_style {
    "snapshot" => ConflictMarkerStyle::Snapshot,
    "diff" => ConflictMarkerStyle::Diff,
    _ => ConflictMarkerStyle::Git,
  }
}

/// Parse git diff output into hunks
fn parse_git_diff_hunks(diff: &str) -> Result<Vec<JjDiffHunk>, JjError> {
  let mut hunks = Vec::new();
  let mut current_hunk: Option<(String, Vec<String>)> = None;
  let mut hunk_index = 0;

  for line in diff.lines() {
    if line.starts_with("@@") {
      // Save previous hunk if exists
      if let Some((header, lines)) = current_hunk.take() {
        hunks.push(JjDiffHunk {
          id: format!("hunk-{}", hunk_index),
          header: header.clone(),
          lines: lines.clone(),
          patch: format!("{}\n{}", header, lines.join("\n")),
          conflict_style: ConflictStyle::Unknown,
          conflict_regions: Vec::new(),
        });
        hunk_index += 1;
      }

      // Start new hunk
      current_hunk = Some((line.to_string(), Vec::new()));
    } else if let Some((_, ref mut lines)) = current_hunk {
      // Skip diff metadata lines (be specific to avoid filtering conflict markers)
      if !line.starts_with("diff --git")
        && !line.starts_with("index ")
        && !line.starts_with("--- ")
        && !line.starts_with("+++ ")
      {
        lines.push(line.to_string());
      }
    }
  }

  // Save last hunk
  if let Some((header, lines)) = current_hunk {
    hunks.push(JjDiffHunk {
      id: format!("hunk-{}", hunk_index),
      header: header.clone(),
      lines: lines.clone(),
      patch: format!("{}\n{}", header, lines.join("\n")),
      conflict_style: ConflictStyle::Unknown,
      conflict_regions: Vec::new(),
    });
  }

  Ok(hunks)
}

/// Reads `file_path` from git HEAD for a plain git repo with no `.jj`.
fn read_git_head_blob(repo_path: &str, file_path: &str) -> Result<String, JjError> {
  let repo = gix::open(repo_path).map_err(|e| JjError::IoError(e.to_string()))?;
  let head_commit = repo
    .head_commit()
    .map_err(|e| JjError::IoError(e.to_string()))?;
  let tree = head_commit
    .tree()
    .map_err(|e| JjError::IoError(e.to_string()))?;
  let entry = tree
    .lookup_entry_by_path(file_path)
    .map_err(|e| JjError::IoError(e.to_string()))?
    .ok_or_else(|| JjError::IoError(format!("Path '{}' not found in HEAD", file_path)))?;
  let blob = entry
    .object()
    .map_err(|e| JjError::IoError(e.to_string()))?
    .try_into_blob()
    .map_err(|e| JjError::IoError(e.to_string()))?;
  Ok(String::from_utf8_lossy(&blob.data).to_string())
}

/// Get file content at specific lines for context expansion
pub fn jj_get_file_lines(
  workspace_path: &str,
  file_path: &str,
  from_parent: bool,
  start_line: usize,
  end_line: usize,
) -> Result<JjFileLines, JjError> {
  // CLI and remote callers pass these paths: keep reads inside the workspace.
  let relative = Path::new(file_path);
  if !relative.components().all(|c| {
    matches!(
      c,
      std::path::Component::Normal(_) | std::path::Component::CurDir
    )
  }) {
    return Err(JjError::IoError(format!(
      "Path '{}' is outside the workspace",
      file_path
    )));
  }
  let content = if from_parent && !Path::new(workspace_path).join(".jj").exists() {
    read_git_head_blob(workspace_path, file_path)?
  } else if from_parent {
    // Read through jj-lib: a secondary workspace has no `.git` of its own.
    let mut loaded = load_workspace_repo(workspace_path)?;
    import_colocated_git_state(&mut loaded, workspace_path)?;
    let wc_commit = get_workspace_wc_commit(&loaded)?
      .ok_or_else(|| JjError::IoError("No working-copy commit found".to_string()))?;
    let parent_tree = block_on(wc_commit.parent_tree(loaded.repo.as_ref()))
      .map_err(|e| JjError::IoError(format!("Failed to read parent tree: {}", e)))?;
    let repo_path = RepoPathBuf::from_relative_path(relative)
      .map_err(|_| JjError::IoError("Invalid file path".to_string()))?;
    let value = block_on(parent_tree.path_value(&repo_path))
      .map_err(|e| JjError::IoError(format!("Failed to read parent path value: {}", e)))?;
    let materialized = block_on(jj_lib::conflicts::materialize_tree_value(
      loaded.repo.store(),
      &repo_path,
      value,
      &ConflictLabels::unlabeled(),
    ))
    .map_err(|e| JjError::IoError(format!("Failed to materialize parent value: {}", e)))?;
    let bytes = match materialized {
      MaterializedTreeValue::File(mut file) => block_on(file.read_all(&repo_path))
        .map_err(|e| JjError::IoError(format!("Failed to read file: {}", e)))?,
      other => {
        let merge_options = MergeOptions::from_settings(&loaded.settings)
          .map_err(|e| JjError::IoError(format!("Failed to load merge options: {}", e)))?;
        // Git markers, as `treq changes diff` renders the same file.
        block_on(materialized_value_to_bytes(
          &repo_path,
          other,
          ConflictMarkerStyle::Git,
          Some(&merge_options),
        ))
        .ok_or_else(|| JjError::IoError(format!("Path '{}' not found in parent", file_path)))?
      }
    };
    String::from_utf8_lossy(&bytes).to_string()
  } else {
    // Read file from working directory
    let full_path = Path::new(workspace_path).join(file_path);
    fs::read_to_string(&full_path)
      .map_err(|e| JjError::IoError(format!("Failed to read file: {}", e)))?
  };

  let all_lines: Vec<&str> = content.lines().collect();
  let start_idx = start_line.saturating_sub(1).min(all_lines.len());
  let end_idx = end_line.min(all_lines.len()).max(start_idx);

  let lines: Vec<String> = all_lines[start_idx..end_idx]
    .iter()
    .map(|s| s.to_string())
    .collect();

  Ok(JjFileLines {
    lines,
    start_line: start_idx + 1,
    end_line: end_idx,
  })
}

// Mutation Operations (CLI fallbacks)

/// Restore a file to parent state (discard changes).
///
/// Snapshots on-disk content and restores in a single WC rewrite so we never
/// publish an intermediate dirty WC commit (which would rebase sibling
/// workspace WCs onto unrelated dirty paths).
// home-head-test: restore_file
pub fn jj_restore_file(workspace_path: &str, file_path: &str) -> Result<String, JjError> {
  if !Path::new(workspace_path).exists() {
    return Ok(String::new());
  }
  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let ws_name = loaded.workspace.workspace_name().to_owned();
  let wc_commit = get_workspace_wc_commit(&loaded)?
    .ok_or_else(|| JjError::IoError("No working-copy commit".to_string()))?;
  let parents = block_on(wc_commit.parents())
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy parents: {}", e)))?;
  let parent = parents
    .first()
    .ok_or_else(|| JjError::IoError("Working-copy commit has no parent".to_string()))?;
  let source_tree = parent.tree();

  let ignore_root =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  let tracking_matcher = repo_root_matcher();
  let opts = snapshot_options_for_all_paths(&ignore_root, &tracking_matcher);
  let mut locked_ws = loaded
    .workspace
    .start_working_copy_mutation()
    .map_err(|e| JjError::IoError(format!("Failed to lock working copy: {}", e)))?;
  let (snapshotted_tree, _) = block_on(locked_ws.locked_wc().snapshot(&opts))
    .map_err(|e| JjError::IoError(format!("Failed to snapshot working copy: {}", e)))?;

  let repo_file = RepoPathBuf::from_internal_string(file_path)
    .map_err(|e| JjError::IoError(format!("Invalid file path '{}': {}", file_path, e)))?;
  let matcher = PrefixMatcher::new([repo_file.as_ref()]);
  let restored_tree = block_on(rewrite::restore_tree(
    &source_tree,
    &snapshotted_tree,
    "source".to_string(),
    "destination".to_string(),
    &matcher,
  ))
  .map_err(|e| JjError::IoError(format!("Failed to restore file: {}", e)))?;

  let commit_needs_rewrite = restored_tree.tree_ids() != wc_commit.tree_ids();
  let disk_needs_checkout = restored_tree.tree_ids() != snapshotted_tree.tree_ids();
  if !commit_needs_rewrite && !disk_needs_checkout {
    block_on(locked_ws.finish(loaded.repo.op_id().clone()))
      .map_err(|e| JjError::IoError(format!("Failed to finish working copy snapshot: {}", e)))?;
    return Ok(String::new());
  }

  let new_repo = if commit_needs_rewrite {
    let mut tx = loaded.repo.start_transaction();
    let rewritten = block_on(
      tx.repo_mut()
        .rewrite_commit(&wc_commit)
        .set_tree(restored_tree)
        .write(),
    )
    .map_err(|e| JjError::IoError(format!("Failed to rewrite working-copy commit: {}", e)))?;
    block_on(tx.repo_mut().edit(ws_name, &rewritten)).map_err(|e| {
      JjError::IoError(format!(
        "Failed to update working-copy commit pointer: {}",
        e
      ))
    })?;
    block_on(tx.repo_mut().rebase_descendants())
      .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;
    let new_repo = block_on(tx.commit("restore file"))
      .map_err(|e| JjError::IoError(format!("Failed to commit restore: {}", e)))?;
    block_on(locked_ws.finish(new_repo.op_id().clone()))
      .map_err(|e| JjError::IoError(format!("Failed to finish working copy snapshot: {}", e)))?;
    new_repo
  } else {
    block_on(locked_ws.finish(loaded.repo.op_id().clone()))
      .map_err(|e| JjError::IoError(format!("Failed to finish working copy snapshot: {}", e)))?;
    Arc::clone(&loaded.repo)
  };

  // old_wc is None: tree_state already reflects the snapshotted disk, which
  // may differ from wc_commit.tree(), so the ConcurrentCheckout guard would
  // false-positive if we passed the pre-snapshot commit.
  update_workspace_after_history_edit(&mut loaded, &new_repo, None, CheckoutMode::Immediate)?;
  Ok(String::new())
}

/// Restore all changes
// home-head-test: restore_discarded_changes
pub fn jj_restore_all(workspace_path: &str) -> Result<String, JjError> {
  if !Path::new(workspace_path).exists() {
    return Ok(String::new());
  }
  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let ws_name = loaded.workspace.workspace_name().to_owned();
  let wc_commit = get_workspace_wc_commit(&loaded)?
    .ok_or_else(|| JjError::IoError("No working-copy commit".to_string()))?;
  let parents = block_on(wc_commit.parents())
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy parents: {}", e)))?;
  let parent = parents
    .first()
    .ok_or_else(|| JjError::IoError("Working-copy commit has no parent".to_string()))?;
  let source_tree = parent.tree();

  let ignore_root =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  let tracking_matcher = repo_root_matcher();
  let opts = snapshot_options_for_all_paths(&ignore_root, &tracking_matcher);
  let mut locked_ws = loaded
    .workspace
    .start_working_copy_mutation()
    .map_err(|e| JjError::IoError(format!("Failed to lock working copy: {}", e)))?;
  let (snapshotted_tree, _) = block_on(locked_ws.locked_wc().snapshot(&opts))
    .map_err(|e| JjError::IoError(format!("Failed to snapshot working copy: {}", e)))?;

  let commit_needs_rewrite = source_tree.tree_ids() != wc_commit.tree_ids();
  let disk_needs_checkout = source_tree.tree_ids() != snapshotted_tree.tree_ids();
  if !commit_needs_rewrite && !disk_needs_checkout {
    block_on(locked_ws.finish(loaded.repo.op_id().clone()))
      .map_err(|e| JjError::IoError(format!("Failed to finish working copy snapshot: {}", e)))?;
    return Ok(String::new());
  }

  let new_repo = if commit_needs_rewrite {
    let mut tx = loaded.repo.start_transaction();
    let rewritten = block_on(
      tx.repo_mut()
        .rewrite_commit(&wc_commit)
        .set_tree(source_tree)
        .write(),
    )
    .map_err(|e| JjError::IoError(format!("Failed to rewrite working-copy commit: {}", e)))?;
    block_on(tx.repo_mut().edit(ws_name, &rewritten)).map_err(|e| {
      JjError::IoError(format!(
        "Failed to update working-copy commit pointer: {}",
        e
      ))
    })?;
    block_on(tx.repo_mut().rebase_descendants())
      .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;
    let new_repo = block_on(tx.commit("restore all"))
      .map_err(|e| JjError::IoError(format!("Failed to commit restore: {}", e)))?;
    block_on(locked_ws.finish(new_repo.op_id().clone()))
      .map_err(|e| JjError::IoError(format!("Failed to finish working copy snapshot: {}", e)))?;
    new_repo
  } else {
    block_on(locked_ws.finish(loaded.repo.op_id().clone()))
      .map_err(|e| JjError::IoError(format!("Failed to finish working copy snapshot: {}", e)))?;
    Arc::clone(&loaded.repo)
  };

  update_workspace_after_history_edit(&mut loaded, &new_repo, None, CheckoutMode::Immediate)?;
  Ok(String::new())
}

/// Capture a token for the current working-copy content, so it can later be
/// restored with `jj_restore_snapshot` (used to undo a discard).
///
/// The token is the hex id of the current working-copy commit. Since jj
/// backends are content-addressed and don't delete objects until an explicit
/// gc, this commit's tree remains fetchable even after the working copy is
/// rewritten out from under it (e.g. by `jj_restore_file`/`jj_restore_all`).
pub fn jj_snapshot_working_copy(workspace_path: &str) -> Result<String, JjError> {
  let loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let wc_commit = get_workspace_wc_commit(&loaded)?
    .ok_or_else(|| JjError::IoError("No working-copy commit".to_string()))?;
  Ok(wc_commit.id().hex())
}

/// Restore the working copy to a snapshot previously captured with
/// `jj_snapshot_working_copy`.
// home-head-test: restore_discarded_changes
pub fn jj_restore_snapshot(workspace_path: &str, snapshot_id: &str) -> Result<String, JjError> {
  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let ws_name = loaded.workspace.workspace_name().to_owned();
  let wc_commit = get_workspace_wc_commit(&loaded)?
    .ok_or_else(|| JjError::IoError("No working-copy commit".to_string()))?;
  let snapshot_commit_id = jj_lib::backend::CommitId::try_from_hex(snapshot_id)
    .ok_or_else(|| JjError::IoError("Invalid snapshot id".to_string()))?;
  let snapshot_commit = loaded
    .repo
    .store()
    .get_commit(&snapshot_commit_id)
    .map_err(|e| JjError::IoError(format!("Snapshot is no longer available to restore: {}", e)))?;
  let source_tree = snapshot_commit.tree();
  let destination_tree = wc_commit.tree();
  if source_tree.tree_ids() == destination_tree.tree_ids() {
    return Ok(String::new());
  }
  let mut tx = loaded.repo.start_transaction();
  let rewritten = block_on(
    tx.repo_mut()
      .rewrite_commit(&wc_commit)
      .set_tree(source_tree)
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to rewrite working-copy commit: {}", e)))?;
  block_on(tx.repo_mut().edit(ws_name, &rewritten)).map_err(|e| {
    JjError::IoError(format!(
      "Failed to update working-copy commit pointer: {}",
      e
    ))
  })?;
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;
  let new_repo = block_on(tx.commit("undo discard"))
    .map_err(|e| JjError::IoError(format!("Failed to commit restore: {}", e)))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    Some(&wc_commit),
    CheckoutMode::Immediate,
  )?;
  Ok(String::new())
}

/// Prefix for bookmarks that keep stash commits reachable. Filtered from branch lists.
pub const STASH_BOOKMARK_PREFIX: &str = "treq/stash/";

/// Result of creating an immutable stash commit from a working copy.
#[derive(Debug, Clone)]
pub struct JjStashCommit {
  pub commit_id: String,
  pub change_id: String,
  pub short_commit_id: String,
  pub additions: u32,
  pub deletions: u32,
  pub files_changed: Vec<String>,
  pub bookmark_name: String,
}

/// Capture working-copy changes into an immutable stash commit, then restore the
/// working copy to a clean state (changes are moved out of the WC).
// home-head-test: stash_and_apply_working_copy
pub fn jj_stash_working_copy(
  workspace_path: &str,
  bookmark_name: &str,
) -> Result<JjStashCommit, JjError> {
  if !bookmark_name.starts_with(STASH_BOOKMARK_PREFIX) {
    return Err(JjError::IoError(format!(
      "Stash bookmark must start with '{}'",
      STASH_BOOKMARK_PREFIX
    )));
  }
  if !Path::new(workspace_path).exists() {
    return Err(JjError::IoError(
      "Workspace path does not exist".to_string(),
    ));
  }

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let ws_name = loaded.workspace.workspace_name().to_owned();
  let (wc_commit_id, stash_tree) = snapshot_working_copy_tree(&mut loaded, workspace_path)?
    .ok_or_else(|| JjError::IoError("No working-copy commit to stash".to_string()))?;
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {}", e)))?;
  let parent_tree = block_on(wc_commit.parent_tree(loaded.repo.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to load parent tree: {}", e)))?;

  if stash_tree.tree_ids() == parent_tree.tree_ids() {
    return Err(JjError::IoError(
      "Nothing to stash: working copy has no changes".to_string(),
    ));
  }

  let parents = block_on(wc_commit.parents())
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy parents: {}", e)))?;
  let parent_ids: Vec<_> = if parents.is_empty() {
    vec![loaded.repo.store().root_commit_id().clone()]
  } else {
    parents.iter().map(|p| p.id().clone()).collect()
  };

  let matcher = repo_root_matcher();
  let mut files_changed = Vec::new();
  let mut diff_stream = parent_tree.diff_stream(&stash_tree, &matcher);
  while let Some(entry) = block_on(diff_stream.next()) {
    files_changed.push(entry.path.as_internal_file_string().to_string());
  }
  if files_changed.is_empty() {
    return Err(JjError::IoError(
      "Nothing to stash: working copy has no changes".to_string(),
    ));
  }

  let (additions, deletions) = {
    // Temporarily wrap stash tree as stats against parent via a throwaway commit shape.
    let stats_commit = &wc_commit;
    compute_commit_stats(&loaded.repo, stats_commit, Some(&stash_tree))
  };

  let mut tx = loaded.repo.start_transaction();
  let stash_commit = block_on(
    tx.repo_mut()
      .new_commit(parent_ids, stash_tree)
      .set_description(format!("treq stash: {}", bookmark_name))
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to write stash commit: {}", e)))?;

  set_local_bookmark_target(tx.repo_mut(), bookmark_name, stash_commit.id().clone());

  // Restore WC to the parent tree (move changes out of the working copy).
  let restored = block_on(
    tx.repo_mut()
      .rewrite_commit(&wc_commit)
      .set_tree(parent_tree)
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to restore working copy after stash: {}", e)))?;
  block_on(tx.repo_mut().edit(ws_name, &restored)).map_err(|e| {
    JjError::IoError(format!(
      "Failed to update working-copy pointer after stash: {}",
      e
    ))
  })?;
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants after stash: {}", e)))?;
  let _ = git::export_refs(tx.repo_mut());

  let new_repo = block_on(tx.commit("stash working copy"))
    .map_err(|e| JjError::IoError(format!("Failed to commit stash transaction: {}", e)))?;
  // old_wc is None: snapshot_working_copy_tree already updated tree_state to the
  // snapshotted disk tree, which may differ from wc_commit.tree(), so the
  // ConcurrentCheckout guard would false-positive if we passed the pre-snapshot commit.
  update_workspace_after_history_edit(&mut loaded, &new_repo, None, CheckoutMode::Immediate)?;

  let change_id = HexPrefix::from_id(stash_commit.change_id()).reverse_hex()[..12].to_string();
  let short_commit_id = short_commit_id(&stash_commit);
  Ok(JjStashCommit {
    commit_id: stash_commit.id().hex(),
    change_id,
    short_commit_id,
    additions,
    deletions,
    files_changed,
    bookmark_name: bookmark_name.to_string(),
  })
}

/// Park an existing commit into an immutable stash: duplicate its tree as a
/// stash commit under `bookmark_name`, then abandon the original change so it
/// leaves the branch. The stash can later be applied (copied) onto any workspace.
// home-head-test: stash_commit
pub fn jj_stash_commit(
  workspace_path: &str,
  change_id: &str,
  bookmark_name: &str,
) -> Result<JjStashCommit, JjError> {
  if !bookmark_name.starts_with(STASH_BOOKMARK_PREFIX) {
    return Err(JjError::IoError(format!(
      "Stash bookmark must start with '{}'",
      STASH_BOOKMARK_PREFIX
    )));
  }
  if !Path::new(workspace_path).exists() {
    return Err(JjError::IoError(
      "Workspace path does not exist".to_string(),
    ));
  }

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, change_id)?;

  if commit.id() == loaded.repo.store().root_commit_id() {
    return Err(JjError::IoError("Cannot stash the root commit".to_string()));
  }

  let parents = block_on(commit.parents())
    .map_err(|e| JjError::IoError(format!("Failed to load commit parents: {}", e)))?;
  let parent_ids: Vec<_> = if parents.is_empty() {
    vec![loaded.repo.store().root_commit_id().clone()]
  } else {
    parents.iter().map(|p| p.id().clone()).collect()
  };
  let commit_tree = commit.tree();
  let parent_tree = block_on(commit.parent_tree(loaded.repo.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to load parent tree: {}", e)))?;

  if commit_tree.tree_ids() == parent_tree.tree_ids() {
    return Err(JjError::IoError(
      "Nothing to stash: commit has no file changes".to_string(),
    ));
  }

  let matcher = repo_root_matcher();
  let mut files_changed = Vec::new();
  let mut diff_stream = parent_tree.diff_stream(&commit_tree, &matcher);
  while let Some(entry) = block_on(diff_stream.next()) {
    files_changed.push(entry.path.as_internal_file_string().to_string());
  }
  if files_changed.is_empty() {
    return Err(JjError::IoError(
      "Nothing to stash: commit has no file changes".to_string(),
    ));
  }

  let (additions, deletions) = compute_commit_stats(&loaded.repo, &commit, None);
  let original_description = commit.description().to_string();
  let stash_description = if original_description.trim().is_empty() {
    format!("treq stash: {}", bookmark_name)
  } else {
    format!("treq stash: {}", original_description.trim())
  };

  let mut tx = loaded.repo.start_transaction();
  let stash_commit = block_on(
    tx.repo_mut()
      .new_commit(parent_ids, commit_tree)
      .set_description(stash_description)
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to write stash commit: {}", e)))?;

  set_local_bookmark_target(tx.repo_mut(), bookmark_name, stash_commit.id().clone());

  // Remove the original from the branch while keeping the stash bookmark reachable.
  tx.repo_mut().record_abandoned_commit(&commit);
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants after stash: {}", e)))?;
  let _ = git::export_refs(tx.repo_mut());

  let new_repo = block_on(tx.commit("stash commit"))
    .map_err(|e| JjError::IoError(format!("Failed to commit stash-commit transaction: {}", e)))?;
  update_workspace_after_history_edit(&mut loaded, &new_repo, None, CheckoutMode::Immediate)?;

  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  let _ = reconcile_all_workspaces_after_rewrite(&repo_path, None);

  let change_id_hex = HexPrefix::from_id(stash_commit.change_id()).reverse_hex()[..12].to_string();
  let short_commit_id = short_commit_id(&stash_commit);
  Ok(JjStashCommit {
    commit_id: stash_commit.id().hex(),
    change_id: change_id_hex,
    short_commit_id,
    additions,
    deletions,
    files_changed,
    bookmark_name: bookmark_name.to_string(),
  })
}

/// Copy the file changes from a stash commit onto a target working copy.
/// The stash commit itself is left untouched (immutable / re-applicable).
// home-head-test: stash_and_apply_working_copy
pub fn jj_apply_stash_commit(workspace_path: &str, stash_commit_id: &str) -> Result<(), JjError> {
  if !Path::new(workspace_path).exists() {
    return Err(JjError::IoError(
      "Workspace path does not exist".to_string(),
    ));
  }

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let ws_name = loaded.workspace.workspace_name().to_owned();
  let (wc_commit_id, current_tree) = snapshot_working_copy_tree(&mut loaded, workspace_path)?
    .ok_or_else(|| JjError::IoError("No working-copy commit on target".to_string()))?;
  let wc_commit = loaded
    .repo
    .store()
    .get_commit(&wc_commit_id)
    .map_err(|e| JjError::IoError(format!("Failed to load working-copy commit: {}", e)))?;

  let stash_commit_id = jj_lib::backend::CommitId::try_from_hex(stash_commit_id)
    .ok_or_else(|| JjError::IoError("Invalid stash commit id".to_string()))?;
  let stash_commit = loaded
    .repo
    .store()
    .get_commit(&stash_commit_id)
    .map_err(|e| JjError::IoError(format!("Stash commit is no longer available: {}", e)))?;
  let stash_tree = stash_commit.tree();
  let stash_parent_tree = block_on(stash_commit.parent_tree(loaded.repo.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to load stash parent tree: {}", e)))?;

  let matcher = repo_root_matcher();
  let mut applied_tree_builder = MergedTreeBuilder::new(current_tree.clone());
  let diff_entries = block_on(
    stash_parent_tree
      .diff_stream(&stash_tree, &matcher)
      .collect::<Vec<_>>(),
  );
  let mut applied_any = false;
  for entry in diff_entries {
    let values = entry
      .values
      .map_err(|e| JjError::IoError(format!("Failed to read stash diff entry: {}", e)))?;
    applied_tree_builder.set_or_remove(entry.path, values.after);
    applied_any = true;
  }
  if !applied_any {
    return Ok(());
  }

  let applied_tree = block_on(applied_tree_builder.write_tree())
    .map_err(|e| JjError::IoError(format!("Failed to write applied stash tree: {}", e)))?;

  if applied_tree.tree_ids() == current_tree.tree_ids() {
    return Ok(());
  }

  let mut tx = loaded.repo.start_transaction();
  let rewritten = block_on(
    tx.repo_mut()
      .rewrite_commit(&wc_commit)
      .set_tree(applied_tree)
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to rewrite working copy with stash: {}", e)))?;
  block_on(tx.repo_mut().edit(ws_name, &rewritten)).map_err(|e| {
    JjError::IoError(format!(
      "Failed to update working-copy pointer after apply: {}",
      e
    ))
  })?;
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants after apply: {}", e)))?;
  let new_repo = block_on(tx.commit("apply stash"))
    .map_err(|e| JjError::IoError(format!("Failed to commit apply-stash transaction: {}", e)))?;
  // Same ConcurrentCheckout caveat as stash: snapshot may have moved tree_state.
  update_workspace_after_history_edit(&mut loaded, &new_repo, None, CheckoutMode::Immediate)?;
  Ok(())
}

/// Export a stash (or any) commit as a unified git patch string.
pub fn jj_export_commit_git_patch(
  workspace_path: &str,
  commit_id_hex: &str,
) -> Result<String, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let commit_id = jj_lib::backend::CommitId::try_from_hex(commit_id_hex)
    .ok_or_else(|| JjError::IoError("Invalid commit id".to_string()))?;
  let commit = loaded
    .repo
    .store()
    .get_commit(&commit_id)
    .map_err(|e| JjError::IoError(format!("Commit is no longer available: {}", e)))?;
  let parent_tree = block_on(commit.parent_tree(loaded.repo.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to load parent tree: {}", e)))?;
  let commit_tree = commit.tree();
  let matcher = repo_root_matcher();
  let copy_records = CopyRecords::default();
  let conflict_labels = Diff::new(parent_tree.labels(), commit_tree.labels());
  let diff_stream = materialized_diff_stream(
    loaded.repo.store(),
    parent_tree.diff_stream_with_copies(&commit_tree, &matcher, &copy_records),
    conflict_labels,
  );

  let mut patches = Vec::new();
  block_on(async {
    futures::pin_mut!(diff_stream);
    while let Some(entry) = diff_stream.next().await {
      let values = match entry.values {
        Ok(values) => values,
        Err(_) => continue,
      };
      let path = entry.path.target().as_internal_file_string().to_string();
      let before_absent = matches!(&values.before, MaterializedTreeValue::Absent);
      let after_absent = matches!(&values.after, MaterializedTreeValue::Absent);
      let style = ConflictMarkerStyle::Git;
      let before_bytes =
        materialized_value_to_bytes(entry.path.source(), values.before, style, None).await;
      let after_bytes =
        materialized_value_to_bytes(entry.path.target(), values.after, style, None).await;

      let before_text = before_bytes
        .as_ref()
        .and_then(|b| String::from_utf8(b.clone()).ok())
        .unwrap_or_default();
      let after_text = after_bytes
        .as_ref()
        .and_then(|b| String::from_utf8(b.clone()).ok())
        .unwrap_or_default();

      let input = InternedInput::new(before_text.as_str(), after_text.as_str());
      let unified = diff(
        Algorithm::Histogram,
        &input,
        UnifiedDiffBuilder::new(&input),
      );

      let mut file_patch = String::new();
      file_patch.push_str(&format!("diff --git a/{path} b/{path}\n"));
      if before_absent && !after_absent {
        file_patch.push_str("new file mode 100644\n");
        file_patch.push_str(&format!("--- /dev/null\n+++ b/{path}\n"));
      } else if !before_absent && after_absent {
        file_patch.push_str("deleted file mode 100644\n");
        file_patch.push_str(&format!("--- a/{path}\n+++ /dev/null\n"));
      } else {
        file_patch.push_str(&format!("--- a/{path}\n+++ b/{path}\n"));
      }
      if unified.is_empty() {
        if before_absent && !after_absent {
          // Binary or empty new file — still emit a header-only section.
        } else if !before_absent && after_absent {
          // deleted
        }
      } else {
        file_patch.push_str(&unified);
        if !file_patch.ends_with('\n') {
          file_patch.push('\n');
        }
      }
      patches.push(file_patch);
    }
  });

  Ok(patches.join(""))
}

/// Delete the bookmark that keeps a stash commit reachable.
pub fn jj_delete_stash_bookmark(workspace_path: &str, bookmark_name: &str) -> Result<(), JjError> {
  if !bookmark_name.starts_with(STASH_BOOKMARK_PREFIX) {
    return Err(JjError::IoError(format!(
      "Refusing to delete non-stash bookmark '{}'",
      bookmark_name
    )));
  }
  jj_delete_bookmark(workspace_path, bookmark_name)
}

/// Set (or create) a jj bookmark to point at a specific revision
/// Uses: jj bookmark set <name> -r <revision>
pub fn jj_set_bookmark(
  workspace_path: &str,
  bookmark_name: &str,
  revision: &str,
) -> Result<(), JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, revision)?;
  let mut tx = loaded.repo.start_transaction();
  tx.repo_mut().set_local_bookmark_target(
    RefName::new(bookmark_name),
    RefTarget::normal(commit.id().clone()),
  );
  block_on(tx.commit("set bookmark target"))
    .map_err(|e| JjError::IoError(format!("Failed to set bookmark '{}': {}", bookmark_name, e)))?;
  Ok(())
}

/// Rename a local bookmark in a single transaction: the new name takes the old
/// name's target and the old name is removed.
pub fn jj_rename_bookmark(
  workspace_path: &str,
  old_name: &str,
  new_name: &str,
) -> Result<(), JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let target = loaded
    .repo
    .view()
    .get_local_bookmark(RefName::new(old_name))
    .clone();
  if target.is_absent() {
    return Err(JjError::IoError(format!(
      "Bookmark '{}' does not exist",
      old_name
    )));
  }
  let mut tx = loaded.repo.start_transaction();
  tx.repo_mut()
    .set_local_bookmark_target(RefName::new(new_name), target);
  tx.repo_mut()
    .set_local_bookmark_target(RefName::new(old_name), RefTarget::absent());
  block_on(tx.commit("rename bookmark")).map_err(|e| {
    JjError::IoError(format!(
      "Failed to rename bookmark '{}' to '{}': {}",
      old_name, new_name, e
    ))
  })?;
  Ok(())
}

/// Delete a jj bookmark
/// Uses: jj bookmark delete <name>
pub fn jj_delete_bookmark(workspace_path: &str, bookmark_name: &str) -> Result<(), JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let mut tx = loaded.repo.start_transaction();
  tx.repo_mut()
    .set_local_bookmark_target(RefName::new(bookmark_name), RefTarget::absent());
  let _ = git::export_refs(tx.repo_mut());
  block_on(tx.commit("delete bookmark")).map_err(|e| {
    JjError::IoError(format!(
      "Failed to delete bookmark '{}': {}",
      bookmark_name, e
    ))
  })?;
  Ok(())
}

/// Track a remote bookmark
/// Uses: jj bookmark track <name>@<remote>
pub fn jj_bookmark_track(
  workspace_path: &str,
  bookmark_name: &str,
  remote_name: &str,
) -> Result<(), JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let symbol = RemoteRefSymbol {
    name: RefName::new(bookmark_name),
    remote: RemoteName::new(remote_name),
  };
  let remotes = git::get_all_remote_names(loaded.repo.store())
    .map_err(|e| JjError::IoError(format!("Failed to list remotes: {}", e)))?;
  if !remotes.iter().any(|r| r.as_str() == remote_name) {
    return Err(JjError::IoError(format!(
      "Remote '{}' not found",
      remote_name
    )));
  }
  if !loaded.repo.view().get_remote_bookmark(symbol).is_present() {
    return Err(JjError::IoError(format!(
      "Remote bookmark '{}@{}' not found",
      bookmark_name, remote_name
    )));
  }
  let mut tx = loaded.repo.start_transaction();
  tx.repo_mut().track_remote_bookmark(symbol).map_err(|e| {
    JjError::IoError(format!(
      "Failed to track remote bookmark '{}@{}': {}",
      bookmark_name, remote_name, e
    ))
  })?;
  block_on(tx.commit("track remote bookmark"))
    .map_err(|e| JjError::IoError(format!("Failed to persist bookmark tracking: {}", e)))?;
  Ok(())
}

/// Check if a bookmark is tracked with a remote
/// Uses: jj bookmark list --all-remotes
/// Returns true if the bookmark has a tracking relationship with the specified remote
pub fn is_bookmark_tracked(
  workspace_path: &str,
  bookmark_name: &str,
  remote_name: &str,
) -> Result<bool, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let symbol = RemoteRefSymbol {
    name: RefName::new(bookmark_name),
    remote: RemoteName::new(remote_name),
  };
  Ok(loaded.repo.view().get_remote_bookmark(symbol).is_tracked())
}

fn remote_bookmark_exists(workspace_path: &str, bookmark_name: &str, remote_name: &str) -> bool {
  load_workspace_repo(workspace_path).is_ok_and(|loaded| {
    let symbol = RemoteRefSymbol {
      name: RefName::new(bookmark_name),
      remote: RemoteName::new(remote_name),
    };
    loaded.repo.view().get_remote_bookmark(symbol).is_present()
  })
}

/// Edit/switch to a bookmark (similar to git checkout)
/// Uses a multi-strategy approach to handle immutable commits
/// For colocated repos, also syncs git HEAD
// home-head-test: switch_branch, merge_workspace_into_home
pub fn jj_edit_bookmark(repo_path: &str, bookmark_name: &str) -> Result<String, JjError> {
  let mut loaded = load_workspace_repo_for_history_edit(repo_path)?;
  let destination = resolve_commit_by_revision(&loaded, bookmark_name)?;
  let current = resolve_commit_by_revision(&loaded, "@")?;
  if current.id() == destination.id() {
    return Ok(format!("Already at {}", bookmark_name));
  }

  let old_wc_commit = get_workspace_wc_commit(&loaded)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let mut tx = loaded.repo.start_transaction();
  let new_wc = block_on(
    tx.repo_mut()
      .new_commit(vec![destination.id().clone()], destination.tree())
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to create working copy commit: {}", e)))?;
  block_on(tx.repo_mut().edit(workspace_name, &new_wc))
    .map_err(|e| JjError::IoError(format!("Failed to move working copy: {}", e)))?;
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;
  let _ = git::export_refs(tx.repo_mut());
  let new_repo = block_on(tx.commit("edit bookmark"))
    .map_err(|e| JjError::IoError(format!("Failed to commit bookmark edit: {}", e)))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    old_wc_commit.as_ref(),
    CheckoutMode::Immediate,
  )?;

  // Home repo only: a workspace dir has no .git of its own.
  if derive_repo_path_from_workspace(repo_path).is_none() {
    sync_home_git_head_to_wc_parent(repo_path, Some(bookmark_name));
  }

  Ok(format!("Switched to {}", bookmark_name))
}

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
            let _ = git::export_refs(rtx.repo_mut());
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
  let _ = git::export_refs(tx.repo_mut());

  // Commit the transaction and finalize working copy
  let new_repo = block_on(tx.commit("commit_workspace"))
    .map_err(|e| JjError::IoError(format!("Failed to commit transaction: {}", e)))?;
  block_on(locked_ws.finish(new_repo.op_id().clone()))
    .map_err(|e| JjError::IoError(format!("Failed to finish wc mutation: {}", e)))?;

  // Reconcile sibling workspaces after rebase_descendants rewrites WCs (mirrors update_workspace_after_history_edit).
  let reconcile_repo_path = repo_path_opt.as_deref().unwrap_or(workspace_path);
  let _ =
    reconcile_all_workspaces_after_rewrite(reconcile_repo_path, Some(workspace_name.as_str()));

  // For home repos, keep Git HEAD symbolic to the active branch (avoid detached HEAD after jj commit).
  if repo_path_opt.is_none() {
    sync_home_git_head_to_wc_parent(
      workspace_path,
      Some(branch.as_str()).filter(|b| *b != "HEAD"),
    );
  }

  Ok(format!("Committed successfully to branch '{}'", branch))
}

/// Read the current git branch from .git/HEAD without shelling out
fn read_git_head_branch(repo_path: &str) -> Result<String, String> {
  let head_path = Path::new(repo_path).join(".git").join("HEAD");
  let content =
    fs::read_to_string(&head_path).map_err(|e| format!("Failed to read .git/HEAD: {}", e))?;
  let trimmed = content.trim();
  if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
    Ok(branch.to_string())
  } else {
    Ok("HEAD".to_string())
  }
}

fn git_local_branch_commit_hex(repo_path: &str, branch_name: &str) -> Option<String> {
  git_ref_commit_hex(repo_path, &format!("refs/heads/{branch_name}"))
}

fn git_remote_branch_commit_hex(
  repo_path: &str,
  remote_name: &str,
  branch_name: &str,
) -> Option<String> {
  git_ref_commit_hex(
    repo_path,
    &format!("refs/remotes/{remote_name}/{branch_name}"),
  )
}

fn git_ref_commit_hex(repo_path: &str, full_ref_name: &str) -> Option<String> {
  let repo = gix::open(repo_path).ok()?;
  let ref_name: gix::refs::FullName = full_ref_name.try_into().ok()?;
  let reference = repo.find_reference(&ref_name).ok()?;
  Some(reference.id().to_hex().to_string())
}

fn read_origin_head_default_branch(repo_path: &str) -> Option<String> {
  let origin_head = Path::new(repo_path)
    .join(".git")
    .join("refs")
    .join("remotes")
    .join("origin")
    .join("HEAD");
  let content = fs::read_to_string(origin_head).ok()?;
  let trimmed = content.trim();
  let prefix = "ref: refs/remotes/origin/";
  Some(trimmed.strip_prefix(prefix)?.to_string())
}

/// Moves colocated Git HEAD to the home `@-` after jj rewrote history. Best effort.
///
/// Git HEAD is the home repo's source of truth: [`reconcile_colocated_home_repo`]
/// checks `@` out onto HEAD whenever the two differ, without touching disk. A
/// HEAD left on a pre-rewrite commit would therefore turn every file the
/// rewrite changed into a working-copy change.
///
/// Exports jj's bookmarks first so the branch refs are current, then attaches
/// HEAD to a branch at `@-`: the one HEAD is on, else `preferred_branch` (HEAD's
/// branch before the rewrite detached it), else the only local branch there.
/// With no such branch HEAD is detached at `@-`. The index is reset only when
/// HEAD's commit changes, so staged state survives a no-op. A HEAD that Git
/// moved since jj last imported it is left alone.
fn sync_home_git_head_to_wc_parent(repo_path: &str, preferred_branch: Option<&str>) {
  if let Err(e) = sync_home_git_head_to_wc_parent_inner(repo_path, preferred_branch) {
    tracing::warn!("Warning: Failed to sync git HEAD to the working-copy parent: {e}");
  }
}

fn sync_home_git_head_to_wc_parent_inner(
  repo_path: &str,
  preferred_branch: Option<&str>,
) -> Result<(), String> {
  if derive_repo_path_from_workspace(repo_path).is_some()
    || !Path::new(repo_path).join(".git").exists()
  {
    return Ok(());
  }
  let attached_branch = read_git_head_branch(repo_path)
    .ok()
    .filter(|branch| branch != "HEAD");

  let loaded = load_workspace_repo(repo_path).map_err(|e| e.to_string())?;
  let _git_lock = FileLock::lock(loaded.workspace.repo_path().join("git_import_export.lock"))
    .map_err(|e| format!("Failed to lock Git import/export: {e}"))?;
  let loaded = load_workspace_repo(repo_path).map_err(|e| e.to_string())?;
  let Some(wc_commit) = get_workspace_wc_commit(&loaded).map_err(|e| e.to_string())? else {
    return Ok(());
  };
  let [parent_id] = wc_commit.parent_ids() else {
    return Ok(());
  };
  let parent_hex = parent_id.hex();
  let wc_hex = wc_commit.id().hex();

  let head_before = gix::open(repo_path)
    .ok()
    .and_then(|repo| repo.head_id().ok().map(|id| id.to_hex().to_string()));
  let recorded_head = loaded.repo.view().git_head().as_normal().map(|id| id.hex());
  if head_before.is_some() && recorded_head.is_some() && head_before != recorded_head {
    // Git moved HEAD since jj last read it (a terminal `git switch`/`commit`).
    // That move wins; the next read reconciles `@` onto it.
    return Ok(());
  }

  let mut tx = loaded.repo.start_transaction();
  git::export_refs(tx.repo_mut()).map_err(|e| format!("Failed to export refs: {e}"))?;
  let known_branches: Vec<&str> = [attached_branch.as_deref(), preferred_branch]
    .into_iter()
    .flatten()
    .collect();
  let known_branch_at = |commit_hex: &str| {
    known_branches
      .iter()
      .find(|branch| {
        git_local_branch_commit_hex(repo_path, branch).is_some_and(|tip| tip == commit_hex)
      })
      .map(|branch| branch.to_string())
  };
  // `@` can be the branch commit itself (a working copy synced by editing the
  // bookmark). HEAD then goes on that branch; the next read starts a fresh `@`
  // on top of it without touching disk.
  let (branch, head_hex) = match known_branch_at(&parent_hex) {
    Some(branch) => (Some(branch), &parent_hex),
    None => match known_branch_at(&wc_hex) {
      Some(branch) => (Some(branch), &wc_hex),
      None => (
        find_unambiguous_local_branch_at_commit(repo_path, &parent_hex),
        &parent_hex,
      ),
    },
  };
  match &branch {
    Some(branch) => set_git_head_branch_with_gix(repo_path, branch)?,
    None if head_before.as_deref() != Some(head_hex.as_str()) => {
      set_git_head_detached_with_gix(repo_path, head_hex)?
    }
    None => {}
  }
  if head_before.as_deref() != Some(head_hex.as_str()) {
    reset_git_index_to_head_with_gix(repo_path)?;
  }

  // Record the new HEAD in the same operation, so the next read has nothing
  // to import and an Undo of the rewrite still sees it as the latest action.
  block_on(git::import_head(tx.repo_mut())).map_err(|e| format!("Failed to import HEAD: {e}"))?;
  if tx.repo().has_changes() {
    block_on(tx.commit(GIT_REFS_EXPORT_OPERATION))
      .map_err(|e| format!("Failed to commit export: {e}"))?;
  }
  Ok(())
}

fn set_git_head_detached_with_gix(repo_path: &str, commit_hex: &str) -> Result<(), String> {
  let repo = gix::open(repo_path).map_err(|e| format!("Failed to open git repo with gix: {e}"))?;
  let id = gix::ObjectId::from_hex(commit_hex.as_bytes())
    .map_err(|e| format!("Invalid commit id {commit_hex}: {e}"))?;
  let head_name: gix::refs::FullName = "HEAD"
    .try_into()
    .map_err(|e| format!("Invalid HEAD ref name: {e}"))?;
  repo
    .edit_reference(RefEdit {
      change: Change::Update {
        log: Default::default(),
        expected: PreviousValue::Any,
        new: Target::Object(id),
      },
      name: head_name,
      deref: false,
    })
    .map_err(|e| format!("Failed to detach HEAD: {e}"))?;
  Ok(())
}

fn set_git_head_branch_with_gix(repo_path: &str, branch: &str) -> Result<(), String> {
  let repo = gix::open(repo_path).map_err(|e| format!("Failed to open git repo with gix: {e}"))?;
  let target: gix::refs::FullName = format!("refs/heads/{branch}")
    .try_into()
    .map_err(|e| format!("Invalid branch ref name: {e}"))?;
  let head_name: gix::refs::FullName = "HEAD"
    .try_into()
    .map_err(|e| format!("Invalid HEAD ref name: {e}"))?;

  repo
    .edit_reference(RefEdit {
      change: Change::Update {
        log: Default::default(),
        expected: PreviousValue::Any,
        new: Target::Symbolic(target),
      },
      name: head_name,
      deref: false,
    })
    .map_err(|e| format!("Failed to update HEAD ref: {e}"))?;
  Ok(())
}

/// Finds the single local branch (refs/heads/*) whose tip commit matches `commit_hex`.
///
/// Returns `None` when no local branch matches, or when more than one does (an ambiguous
/// match is left alone rather than guessed at).
fn find_unambiguous_local_branch_at_commit(repo_path: &str, commit_hex: &str) -> Option<String> {
  let repo = gix::open(repo_path).ok()?;
  let platform = repo.references().ok()?;
  let mut matches = platform
    .prefixed("refs/heads/")
    .ok()?
    .filter_map(Result::ok)
    .filter_map(|mut r| {
      let id = r.peel_to_id().ok()?;
      if id.to_hex().to_string() != commit_hex {
        return None;
      }
      r.name()
        .as_bstr()
        .to_string()
        .strip_prefix("refs/heads/")
        .map(|s| s.to_string())
    });
  let first = matches.next()?;
  if matches.next().is_some() {
    return None;
  }
  Some(first)
}

/// Reattach a detached colocated Git HEAD when it still points at a local branch tip.
///
/// jj history rewrites and ref export can leave Git's HEAD direct even though the checkout
/// is still exactly on a branch (the default branch, or the branch that was checked out
/// before an operation like commit-and-push detached HEAD at its own tip). In that state the
/// UI displays a commit hash and subsequent workspace creation can import the detached HEAD as
/// a separate line of history. A genuinely detached checkout is preserved unless its commit
/// unambiguously matches exactly one local branch, preferring the default branch when it
/// matches.
pub fn repair_detached_home_head_at_default_branch(
  repo_path: &str,
) -> Result<Option<String>, JjError> {
  if read_git_head_branch(repo_path).map_err(JjError::IoError)? != "HEAD" {
    return Ok(None);
  }

  let head_path = Path::new(repo_path).join(".git").join("HEAD");
  let detached_commit = fs::read_to_string(head_path)
    .map_err(|e| JjError::IoError(format!("Failed to read detached Git HEAD: {e}")))?;
  let detached_commit = detached_commit.trim();

  let default_branch = get_default_branch(repo_path)?;
  let matches_default = git_local_branch_commit_hex(repo_path, &default_branch)
    .is_some_and(|tip| tip == detached_commit);

  let branch = if matches_default {
    Some(default_branch)
  } else {
    find_unambiguous_local_branch_at_commit(repo_path, detached_commit)
  };

  let Some(branch) = branch else {
    return Ok(None);
  };

  set_git_head_branch_with_gix(repo_path, &branch).map_err(JjError::IoError)?;
  Ok(Some(branch))
}

fn reset_git_index_to_head_with_gix(repo_path: &str) -> Result<(), String> {
  let repo = gix::open(repo_path).map_err(|e| format!("Failed to open git repo with gix: {e}"))?;
  let head_tree = repo
    .head_commit()
    .map_err(|e| format!("Failed to read HEAD commit: {e}"))?
    .tree_id()
    .map_err(|e| format!("Failed to read HEAD tree id: {e}"))?;
  let mut index = repo
    .index_from_tree(&head_tree)
    .map_err(|e| format!("Failed to create index from HEAD tree: {e}"))?;
  index
    .write(Default::default())
    .map_err(|e| format!("Failed to write index: {e}"))?;
  Ok(())
}

pub fn resolve_home_repo_branch(repo_path: &str) -> Result<String, JjError> {
  let git_branch = read_git_head_branch(repo_path)
    .map_err(|e| JjError::IoError(format!("Failed to determine branch: {}", e)))?;
  Ok(git_branch)
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
  let _ = git::export_refs(tx.repo_mut());
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

// home-head-test: workspace_bookmark_rebase
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
  let workspace_revision = resolve_branch_revision_for_rebase(workspace_path, workspace_branch)?;
  let target_revision = resolve_branch_revision_for_rebase(workspace_path, target_branch)?;
  let workspace_tip_commit = resolve_commit_by_revision(&loaded, &workspace_revision)?;
  let target_commit = resolve_commit_by_revision(&loaded, &target_revision)?;
  let mut lineage_base = target_commit.id().hex();
  if let Some(old_target) = old_target_branch.filter(|old| *old != target_branch) {
    // Without the old parent's tip the rebase would drag its commits along.
    let old_revision = resolve_branch_revision_for_rebase(workspace_path, old_target)?;
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

  let _ = git::export_refs(tx.repo_mut());

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
  import_colocated_git_state(&mut loaded, workspace_path)?;

  let snapshotted_tree_override =
    snapshot_working_copy_tree(&mut loaded, workspace_path)?.map(|(_, tree)| tree);

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
  let loaded = load_workspace_repo(repo_path)?;
  match resolve_commit_by_revision(&loaded, revision) {
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
    let _ = update_workspace_after_history_edit(
      &mut loaded,
      &current_repo,
      old_wc_commit.as_ref(),
      CheckoutMode::Immediate,
    );
    return Ok(JjRebaseResult {
      success: true,
      message: "Nothing to rebase (empty revision set)".to_string(),
    });
  }
  let target_revision = resolve_branch_revision_for_rebase(working_dir, target_branch)?;
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
  let _ = git::export_refs(tx.repo_mut());
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
  let _ = git::export_refs(tx.repo_mut());
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

/// Get the default branch of the repository.
/// Checks git symbolic-ref for origin/HEAD, then checks for main/master,
/// and finally falls back to the currently checked-out branch.
pub fn get_default_branch(repo_path: &str) -> Result<String, JjError> {
  // Try origin/HEAD first.
  if let Some(branch) = read_origin_head_default_branch(repo_path) {
    if !branch.is_empty() {
      return Ok(branch);
    }
  }

  // Fallback: check for main or master branches.
  for branch in &["main", "master"] {
    if git_local_branch_commit_hex(repo_path, branch).is_some() {
      return Ok(branch.to_string());
    }
  }

  // Final fallback: git config init.defaultBranch (merged global + local config).
  if let Some(branch) = gix::open(repo_path).ok().and_then(|r| {
    r.config_snapshot()
      .string("init.defaultBranch")
      .map(|s| s.to_string())
  }) {
    if !branch.is_empty() {
      return Ok(branch);
    }
  }

  Ok("main".to_string())
}

/// Push changes to remote using jj git push
pub fn jj_push(workspace_path: &str) -> Result<String, JjError> {
  // Get current branch name to check/ensure tracking
  let branch_name = get_workspace_branch(workspace_path)?;
  if branch_name.is_empty() || branch_name == "HEAD" {
    return Err(JjError::IoError(
      "Cannot push: not on a branch (HEAD is detached). Check out a branch first.".to_string(),
    ));
  }

  // Ensure bookmark is tracked before push to avoid non-tracking bookmark warnings.
  let mut tracking_message = String::new();

  match is_bookmark_tracked(workspace_path, &branch_name, "origin") {
    Ok(true) => {
      // Already tracked, proceed normally
    }
    // A branch not yet on origin has nothing to track; the push creates it.
    Ok(false) if !remote_bookmark_exists(workspace_path, &branch_name, "origin") => {}
    Ok(false) => {
      // Not tracked, attempt to set up tracking
      tracking_message.push_str(&format!(
        "Warning: Bookmark '{}' was not tracked. Attempting to set up tracking...\n",
        branch_name
      ));

      if let Err(e) = jj_bookmark_track(workspace_path, &branch_name, "origin") {
        tracking_message.push_str(&format!(
          "Warning: Could not set up tracking: {}. Attempting push anyway...\n",
          e
        ));
      }
    }
    Err(e) => {
      // Error checking, log but continue
      tracking_message.push_str(&format!(
        "Warning: Could not verify tracking status: {}. Attempting push anyway...\n",
        e
      ));
    }
  }

  let loaded = load_workspace_repo(workspace_path)?;
  let mut tx = loaded.repo.start_transaction();
  let subprocess_options = git_subprocess_options(&loaded.settings)?;
  let local_target = tx
    .repo()
    .view()
    .get_local_bookmark(RefName::new(&branch_name))
    .clone();
  let remote_symbol = RemoteRefSymbol {
    name: RefName::new(&branch_name),
    remote: RemoteName::new("origin"),
  };
  let remote_ref = tx.repo().view().get_remote_bookmark(remote_symbol);
  let update = match classify_ref_push_action(LocalAndRemoteRef {
    local_target: &local_target,
    remote_ref,
  }) {
    RefPushAction::Update(update) => Some(update),
    RefPushAction::AlreadyMatches => None,
    RefPushAction::LocalConflicted => {
      return Err(JjError::IoError(
        "Cannot push conflicted local bookmark".to_string(),
      ))
    }
    RefPushAction::RemoteConflicted => {
      return Err(JjError::IoError(
        "Cannot push to conflicted remote bookmark".to_string(),
      ))
    }
    RefPushAction::RemoteUntracked => Some(Diff::new(
      remote_ref.target.as_normal().cloned(),
      local_target.as_normal().cloned(),
    )),
  };
  if let Some(update) = update {
    ensure_no_conflicted_commits_to_push(tx.repo(), &update)?;
    let targets = git::GitPushRefTargets {
      bookmarks: vec![(RefName::new(&branch_name).to_owned(), update)],
    };
    let mut callback = SilentGitCallback;
    let _ = git::push_refs(
      tx.repo_mut(),
      subprocess_options,
      RemoteName::new("origin"),
      &targets,
      &mut callback,
      &git::GitPushOptions::default(),
    )
    .map_err(|e| JjError::IoError(format!("Failed to push bookmark: {}", e)))?;
    block_on(tx.commit("push bookmark"))
      .map_err(|e| JjError::IoError(format!("Failed to commit push metadata: {}", e)))?;
  }
  Ok(tracking_message)
}

/// Refuse to push commits with conflicts, like `jj git push`: git readers would
/// only see one side of each conflicted file.
fn ensure_no_conflicted_commits_to_push(
  repo: &dyn jj_lib::repo::Repo,
  update: &Diff<Option<jj_lib::backend::CommitId>>,
) -> Result<(), JjError> {
  let Some(new_head) = update.after.clone() else {
    return Ok(());
  };
  let old_heads: Vec<_> = repo
    .view()
    .remote_bookmarks(RemoteName::new("origin"))
    .flat_map(|(_, remote_ref)| remote_ref.target.added_ids().cloned())
    .collect();
  let to_push = revset::RevsetExpression::commits(old_heads)
    .range(&revset::RevsetExpression::commits(vec![new_head]))
    .evaluate(repo)
    .map_err(|e| JjError::IoError(format!("Failed to find commits to push: {}", e)))?;
  for commit in to_push.iter().commits(repo.store()) {
    let commit =
      commit.map_err(|e| JjError::IoError(format!("Failed to find commits to push: {}", e)))?;
    if commit.has_conflict() {
      return Err(JjError::IoError(format!(
        "Cannot push: commit {} has conflicts. Resolve them first.",
        commit.id().hex()
      )));
    }
  }
  Ok(())
}

/// Get bookmarks on a given revision
pub fn get_bookmarks_on_revision(
  workspace_path: &str,
  revision: &str,
) -> Result<Vec<String>, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, revision)?;
  let bookmarks: Vec<String> = loaded
    .repo
    .view()
    .local_bookmarks_for_commit(commit.id())
    .map(|(name, _)| name.as_str().to_string())
    .collect();
  Ok(bookmarks)
}

/// Get sync status with remote (ahead/behind counts)
/// Returns (ahead_count, behind_count)
///
/// If the branch is not on remote and not_on_remote is false, logs an error.
/// If the branch is not on remote and not_on_remote is true, silently returns (0, 0).
pub fn jj_get_sync_status(
  workspace_path: &str,
  branch_name: &str,
  not_on_remote: bool,
) -> Result<(usize, usize), JjError> {
  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  let remote_branch = format!("{}@origin", branch_name);

  if !check_remote_branch_exists(&repo_path, &remote_branch)? {
    return Ok((0, 0));
  }

  if jj_is_bookmark_conflicted(workspace_path, branch_name) {
    return jj_get_diverged_sync_counts(workspace_path, branch_name);
  }

  let (local_tip, remote_tip) = get_sync_revset_tips(workspace_path, branch_name, false)?;
  let ahead_revset = format!("({}..{}) ~ empty()", remote_tip, local_tip);
  let behind_revset = format!("({}..{}) ~ empty()", local_tip, remote_tip);

  let ahead_ids = match jj_log_revset_commit_ids(workspace_path, &ahead_revset) {
    Ok(ids) => ids,
    Err(e) => {
      if !not_on_remote {
        tracing::warn!("[sync_status] Failed to get ahead count: {}", e);
      }
      Vec::new()
    }
  };
  let behind_ids = match jj_log_revset_commit_ids(workspace_path, &behind_revset) {
    Ok(ids) => ids,
    Err(e) => {
      if !not_on_remote {
        tracing::warn!("[sync_status] Failed to get behind count: {}", e);
      }
      Vec::new()
    }
  };

  let ahead_count = ahead_ids.len();
  let mut behind_count = behind_ids.len();

  if ahead_count > 0 && behind_count > 0 {
    if let Ok(loaded) = load_workspace_repo(workspace_path) {
      let ahead_change_ids: HashSet<String> = ahead_ids
        .iter()
        .filter_map(|id| resolve_commit_by_revision(&loaded, id).ok())
        .map(|commit| HexPrefix::from_id(commit.change_id()).reverse_hex())
        .collect();
      if !ahead_change_ids.is_empty() {
        behind_count = behind_ids
          .iter()
          .filter_map(|id| resolve_commit_by_revision(&loaded, id).ok())
          .map(|commit| HexPrefix::from_id(commit.change_id()).reverse_hex())
          .filter(|change_id| !ahead_change_ids.contains(change_id))
          .count();
      }
    }
  }
  Ok((ahead_count, behind_count))
}

/// The state of a workspace's local bookmark in the jj repo view.
#[derive(Debug, PartialEq)]
pub enum WorkspaceBookmarkState {
  Healthy,
  Conflicted,
  Missing,
}

/// Classify a workspace bookmark as Healthy, Conflicted, or Missing.
///
/// Loads the home repo at `repo_path` (not the workspace subdirectory) so that
/// this works even when the workspace directory does not exist yet.
pub fn classify_workspace_bookmark(
  repo_path: &str,
  branch_name: &str,
) -> Result<WorkspaceBookmarkState, JjError> {
  let mut loaded = load_workspace_repo(repo_path)?;
  import_colocated_git_state(&mut loaded, repo_path)?;
  let view = loaded.repo.view();
  let local = view.get_local_bookmark(RefName::new(branch_name));
  if local.has_conflict() {
    return Ok(WorkspaceBookmarkState::Conflicted);
  }
  if !local.is_absent() {
    return Ok(WorkspaceBookmarkState::Healthy);
  }
  // Local bookmark absent but remote tracking ref exists — branch not yet promoted local.
  let remote = view.get_remote_bookmark(RemoteRefSymbol {
    name: RefName::new(branch_name),
    remote: RemoteName::new("origin"),
  });
  if !remote.is_absent() {
    return Ok(WorkspaceBookmarkState::Healthy);
  }
  Ok(WorkspaceBookmarkState::Missing)
}

/// Like `classify_workspace_bookmark`, but loads from the workspace directory itself
/// so that `import_git_head_if_needed` reads that workspace's own `.git/HEAD` — useful
/// for recovering a missing bookmark when the DB holds the wrong branch name.
pub fn classify_workspace_bookmark_with_import(
  workspace_path: &str,
  branch_name: &str,
) -> Result<WorkspaceBookmarkState, JjError> {
  let mut loaded = load_workspace_repo(workspace_path)?;
  import_colocated_git_state(&mut loaded, workspace_path)?;
  let view = loaded.repo.view();
  let local = view.get_local_bookmark(RefName::new(branch_name));
  if local.has_conflict() {
    return Ok(WorkspaceBookmarkState::Conflicted);
  }
  if !local.is_absent() {
    return Ok(WorkspaceBookmarkState::Healthy);
  }
  let remote = view.get_remote_bookmark(RemoteRefSymbol {
    name: RefName::new(branch_name),
    remote: RemoteName::new("origin"),
  });
  if !remote.is_absent() {
    return Ok(WorkspaceBookmarkState::Healthy);
  }
  Ok(WorkspaceBookmarkState::Missing)
}

/// Check if a bookmark is in a conflicted (diverged) state.
///
/// This happens when both local and remote have diverged from a common ancestor.
pub fn jj_is_bookmark_conflicted(workspace_path: &str, branch_name: &str) -> bool {
  let Ok(loaded) = load_workspace_repo(workspace_path) else {
    return false;
  };
  loaded
    .repo
    .view()
    .get_local_bookmark(RefName::new(branch_name))
    .has_conflict()
}

/// Get sync counts for a diverged (conflicted) bookmark.
///
/// When a bookmark is conflicted, the bookmark name can't be used directly in revsets.
/// Instead, we use `@-` (parent of working copy) as a proxy for the local bookmark tip.
pub fn jj_get_diverged_sync_counts(
  workspace_path: &str,
  branch_name: &str,
) -> Result<(usize, usize), JjError> {
  let (local_tip, remote_tip) = get_sync_revset_tips(workspace_path, branch_name, true)?;
  let ahead_revset = format!("({}..{}) ~ empty()", remote_tip, local_tip);
  let behind_revset = format!("({}..{}) ~ empty()", local_tip, remote_tip);

  let ahead_count = jj_log_revset_commit_ids(workspace_path, &ahead_revset)
    .map(|ids| ids.len())
    .unwrap_or(0);
  let behind_count = jj_log_revset_commit_ids(workspace_path, &behind_revset)
    .map(|ids| ids.len())
    .unwrap_or(0);

  Ok((ahead_count, behind_count))
}

fn get_sync_revset_tips(
  workspace_path: &str,
  branch_name: &str,
  use_working_copy_proxy: bool,
) -> Result<(String, String), JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let view = loaded.repo.view();
  let remote_target = view.get_remote_bookmark(RemoteRefSymbol {
    name: RefName::new(branch_name),
    remote: RemoteName::new("origin"),
  });

  // Compare bookmark tips; conflicted bookmark divergence uses `@-` separately.
  let local_tip = if use_working_copy_proxy {
    "@-".to_string()
  } else {
    format_revset_symbol(branch_name)
  };
  let remote_tip = remote_target
    .target
    .as_normal()
    .map(|id| id.hex())
    .ok_or_else(|| {
      JjError::IoError(format!(
        "Remote bookmark '{}' does not have a normal target",
        branch_name
      ))
    })?;

  Ok((local_tip, remote_tip))
}

/// Fetch remote branches using jj git fetch (without rebasing)
/// This updates remote tracking refs and makes remote branches available
pub fn jj_git_fetch(repo_path: &str) -> Result<String, JjError> {
  if !get_git_remotes(repo_path).contains("origin") {
    return Ok(String::new());
  }
  let mut loaded = load_workspace_repo(repo_path)?;
  let subprocess_options = git_subprocess_options(&loaded.settings)?;
  let import_options = git::GitImportOptions {
    auto_local_bookmark: false,
    abandon_unreachable_commits: true,
    remote_auto_track_bookmarks: HashMap::new(),
  };
  let remote = RemoteName::new("origin");
  let refspecs = git::expand_fetch_refspecs(
    remote,
    git::GitFetchRefExpression {
      bookmark: StringExpression::all(),
      tag: StringExpression::none(),
    },
  )
  .map_err(|e| JjError::IoError(format!("Fetch failed: {}", e)))?;
  let mut tx = loaded.repo.start_transaction();
  let mut fetch = git::GitFetch::new(tx.repo_mut(), subprocess_options, &import_options)
    .map_err(|e| JjError::IoError(format!("Fetch failed: {}", e)))?;
  fetch
    .fetch(remote, refspecs, &mut SilentGitCallback, None, None)
    .map_err(|e| JjError::IoError(format!("Fetch failed: {}", e)))?;
  block_on(fetch.import_refs())
    .map_err(|e| JjError::IoError(format!("Fetch failed to import refs: {}", e)))?;
  if tx.repo().has_changes() {
    block_on(tx.repo_mut().rebase_descendants())
      .map_err(|e| JjError::IoError(format!("Failed to rebase after fetch: {}", e)))?;
    loaded.repo = block_on(tx.commit("fetch from git remote"))
      .map_err(|e| JjError::IoError(format!("Failed to commit fetch: {}", e)))?;
  }
  import_colocated_git_state(&mut loaded, repo_path)?;

  Ok(String::new())
}

/// Git subprocess options for jj-lib fetch/push, using the resolved `git`
/// binary so GUI launches with a minimal PATH still find it.
fn git_subprocess_options(settings: &UserSettings) -> Result<git::GitSubprocessOptions, JjError> {
  let mut options = git::GitSettings::from_settings(settings)
    .map_err(|e| JjError::IoError(format!("Failed to load git settings: {}", e)))?
    .to_subprocess_options();
  if let Some(path) = binary_paths::get_binary_path("git") {
    options.executable_path = PathBuf::from(path);
  }
  Ok(options)
}

/// Branch status indicating whether a branch exists locally and/or remotely
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BranchStatus {
  pub local_exists: bool,
  pub remote_exists: bool,
  pub remote_name: Option<String>, // The remote name (e.g., "origin") if remote exists
  pub remote_ref: Option<String>,  // Full remote ref (e.g., "origin/branch") if remote exists
}

/// Check if a branch exists locally and/or remotely
/// Uses git rev-parse to check refs/heads/{branch} and refs/remotes/{remote}/{branch}
/// Currently only checks 'origin' remote
pub fn check_branch_exists(repo_path: &str, branch_name: &str) -> Result<BranchStatus, JjError> {
  let local_exists = git_local_branch_commit_hex(repo_path, branch_name).is_some();

  // Check branch existence against origin (future: all remotes).
  let remote_name = "origin";
  let remote_exists = git_remote_branch_commit_hex(repo_path, remote_name, branch_name).is_some();

  let remote_ref_short = if remote_exists {
    Some(format!("{}/{}", remote_name, branch_name))
  } else {
    None
  };

  Ok(BranchStatus {
    local_exists,
    remote_exists,
    remote_name: if remote_exists {
      Some(remote_name.to_string())
    } else {
      None
    },
    remote_ref: remote_ref_short,
  })
}

/// Get list of git remotes in the repository with graceful fallback
/// Uses jj git remote list which returns format: "<remote_name> <remote_url>"
pub fn get_git_remotes(repo_path: &str) -> std::collections::HashSet<String> {
  let Ok(repo) = gix::open(repo_path) else {
    return std::collections::HashSet::new();
  };
  repo
    .remote_names()
    .into_iter()
    .map(|name| name.to_string())
    .collect()
}

/// Information about a jj bookmark/branch
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JjBranch {
  pub name: String,
  pub is_current: bool,
}

/// Get list of branches in the repository using jj-lib (no subprocess)
pub fn get_branches(repo_path: &str) -> Result<Vec<JjBranch>, JjError> {
  let settings = create_user_settings(repo_path)?;
  let workspace = Workspace::load(
    &settings,
    Path::new(repo_path),
    &StoreFactories::default(),
    &default_working_copy_factories(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to load workspace: {}", e)))?;
  let repo = block_on(workspace.repo_loader().load_at_head())
    .map_err(|e| JjError::IoError(format!("Failed to load repo: {}", e)))?;

  // Determine which commit IDs are parents of the current wc to mark is_current
  let parent_ids: std::collections::HashSet<jj_lib::backend::CommitId> = {
    let ws_name = workspace.workspace_name();
    if let Some(wc_id) = repo.view().get_wc_commit_id(ws_name) {
      match repo.store().get_commit(wc_id) {
        Ok(wc_commit) => wc_commit.parent_ids().iter().cloned().collect(),
        Err(_) => std::collections::HashSet::new(),
      }
    } else {
      std::collections::HashSet::new()
    }
  };

  let branches = repo
    .view()
    .local_bookmarks()
    .filter_map(|(name, target)| {
      let name_str = name.as_str();
      if name_str.starts_with(STASH_BOOKMARK_PREFIX) {
        return None;
      }
      let is_current = target.added_ids().any(|id| parent_ids.contains(id));
      Some(JjBranch {
        name: name_str.to_string(),
        is_current,
      })
    })
    .collect();

  Ok(branches)
}

/// Check if a remote branch exists in jj
///
/// # Arguments
/// * `repo_path` - Path to the repository
/// * `branch_ref` - Branch reference in jj format (e.g., "feature@origin")
///
/// # Returns
/// Returns true if the remote branch exists, false otherwise
/// Check if a remote bookmark exists using jj-lib (no subprocess).
/// Expects `branch_ref` in jj format: "name@remote" (e.g. "feature@origin").
pub fn check_remote_branch_exists(repo_path: &str, branch_ref: &str) -> Result<bool, JjError> {
  let (bookmark_name, remote_name) = match branch_ref.split_once('@') {
    Some((n, r)) if !n.is_empty() && !r.is_empty() => (n, r),
    _ => return Ok(false),
  };

  let settings = create_user_settings(repo_path)?;
  let workspace = Workspace::load(
    &settings,
    Path::new(repo_path),
    &StoreFactories::default(),
    &default_working_copy_factories(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to load workspace: {}", e)))?;
  let repo = block_on(workspace.repo_loader().load_at_head())
    .map_err(|e| JjError::IoError(format!("Failed to load repo: {}", e)))?;

  let symbol = RemoteRefSymbol {
    name: RefName::new(bookmark_name),
    remote: RemoteName::new(remote_name),
  };
  let remote_ref = repo.view().get_remote_bookmark(symbol);
  Ok(!remote_ref.target.is_absent())
}

/// Returns true when the Git local branch tip is an ancestor of its remote tip.
/// This distinguishes a stale local tracking branch from local-only or diverged work.
pub fn is_local_branch_behind_remote(repo_path: &str, branch_name: &str) -> bool {
  let Some(local_id) = git_local_branch_commit_hex(repo_path, branch_name) else {
    return false;
  };
  let Some(remote_id) = git_remote_branch_commit_hex(repo_path, "origin", branch_name) else {
    return false;
  };
  if local_id == remote_id {
    return false;
  }

  let Ok(loaded) = load_workspace_repo(repo_path) else {
    return false;
  };
  let expr = format!(
    "{} & {}::",
    format_revset_symbol(&remote_id),
    format_revset_symbol(&local_id)
  );
  evaluate_revset(&loaded, &expr)
    .ok()
    .and_then(|revset| revset.iter().next().transpose().ok().flatten())
    .is_some()
}

/// Build the revset string for jj_get_log based on context
fn build_jj_get_log_revset(target_ref: &str, is_home_repo: bool, _limit: Option<usize>) -> String {
  let target_ref = format_revset_symbol(target_ref);
  if is_home_repo {
    // Walk full branch history in topological order; we cap after `iter()` because `latest(S, n)` is timestamp-based and breaks merge order.
    format!("::{}", target_ref)
  } else {
    // Match jj_get_commits_ahead: ancestors up to @- (exclude WC tip), drop empty commits.
    format!("({}..@-) ~ empty()", target_ref)
  }
}

fn build_home_repo_fallback_revset(limit: Option<usize>) -> String {
  let n = limit.unwrap_or(15);
  format!("latest(::@, {})", n)
}

fn format_revset_symbol(symbol: &str) -> String {
  if symbol.starts_with('@') {
    return symbol.to_string();
  }
  if let Some((name, remote)) = symbol.split_once('@') {
    if !name.is_empty() && !remote.is_empty() {
      return revset::format_remote_symbol(name, remote);
    }
  }
  revset::format_symbol(symbol)
}

/// A squash merge rewrites a branch to a new commit while preserving its final
/// tree. When importing a tracked bookmark, jj represents that as a conflict
/// between the old local tip and the new remote tip. In that specific case the
/// remote commit is the resolved branch: retaining the old tip makes stacked
/// children compare against the unsquashed history as well as the squash.
fn select_equivalent_remote_tip<T: PartialEq>(
  candidates: &[(String, T)],
  remote_id: Option<&String>,
) -> Option<String> {
  let remote_id = remote_id?;
  let remote_tree = candidates
    .iter()
    .find(|(id, _)| id == remote_id)
    .map(|(_, tree)| tree)?;
  candidates
    .iter()
    .any(|(id, tree)| id != remote_id && tree == remote_tree)
    .then(|| remote_id.clone())
}

/// Resolves the revset for a local target-branch bookmark.
/// When the bookmark is conflicted, prefers the commit matching the git local branch tip;
/// otherwise picks the newest non-immutable tip (author timestamp, then commit id).
fn resolve_local_target_branch_revision(
  loaded: &LoadedWorkspaceRepo,
  workspace_path: &str,
  target_branch: &str,
) -> Result<String, JjError> {
  let target = loaded
    .repo
    .view()
    .get_local_bookmark(RefName::new(target_branch));
  let ids: Vec<_> = target.added_ids().cloned().collect();
  if ids.is_empty() {
    return Err(JjError::IoError(format!(
      "Local target branch '{}' was not found",
      target_branch
    )));
  }
  if ids.len() == 1 {
    return Ok(format_revset_symbol(target_branch));
  }

  // GitHub squash-and-merge replaces the remote tip with a commit that has
  // the same tree as the old branch tip. Resolve that imported bookmark
  // conflict to the squash commit before considering the (stale) git-local
  // branch or timestamp heuristics.
  let remote_symbol = RemoteRefSymbol {
    name: RefName::new(target_branch),
    remote: RemoteName::new("origin"),
  };
  let remote_id = loaded
    .repo
    .view()
    .get_remote_bookmark(remote_symbol)
    .target
    .added_ids()
    .next()
    .map(|id| id.hex());
  let candidates: Vec<_> = ids
    .iter()
    .filter_map(|id| {
      loaded
        .repo
        .store()
        .get_commit(id)
        .ok()
        .map(|commit| (id.hex(), commit.tree_ids().clone()))
    })
    .collect();
  if let Some(selected) = select_equivalent_remote_tip(&candidates, remote_id.as_ref()) {
    return Ok(selected);
  }

  // Prefer git local branch tip when it matches a conflict term.
  let repo_path = workspace_repo_path(workspace_path);
  if let Some(git_local) = git_local_branch_commit_hex(&repo_path, target_branch) {
    if let Some(matched) = ids.iter().find(|id| id.hex() == git_local) {
      return Ok(matched.hex());
    }
  }

  let immutable_revset = evaluate_revset(loaded, "immutable()").ok();
  let is_immutable = immutable_revset.as_ref().map(|r| r.containing_fn());
  let mut preferred_ids: Vec<_> = ids
    .iter()
    .filter(|id| match is_immutable.as_ref() {
      Some(check) => matches!(check(id), Ok(false)),
      None => true,
    })
    .cloned()
    .collect();
  if preferred_ids.is_empty() {
    preferred_ids = ids.clone();
  }

  // Newest author timestamp, then commit id as tie-breaker.
  preferred_ids.sort_by(|a, b| {
    let a_ts = loaded
      .repo
      .store()
      .get_commit(a)
      .map(|c| c.author().timestamp.timestamp.0)
      .unwrap_or(i64::MIN);
    let b_ts = loaded
      .repo
      .store()
      .get_commit(b)
      .map(|c| c.author().timestamp.timestamp.0)
      .unwrap_or(i64::MIN);
    b_ts.cmp(&a_ts).then_with(|| a.hex().cmp(&b.hex()))
  });

  let selected = preferred_ids.first().map(|id| id.hex()).ok_or_else(|| {
    JjError::IoError("Failed to select conflicted local bookmark tip".to_string())
  })?;
  Ok(selected)
}

/// Find the newest workspace ancestor whose tree is identical to the target.
/// This recognizes the old tip represented by a GitHub squash commit, allowing
/// stacked child commits to be separated from the already-merged parent commits.
fn resolve_tree_equivalent_workspace_base(
  loaded: &LoadedWorkspaceRepo,
  target_revision: &str,
) -> Option<String> {
  let target = resolve_commit_by_revision(loaded, target_revision).ok()?;
  let target_descendants = evaluate_revset(
    loaded,
    &format!("{}::", format_revset_symbol(&target.id().hex())),
  )
  .ok()?;
  let is_target_descendant = target_descendants.containing_fn();
  let ancestors = evaluate_revset(loaded, "::@-").ok()?;
  ancestors
    .iter()
    .commits(loaded.repo.store())
    .filter_map(Result::ok)
    .find(|commit| {
      commit.tree_ids() == target.tree_ids()
        && matches!(is_target_descendant(commit.id()), Ok(false))
    })
    .map(|commit| commit.id().hex())
}

/// Resolve the graph base to use when rebasing a workspace onto `target_revision`.
/// If a squash commit has the same tree as an ancestor in the workspace's old
/// lineage, that ancestor is the correct source boundary; commits below it have
/// already landed in the target as part of the squash.
pub fn jj_resolve_workspace_rebase_base(
  workspace_path: &str,
  target_revision: &str,
) -> Result<String, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  Ok(
    resolve_tree_equivalent_workspace_base(&loaded, target_revision)
      .unwrap_or_else(|| target_revision.to_string()),
  )
}

/// Resolve a branch/bookmark to a concrete commit ID for rebase operations.
///
/// This avoids embedding conflicted bookmark names in revsets or move-commit targets.
pub fn resolve_branch_revision_for_rebase(
  workspace_path: &str,
  branch_name: &str,
) -> Result<String, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  resolve_local_target_branch_revision(&loaded, workspace_path, branch_name)
}

pub fn jj_get_log(
  workspace_path: &str,
  target_branch: &str,
  is_home_repo: Option<bool>,
  limit: Option<usize>,
) -> Result<JjLogResult, JjError> {
  if target_branch.starts_with('-') || target_branch.contains('\0') || target_branch.is_empty() {
    return Err(JjError::IoError("Invalid target branch name".to_string()));
  }

  let cache_repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  let workspace_branch = get_workspace_branch(workspace_path)?;
  let mut loaded = load_workspace_repo(workspace_path)?;
  if is_home_repo.unwrap_or(false) {
    import_git_head_if_needed(&mut loaded, workspace_path)?;
  }
  let wc_tree_override = snapshot_working_copy_tree(&mut loaded, workspace_path)
    .ok()
    .flatten();
  let selected_target_ref =
    resolve_local_target_branch_revision(&loaded, workspace_path, target_branch)?;
  let comparison_base = if is_home_repo.unwrap_or(false) {
    selected_target_ref.clone()
  } else {
    resolve_tree_equivalent_workspace_base(&loaded, &selected_target_ref)
      .unwrap_or_else(|| selected_target_ref.clone())
  };
  let revset_expr = build_jj_get_log_revset(&comparison_base, is_home_repo.unwrap_or(false), limit);
  let mut last_error = None;
  let mut revset = match evaluate_revset(&loaded, &revset_expr) {
    Ok(evaluated) => Some((revset_expr, evaluated)),
    Err(err) => {
      last_error = Some(err);
      None
    }
  };
  if revset.is_none() && is_home_repo.unwrap_or(false) {
    let revset_expr = build_home_repo_fallback_revset(limit);
    match evaluate_revset(&loaded, &revset_expr) {
      Ok(evaluated) => {
        let _ = &selected_target_ref;
        revset = Some((revset_expr, evaluated));
      }
      Err(err) => last_error = Some(err),
    }
  }
  let selected_target_ref = if revset.is_some() {
    selected_target_ref
  } else {
    return Err(
      last_error.unwrap_or_else(|| JjError::IoError("Failed to evaluate log revset".to_string())),
    );
  };
  let (revset_expr, revset) = revset.expect("selected target ref must include a revset");
  // Home-repo immutability uses the remote tracking bookmark (e.g. `main@origin`); unpushed commits stay mutable, pushed commits immutable; falls back to local bookmark when no remote ref exists.
  let immutable_ref = if is_home_repo.unwrap_or(false) {
    let remote_symbol = RemoteRefSymbol {
      name: RefName::new(target_branch),
      remote: RemoteName::new("origin"),
    };
    let remote_ref = loaded.repo.view().get_remote_bookmark(remote_symbol);
    if !remote_ref.target.is_absent() {
      format!("{}@origin", target_branch)
    } else {
      selected_target_ref.clone()
    }
  } else {
    selected_target_ref.clone()
  };
  let immutable_revset = evaluate_revset(
    &loaded,
    &format!("::{}", format_revset_symbol(&immutable_ref)),
  )?;
  let is_immutable = immutable_revset.containing_fn();
  let wc_commit_ids: HashSet<_> = loaded
    .repo
    .view()
    .wc_commit_ids()
    .values()
    .cloned()
    .collect();
  let log_limit = limit.unwrap_or(15);
  let fetch_cap = log_limit.saturating_add(10);
  let is_home = is_home_repo.unwrap_or(false);
  let commits: Vec<Commit> = if is_home {
    let mut commits: Vec<_> = revset
      .iter()
      .take(fetch_cap)
      .commits(loaded.repo.store())
      .collect::<Result<Vec<_>, _>>()
      .map_err(|e| {
        JjError::IoError(format!("Failed to iterate revset '{}': {}", revset_expr, e))
      })?;
    commits.retain(|commit| {
      !(commit_description_first_line(commit) == "(no description)"
        && loaded
          .repo
          .view()
          .local_bookmarks_for_commit(commit.id())
          .next()
          .is_none()
        && is_empty_commit(&loaded.repo, commit))
    });
    commits.truncate(log_limit);
    commits
  } else {
    revset
      .iter()
      .commits(loaded.repo.store())
      .collect::<Result<Vec<_>, _>>()
      .map_err(|e| JjError::IoError(format!("Failed to iterate revset '{}': {}", revset_expr, e)))?
  };
  let commits = build_log_commits(
    &cache_repo_path,
    &loaded.repo,
    commits,
    &wc_commit_ids,
    wc_tree_override
      .as_ref()
      .map(|(commit_id, tree)| (commit_id, tree)),
    &is_immutable,
  );
  let tentative_working_copy = if is_home || jj_is_working_copy_empty(workspace_path)? {
    None
  } else {
    get_workspace_wc_commit(&loaded).map(|wc_commit| {
      wc_commit.map(|commit| JjTentativeWorkingCopy {
        workspace_label: workspace_branch.clone(),
        commit: build_log_commit(
          &cache_repo_path,
          &loaded.repo,
          &commit,
          &wc_commit_ids,
          wc_tree_override
            .as_ref()
            .map(|(commit_id, tree)| (commit_id, tree)),
          &is_immutable,
          &std::collections::HashMap::<String, local_db::CachedCommitDiffStat>::new(),
        ),
      })
    })?
  };

  Ok(JjLogResult {
    commits,
    tentative_working_copy,
    target_branch: target_branch.to_string(),
    workspace_branch,
    target_branch_commits: Vec::new(),
    merge_base_id: None,
  })
}

/// Get the most recent commits on a target branch (for display below active workspace commits).
///
/// Uses revset: `latest(::{target_branch}, {limit})` to get the N most recent commits
/// on the target branch.
pub fn jj_get_target_branch_log(
  workspace_path: &str,
  target_branch: &str,
  limit: usize,
) -> Result<Vec<JjLogCommit>, JjError> {
  if target_branch.starts_with('-') || target_branch.contains('\0') || target_branch.is_empty() {
    return Err(JjError::IoError("Invalid target branch name".to_string()));
  }

  let cache_repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  // Over-fetch to allow dropping WC placeholders without truncating visible target history.
  let fetch_limit = limit.saturating_add(5);
  let loaded = load_workspace_repo(workspace_path)?;
  let selected_target_ref =
    resolve_local_target_branch_revision(&loaded, workspace_path, target_branch)?;
  let revset_expr = build_target_branch_revset(&selected_target_ref, fetch_limit);
  let revset = evaluate_revset(&loaded, &revset_expr).map_err(|e| {
    JjError::IoError(format!(
      "Failed to evaluate target branch revset '{}': {}",
      revset_expr, e
    ))
  })?;
  let immutable_revset = evaluate_revset(
    &loaded,
    &format!("::{}", format_revset_symbol(&selected_target_ref)),
  )?;
  let is_immutable = immutable_revset.containing_fn();
  let wc_commit_ids: HashSet<_> = loaded
    .repo
    .view()
    .wc_commit_ids()
    .values()
    .cloned()
    .collect();
  let commits = revset
    .iter()
    .commits(loaded.repo.store())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| JjError::IoError(format!("Failed to iterate revset '{}': {}", revset_expr, e)))?;
  let mut commits: Vec<_> = commits
    .into_iter()
    .filter(|commit| {
      !(wc_commit_ids.contains(commit.id())
        || commit_description_first_line(commit) == "(no description)"
          && loaded
            .repo
            .view()
            .local_bookmarks_for_commit(commit.id())
            .next()
            .is_none()
          && is_empty_commit(&loaded.repo, commit))
    })
    .collect();
  commits.truncate(limit);
  Ok(build_log_commits(
    &cache_repo_path,
    &loaded.repo,
    commits,
    &wc_commit_ids,
    None,
    &is_immutable,
  ))
}

fn build_target_branch_revset(target_ref: &str, limit: usize) -> String {
  format!("latest(::{}, {})", format_revset_symbol(target_ref), limit)
}

/// Get commits that are in workspace but not in target branch
/// Uses revset: target_branch..@- (commits reachable from @- but not from target)
/// Note: Uses @- (parent of working copy) to exclude the empty working copy commit
pub fn jj_get_commits_ahead(
  workspace_path: &str,
  target_branch: &str,
) -> Result<JjCommitsAhead, JjError> {
  if target_branch.starts_with('-') || target_branch.contains('\0') || target_branch.is_empty() {
    return Err(JjError::IoError("Invalid target branch name".to_string()));
  }

  let cache_repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  let loaded = load_workspace_repo(workspace_path)?;
  let selected_target_ref =
    resolve_local_target_branch_revision(&loaded, workspace_path, target_branch)?;
  let comparison_base = resolve_tree_equivalent_workspace_base(&loaded, &selected_target_ref)
    .unwrap_or_else(|| selected_target_ref.clone());
  let revset_expr = format!("({}..@-) ~ empty()", format_revset_symbol(&comparison_base));
  let revset = evaluate_revset(&loaded, &revset_expr).map_err(|e| {
    JjError::IoError(format!(
      "Failed to evaluate commits-ahead revset '{}': {}",
      revset_expr, e
    ))
  })?;
  let immutable_revset = evaluate_revset(
    &loaded,
    &format!("::{}", format_revset_symbol(&selected_target_ref)),
  )?;
  let is_immutable = immutable_revset.containing_fn();
  let wc_commit_ids: HashSet<_> = loaded
    .repo
    .view()
    .wc_commit_ids()
    .values()
    .cloned()
    .collect();
  let commits = revset
    .iter()
    .commits(loaded.repo.store())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| {
      JjError::IoError(format!(
        "Failed to iterate commits-ahead revset '{}': {}",
        revset_expr, e
      ))
    })?;
  let commits = build_log_commits(
    &cache_repo_path,
    &loaded.repo,
    commits,
    &wc_commit_ids,
    None,
    &is_immutable,
  );
  let total_count = commits.len();

  Ok(JjCommitsAhead {
    commits,
    total_count,
  })
}

/// Returns the diverged commit view for a home-repo non-default branch.
///
/// - `commits` = commits unique to `current_branch`, followed by commits that
///   `target_branch` is ahead by (marked with `on_target_only = true`)
/// - `merge_base_id` = the commit ID of the latest common ancestor
pub fn jj_get_home_repo_diverged_log(
  repo_path: &str,
  current_branch: &str,
  target_branch: &str,
  limit: Option<usize>,
) -> Result<JjLogResult, JjError> {
  validate_branch_name(current_branch, "current")?;
  validate_branch_name(target_branch, "target")?;

  let cache_repo_path = repo_path.to_string();
  let mut loaded = load_workspace_repo(repo_path)?;
  import_colocated_git_state(&mut loaded, repo_path)?;

  let current_sym = format_revset_symbol(current_branch);
  let target_sym = format_revset_symbol(target_branch);

  // Commits unique to current branch
  let branch_revset = format!("({target_sym}..{current_sym}) ~ empty()");
  // Commits target is ahead by
  let target_ahead_revset = format!("({current_sym}..{target_sym}) ~ empty()");
  // Latest common ancestor (merge base)
  let merge_base_revset = format!("latest(::{current_sym} & ::{target_sym})");

  let wc_commit_ids: HashSet<_> = loaded
    .repo
    .view()
    .wc_commit_ids()
    .values()
    .cloned()
    .collect();

  let immutable_revset_expr = format!("::{target_sym}");
  let immutable_revset = evaluate_revset(&loaded, &immutable_revset_expr)?;
  let is_immutable = immutable_revset.containing_fn();

  // Branch-unique commits
  let branch_commits = match evaluate_revset(&loaded, &branch_revset) {
    Ok(revset) => {
      let cap = limit.unwrap_or(20).saturating_add(5);
      let mut commits: Vec<_> = revset
        .iter()
        .take(cap)
        .commits(loaded.repo.store())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| JjError::IoError(format!("Failed to iterate branch revset: {}", e)))?;
      commits.truncate(limit.unwrap_or(20));
      build_log_commits(
        &cache_repo_path,
        &loaded.repo,
        commits,
        &wc_commit_ids,
        None,
        &is_immutable,
      )
    }
    Err(_) => Vec::new(),
  };

  // Target-ahead commits (mark with on_target_only)
  let target_only_commits = match evaluate_revset(&loaded, &target_ahead_revset) {
    Ok(revset) => {
      let cap = limit.unwrap_or(20).saturating_add(5);
      let commits: Vec<_> = revset
        .iter()
        .take(cap)
        .commits(loaded.repo.store())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| JjError::IoError(format!("Failed to iterate target-ahead revset: {}", e)))?;
      let mut built = build_log_commits(
        &cache_repo_path,
        &loaded.repo,
        commits,
        &wc_commit_ids,
        None,
        &is_immutable,
      );
      for c in &mut built {
        c.on_target_only = true;
      }
      built
    }
    Err(_) => Vec::new(),
  };

  // Merge base
  let merge_base_id = evaluate_revset(&loaded, &merge_base_revset)
    .ok()
    .and_then(|revset| {
      revset
        .iter()
        .commits(loaded.repo.store())
        .next()
        .and_then(|r| r.ok())
        .map(|c| c.id().hex()[..12].to_string())
    });

  let workspace_branch =
    get_workspace_branch(repo_path).unwrap_or_else(|_| current_branch.to_string());

  let mut commits = branch_commits;
  let existing_ids: std::collections::HashSet<String> =
    commits.iter().map(|c| c.commit_id.clone()).collect();
  for commit in target_only_commits {
    if existing_ids.contains(&commit.commit_id) {
      continue;
    }
    commits.push(commit);
  }

  Ok(JjLogResult {
    commits,
    tentative_working_copy: None,
    target_branch: target_branch.to_string(),
    workspace_branch,
    target_branch_commits: Vec::new(),
    merge_base_id,
  })
}

/// Rebase the home-repo branch `current_branch` onto `target_branch`.
///
/// This is the manual rebase affordance for home-repo branches (workspaces auto-rebase;
/// home-repo branches do not). Does not touch any workspace DB records.
pub fn jj_rebase_home_repo_branch(
  repo_path: &str,
  current_branch: &str,
  target_branch: &str,
) -> Result<JjRebaseResult, JjError> {
  validate_branch_name(current_branch, "current")?;
  validate_branch_name(target_branch, "target")?;

  {
    let mut loaded = load_workspace_repo(repo_path)?;
    import_colocated_git_state(&mut loaded, repo_path)?;
  }

  let current_sym = format_revset_symbol(current_branch);
  let target_sym = format_revset_symbol(target_branch);
  // Rebase all commits unique to current branch onto target
  let revset = format!("({target_sym}..{current_sym}) ~ empty()");

  jj_rebase_with_revset(repo_path, &revset, target_branch, current_branch, "git")
}

/// Dry-run a home-repo rebase: check whether rebasing `current_branch` onto `target_branch`
/// would produce conflicts, without mutating the repository.
///
/// Uses a file-overlap heuristic: if both branches changed the same file since their
/// common ancestor, a conflict is likely.
pub fn jj_dry_run_home_repo_rebase(
  repo_path: &str,
  current_branch: &str,
  target_branch: &str,
) -> Result<HomeRebaseDryRunResult, JjError> {
  validate_branch_name(current_branch, "current")?;
  validate_branch_name(target_branch, "target")?;

  let mut loaded = load_workspace_repo(repo_path)?;
  import_colocated_git_state(&mut loaded, repo_path)?;

  let current_sym = format_revset_symbol(current_branch);
  let target_sym = format_revset_symbol(target_branch);
  let merge_base_revset = format!("latest(::{current_sym} & ::{target_sym})");

  let merge_base_commit = {
    let revset = evaluate_revset(&loaded, &merge_base_revset)
      .map_err(|e| JjError::IoError(format!("Failed to find merge base: {}", e)))?;
    revset
      .iter()
      .commits(loaded.repo.store())
      .next()
      .ok_or_else(|| JjError::IoError("No common ancestor found".to_string()))?
      .map_err(|e| JjError::IoError(format!("Failed to load merge base commit: {}", e)))?
  };

  let current_revision = resolve_branch_revision_for_rebase(repo_path, current_branch)?;
  let target_revision = resolve_branch_revision_for_rebase(repo_path, target_branch)?;
  let current_commit = resolve_commit_by_revision(&loaded, &current_revision)?;
  let target_commit = resolve_commit_by_revision(&loaded, &target_revision)?;

  let merge_base_tree = merge_base_commit.tree();
  let current_tree = current_commit.tree();
  let target_tree = target_commit.tree();

  let diff_matcher = repo_root_matcher();

  // Files changed on current branch since merge base
  let current_changed: HashSet<String> = block_on(
    merge_base_tree
      .diff_stream(&current_tree, &diff_matcher)
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

  // Files changed on target branch since merge base
  let target_changed: HashSet<String> = block_on(
    merge_base_tree
      .diff_stream(&target_tree, &diff_matcher)
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

  let mut conflicted_files: Vec<String> = current_changed
    .intersection(&target_changed)
    .cloned()
    .collect();
  conflicted_files.sort();

  Ok(HomeRebaseDryRunResult {
    would_conflict: !conflicted_files.is_empty(),
    conflicted_files,
  })
}

/// Abandon every commit matched by `revset_expr` (by change id) in `workspace_path`.
/// Returns the list of abandoned change IDs.
fn abandon_commits_matching_revset(
  workspace_path: &str,
  revset_expr: &str,
) -> Result<Vec<String>, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let revset = evaluate_revset(&loaded, revset_expr).map_err(|e| {
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
  abandon_commits_matching_revset(workspace_path, &revset_expr)
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
    if parents.len() != 1 {
      break;
    }
    let parent = parents.into_iter().next().expect("checked length 1");
    if !is_autosave_description(parent.description()) {
      break;
    }
    autosaves.push(parent.clone());
    current = parent;
  }
  if autosaves.is_empty() {
    return Ok(Vec::new());
  }

  let oldest = autosaves.last().expect("non-empty autosaves");
  let oldest_parents = block_on(oldest.parents())
    .map_err(|e| JjError::IoError(format!("Failed to load autosave parents: {e}")))?;
  if oldest_parents.len() != 1 {
    return Ok(Vec::new());
  }
  let base = oldest_parents.into_iter().next().expect("checked length 1");

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
  let _ = git::export_refs(tx.repo_mut());
  block_on(tx.commit("drop autosave commits"))
    .map_err(|e| JjError::IoError(format!("Failed to commit autosave drop: {e}")))?;

  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  let _ = reconcile_all_workspaces_after_rewrite(&repo_path, None);

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
    1 => Ok(commits.into_iter().next().expect("single commit expected")),
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

fn build_tentative_working_copy(
  cache_repo_path: &str,
  repo: &Arc<ReadonlyRepo>,
  commit: &Commit,
  workspace_label: &str,
  wc_commit_ids: &HashSet<jj_lib::backend::CommitId>,
  wc_tree_override: Option<(&jj_lib::backend::CommitId, &MergedTree)>,
  is_immutable: &impl Fn(
    &jj_lib::backend::CommitId,
  ) -> Result<bool, jj_lib::revset::RevsetEvaluationError>,
) -> JjTentativeWorkingCopy {
  JjTentativeWorkingCopy {
    workspace_label: workspace_label.to_string(),
    commit: {
      let mut commit = build_log_commit(
        cache_repo_path,
        repo,
        commit,
        wc_commit_ids,
        wc_tree_override,
        is_immutable,
        &std::collections::HashMap::<String, local_db::CachedCommitDiffStat>::new(),
      );
      commit.workspace_label = Some(workspace_label.to_string());
      commit
    },
  }
}

pub fn jj_get_tentative_working_copy(
  workspace_path: &str,
  workspace_label: &str,
) -> Result<Option<JjTentativeWorkingCopy>, JjError> {
  if jj_is_working_copy_empty(workspace_path)? {
    return Ok(None);
  }

  let loaded = load_workspace_repo(workspace_path)?;
  let immutable_revset = evaluate_revset(
    &loaded,
    &format!("::{}", format_revset_symbol(workspace_label)),
  )?;
  let is_immutable_val = immutable_revset.containing_fn();
  let wc_commit_ids: HashSet<_> = loaded
    .repo
    .view()
    .wc_commit_ids()
    .values()
    .cloned()
    .collect();

  get_workspace_wc_commit(&loaded).map(|maybe_commit| {
    maybe_commit.map(|commit| {
      build_tentative_working_copy(
        workspace_path,
        &loaded.repo,
        &commit,
        workspace_label,
        &wc_commit_ids,
        None,
        &is_immutable_val,
      )
    })
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
    let _ = reconcile_all_workspaces_after_rewrite(&repo_path, Some(&primary_workspace_name));
    return Ok(());
  }

  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let Some(new_wc_commit_id) = new_repo.view().get_wc_commit_id(&workspace_name).cloned() else {
    // Primary workspace has no WC commit — still reconcile siblings.
    let _ = reconcile_all_workspaces_after_rewrite(&repo_path, Some(&primary_workspace_name));
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
  let _ = reconcile_all_workspaces_after_rewrite(&repo_path, Some(&primary_workspace_name));

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

fn get_commit_copy_records(
  store: &jj_lib::store::Store,
  commit: &Commit,
) -> Result<CopyRecords, JjError> {
  let mut copy_records = CopyRecords::default();
  for parent_id in commit.parent_ids() {
    let records_stream = store
      .get_copy_records(None, parent_id, commit.id())
      .map_err(|e| JjError::IoError(format!("Failed to read copy records: {}", e)))?;
    let records = block_on(records_stream.try_collect::<Vec<_>>())
      .map_err(|e| JjError::IoError(format!("Failed to collect copy records: {}", e)))?;
    copy_records.add_records(records);
  }
  Ok(copy_records)
}

fn get_copy_records_between(
  store: &jj_lib::store::Store,
  root: &jj_lib::backend::CommitId,
  head: &jj_lib::backend::CommitId,
  matcher: &PrefixMatcher,
) -> Result<CopyRecords, JjError> {
  let stream = store
    .get_copy_records(None, root, head)
    .map_err(|e| JjError::IoError(format!("Failed to read copy records: {}", e)))?;
  let records = block_on(
    stream
      .try_filter(|record| futures::future::ready(matcher.matches(&record.target)))
      .try_collect::<Vec<_>>(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to collect copy records: {}", e)))?;
  let mut copy_records = CopyRecords::default();
  copy_records.add_records(records);
  Ok(copy_records)
}

fn build_file_change(
  entry_path: &jj_lib::copies::CopiesTreeDiffEntryPath,
  changed_line_count: usize,
  diff_deferred: bool,
  before_absent: bool,
  after_absent: bool,
) -> JjFileChange {
  let path = entry_path.target().as_internal_file_string().to_string();
  let previous_path = entry_path
    .to_diff()
    .map(|diff| diff.before.as_internal_file_string().to_string());
  let status = match entry_path.copy_operation() {
    Some(jj_lib::copies::CopyOperation::Rename) => "R",
    Some(jj_lib::copies::CopyOperation::Copy) => "C",
    None if before_absent => "A",
    None if after_absent => "D",
    None => "M",
  };

  JjFileChange {
    path,
    status: status.to_string(),
    previous_path,
    changed_line_count,
    diff_deferred,
  }
}

fn build_file_diff_from_bytes(
  before_bytes: Option<Vec<u8>>,
  after_bytes: Option<Vec<u8>>,
) -> (
  usize,
  Vec<JjDiffHunk>,
  ConflictStyle,
  Vec<ConflictRegionView>,
) {
  match (before_bytes, after_bytes) {
    (None, Some(after)) => {
      let after = String::from_utf8_lossy(&after).into_owned();
      let hunks = build_text_hunks("", &after);
      let regions = conflict_markers::parse_conflict_markers(&after, "");
      let style = regions
        .first()
        .map(|r| r.marker_style)
        .unwrap_or(ConflictStyle::Unknown);
      (count_changed_lines(&hunks), hunks, style, regions)
    }
    (Some(before), None) => {
      let before = String::from_utf8_lossy(&before).into_owned();
      let hunks = build_text_hunks(&before, "");
      (
        count_changed_lines(&hunks),
        hunks,
        ConflictStyle::Unknown,
        Vec::new(),
      )
    }
    (Some(before), Some(after)) => {
      let before = String::from_utf8_lossy(&before).into_owned();
      let after = String::from_utf8_lossy(&after).into_owned();
      let hunks = build_text_hunks(&before, &after);
      let regions = conflict_markers::parse_conflict_markers(&after, "");
      let style = regions
        .first()
        .map(|r| r.marker_style)
        .unwrap_or(ConflictStyle::Unknown);
      (count_changed_lines(&hunks), hunks, style, regions)
    }
    (None, None) => (0, Vec::new(), ConflictStyle::Unknown, Vec::new()),
  }
}

fn annotate_conflict_regions(
  file_path: &str,
  regions: Vec<ConflictRegionView>,
) -> Vec<ConflictRegionView> {
  regions
    .into_iter()
    .map(|mut region| {
      region.file_path = file_path.to_string();
      region.id = format!("{}-conflict-{}", file_path, region.conflict_number);
      region
    })
    .collect()
}

/// Get combined diff between two revisions.
pub fn jj_get_merge_diff_between_revisions(
  workspace_path: &str,
  from_revision: &str,
  to_revision: &str,
  _conflict_marker_style: &str,
) -> Result<JjRevisionDiff, JjError> {
  validate_branch_name(from_revision, "target")?;
  validate_branch_name(to_revision, "revision")?;
  let loaded = load_workspace_repo(workspace_path)?;
  let from_commit = resolve_commit_by_revision(&loaded, from_revision)?;
  let to_commit = resolve_commit_by_revision(&loaded, to_revision)?;
  let from_tree = from_commit.tree();
  let to_tree = to_commit.tree();

  let matcher = repo_root_matcher();
  let copy_records = get_copy_records_between(
    loaded.repo.store(),
    from_commit.id(),
    to_commit.id(),
    &matcher,
  )?;
  let conflict_labels = Diff::new(from_tree.labels(), to_tree.labels());
  let diff_stream = materialized_diff_stream(
    loaded.repo.store(),
    from_tree.diff_stream_with_copies(&to_tree, &matcher, &copy_records),
    conflict_labels,
  );

  let mut committed_files = Vec::new();
  let mut hunks_by_file = Vec::new();
  block_on(async {
    futures::pin_mut!(diff_stream);
    while let Some(entry) = diff_stream.next().await {
      let values = match entry.values {
        Ok(values) => values,
        Err(_) => continue,
      };
      let before_absent = matches!(&values.before, MaterializedTreeValue::Absent);
      let after_absent = matches!(&values.after, MaterializedTreeValue::Absent);
      let style = parse_conflict_marker_style(_conflict_marker_style);
      let merge_options = match MergeOptions::from_settings(&loaded.settings) {
        Ok(options) => options,
        Err(_) => continue,
      };
      let before_bytes = materialized_value_to_bytes(
        entry.path.source(),
        values.before,
        style,
        Some(&merge_options),
      )
      .await;
      let after_bytes = materialized_value_to_bytes(
        entry.path.target(),
        values.after,
        style,
        Some(&merge_options),
      )
      .await;
      let (changed_line_count, mut hunks, conflict_style, conflict_regions_raw) =
        build_file_diff_from_bytes(before_bytes, after_bytes);
      let file = build_file_change(
        &entry.path,
        changed_line_count,
        false,
        before_absent,
        after_absent,
      );
      let conflict_regions = annotate_conflict_regions(&file.path, conflict_regions_raw);
      for hunk in &mut hunks {
        hunk.conflict_style = conflict_style;
        hunk.conflict_regions = conflict_regions.clone();
      }
      hunks_by_file.push(JjFileDiff {
        path: file.path.clone(),
        hunks,
        conflict_style,
        conflict_regions,
      });
      committed_files.push(file);
    }
  });

  Ok(JjRevisionDiff {
    committed_files,
    hunks_by_file,
    uncommitted_files: Vec::new(),
    conflicted_files: Vec::new(),
    too_large_to_render: false,
    render_block_reason: None,
  })
}

pub fn jj_get_commit_diff(
  workspace_path: &str,
  revision: &str,
  _conflict_marker_style: &str,
) -> Result<JjRevisionDiff, JjError> {
  if revision.starts_with('-') || revision.contains('\0') || revision.is_empty() {
    return Err(JjError::IoError("Invalid revision".to_string()));
  }

  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, revision)?;
  let parent_tree = block_on(commit.parent_tree(loaded.repo.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to read parent tree: {}", e)))?;
  let commit_tree = commit.tree();
  let copy_records = get_commit_copy_records(loaded.repo.store(), &commit)?;
  let conflict_labels = Diff::new(parent_tree.labels(), commit_tree.labels());
  let matcher = repo_root_matcher();
  let diff_stream = materialized_diff_stream(
    loaded.repo.store(),
    parent_tree.diff_stream_with_copies(&commit_tree, &matcher, &copy_records),
    conflict_labels,
  );
  let mut committed_files = Vec::new();
  let mut hunks_by_file = Vec::new();
  let too_large = block_on(async {
    futures::pin_mut!(diff_stream);
    let mut total_changed_lines = 0usize;

    while let Some(entry) = diff_stream.next().await {
      let values = match entry.values {
        Ok(values) => values,
        Err(_) => continue,
      };
      let before_absent = matches!(&values.before, MaterializedTreeValue::Absent);
      let after_absent = matches!(&values.after, MaterializedTreeValue::Absent);
      let style = parse_conflict_marker_style(_conflict_marker_style);
      let merge_options = match MergeOptions::from_settings(&loaded.settings) {
        Ok(options) => options,
        Err(_) => continue,
      };
      let before_bytes = materialized_value_to_bytes(
        entry.path.source(),
        values.before,
        style,
        Some(&merge_options),
      )
      .await;
      let after_bytes = materialized_value_to_bytes(
        entry.path.target(),
        values.after,
        style,
        Some(&merge_options),
      )
      .await;
      let (changed_line_count, mut hunks, conflict_style, conflict_regions_raw) =
        build_file_diff_from_bytes(before_bytes, after_bytes);

      total_changed_lines += changed_line_count;
      if total_changed_lines > TOO_LARGE_COMMIT_DIFF_THRESHOLD {
        return true;
      }

      let diff_deferred = changed_line_count > LARGE_COMMIT_FILE_DIFF_THRESHOLD;
      let file = build_file_change(
        &entry.path,
        changed_line_count,
        diff_deferred,
        before_absent,
        after_absent,
      );
      if !diff_deferred {
        let conflict_regions = annotate_conflict_regions(&file.path, conflict_regions_raw);
        for hunk in &mut hunks {
          hunk.conflict_style = conflict_style;
          hunk.conflict_regions = conflict_regions.clone();
        }
        hunks_by_file.push(JjFileDiff {
          path: file.path.clone(),
          hunks,
          conflict_style,
          conflict_regions,
        });
      }
      committed_files.push(file);
    }

    false
  });

  if too_large {
    return Ok(too_large_revision_diff());
  }

  Ok(JjRevisionDiff {
    committed_files,
    hunks_by_file,
    uncommitted_files: Vec::new(),
    conflicted_files: Vec::new(),
    too_large_to_render: false,
    render_block_reason: None,
  })
}

pub fn jj_get_commit_file_diff(
  workspace_path: &str,
  revision: &str,
  file_path: &str,
  _conflict_marker_style: &str,
) -> Result<JjFileDiff, JjError> {
  if revision.starts_with('-') || revision.contains('\0') || revision.is_empty() {
    return Err(JjError::IoError("Invalid revision".to_string()));
  }
  if file_path.contains('\0') || file_path.is_empty() {
    return Err(JjError::IoError("Invalid file path".to_string()));
  }

  let loaded = load_workspace_repo(workspace_path)?;
  let commit = resolve_commit_by_revision(&loaded, revision)?;
  let parent_tree = block_on(commit.parent_tree(loaded.repo.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to read parent tree: {}", e)))?;
  let commit_tree = commit.tree();
  let copy_records = get_commit_copy_records(loaded.repo.store(), &commit)?;
  let conflict_labels = Diff::new(parent_tree.labels(), commit_tree.labels());
  let matcher = repo_root_matcher();
  let diff_stream = materialized_diff_stream(
    loaded.repo.store(),
    parent_tree.diff_stream_with_copies(&commit_tree, &matcher, &copy_records),
    conflict_labels,
  );
  block_on(async {
    futures::pin_mut!(diff_stream);
    while let Some(entry) = diff_stream.next().await {
      let target_path = entry.path.target().as_internal_file_string().to_string();
      if target_path != file_path {
        continue;
      }
      let values = match entry.values {
        Ok(values) => values,
        Err(_) => continue,
      };
      let style = parse_conflict_marker_style(_conflict_marker_style);
      let merge_options = match MergeOptions::from_settings(&loaded.settings) {
        Ok(options) => options,
        Err(_) => continue,
      };
      let before_bytes = materialized_value_to_bytes(
        entry.path.source(),
        values.before,
        style,
        Some(&merge_options),
      )
      .await;
      let after_bytes = materialized_value_to_bytes(
        entry.path.target(),
        values.after,
        style,
        Some(&merge_options),
      )
      .await;
      let (_, mut hunks, conflict_style, conflict_regions_raw) =
        build_file_diff_from_bytes(before_bytes, after_bytes);
      let conflict_regions = annotate_conflict_regions(file_path, conflict_regions_raw);
      for hunk in &mut hunks {
        hunk.conflict_style = conflict_style;
        hunk.conflict_regions = conflict_regions.clone();
      }
      return Ok(JjFileDiff {
        path: file_path.to_string(),
        hunks,
        conflict_style,
        conflict_regions,
      });
    }

    Ok(JjFileDiff {
      path: file_path.to_string(),
      hunks: Vec::new(),
      conflict_style: ConflictStyle::Unknown,
      conflict_regions: Vec::new(),
    })
  })
}

/// Get all tracked files in the current workspace without shelling out to `jj file list`.
pub fn jj_get_tracked_files(workspace_path: &str) -> Result<Vec<String>, JjError> {
  let mut loaded = load_workspace_repo(workspace_path)?;
  import_colocated_git_state(&mut loaded, workspace_path)?;

  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let tree = if let Some((_, tree)) = snapshot_working_copy_tree(&mut loaded, workspace_path)? {
    tree
  } else {
    let wc_commit_id = loaded
      .repo
      .view()
      .get_wc_commit_id(&workspace_name)
      .cloned()
      .ok_or_else(|| JjError::IoError("No working-copy commit found".to_string()))?;
    let wc_commit = loaded
      .repo
      .store()
      .get_commit(&wc_commit_id)
      .map_err(|e| JjError::IoError(format!("Failed to load wc commit: {}", e)))?;
    wc_commit.tree()
  };

  let mut files = Vec::new();
  for (path, value) in tree.entries() {
    let value =
      value.map_err(|e| JjError::IoError(format!("Failed to read tracked file entry: {}", e)))?;
    if value.is_present() && !value.is_tree() {
      files.push(path.as_internal_file_string().to_string());
    }
  }
  Ok(files)
}

fn count_changed_lines(hunks: &[JjDiffHunk]) -> usize {
  hunks
    .iter()
    .map(|hunk| {
      hunk
        .lines
        .iter()
        .filter(|line| line.starts_with('+') || line.starts_with('-'))
        .count()
    })
    .sum()
}

/// Create a new working-copy commit on top of the given parent revisions.
///
/// Library equivalent of `jj new <rev>...`: resolves every parent revision in the
/// workspace, merges their trees, writes an empty commit with those parents and
/// edits it, so a multi-parent call leaves the working copy conflicted exactly the
/// way the CLI does.
// home-head-test: new_on_other_branch
pub fn jj_new_with_parents(
  workspace_path: &str,
  parent_revisions: &[String],
) -> Result<String, JjError> {
  if parent_revisions.is_empty() {
    return Err(JjError::IoError(
      "At least one parent revision is required".to_string(),
    ));
  }

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let old_wc_commit = get_workspace_wc_commit(&loaded)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let parent_commits = parent_revisions
    .iter()
    .map(|revision| resolve_commit_by_revision(&loaded, revision))
    .collect::<Result<Vec<_>, _>>()?;

  let mut tx = loaded.repo.start_transaction();
  let merged_tree = block_on(merge_commit_trees(tx.repo(), &parent_commits))
    .map_err(|e| JjError::IoError(format!("Failed to build parent tree: {}", e)))?;
  let new_wc_commit = block_on(
    tx.repo_mut()
      .new_commit(
        parent_commits
          .iter()
          .map(|commit| commit.id().clone())
          .collect(),
        merged_tree,
      )
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to write commit: {}", e)))?;
  block_on(tx.repo_mut().edit(workspace_name, &new_wc_commit))
    .map_err(|e| JjError::IoError(format!("Failed to move working copy: {}", e)))?;
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;
  let _ = git::export_refs(tx.repo_mut());

  let new_repo = block_on(tx.commit("new commit"))
    .map_err(|e| JjError::IoError(format!("Failed to commit transaction: {}", e)))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    old_wc_commit.as_ref(),
    CheckoutMode::Immediate,
  )?;

  Ok(short_commit_id(&new_wc_commit))
}

/// Create a merge commit using jj-lib commit writes.
///
/// Flow:
/// 1. Resolve workspace and target branch tips and create a two-parent merge commit
/// 2. Check out a fresh working-copy commit on top
/// 3. jj bookmark set target_branch -r @- - move target_branch to merge commit
///
/// This is executed in the context of the workspace directory, @ refers to workspace HEAD
// home-head-test: merge_workspace_into_home
pub fn jj_create_merge_commit(
  workspace_path: &str,
  workspace_branch: &str,
  target_branch: &str,
  message: &str,
  _conflict_marker_style: &str,
) -> Result<JjMergeResult, JjError> {
  validate_branch_name(workspace_branch, "workspace")?;
  validate_branch_name(target_branch, "target")?;
  validate_commit_message(message)?;

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let old_wc_commit = get_workspace_wc_commit(&loaded)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let workspace_parent = resolve_commit_by_revision(&loaded, workspace_branch)?;
  let target_parent = resolve_merge_target_parent(&loaded, workspace_path, target_branch)?;

  let mut tx = loaded.repo.start_transaction();
  let parent_commits = vec![workspace_parent, target_parent];
  let merged_tree = block_on(merge_commit_trees(tx.repo(), &parent_commits))
    .map_err(|e| JjError::IoError(format!("Failed to build merge tree: {}", e)))?;
  let merge_commit = block_on(async {
    tx.repo_mut()
      .new_commit(
        parent_commits
          .iter()
          .map(|commit| commit.id().clone())
          .collect(),
        merged_tree,
      )
      .set_description(message)
      .write()
      .await
  })
  .map_err(|e| JjError::IoError(format!("Failed to write merge commit: {}", e)))?;

  let new_wc_commit = block_on(tx.repo_mut().check_out(workspace_name, &merge_commit))
    .map_err(|e| JjError::IoError(format!("Failed to create working-copy commit: {}", e)))?;
  let _ = new_wc_commit;
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants after merge: {}", e)))?;

  set_local_bookmark_target(tx.repo_mut(), target_branch, merge_commit.id().clone());
  let _ = git::export_refs(tx.repo_mut());

  let new_repo = block_on(tx.commit("create merge commit"))
    .map_err(|e| JjError::IoError(format!("Failed to commit merge transaction: {}", e)))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    old_wc_commit.as_ref(),
    CheckoutMode::Immediate,
  )?;

  let has_conflicts = !merge_commit.tree_ids().is_resolved();
  let conflicted_files = if has_conflicts {
    get_conflicted_files(workspace_path, None).unwrap_or_default()
  } else {
    Vec::new()
  };
  let merge_commit_id = Some(short_commit_id(&merge_commit));
  let message = if has_conflicts {
    format!(
      "Created merge commit {} with conflicts",
      merge_commit_id.as_deref().unwrap_or("")
    )
  } else {
    format!(
      "Created merge commit {}",
      merge_commit_id.as_deref().unwrap_or("")
    )
  };

  Ok(JjMergeResult {
    success: true,
    message,
    has_conflicts,
    conflicted_files,
    merge_commit_id,
  })
}

/// Rebases a workspace branch onto the target branch, then records a two-parent merge commit
/// on the target branch (first parent = target tip before merge, second = rebased workspace tip)
/// with the given description—similar to `git merge --no-ff` after rebasing.
///
/// # Arguments
/// * `workspace_path` - Path to the workspace directory
/// * `workspace_branch` - Name of the workspace branch to rebase
/// * `target_branch` - Name of the target branch
/// * `message` - Description for the merge commit
///
/// # Returns
/// Returns the rebase result or a JjError on failure.
// home-head-test: merge_workspace_into_home
pub fn jj_rebase_merge_commit(
  workspace_path: &str,
  workspace_branch: &str,
  target_branch: &str,
  message: &str,
) -> Result<JjRebaseResult, JjError> {
  validate_branch_name(workspace_branch, "workspace")?;
  validate_branch_name(target_branch, "target")?;
  validate_commit_message(message)?;

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let old_wc_commit = get_workspace_wc_commit(&loaded)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let workspace_commit = resolve_commit_by_revision(&loaded, workspace_branch)?;
  let target_commit = resolve_commit_by_revision(
    &loaded,
    &resolve_target_branch_symbol(&loaded, workspace_path, target_branch)?,
  )?;
  let target_tip_id = target_commit.id().clone();

  let rebase_options = RebaseOptions {
    empty: EmptyBehavior::Keep,
    rewrite_refs: RewriteRefsOptions {
      delete_abandoned_bookmarks: false,
    },
    simplify_ancestor_merge: false,
  };

  let mut tx = loaded.repo.start_transaction();
  let loc = MoveCommitsLocation {
    new_parent_ids: vec![target_commit.id().clone()],
    new_child_ids: Vec::new(),
    target: MoveCommitsTarget::Roots(vec![workspace_commit.id().clone()]),
  };
  let stats = block_on(async {
    rewrite::compute_move_commits(tx.repo_mut(), &loc)
      .await?
      .apply(tx.repo_mut(), &rebase_options)
      .await
  })
  .map_err(|e| JjError::IoError(format!("Failed to rebase workspace branch: {}", e)))?;

  let rebased_workspace_id = tx
    .repo()
    .view()
    .get_local_bookmark(RefName::new(workspace_branch))
    .as_normal()
    .cloned()
    .ok_or_else(|| {
      JjError::IoError(format!(
        "Workspace branch '{}' no longer resolves after rebase",
        workspace_branch
      ))
    })?;

  let rebased_tip_commit = tx
    .repo()
    .store()
    .get_commit(&rebased_workspace_id)
    .map_err(|e| {
      JjError::IoError(format!(
        "Failed to load rebased workspace tip after rebase: {}",
        e
      ))
    })?;
  let target_tip_commit = tx.repo().store().get_commit(&target_tip_id).map_err(|e| {
    JjError::IoError(format!(
      "Failed to load target branch tip before merge: {}",
      e
    ))
  })?;

  let parent_commits = vec![target_tip_commit, rebased_tip_commit];
  let merged_tree = block_on(merge_commit_trees(tx.repo(), &parent_commits)).map_err(|e| {
    JjError::IoError(format!(
      "Failed to build merge tree for rebase merge: {}",
      e
    ))
  })?;
  let merge_commit = block_on(async {
    tx.repo_mut()
      .new_commit(
        parent_commits
          .iter()
          .map(|commit| commit.id().clone())
          .collect(),
        merged_tree,
      )
      .set_description(message)
      .write()
      .await
  })
  .map_err(|e| JjError::IoError(format!("Failed to write rebase merge commit: {}", e)))?;

  let _ = block_on(tx.repo_mut().check_out(workspace_name, &merge_commit))
    .map_err(|e| JjError::IoError(format!("Failed to create working-copy commit: {}", e)))?;
  block_on(tx.repo_mut().rebase_descendants()).map_err(|e| {
    JjError::IoError(format!(
      "Failed to rebase descendants after rebase merge: {}",
      e
    ))
  })?;
  set_local_bookmark_target(tx.repo_mut(), target_branch, merge_commit.id().clone());
  let _ = git::export_refs(tx.repo_mut());

  let new_repo = block_on(tx.commit("rebase merge workspace"))
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

/// Squashes a workspace branch into the target branch and updates the description.
///
/// # Arguments
/// * `workspace_path` - Path to the workspace directory
/// * `workspace_branch` - Name of the workspace branch to squash
/// * `target_branch` - Name of the target branch to squash into
/// * `message` - Commit message for the squashed change
///
/// # Returns
/// Returns Ok(()) on success, or a JjError on failure.
// home-head-test: merge_workspace_into_home
pub fn jj_squash_merge_commit(
  workspace_path: &str,
  workspace_branch: &str,
  target_branch: &str,
  message: &str,
) -> Result<(), JjError> {
  validate_branch_name(workspace_branch, "workspace")?;
  validate_branch_name(target_branch, "target")?;
  validate_commit_message(message)?;

  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  let old_wc_commit = get_workspace_wc_commit(&loaded)?;
  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let target_symbol = resolve_target_branch_symbol(&loaded, workspace_path, target_branch)?;
  let destination_commit = resolve_commit_by_revision(&loaded, &target_symbol)?;
  let workspace_tip_commit = resolve_commit_by_revision(&loaded, workspace_branch)?;

  let mut tx = loaded.repo.start_transaction();
  let parent_commits = vec![destination_commit.clone(), workspace_tip_commit];
  let squashed_tree = block_on(merge_commit_trees(tx.repo(), &parent_commits))
    .map_err(|e| JjError::IoError(format!("Failed to build squash merge tree: {}", e)))?;
  let squashed_commit = block_on(async {
    tx.repo_mut()
      .new_commit(vec![destination_commit.id().clone()], squashed_tree)
      .set_description(message)
      .write()
      .await
  })
  .map_err(|e| JjError::IoError(format!("Failed to write squashed commit: {}", e)))?;
  let _ = block_on(tx.repo_mut().check_out(workspace_name, &squashed_commit))
    .map_err(|e| JjError::IoError(format!("Failed to create working-copy commit: {}", e)))?;
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants after squash: {}", e)))?;
  set_local_bookmark_target(tx.repo_mut(), target_branch, squashed_commit.id().clone());
  let _ = git::export_refs(tx.repo_mut());

  let new_repo = block_on(tx.commit("squash merge workspace"))
    .map_err(|e| JjError::IoError(format!("Failed to commit squash transaction: {}", e)))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    old_wc_commit.as_ref(),
    CheckoutMode::Immediate,
  )?;

  Ok(())
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

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use std::process::Command;
  use tempfile::TempDir;

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
    let status = Command::new("git")
      .current_dir(temp.path())
      .args(["init"])
      .status()
      .expect("git init should run");
    assert!(status.success(), "git init should succeed");
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
    let status = Command::new("jj")
      .current_dir(temp.path())
      .args(["git", "init", "--colocate", "."])
      .status()
      .expect("jj git init should run");
    assert!(status.success(), "jj git init should succeed");
  }

  fn git(temp: &TempDir, args: &[&str]) {
    let status = Command::new("git")
      .current_dir(temp.path())
      .args(args)
      .status()
      .expect("git command should run");
    assert!(status.success(), "git {:?} should succeed", args);
  }

  fn git_output(temp: &TempDir, args: &[&str]) -> String {
    let output = Command::new("git")
      .current_dir(temp.path())
      .args(args)
      .output()
      .expect("git command should run");
    assert!(output.status.success(), "git {:?} should succeed", args);
    String::from_utf8(output.stdout)
      .expect("git output should be utf8")
      .trim()
      .to_string()
  }

  fn init_colocated_repo_with_two_branches() -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    git(&temp, &["config", "user.name", "Test User"]);
    git(&temp, &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(&temp, &["add", "base.txt"]);
    git(&temp, &["commit", "-m", "base"]);
    git(&temp, &["branch", "branch-a"]);
    git(&temp, &["switch", "-c", "branch-b"]);
    fs::write(temp.path().join("branch.txt"), "branch b\n").expect("write branch file");
    git(&temp, &["add", "branch.txt"]);
    git(&temp, &["commit", "-m", "branch b"]);
    git(&temp, &["switch", "branch-a"]);
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
  fn reconcile_external_checkout_reparents_home_without_false_changes() {
    let temp = init_colocated_repo_with_two_branches();
    git(&temp, &["switch", "branch-b"]);
    let repo_path = temp.path().to_str().expect("utf8 path");

    let changes = jj_get_changed_files(repo_path).expect("reconcile changes");

    assert!(
      changes.is_empty(),
      "branch delta must not appear as local changes"
    );
    assert_eq!(
      home_wc_parent_id(repo_path),
      git_output(&temp, &["rev-parse", "HEAD"])
    );
  }

  #[test]
  fn reconcile_reparents_when_git_head_was_already_imported() {
    let temp = init_colocated_repo_with_two_branches();
    git(&temp, &["switch", "branch-b"]);
    let repo_path = temp.path().to_str().expect("utf8 path");
    let mut loaded = load_workspace_repo(repo_path).expect("load workspace");
    import_git_head_if_needed(&mut loaded, repo_path).expect("lightweight import");
    assert_ne!(
      home_wc_parent_id(repo_path),
      git_output(&temp, &["rev-parse", "HEAD"])
    );

    let changes = jj_get_changed_files(repo_path).expect("reconcile changes");

    assert!(changes.is_empty());
    assert_eq!(
      home_wc_parent_id(repo_path),
      git_output(&temp, &["rev-parse", "HEAD"])
    );
  }

  #[test]
  fn reconcile_external_checkout_preserves_real_local_edit() {
    let temp = init_colocated_repo_with_two_branches();
    fs::write(temp.path().join("local.txt"), "local edit\n").expect("write local edit");
    git(&temp, &["switch", "branch-b"]);
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
      git_output(&temp, &["rev-parse", "HEAD"])
    );
  }

  #[test]
  fn reconcile_external_commit_reparents_without_false_changes() {
    let temp = init_colocated_repo_with_two_branches();
    let repo_path = temp.path().to_str().expect("utf8 path");
    fs::write(temp.path().join("pulled.txt"), "new upstream content\n").expect("write file");
    git(&temp, &["add", "pulled.txt"]);
    git(&temp, &["commit", "-m", "advance checked-out branch"]);

    let changes = jj_get_changed_files(repo_path).expect("reconcile changes");

    assert!(
      changes.is_empty(),
      "new committed files must not appear as local changes"
    );
    assert_eq!(
      home_wc_parent_id(repo_path),
      git_output(&temp, &["rev-parse", "HEAD"])
    );
  }

  #[test]
  fn reconcile_detached_external_checkout_reparents_home() {
    let temp = init_colocated_repo_with_two_branches();
    git(&temp, &["checkout", "--detach", "branch-b"]);
    let repo_path = temp.path().to_str().expect("utf8 path");

    let changes = jj_get_changed_files(repo_path).expect("reconcile changes");

    assert!(changes.is_empty());
    assert_eq!(
      home_wc_parent_id(repo_path),
      git_output(&temp, &["rev-parse", "HEAD"])
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
    git(&temp, &["switch", "branch-b"]);

    reconcile_colocated_home_repo(repo_path).expect("reconcile home");

    assert_eq!(home_wc_parent_id(&child_path), child_parent_before);
    assert_eq!(
      home_wc_parent_id(repo_path),
      git_output(&temp, &["rev-parse", "HEAD"])
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
    git(&temp, &["config", "user.name", "Test User"]);
    git(&temp, &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(&temp, &["add", "base.txt"]);
    git(&temp, &["commit", "-m", "base"]);
    git(&temp, &["switch", "-c", "feature"]);
    fs::write(temp.path().join("feature.txt"), "feature\n").expect("write feature");
    git(&temp, &["add", "feature.txt"]);
    git(&temp, &["commit", "-m", "feature work"]);
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");
    let feature_tip = git_output(&temp, &["rev-parse", "feature"]);
    git(&temp, &["checkout", "--detach", &feature_tip]);
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
  fn ensure_gitignore_entries_does_not_add_opt_in_paths() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8");
    ensure_gitignore_entries(repo_path).expect("write gitignore");
    let gitignore = fs::read_to_string(temp.path().join(".gitignore")).expect("read gitignore");
    assert!(!gitignore.contains(".jj*/"));
    assert!(!gitignore.contains(".agents/skills/treq*/"));
    assert!(!gitignore.contains(".claude/skills/treq*/"));
    ensure_gitignore_entries(repo_path).expect("rewrite gitignore");
    let again = fs::read_to_string(temp.path().join(".gitignore")).expect("read gitignore");
    assert_eq!(again, gitignore);
    assert!(
      !gitignore.lines().any(|line| line == ".agents/skills/"),
      "must not ignore all Codex/Cursor user skills, got:\n{gitignore}"
    );
    assert!(
      !gitignore.lines().any(|line| line == ".claude/skills/"),
      "must not ignore all Claude user skills, got:\n{gitignore}"
    );

    ensure_optional_gitignore_entries(repo_path).expect("write optional entries");
    ensure_optional_gitignore_entries(repo_path).expect("rewrite optional entries");
    let opted_in = fs::read_to_string(temp.path().join(".gitignore")).expect("read gitignore");
    for entry in [".jj*/", ".agents/skills/treq*/", ".claude/skills/treq*/"] {
      assert_eq!(opted_in.matches(entry).count(), 1);
    }
  }

  #[test]
  fn ensure_jj_initialized_appends_opt_in_gitignore_entries() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8");
    fs::create_dir(temp.path().join(".jj")).expect("mkdir .jj");
    fs::write(temp.path().join(".gitignore"), ".jj/\n.jj*/\n.treq/\n").expect("write gitignore");
    let db = crate::db::Database::new(temp.path().join("test.db")).expect("open db");
    db.init().expect("init db");
    db.set_repo_setting(repo_path, "jj_initialized", "true")
      .expect("set flag");
    db.set_repo_setting(repo_path, "ignore_generated_treq_paths", "true")
      .expect("set opt in");
    ensure_jj_initialized(&db, repo_path).expect("ensure init");
    let gitignore = fs::read_to_string(temp.path().join(".gitignore")).expect("read gitignore");
    assert!(
      gitignore.contains(".agents/skills/treq*/") && gitignore.contains(".claude/skills/treq*/"),
      "opening an existing repo should append treq skill globs, got:\n{gitignore}"
    );
  }

  #[test]
  fn git_status_ignores_treq_prefixed_skill_dirs_but_reports_user_skills() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8");
    ensure_gitignore_entries(repo_path).expect("write gitignore");
    ensure_optional_gitignore_entries(repo_path).expect("write optional gitignore entries");
    for relative in [
      ".agents/skills/treq/SKILL.md",
      ".agents/skills/treq-extra/SKILL.md",
      ".claude/skills/treq/SKILL.md",
      ".claude/skills/treq-extra/SKILL.md",
      ".agents/skills/user-skill/SKILL.md",
      ".claude/skills/user-skill/SKILL.md",
    ] {
      let path = temp.path().join(relative);
      fs::create_dir_all(path.parent().expect("parent")).expect("mkdir skill");
      fs::write(&path, "skill\n").expect("write skill");
    }
    let status = Command::new("git")
      .current_dir(temp.path())
      .args(["status", "--porcelain"])
      .output()
      .expect("git status");
    assert!(status.status.success(), "git status should succeed");
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(
      !stdout.contains(".agents/skills/treq") && !stdout.contains(".claude/skills/treq"),
      "external git should ignore treq-prefixed skills, got:\n{stdout}"
    );
    assert!(stdout.contains(".agents/") && stdout.contains(".claude/"));
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
      (local_id.clone(), "same-tree".to_string()),
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
    git(&temp, &["config", "user.name", "Test User"]);
    git(&temp, &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(&temp, &["add", "base.txt"]);
    git(&temp, &["commit", "-m", "base"]);
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
    git(&temp, &["config", "user.name", "Test User"]);
    git(&temp, &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(&temp, &["add", "base.txt"]);
    git(&temp, &["commit", "-m", "base"]);
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");

    let names = list_all_workspace_names(repo_path).expect("list workspace names");
    assert_eq!(names, vec!["default".to_string()]);
  }

  #[test]
  fn jj_list_files_at_revision_lists_tree_contents() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(&temp);
    git(&temp, &["config", "user.name", "Test User"]);
    git(&temp, &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    fs::create_dir_all(temp.path().join("dir")).expect("mkdir");
    fs::write(temp.path().join("dir/nested.txt"), "nested\n").expect("write nested");
    git(&temp, &["add", "base.txt", "dir/nested.txt"]);
    git(&temp, &["commit", "-m", "base"]);
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
    git(&temp, &["config", "user.name", "Test User"]);
    git(&temp, &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(&temp, &["add", "base.txt"]);
    git(&temp, &["commit", "-m", "base"]);
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
    git(&temp, &["config", "user.name", "Test User"]);
    git(&temp, &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(&temp, &["add", "base.txt"]);
    git(&temp, &["commit", "-m", "base"]);
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
    git(&temp, &["config", "user.name", "Test User"]);
    git(&temp, &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").expect("write base");
    git(&temp, &["add", "base.txt"]);
    git(&temp, &["commit", "-m", "base"]);
    init_jj_repo(&temp);
    let repo_path = temp.path().to_str().expect("utf8 path");

    // A branch created purely via git, never touched by jj, isn't yet a jj bookmark.
    git(&temp, &["branch", "git-only-branch"]);
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
}
