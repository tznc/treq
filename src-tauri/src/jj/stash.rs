use super::*;

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
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");

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
  git::export_refs(tx.repo_mut()).warn_on_err("export refs");

  let new_repo = block_on(tx.commit("stash commit"))
    .map_err(|e| JjError::IoError(format!("Failed to commit stash-commit transaction: {}", e)))?;
  update_workspace_after_history_edit(&mut loaded, &new_repo, None, CheckoutMode::Immediate)?;

  let repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  reconcile_all_workspaces_after_rewrite(&repo_path, None)
    .warn_on_err("reconcile all workspaces after rewrite");

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
      let Ok(values) = entry.values else { continue };
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
