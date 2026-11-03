use super::*;

/// Keep `.jj/` and `.treq/` out of version control without editing the repo's
/// `.gitignore`. jj-lib, unlike the jj CLI, does not write `.jj/.gitignore`.
/// `.treq/config.yaml` and `.treq/workflows/` are the user's, so stay trackable.
pub fn ensure_treq_dirs_ignored(repo_path: &str) -> Result<(), JjError> {
  let repo = Path::new(repo_path);
  crate::core::ignore_generated_dir(&repo.join(".jj"))
    .map_err(|e| JjError::InitFailed(format!("Failed to ignore .jj/: {e}")))?;
  crate::core::ignore_generated_dir_except(
    &repo.join(".treq"),
    &["config.yaml", "workflows/", "workflows/**"],
  )
  .map_err(|e| JjError::InitFailed(format!("Failed to ignore .treq/: {e}")))
}

/// Initialize jj for an existing git repository (colocated mode)
/// This creates a .jj/ directory alongside the existing .git/ directory
pub(super) fn init_jj_for_git_repo(repo_path: &str) -> Result<(), JjError> {
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
  ensure_treq_dirs_ignored(repo_path)?;

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

  if already_configured {
    if is_jj_workspace(repo_path) {
      ensure_treq_dirs_ignored(repo_path)?;
      return Ok(true); // Flag valid, .jj exists
    }
    // Flag stale — .jj was deleted. Clear flag, fall through to reinit
    db.set_repo_setting(repo_path, flag_key, "false")
      .warn_on_err("set repo setting");
  } else if is_jj_workspace(repo_path) {
    // Double-check filesystem in case flag got out of sync
    db.set_repo_setting(repo_path, flag_key, "true")
      .warn_on_err("set repo setting");
    ensure_treq_dirs_ignored(repo_path)?;
    return Ok(true);
  }

  // Check if it's actually a git repo before trying to initialize
  if !Path::new(repo_path).join(".git").exists() {
    return Err(JjError::NotGitRepository);
  }

  // Cache git user config so subsequent commits don't shell out
  cache_git_user_config(db, repo_path).warn_on_err("cache git user config");

  // Initialize jj
  init_jj_for_git_repo(repo_path)?;

  // Mark as configured in database
  db.set_repo_setting(repo_path, flag_key, "true")
    .map_err(|e| JjError::ConfigError(format!("Failed to save flag: {}", e)))?;

  Ok(true)
}

/// Sanitize workspace name for filesystem use
pub fn sanitize_workspace_name(name: &str) -> String {
  let name = name
    .replace(['/', '\\'], "-")
    .replace(['*', '?', '<', '>', '|', '"', ':'], "_")
    .trim_matches(|c: char| c == '.' || c.is_whitespace())
    .to_string();
  // An empty name would make the workspace dir `.treq/workspaces/` itself.
  if name.is_empty() {
    "workspace".to_string()
  } else {
    name
  }
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
pub(super) fn parse_sparse_patterns(patterns: &[String]) -> Result<Vec<RepoPathBuf>, JjError> {
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
pub(super) fn is_interrupted_workspace_init(
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
      // jj 0.46+ also writes a `.git` worktree file into colocated workspaces.
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
  jj_util_import_git_refs(repo_path).warn_on_err("jj util import git refs");

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
      import_tx
        .repo_mut()
        .track_remote_bookmark(sym)
        .warn_on_err("track remote bookmark");
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
        // On the bookmark, not the parent workspace's working copy: a child
        // there would pick up the parent's uncommitted changes on every
        // snapshot of the parent.
        parent_repo
          .view()
          .get_local_bookmark(RefName::new(&jj_ref))
          .added_ids()
          .next()
          .cloned()
          .ok_or_else(|| JjError::GitWorkspaceError(format!("Bookmark '{}' not found", jj_ref)))?
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
      .new_commit(vec![source_commit_id], parent_tree)
      .write(),
  )
  .map_err(|e| JjError::GitWorkspaceError(format!("Failed to create wc commit: {}", e)))?;

  // Point the new workspace's @ to the new wc commit
  block_on(tx.repo_mut().edit(new_ws_name.clone(), &new_wc))
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to set wc: {}", e)))?;

  // The bookmark stays on the source commit, below the working copy: only a
  // commit moves it, so uncommitted edits never reach workspaces stacked on it.
  tx.repo_mut().set_local_bookmark_target(
    RefName::new(branch_name),
    RefTarget::normal(source_commit.id().clone()),
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
      tx.repo_mut()
        .track_remote_bookmark(sym)
        .warn_on_err("track remote bookmark");
    }
  }

  // Rebase any descendants of rewritten commits (required before tx.commit)
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to rebase descendants: {}", e)))?;

  // Export to git refs (colocated repo)
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");

  let _committed_repo = block_on(tx.commit("create_workspace"))
    .map_err(|e| JjError::GitWorkspaceError(format!("Failed to commit transaction: {}", e)))?;

  let verified_repo = verify_created_workspace_bookmark(
    &new_workspace,
    branch_name,
    &new_ws_name,
    new_wc.id(),
    source_commit.id(),
  )?;

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
  block_on(tx.repo_mut().edit(new_ws_name, &target_commit)).map_err(|e| {
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
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");
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
  let repo_path = workspace_file_path(file_path).map_err(invalid)?;
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
  let Some(wc_commit) = get_workspace_wc_commit(&loaded)? else {
    return Ok(());
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
      fs::create_dir_all(parent).warn_on_err("create dir all");
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

pub(super) fn apply_sides_to_conflict_text(
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
pub(super) fn verify_created_workspace_bookmark(
  workspace: &Workspace,
  branch_name: &str,
  workspace_name: &WorkspaceNameBuf,
  expected_wc_id: &jj_lib::backend::CommitId,
  expected_bookmark_id: &jj_lib::backend::CommitId,
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
  if target.as_normal() == Some(expected_bookmark_id) {
    return Ok(latest);
  }

  if target.has_conflict() && target.added_ids().any(|id| id == expected_bookmark_id) {
    let mut tx = latest.start_transaction();
    tx.repo_mut().set_local_bookmark_target(
      RefName::new(branch_name),
      RefTarget::normal(expected_bookmark_id.clone()),
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
    if resolved_target.as_normal() == Some(expected_bookmark_id) {
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
pub(super) struct RegisteredWorkspace {
  workspace_name: String,
  workspace_path: String,
  branch_name: String,
  has_conflicts: bool,
  last_activity_at: Option<String>,
  bookmark_conflicted: bool,
}

pub(super) fn load_home_workspace(repo_path: &str) -> Result<Workspace, JjError> {
  let settings = create_user_settings(repo_path)?;
  Workspace::load(
    &settings,
    Path::new(repo_path),
    &StoreFactories::default(),
    &default_working_copy_factories(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to load workspace: {}", e)))
}

pub(super) fn load_home_repo(
  repo_path: &str,
) -> Result<(Workspace, Arc<ReadonlyRepo>, SimpleWorkspaceStore), JjError> {
  let workspace = load_home_workspace(repo_path)?;
  let repo = block_on(workspace.repo_loader().load_at_head())
    .map_err(|e| JjError::IoError(format!("Failed to load repo: {}", e)))?;
  let workspace_store = SimpleWorkspaceStore::load(workspace.repo_path())
    .map_err(|e| JjError::IoError(format!("Failed to load workspace store: {}", e)))?;
  Ok((workspace, repo, workspace_store))
}

/// Every workspace's recorded path, read from the store index in one pass.
/// `SimpleWorkspaceStore::get_workspace_path` re-reads and decodes the whole
/// index per call, which makes a loop over N workspaces O(N^2).
pub(super) struct WorkspacePaths(HashMap<String, PathBuf>);

impl WorkspacePaths {
  pub(super) fn load(repo_path: &Path) -> Result<Self, JjError> {
    use prost::Message as _;
    let index = repo_path.join("workspace_store").join("index");
    let bytes = fs::read(&index)
      .map_err(|e| JjError::IoError(format!("Failed to read workspace store: {}", e)))?;
    let decoded = jj_lib::protos::simple_workspace_store::Workspaces::decode(&*bytes)
      .map_err(|e| JjError::IoError(format!("Failed to decode workspace store: {}", e)))?;
    let mut paths = HashMap::with_capacity(decoded.workspaces.len());
    for workspace in decoded.workspaces {
      let path = jj_lib::file_util::path_from_bytes(&workspace.path)
        .map_err(|e| JjError::IoError(format!("Failed to read workspace path: {}", e)))?
        .to_path_buf();
      paths.insert(workspace.name, path);
    }
    Ok(Self(paths))
  }

  pub(super) fn get(&self, workspace_name: &jj_lib::ref_name::WorkspaceName) -> Option<PathBuf> {
    self.get_by_name(workspace_name.as_str())
  }

  pub(super) fn get_by_name(&self, workspace_name: &str) -> Option<PathBuf> {
    self.0.get(workspace_name).cloned()
  }
}

pub(super) fn list_registered_workspaces(
  repo_path: &str,
) -> Result<Vec<RegisteredWorkspace>, JjError> {
  let (home_workspace, repo, _workspace_store) = load_home_repo(repo_path)?;
  let workspace_paths = WorkspacePaths::load(home_workspace.repo_path())?;
  let bookmark_index = BookmarkIndex::new(repo.view());

  let mut workspaces = Vec::new();
  for (workspace_name, wc_commit_id) in repo.view().wc_commit_ids() {
    if workspace_name.as_str() == "default" {
      continue;
    }
    // Short-lived conflict-resolve sandboxes are not product workspaces.
    if workspace_name.as_str().starts_with("_resolve-") {
      continue;
    }

    let stored_path = workspace_paths.get(workspace_name).ok_or_else(|| {
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
      branch_name_for_workspace_commit(&bookmark_index, workspace_name.as_str(), &wc_commit);
    let last_activity_at = commit_author_timestamp_rfc3339(&wc_commit);
    let bookmark_conflicted = repo
      .view()
      .get_local_bookmark(RefName::new(&branch_name))
      .has_conflict();

    workspaces.push(RegisteredWorkspace {
      workspace_name: workspace_name.as_str().to_string(),
      workspace_path,
      branch_name,
      has_conflicts: bookmark_has_conflicts || !wc_commit.tree_ids().is_resolved(),
      last_activity_at,
      bookmark_conflicted,
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

pub(super) fn commit_author_timestamp_rfc3339(commit: &jj_lib::commit::Commit) -> Option<String> {
  commit
    .author()
    .timestamp
    .to_datetime()
    .ok()
    .map(|ts| ts.to_rfc3339())
}

/// Bookmark lookups for one pass over the workspaces, built from a single scan
/// of the view so each workspace costs O(1) instead of O(bookmarks).
pub(super) struct BookmarkIndex<'a> {
  /// Commit id to the names of bookmarks that point at it with a normal target.
  by_commit: HashMap<&'a jj_lib::backend::CommitId, Vec<&'a str>>,
  /// Workspace name to the bookmarks that match it (exact or sanitized), in
  /// view order, with whether the bookmark target is conflicted and the
  /// commit ids it adds.
  by_workspace: HashMap<String, Vec<(&'a str, &'a jj_lib::op_store::RefTarget)>>,
}

impl<'a> BookmarkIndex<'a> {
  fn new(view: &'a jj_lib::view::View) -> Self {
    let mut by_commit: HashMap<&jj_lib::backend::CommitId, Vec<&str>> = HashMap::new();
    let mut by_workspace: HashMap<String, Vec<(&str, &jj_lib::op_store::RefTarget)>> =
      HashMap::new();
    for (name, target) in view.local_bookmarks() {
      let name = name.as_str();
      if let Some(id) = target.as_normal() {
        by_commit.entry(id).or_default().push(name);
      }
      by_workspace
        .entry(name.to_string())
        .or_default()
        .push((name, target));
      let sanitized = sanitize_workspace_name(name);
      if sanitized != name {
        by_workspace
          .entry(sanitized)
          .or_default()
          .push((name, target));
      }
    }
    Self {
      by_commit,
      by_workspace,
    }
  }

  fn matching(&self, workspace_name: &str) -> &[(&'a str, &'a jj_lib::op_store::RefTarget)] {
    self
      .by_workspace
      .get(workspace_name)
      .map(Vec::as_slice)
      .unwrap_or(&[])
  }

  fn exact_for(&self, id: &jj_lib::backend::CommitId, workspace_name: &str) -> Option<&'a str> {
    self.by_commit.get(id).and_then(|names| {
      names
        .iter()
        .copied()
        .find(|name| *name == workspace_name || sanitize_workspace_name(name) == workspace_name)
    })
  }
}

/// The sanitized branch part of a `<id>-<branch>` workspace name.
fn strip_workspace_id_prefix(workspace_name: &str) -> Option<&str> {
  let (id, rest) = workspace_name.split_once('-')?;
  (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && !rest.is_empty()).then_some(rest)
}

pub(super) fn branch_name_for_workspace_commit(
  bookmarks: &BookmarkIndex<'_>,
  workspace_name: &str,
  wc_commit: &jj_lib::commit::Commit,
) -> (String, bool) {
  // New workspaces are named `<id>-<branch>`: fall back to matching the branch part.
  let registered_name = workspace_name;
  let workspace_name = if bookmarks.matching(workspace_name).is_empty() {
    strip_workspace_id_prefix(workspace_name).unwrap_or(workspace_name)
  } else {
    workspace_name
  };
  if let Some(name) = bookmarks.exact_for(wc_commit.id(), workspace_name) {
    return (name.to_string(), false);
  }

  // A conflicted bookmark does not appear as an exact bookmark because it is
  // not a normal RefTarget. Still retain its canonical (possibly slash-
  // containing) name when the registered working-copy commit is one of the
  // conflict's targets, and surface the ambiguity to callers.
  if let Some((name, _)) = bookmarks
    .matching(workspace_name)
    .iter()
    .find(|(_, target)| target.has_conflict() && target.added_ids().any(|id| id == wc_commit.id()))
  {
    return (name.to_string(), true);
  }

  if let Some(name) = wc_commit
    .parent_ids()
    .iter()
    .find_map(|parent_id| bookmarks.exact_for(parent_id, workspace_name))
  {
    return (name.to_string(), false);
  }

  // After a commit the WC tip is an empty commit above the bookmark. A later
  // divergent fetch conflicted-bookmarks the parent tip — not the WC — so the
  // lookups above miss. Prefer any matching bookmark (including conflicted)
  // over inventing a dashed workspace-name fallback that breaks PR / tip
  // revsets (`Revision 'feat-…' doesn't exist`).
  if let Some((name, target)) = bookmarks.matching(workspace_name).first() {
    return (name.to_string(), target.has_conflict());
  }

  (registered_name.to_string(), false)
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
        bookmark_conflicted: workspace.bookmark_conflicted,
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
  if !Path::new(workspace_path).exists() {
    return Ok(String::new());
  }
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
    local_db::release_connections_under(workspace_dir);
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
      let rebased = block_on(tx.repo_mut().rebase_descendants()).map_err(|e| {
        JjError::IoError(format!(
          "Failed to rebase descendants while forgetting workspace: {}",
          e
        ))
      })?;
      block_on(tx.commit("forget workspace"))
        .map_err(|e| JjError::IoError(format!("Failed to forget workspace: {}", e)))?;

      // Reconcile remaining workspaces — WC commits may have been rewritten by rebase_descendants; skip the workspace being removed.
      // With nothing rebased no WC commit moved, so every other working copy is current; reconciling
      // would only load each workspace to find that out.
      if rebased > 0 {
        reconcile_all_workspaces_after_rewrite(repo_path, Some(workspace_name))
          .warn_on_err("reconcile all workspaces after rewrite");
      }
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
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");
  block_on(tx.commit("move commit to workspace"))
    .map_err(|e| JjError::IoError(format!("Failed to commit move: {}", e)))?;

  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  reconcile_all_workspaces_after_rewrite(&repo_path, Some(target_name.as_str()))
    .warn_on_err("reconcile all workspaces after rewrite");
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

    let Ok(source_content) = fs::read_to_string(&source_file) else {
      outcome.skipped += 1;
      outcome.warnings.push(format!(
        "Skipped {}:{}-{}: source file not found",
        hunk.file_path, hunk.start_line, hunk.end_line
      ));
      continue;
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

pub(super) fn copy_path_recursive(source: &Path, destination: &Path) -> Result<(), JjError> {
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

/// Parses a path inside a workspace, refusing the root, anything that leaves the
/// workspace (absolute paths, `..`) and jj/git metadata.
pub(crate) fn workspace_file_path(path: &str) -> Result<RepoPathBuf, String> {
  let repo_path =
    RepoPathBuf::from_relative_path(path).map_err(|e| format!("path '{path}': {e}"))?;
  if repo_path.is_root()
    || matches!(
      repo_path.components().next().map(|c| c.as_internal_str()),
      Some(".jj" | ".git")
    )
  {
    return Err(format!("'{path}' is not a workspace file"));
  }
  Ok(repo_path)
}

pub(super) fn move_paths_between_workspaces(
  source_workspace_path: &Path,
  target_workspace_path: &Path,
  paths: &[String],
) -> Result<(), JjError> {
  // Validate every path before touching the disk so a bad entry moves nothing.
  let repo_paths = paths
    .iter()
    .map(|path| workspace_file_path(path).map_err(|e| JjError::IoError(format!("Cannot move {e}"))))
    .collect::<Result<Vec<_>, _>>()?;
  for repo_path in repo_paths {
    let source = repo_path.to_fs_path(source_workspace_path).map_err(|e| {
      JjError::IoError(format!(
        "Cannot move {}: {e}",
        repo_path.as_internal_file_string()
      ))
    })?;
    let target = repo_path.to_fs_path(target_workspace_path).map_err(|e| {
      JjError::IoError(format!(
        "Cannot move {}: {e}",
        repo_path.as_internal_file_string()
      ))
    })?;
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
