use super::*;

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
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");
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

/// Whether origin's `bookmark_name` is tracked, and whether it is there at
/// all, from one load.
pub(super) fn origin_bookmark_state(
  workspace_path: &str,
  bookmark_name: &str,
) -> Result<(bool, bool), JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  let symbol = RemoteRefSymbol {
    name: RefName::new(bookmark_name),
    remote: RemoteName::new("origin"),
  };
  let remote_ref = loaded.repo.view().get_remote_bookmark(symbol);
  Ok((remote_ref.is_tracked(), remote_ref.is_present()))
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
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");
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

/// Points `branch_name` at `<branch>@origin` and gives the workspace a clean
/// working copy on top of it, like `git reset --hard origin/<branch>`. The
/// commits only this branch had, uncommitted changes included, are
/// abandoned; ones another bookmark or working copy builds on are kept.
/// Returns how many commits were abandoned, not counting the working copy.
// home-head-test: reset_to_remote
pub fn jj_reset_bookmark_to_remote(
  workspace_path: &str,
  branch_name: &str,
) -> Result<usize, JjError> {
  if !Path::new(workspace_path).exists() {
    return Ok(0);
  }
  validate_branch_name(branch_name, "workspace")?;
  let mut loaded = load_workspace_repo_for_history_edit(workspace_path)?;
  snapshot_loaded_working_copy(&mut loaded, workspace_path)?;
  let upstream = RemoteRefSymbol {
    name: RefName::new(branch_name),
    remote: RemoteName::new("origin"),
  };
  if !loaded
    .repo
    .view()
    .get_remote_bookmark(upstream)
    .is_present()
  {
    return Err(JjError::IoError(format!(
      "origin/{branch_name} does not exist"
    )));
  }
  let remote = format_revset_symbol(&format!("{branch_name}@origin"));
  // The fetch leaves the bookmark conflicted when it diverged, so name its
  // targets by id.
  let local = loaded
    .repo
    .view()
    .get_local_bookmark(RefName::new(branch_name))
    .added_ids()
    .map(|id| id.hex())
    .chain(["@".to_string()])
    .collect::<Vec<_>>()
    .join(" | ");
  let remote_commit = resolve_commit_by_revision(&loaded, &remote)?;
  let old_wc = get_workspace_wc_commit(&loaded)?;
  let kept = format!("::((bookmarks() | remote_bookmarks() | working_copies()) ~ ({local}))");
  let dropped = evaluate_revset(&loaded, &format!("({remote}..({local})) ~ {kept}"))?
    .iter()
    .commits(loaded.repo.store())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| JjError::IoError(format!("Failed to list local commits: {}", e)))?;

  let workspace_name = loaded.workspace.workspace_name().to_owned();
  let mut tx = loaded.repo.start_transaction();
  let new_wc = block_on(
    tx.repo_mut()
      .new_commit(vec![remote_commit.id().clone()], remote_commit.tree())
      .write(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to create working copy commit: {}", e)))?;
  tx.repo_mut()
    .set_wc_commit(workspace_name, new_wc.id().clone())
    .map_err(|e| JjError::IoError(format!("Failed to move working copy: {}", e)))?;
  for commit in &dropped {
    tx.repo_mut().record_abandoned_commit(commit);
  }
  set_local_bookmark_target(tx.repo_mut(), branch_name, remote_commit.id().clone());
  block_on(tx.repo_mut().rebase_descendants())
    .map_err(|e| JjError::IoError(format!("Failed to rebase descendants: {}", e)))?;
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");
  let new_repo = block_on(tx.commit(format!("reset {branch_name} to origin")))
    .map_err(|e| JjError::IoError(format!("Failed to commit reset: {}", e)))?;
  update_workspace_after_history_edit(
    &mut loaded,
    &new_repo,
    old_wc.as_ref(),
    CheckoutMode::Immediate,
  )?;

  let old_wc_id = old_wc.map(|commit| commit.id().clone());
  Ok(
    dropped
      .iter()
      .filter(|c| Some(c.id()) != old_wc_id.as_ref())
      .count(),
  )
}
