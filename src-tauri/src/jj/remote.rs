use super::*;

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
  if !Path::new(workspace_path).exists() {
    return Ok(String::new());
  }
  // Get current branch name to check/ensure tracking
  let branch_name = get_workspace_branch(workspace_path)?;
  if branch_name.is_empty() || branch_name == "HEAD" {
    return Err(JjError::IoError(
      "Cannot push: not on a branch (HEAD is detached). Check out a branch first.".to_string(),
    ));
  }
  jj_push_bookmark(workspace_path, &branch_name)
}

/// Push one bookmark to origin, whichever branch the workspace is on.
pub fn jj_push_bookmark(workspace_path: &str, branch_name: &str) -> Result<String, JjError> {
  let branch_name = branch_name.to_string();

  // Ensure bookmark is tracked before push to avoid non-tracking bookmark warnings.
  let mut tracking_message = String::new();

  match origin_bookmark_state(workspace_path, &branch_name) {
    Ok((true, _)) => {
      // Already tracked, proceed normally
    }
    // A branch not yet on origin has nothing to track; the push creates it.
    Ok((false, false)) => {}
    Ok((false, true)) => {
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

  let repo_lock = repo_op_lock(workspace_path);
  let _guard = repo_lock
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        format!(
          "Cannot push: local bookmark '{branch_name}' is conflicted. Run `jj bookmark list --conflicted`, then `jj bookmark set {branch_name} -r <commit>` to pick a side."
        ),
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
pub(super) fn ensure_no_conflicted_commits_to_push(
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
  WorkspaceRepo::load(workspace_path).is_ok_and(|repo| repo.is_bookmark_conflicted(branch_name))
}

pub(super) fn is_bookmark_conflicted_in(loaded: &LoadedWorkspaceRepo, branch_name: &str) -> bool {
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

/// How many commits the local `bookmark` has that `bookmark@origin` lacks,
/// and the reverse. Errors when either side is missing or conflicted.
pub fn jj_bookmark_ahead_behind_origin(
  repo_path: &str,
  bookmark: &str,
) -> Result<(usize, usize), JjError> {
  let local = format_revset_symbol(bookmark);
  let remote = format_revset_symbol(&format!("{bookmark}@origin"));
  let ahead = jj_log_revset_commit_ids(repo_path, &format!("{remote}..{local}"))?.len();
  let behind = jj_log_revset_commit_ids(repo_path, &format!("{local}..{remote}"))?.len();
  Ok((ahead, behind))
}

pub(super) fn get_sync_revset_tips(
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

/// When each repository last fetched, so a caller that fetched moments ago
/// for the same purpose can skip the round trip.
pub(super) static LAST_FETCH: std::sync::OnceLock<
  std::sync::Mutex<HashMap<String, std::time::Instant>>,
> = std::sync::OnceLock::new();

/// [`jj_git_fetch`] unless `repo_path` fetched within `max_age`.
pub fn jj_git_fetch_unless_recent(
  repo_path: &str,
  max_age: std::time::Duration,
) -> Result<String, JjError> {
  let recent = LAST_FETCH
    .get()
    .and_then(|m| m.lock().ok()?.get(repo_path).copied())
    .is_some_and(|at| at.elapsed() < max_age);
  if recent {
    return Ok(String::new());
  }
  jj_git_fetch(repo_path)
}

/// Fetch remote branches using jj git fetch (without rebasing)
/// This updates remote tracking refs and makes remote branches available
pub fn jj_git_fetch(repo_path: &str) -> Result<String, JjError> {
  let fetched = fetch_origin(repo_path);
  if fetched.is_ok() {
    if let Ok(mut last) = LAST_FETCH.get_or_init(Default::default).lock() {
      last.insert(repo_path.to_string(), std::time::Instant::now());
    }
  }
  fetched
}

/// The remote the default branch and rebase targets come from: `upstream`
/// in a fork clone that has one, else `origin`. Pushes always go to origin.
pub fn jj_base_remote(repo_path: &str) -> &'static str {
  if get_git_remotes(repo_path).contains("upstream") {
    "upstream"
  } else {
    "origin"
  }
}

/// Fetches origin, then upstream when a fork clone has one.
pub(super) fn fetch_origin(repo_path: &str) -> Result<String, JjError> {
  fetch_remote(repo_path, "origin")?;
  if jj_base_remote(repo_path) == "upstream" {
    fetch_remote(repo_path, "upstream")?;
  }
  Ok(String::new())
}

fn fetch_remote(repo_path: &str, remote_name: &str) -> Result<String, JjError> {
  if !get_git_remotes(repo_path).contains(remote_name) {
    return Ok(String::new());
  }
  let repo_lock = repo_op_lock(repo_path);
  let _guard = repo_lock
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
  let mut loaded = load_workspace_repo(repo_path)?;
  let subprocess_options = git_subprocess_options(&loaded.settings)?;
  let import_options = git::GitImportOptions {
    auto_local_bookmark: false,
    abandon_unreachable_commits: true,
    remote_auto_track_bookmarks: HashMap::new(),
  };
  let remote = RemoteName::new(remote_name);
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
  reset_conflicted_bookmarks_origin_contains(tx.repo_mut());
  // The fetch can move tracked local bookmarks such as `main`. Without an
  // export, git's `refs/heads/main` stays on the old commit, and the next git
  // command that moves it splits jj's `main` into a conflict.
  git::export_refs(tx.repo_mut()).warn_on_err("export refs after fetch");
  if tx.repo().has_changes() {
    block_on(tx.repo_mut().rebase_descendants())
      .map_err(|e| JjError::IoError(format!("Failed to rebase after fetch: {}", e)))?;
    loaded.repo = block_on(tx.commit("fetch from git remote"))
      .map_err(|e| JjError::IoError(format!("Failed to commit fetch: {}", e)))?;
  }
  import_colocated_git_state(&mut loaded, repo_path)?;

  Ok(String::new())
}

/// A local bookmark split by racing operations, every side of which origin
/// already has, loses nothing when reset to its `@origin` target. Any other
/// conflict holds a commit only this machine has and is left for the user.
pub(super) fn reset_conflicted_bookmarks_origin_contains(repo: &mut MutableRepo) {
  let origin = RemoteName::new("origin");
  let resets: Vec<_> = repo
    .view()
    .local_bookmarks()
    .filter(|(_, local)| local.has_conflict())
    .filter_map(|(name, local)| {
      let symbol = RemoteRefSymbol {
        name,
        remote: origin,
      };
      let remote = repo
        .view()
        .get_remote_bookmark(symbol)
        .target
        .as_normal()?
        .clone();
      let contained = local
        .added_ids()
        .all(|id| repo.index().is_ancestor(id, &remote).unwrap_or(false));
      contained.then(|| (name.to_owned(), remote))
    })
    .collect();
  for (name, remote) in resets {
    repo.set_local_bookmark_target(&name, RefTarget::normal(remote));
  }
}

/// Serializes operations that move bookmarks (fetch, push) across the home
/// repo and all its workspaces, which share one operation log. Two of them
/// started from the same operation each move a bookmark, and jj records the
/// merge as a conflicted bookmark.
pub(super) fn repo_op_lock(workspace_path: &str) -> std::sync::Arc<std::sync::Mutex<()>> {
  let repo_root =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  working_copy_lock(
    &Path::new(&repo_root)
      .join(".jj")
      .join("repo")
      .to_string_lossy(),
  )
}

/// Git subprocess options for jj-lib fetch/push, resolving a compatible Git
/// even when GUI launches find an older system Git first.
pub(super) fn git_subprocess_options(
  settings: &UserSettings,
) -> Result<git::GitSubprocessOptions, JjError> {
  let mut options = git::GitSettings::from_settings(settings)
    .map_err(|e| JjError::IoError(format!("Failed to load git settings: {}", e)))?
    .to_subprocess_options();
  options.executable_path =
    PathBuf::from(binary_paths::resolve_remote_git().map_err(JjError::IoError)?);
  disable_auto_maintenance(&mut options.environment);
  Ok(options)
}

/// Keeps `git fetch` and `git push` from spawning `git maintenance run --auto`
/// (and a possible auto-gc that can stall a big repo) after every call. Passed
/// through `GIT_CONFIG_COUNT` so any config the caller's environment already
/// injects stays in force.
pub(super) fn disable_auto_maintenance(environment: &mut HashMap<OsString, OsString>) {
  let existing = std::env::var("GIT_CONFIG_COUNT")
    .ok()
    .and_then(|count| count.parse::<usize>().ok())
    .unwrap_or(0);
  let settings = [("maintenance.auto", "false"), ("gc.auto", "0")];
  for (offset, (key, value)) in settings.into_iter().enumerate() {
    let index = existing + offset;
    environment.insert(format!("GIT_CONFIG_KEY_{index}").into(), key.into());
    environment.insert(format!("GIT_CONFIG_VALUE_{index}").into(), value.into());
  }
  environment.insert(
    "GIT_CONFIG_COUNT".into(),
    (existing + settings.len()).to_string().into(),
  );
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
