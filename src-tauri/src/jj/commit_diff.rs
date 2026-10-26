use super::*;

pub(super) fn get_commit_copy_records(
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

/// Past this many changed paths a diff skips rename and copy tracking: gix's
/// rewrite tracker pairs added files with removed ones, which takes seconds
/// for thousands of paths (1.3 s for 10,000), and a diff this large is
/// nearly always too large to render, as each path changes a line or more.
pub(super) const COPY_TRACKING_MAX_ENTRIES: usize = TOO_LARGE_COMMIT_DIFF_THRESHOLD / 2;

/// Whether the diff from `from_tree` to `to_tree` lists more paths than
/// [`COPY_TRACKING_MAX_ENTRIES`], counted without reading any file.
pub(super) fn too_many_paths_to_track_copies(
  from_tree: &MergedTree,
  to_tree: &MergedTree,
  matcher: &PrefixMatcher,
  limit: usize,
) -> bool {
  let counted = block_on(
    from_tree
      .diff_stream(to_tree, matcher)
      .take(limit + 1)
      .count(),
  );
  counted > limit
}

pub(super) fn get_copy_records_between(
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

pub(super) fn build_file_change(
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
    content_id: None,
  }
}

pub(super) fn build_file_diff_from_bytes(
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

pub(super) fn annotate_conflict_regions(
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
  WorkspaceRepo::load(workspace_path)?.merge_diff_between_revisions(
    from_revision,
    to_revision,
    _conflict_marker_style,
  )
}

pub(super) fn merge_diff_between_revisions_in(
  loaded: &LoadedWorkspaceRepo,
  from_revision: &str,
  to_revision: &str,
  _conflict_marker_style: &str,
) -> Result<JjRevisionDiff, JjError> {
  validate_branch_name(from_revision, "target")?;
  validate_branch_name(to_revision, "revision")?;
  let from_commit = resolve_commit_by_revision(loaded, from_revision)?;
  let to_commit = resolve_commit_by_revision(loaded, to_revision)?;
  let from_tree = from_commit.tree();
  let to_tree = to_commit.tree();

  let key = MergeDiffKey {
    repo: loaded.workspace.repo_path().to_path_buf(),
    from: from_commit.id().clone(),
    to: to_commit.id().clone(),
    style: _conflict_marker_style.to_string(),
  };
  if let Some(diff) = merge_diff_cache().get(&key) {
    return Ok(diff);
  }
  let matcher = repo_root_matcher();
  let copy_records =
    if too_many_paths_to_track_copies(&from_tree, &to_tree, &matcher, COPY_TRACKING_MAX_ENTRIES) {
      CopyRecords::default()
    } else {
      get_copy_records_between(
        loaded.repo.store(),
        from_commit.id(),
        to_commit.id(),
        &matcher,
      )?
    };
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
      let Ok(values) = entry.values else { continue };
      let before_absent = matches!(&values.before, MaterializedTreeValue::Absent);
      let after_absent = matches!(&values.after, MaterializedTreeValue::Absent);
      let style = parse_conflict_marker_style(_conflict_marker_style);
      let Ok(merge_options) = MergeOptions::from_settings(&loaded.settings) else {
        continue;
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

  let diff = JjRevisionDiff {
    committed_files,
    hunks_by_file,
    uncommitted_files: Vec::new(),
    conflicted_files: Vec::new(),
    too_large_to_render: false,
    render_block_reason: None,
    working_copy_token: None,
  };
  merge_diff_cache().put(key, &diff);
  Ok(diff)
}

/// What a merge diff was read from: commit ids name their trees, so the
/// same key always yields the same diff.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct MergeDiffKey {
  repo: PathBuf,
  from: jj_lib::backend::CommitId,
  to: jj_lib::backend::CommitId,
  style: String,
}

/// The last few merge diffs: a Changes reload that moved only the working
/// copy re-reads the committed part from here, not from the store.
pub(super) struct MergeDiffCache {
  entries: std::sync::Mutex<Vec<(MergeDiffKey, Arc<JjRevisionDiff>)>>,
}

impl MergeDiffCache {
  const CAPACITY: usize = 3;
  /// Diffs with more text than this are not kept.
  const MAX_BYTES: usize = 64 << 20;

  fn get(&self, key: &MergeDiffKey) -> Option<JjRevisionDiff> {
    let mut entries = self.entries.lock().ok()?;
    let at = entries.iter().position(|(k, _)| k == key)?;
    // Most recently used last.
    let hit = entries.remove(at);
    let diff = hit.1.clone();
    entries.push(hit);
    drop(entries);
    Some((*diff).clone())
  }

  fn put(&self, key: MergeDiffKey, diff: &JjRevisionDiff) {
    let bytes: usize = diff
      .hunks_by_file
      .iter()
      .flat_map(|f| &f.hunks)
      .map(|h| h.lines.iter().map(String::len).sum::<usize>())
      .sum();
    if bytes > Self::MAX_BYTES {
      return;
    }
    let Ok(mut entries) = self.entries.lock() else {
      return;
    };
    entries.retain(|(k, _)| *k != key);
    if entries.len() >= Self::CAPACITY {
      entries.remove(0);
    }
    entries.push((key, Arc::new(diff.clone())));
  }
}

pub(super) fn merge_diff_cache() -> &'static MergeDiffCache {
  static CACHE: std::sync::OnceLock<MergeDiffCache> = std::sync::OnceLock::new();
  CACHE.get_or_init(|| MergeDiffCache {
    entries: std::sync::Mutex::new(Vec::new()),
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
  let matcher = repo_root_matcher();
  let copy_records = if too_many_paths_to_track_copies(
    &parent_tree,
    &commit_tree,
    &matcher,
    COPY_TRACKING_MAX_ENTRIES,
  ) {
    CopyRecords::default()
  } else {
    get_commit_copy_records(loaded.repo.store(), &commit)?
  };
  let conflict_labels = Diff::new(parent_tree.labels(), commit_tree.labels());
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
      let Ok(values) = entry.values else { continue };
      let before_absent = matches!(&values.before, MaterializedTreeValue::Absent);
      let after_absent = matches!(&values.after, MaterializedTreeValue::Absent);
      let style = parse_conflict_marker_style(_conflict_marker_style);
      let Ok(merge_options) = MergeOptions::from_settings(&loaded.settings) else {
        continue;
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
    working_copy_token: None,
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
  let matcher = repo_root_matcher();
  // As the commit's file list tracked them.
  let copy_records = if too_many_paths_to_track_copies(
    &parent_tree,
    &commit_tree,
    &matcher,
    COPY_TRACKING_MAX_ENTRIES,
  ) {
    CopyRecords::default()
  } else {
    get_commit_copy_records(loaded.repo.store(), &commit)?
  };
  let conflict_labels = Diff::new(parent_tree.labels(), commit_tree.labels());
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
      let Ok(values) = entry.values else { continue };
      let style = parse_conflict_marker_style(_conflict_marker_style);
      let Ok(merge_options) = MergeOptions::from_settings(&loaded.settings) else {
        continue;
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

pub(super) fn count_changed_lines(hunks: &[JjDiffHunk]) -> usize {
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
