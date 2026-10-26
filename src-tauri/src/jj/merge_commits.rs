use super::*;

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
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");

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
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");

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
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");

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
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");

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
