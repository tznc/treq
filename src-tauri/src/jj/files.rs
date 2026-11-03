use super::*;

/// Get list of changed files in working copy using jj-lib (no subprocess)
/// Snapshots the working copy and diffs against the parent commit tree.
pub fn jj_get_changed_files(workspace_path: &str) -> Result<Vec<JjFileChange>, JjError> {
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
  let Ok(settings) = create_user_settings(settings_path) else {
    return Ok(Vec::new());
  };
  with_working_copy_lock(workspace_path, || {
    let Ok(mut workspace) = Workspace::load(
      &settings,
      path,
      &StoreFactories::default(),
      &default_working_copy_factories(),
    ) else {
      return Ok(Vec::new());
    };
    let Ok(repo) = block_on(workspace.repo_loader().load_at_head()) else {
      return Ok(Vec::new());
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
    let Ok(wc_commit) = repo.store().get_commit(&wc_commit_id) else {
      return Ok(Vec::new());
    };

    let Ok(mut locked_ws) = workspace.start_working_copy_mutation() else {
      return Ok(Vec::new());
    };
    let Ok((new_tree, _)) = block_on(locked_ws.locked_wc().snapshot(&opts)) else {
      return Ok(Vec::new());
    };

    // If tree changed, create a new WC commit and update op-store to expose the snapshotted state.
    if new_tree.tree_ids() != wc_commit.tree_ids() {
      let mut tx = repo.start_transaction();
      let mut builder = tx.repo_mut().rewrite_commit(&wc_commit).detach();
      builder.set_tree(new_tree.clone());
      match block_on(builder.write(tx.repo_mut())) {
        Ok(rewritten_wc) => {
          block_on(tx.repo_mut().edit(workspace_name, &rewritten_wc))
            .warn_on_err("edit rewritten working copy");
          block_on(tx.repo_mut().rebase_descendants()).warn_on_err("rebase descendants");
          match block_on(tx.commit("snapshot working copy")) {
            Ok(final_repo) => {
              block_on(locked_ws.finish(final_repo.op_id().clone()))
                .warn_on_err("finish working copy update");
            }
            Err(_) => {
              block_on(locked_ws.finish(repo.op_id().clone()))
                .warn_on_err("finish working copy update");
            }
          }
        }
        Err(_) => {
          block_on(locked_ws.finish(repo.op_id().clone()))
            .warn_on_err("finish working copy update");
        }
      }
    } else {
      block_on(locked_ws.finish(repo.op_id().clone())).warn_on_err("finish working copy update");
    }

    Ok(tree_changes(&repo, &wc_commit, &new_tree))
  })
}

/// The files `tree` changes against `wc_commit`'s first parent.
pub(super) fn tree_changes(
  repo: &ReadonlyRepo,
  wc_commit: &Commit,
  tree: &MergedTree,
) -> Vec<JjFileChange> {
  use futures::StreamExt as _;

  let parent_tree = match wc_commit.parent_ids().first() {
    None => repo.store().root_commit().tree(),
    Some(parent_id) => match repo.store().get_commit(parent_id) {
      Ok(parent) => parent.tree(),
      Err(_) => return Vec::new(),
    },
  };

  let diff_matcher = repo_root_matcher();
  let diff_entries = block_on(
    parent_tree
      .diff_stream(tree, &diff_matcher)
      .collect::<Vec<_>>(),
  );
  // Hunks are read against all parents merged; with one parent that is the
  // tree diffed here, so its values name what the hunks are read between.
  let content_ids = wc_commit.parent_ids().len() <= 1;

  let mut changes = Vec::new();
  for entry in diff_entries {
    let file_path = entry.path.as_internal_file_string().to_string();
    let Ok(values) = entry.values else { continue };
    let status = if !values.before.is_resolved() || !values.after.is_resolved() {
      "C"
    } else if values.before.is_absent() {
      "A"
    } else if values.after.is_absent() {
      "D"
    } else {
      "M"
    };
    let content_id = content_ids.then(|| {
      format!(
        "{}>{}",
        tree_value_id(&values.before),
        tree_value_id(&values.after)
      )
    });
    changes.push(JjFileChange {
      path: file_path,
      status: status.to_string(),
      previous_path: None,
      changed_line_count: 0,
      diff_deferred: false,
      content_id,
    });
  }
  changes
}

/// Names one side of a path in a tree diff by its object ids.
pub(super) fn tree_value_id(value: &MergedTreeValue) -> String {
  value
    .iter()
    .map(|term| match term {
      None => "-".to_string(),
      Some(TreeValue::File { id, executable, .. }) => {
        format!("{}{}", if *executable { "x" } else { "f" }, id.hex())
      }
      Some(TreeValue::Symlink(id)) => format!("l{}", id.hex()),
      Some(TreeValue::Tree(id)) => format!("t{}", id.hex()),
      Some(TreeValue::GitSubmodule(id)) => format!("s{}", id.hex()),
    })
    .collect::<Vec<_>>()
    .join(",")
}

/// [`jj_is_working_copy_empty`] from a snapshot the caller already took, so
/// the working copy is not walked again. Without one, it snapshots.
pub(super) fn snapshot_is_empty(
  loaded: &LoadedWorkspaceRepo,
  workspace_path: &str,
  snapshot: Option<&(jj_lib::backend::CommitId, MergedTree)>,
) -> Result<bool, JjError> {
  use futures::StreamExt as _;

  let Some((wc_commit_id, tree)) = snapshot else {
    return jj_is_working_copy_empty(workspace_path);
  };
  let Ok(wc_commit) = loaded.repo.store().get_commit(wc_commit_id) else {
    return Ok(true);
  };
  // Against the first parent, as `jj_get_changed_files` lists changes.
  let parent_tree = match wc_commit.parent_ids().first() {
    None => loaded.repo.store().root_commit().tree(),
    Some(parent_id) => match loaded.repo.store().get_commit(parent_id) {
      Ok(parent) => parent.tree(),
      Err(_) => return Ok(true),
    },
  };
  if parent_tree.tree_ids() == tree.tree_ids() {
    return Ok(true);
  }
  let matcher = repo_root_matcher();
  let entries = block_on(parent_tree.diff_stream(tree, &matcher).collect::<Vec<_>>());
  Ok(entries.iter().all(|entry| entry.values.is_err()))
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
  // A new empty commit above the bookmark, never the bookmark itself: edits
  // there would move it. `jj_working_copy_needs_sync` counts a bookmark below
  // `@` as synced, so this does not loop.
  block_on(tx.repo_mut().check_out(workspace_name, &destination))
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
  let (_, mut hunks) = jj_get_file_hunks_batch(
    workspace_path,
    &[file_path.to_string()],
    conflict_marker_style,
  )?;
  hunks
    .pop()
    .unwrap_or_else(|| Err(JjError::IoError("No hunks read".to_string())))
}

/// The [`working_copy_token`] of a batch read, and each file's hunks.
pub type FileHunksBatch = (String, Vec<Result<Vec<JjDiffHunk>, JjError>>);

/// [`jj_get_file_hunks`] for several files from one load and snapshot,
/// with the [`working_copy_token`] of the diff they were read from.
pub fn jj_get_file_hunks_batch(
  workspace_path: &str,
  file_paths: &[String],
  conflict_marker_style: &str,
) -> Result<FileHunksBatch, JjError> {
  let ignore_root =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  with_working_copy_lock(workspace_path, || {
    let mut loaded = load_workspace_repo(workspace_path)?;
    import_colocated_git_state(&mut loaded, workspace_path)?;
    let (wc_commit, tree) = snapshot_up_to_date_working_copy(&mut loaded, &ignore_root)?
      .ok_or_else(|| JjError::IoError("No working-copy commit found".to_string()))?;
    let parent_tree = block_on(wc_commit.parent_tree(loaded.repo.as_ref()))
      .map_err(|e| JjError::IoError(format!("Failed to read parent tree: {}", e)))?;
    let merge_options = MergeOptions::from_settings(&loaded.settings)
      .map_err(|e| JjError::IoError(format!("Failed to load merge options: {}", e)))?;
    let style = parse_conflict_marker_style(conflict_marker_style);
    let hunks = file_paths
      .iter()
      .map(|file_path| {
        file_hunks_between(
          loaded.repo.store(),
          &parent_tree,
          &tree,
          file_path,
          style,
          &merge_options,
        )
      })
      .collect();
    Ok((working_copy_token(&parent_tree, &tree), hunks))
  })
}

/// [`jj_get_file_hunks_batch`] for the diff `token` (a
/// [`working_copy_token`]) names: its trees are read from the store by id,
/// with no lock or snapshot, so the hunks show the state the token was read
/// at. `None` when the store has no such trees.
pub fn jj_get_file_hunks_at(
  workspace_path: &str,
  token: &str,
  file_paths: &[String],
  conflict_marker_style: &str,
) -> Result<Option<FileHunksBatch>, JjError> {
  let repo_path_opt = derive_repo_path_from_workspace(workspace_path);
  let settings = create_user_settings(repo_path_opt.as_deref().unwrap_or(workspace_path))?;
  // The store is all a read by id needs: no operation or view is loaded.
  let workspace = Workspace::load(
    &settings,
    Path::new(workspace_path),
    &StoreFactories::default(),
    &default_working_copy_factories(),
  )
  .map_err(|e| JjError::IoError(format!("Failed to load workspace: {}", e)))?;
  let store = workspace.repo_loader().store();
  let Some((parent_tree, tree)) = working_copy_token_trees(store, token) else {
    return Ok(None);
  };
  // A token from another repository, or trees since collected.
  if block_on(parent_tree.trees()).is_err() || block_on(tree.trees()).is_err() {
    return Ok(None);
  }
  let merge_options = MergeOptions::from_settings(&settings)
    .map_err(|e| JjError::IoError(format!("Failed to load merge options: {}", e)))?;
  let style = parse_conflict_marker_style(conflict_marker_style);
  let hunks = file_paths
    .iter()
    .map(|file_path| {
      file_hunks_between(store, &parent_tree, &tree, file_path, style, &merge_options)
    })
    .collect();
  Ok(Some((token.to_string(), hunks)))
}

/// The two trees a [`working_copy_token`] names; `None` when it is not one.
pub(super) fn working_copy_token_trees(
  store: &Arc<Store>,
  token: &str,
) -> Option<(MergedTree, MergedTree)> {
  let tree = |hex: &str| {
    let ids = hex
      .split(',')
      .map(TreeId::try_from_hex)
      .collect::<Option<Vec<_>>>()?;
    // A merge has one more side than bases.
    if ids.len() % 2 == 0 {
      return None;
    }
    let ids = Merge::from_vec(ids);
    Some(MergedTree::new(
      store.clone(),
      ids,
      ConflictLabels::unlabeled(),
    ))
  };
  let (parent, wc) = token.split_once("..")?;
  Some((tree(parent)?, tree(wc)?))
}

/// Names a working-copy diff by its two trees: reads with the same token
/// list the same files with the same hunks. [`jj_get_file_hunks_at`]
/// reads the trees back from it.
pub(super) fn working_copy_token(parent_tree: &MergedTree, tree: &MergedTree) -> String {
  let hex = |tree: &MergedTree| {
    tree
      .tree_ids()
      .iter()
      .map(ObjectId::hex)
      .collect::<Vec<_>>()
      .join(",")
  };
  format!("{}..{}", hex(parent_tree), hex(tree))
}

/// One file's hunks between the working copy's parent tree and its tree.
pub(super) fn file_hunks_between(
  store: &Arc<Store>,
  parent_tree: &MergedTree,
  tree: &MergedTree,
  file_path: &str,
  style: ConflictMarkerStyle,
  merge_options: &MergeOptions,
) -> Result<Vec<JjDiffHunk>, JjError> {
  if file_path.contains('\0') || file_path.is_empty() {
    return Err(JjError::IoError("Invalid file path".to_string()));
  }
  let target_repo_path = RepoPathBuf::from_internal_string(file_path)
    .map_err(|_| JjError::IoError("Invalid file path".to_string()))?;
  let before = block_on(parent_tree.path_value(target_repo_path.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to read parent path value: {}", e)))?;
  let after = block_on(tree.path_value(target_repo_path.as_ref()))
    .map_err(|e| JjError::IoError(format!("Failed to read working path value: {}", e)))?;
  let labels = ConflictLabels::unlabeled();
  let before_materialized = block_on(jj_lib::conflicts::materialize_tree_value(
    store,
    target_repo_path.as_ref(),
    before,
    &labels,
  ))
  .map_err(|e| JjError::IoError(format!("Failed to materialize parent value: {}", e)))?;
  let after_materialized = block_on(jj_lib::conflicts::materialize_tree_value(
    store,
    target_repo_path.as_ref(),
    after,
    &labels,
  ))
  .map_err(|e| JjError::IoError(format!("Failed to materialize working value: {}", e)))?;

  let before_bytes = block_on(materialized_value_to_bytes(
    target_repo_path.as_ref(),
    before_materialized,
    style,
    Some(merge_options),
  ));
  let after_bytes = block_on(materialized_value_to_bytes(
    target_repo_path.as_ref(),
    after_materialized,
    style,
    Some(merge_options),
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

pub(super) fn parse_conflict_marker_style(conflict_marker_style: &str) -> ConflictMarkerStyle {
  match conflict_marker_style {
    "snapshot" => ConflictMarkerStyle::Snapshot,
    "diff" => ConflictMarkerStyle::Diff,
    _ => ConflictMarkerStyle::Git,
  }
}

/// Parse git diff output into hunks
pub(super) fn parse_git_diff_hunks(diff: &str) -> Result<Vec<JjDiffHunk>, JjError> {
  let mut hunks = Vec::new();
  let mut current_hunk: Option<(String, Vec<String>)> = None;
  let mut hunk_index = 0;

  // Split on `\n` only: `lines()` would also strip a `\r` that is part of
  // the line's text (the diff already dropped each line's own `\r\n`).
  for line in diff.strip_suffix('\n').unwrap_or(diff).split('\n') {
    if line.starts_with("@@") {
      // Save previous hunk if exists
      if let Some((header, lines)) = current_hunk.take() {
        hunks.push(JjDiffHunk {
          id: format!("hunk-{}", hunk_index),
          header,
          lines,
          patch: String::new(),
          conflict_style: ConflictStyle::Unknown,
          conflict_regions: Vec::new(),
        });
        hunk_index += 1;
      }

      // Start new hunk
      current_hunk = Some((line.to_string(), Vec::new()));
    } else if let Some((_, ref mut lines)) = current_hunk {
      // The input is hunks only (no file headers), so a line such as
      // `--- end` is a removed `-- end`, not metadata.
      lines.push(line.to_string());
    }
  }

  // Save last hunk
  if let Some((header, lines)) = current_hunk {
    hunks.push(JjDiffHunk {
      id: format!("hunk-{}", hunk_index),
      header,
      lines,
      patch: String::new(),
      conflict_style: ConflictStyle::Unknown,
      conflict_regions: Vec::new(),
    });
  }

  Ok(hunks)
}

/// Reads `file_path` from git HEAD for a plain git repo with no `.jj`.
pub(super) fn read_git_head_blob(repo_path: &str, file_path: &str) -> Result<String, JjError> {
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
