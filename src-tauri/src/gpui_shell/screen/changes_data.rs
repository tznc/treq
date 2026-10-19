//! What the workspace Changes tab (`ChangesDiffViewer`) reads from and
//! writes to the backend, and the rules its React hooks apply: the files
//! and their sections, which are picked for the next commit, each file's
//! diff body, context expansion and the conflict regions.

use std::collections::{HashMap, HashSet};

use regex::Regex;

use super::commits_data::CommitsHost;
use crate::{
  conflict_markers::{ConflictLineKind, ConflictLineRole, ConflictRegionView, ConflictStyle},
  core::{self, WorkspaceFileHunksBatch},
  gpui_shell::native::Backend,
  jj::{JjDiffHunk, JjFileChange, JjRevisionDiff},
  local_db::StashEntry,
};

/// `LARGE_DIFF_THRESHOLD`: changed lines past which a file's diff waits
/// behind "View changes".
pub const LARGE_FILE_LINES: usize = 250;
/// Lines across the working copy past which the whole list waits behind
/// "View changes".
pub const LARGE_CHANGESET_LINES: usize = 1000;
/// Lines each "Show more lines" click reads.
pub const CONTEXT_STEP: usize = 20;
/// The first files read before the rest (`INITIAL_FILE_BATCH`).
pub const FIRST_BATCH: usize = 2;
/// Files read per batch after the first.
pub const BATCH: usize = 32;
/// `HOME_MOVE_ENDPOINT`: the home repository as a move source or target.
pub const HOME_ENDPOINT: &str = ".";
/// `performCommit`'s limit.
pub const MAX_MESSAGE: usize = 500;

/// Editors the "Open in…" menus offer, as detected at startup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorApps {
  pub cursor: bool,
  pub vscode: bool,
  pub zed: bool,
}

/// What the Changes tab asks of the backend; faked in tests. Everything
/// but `reveal` blocks, so it is called off the main thread.
pub trait ChangesHost: CommitsHost {
  /// `get_workspace_changed_files`, for the home repository.
  fn changed_files(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
  ) -> Result<Vec<JjFileChange>, String>;
  /// `get_workspace_diff`: working copy, committed files and conflicts.
  fn workspace_diff(&self, repo_path: &str, workspace_id: i64) -> Result<JjRevisionDiff, String>;
  fn file_hunks(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    paths: Vec<String>,
  ) -> Result<WorkspaceFileHunksBatch, String>;
  /// Lines `start..=end` of the working-copy file, for context.
  fn file_lines(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    path: &str,
    start: usize,
    end: usize,
  ) -> Result<Vec<String>, String>;
  fn discard_file(&self, workspace_path: &str, path: &str) -> Result<(), String>;
  /// `create_commit`: the whole working copy, pushed if the repository
  /// auto-pushes.
  fn commit(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    message: &str,
  ) -> Result<String, String>;
  /// `jj_split`: only `paths`, then the stack rebased onto it.
  fn split(
    &self,
    workspace_path: &str,
    message: &str,
    paths: Vec<String>,
  ) -> Result<String, String>;
  fn push_remote(&self, repo_path: &str, workspace_id: Option<i64>) -> Result<String, String>;
  fn stash_all(&self, repo_path: &str, workspace_id: Option<i64>) -> Result<StashEntry, String>;
  fn mark_viewed(&self, workspace_path: &str, path: &str, hash: &str) -> Result<(), String>;
  fn unmark_viewed(&self, workspace_path: &str, path: &str) -> Result<(), String>;
  /// The repository has a GitHub remote (`useGitRemoteInfo`).
  fn github_remote(&self, repo_path: &str) -> bool;
  fn editor_apps(&self) -> EditorApps;
  /// Shows `path` in the file manager.
  fn reveal(&self, path: &str);
}

pub type Host = std::sync::Arc<dyn ChangesHost>;

impl ChangesHost for Backend {
  fn changed_files(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
  ) -> Result<Vec<JjFileChange>, String> {
    core::list_changed_files(repo_path, workspace_id)
  }

  fn workspace_diff(&self, repo_path: &str, workspace_id: i64) -> Result<JjRevisionDiff, String> {
    core::workspace_diff(repo_path, workspace_id)
  }

  fn file_hunks(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    paths: Vec<String>,
  ) -> Result<WorkspaceFileHunksBatch, String> {
    let state = tauri::Manager::state::<crate::AppState>(self.app());
    let style = core::resolve_conflict_marker_style(&state.db);
    core::list_file_hunks_batch(repo_path, workspace_id, paths, &style)
  }

  fn file_lines(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    path: &str,
    start: usize,
    end: usize,
  ) -> Result<Vec<String>, String> {
    core::get_file_lines(repo_path, workspace_id, path, false, start, end).map(|l| l.lines)
  }

  fn discard_file(&self, workspace_path: &str, path: &str) -> Result<(), String> {
    core::discard_file_changes(workspace_path, path).map(|_| ())
  }

  fn commit(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    message: &str,
  ) -> Result<String, String> {
    core::commit_workspace_with_auto_push(repo_path, workspace_id, message)
  }

  fn split(
    &self,
    workspace_path: &str,
    message: &str,
    paths: Vec<String>,
  ) -> Result<String, String> {
    core::split_working_copy(workspace_path, message, paths)
  }

  fn push_remote(&self, repo_path: &str, workspace_id: Option<i64>) -> Result<String, String> {
    core::push_workspace_to_remote(repo_path, workspace_id)
  }

  fn stash_all(&self, repo_path: &str, workspace_id: Option<i64>) -> Result<StashEntry, String> {
    core::stash_workspace_changes(repo_path, workspace_id)
  }

  fn mark_viewed(&self, workspace_path: &str, path: &str, hash: &str) -> Result<(), String> {
    let state = tauri::Manager::state::<crate::AppState>(self.app());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.mark_file_viewed(workspace_path, path, hash)
      .map_err(|e| e.to_string())
  }

  fn unmark_viewed(&self, workspace_path: &str, path: &str) -> Result<(), String> {
    let state = tauri::Manager::state::<crate::AppState>(self.app());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.unmark_file_viewed(workspace_path, path)
      .map_err(|e| e.to_string())
  }

  fn github_remote(&self, repo_path: &str) -> bool {
    crate::github::get_git_remote_url_impl(repo_path)
      .ok()
      .flatten()
      .is_some()
  }

  fn editor_apps(&self) -> EditorApps {
    let apps = crate::binary_paths::get_editor_apps_cache().unwrap_or_default();
    let has = |key: &str| apps.get(key).copied().unwrap_or(false);
    EditorApps {
      cursor: has("cursor"),
      vscode: has("vscode"),
      zed: has("zed"),
    }
  }

  fn reveal(&self, path: &str) {
    use tauri_plugin_opener::OpenerExt;
    if let Err(err) = self.app().opener().reveal_item_in_dir(path) {
      log::warn!("failed to reveal {path}: {err}");
      self.toast(
        crate::gpui_shell::native::ToastTone::Error,
        "Open Failed",
        err.to_string(),
      );
    }
  }
}

/// A working-copy file (`ParsedFileChange`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
  pub path: String,
  /// jj's status letter: `M`, `A`, `D`, `R`, `C`...
  pub status: String,
  pub old_path: Option<String>,
}

impl ChangedFile {
  /// `parseJjChangedFiles`.
  pub fn parse(files: &[JjFileChange]) -> Vec<ChangedFile> {
    files
      .iter()
      .map(|f| ChangedFile {
        path: f.path.clone(),
        status: f.status.clone(),
        old_path: f.previous_path.clone(),
      })
      .collect()
  }
}

/// The files the tab lists, from one read.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
  pub files: Vec<ChangedFile>,
  /// Committed on the branch, conflicted paths included.
  pub committed: Vec<JjFileChange>,
  pub committed_hunks: HashMap<String, Vec<JjDiffHunk>>,
  /// `liveConflictedFiles`, once read; `None` keeps the status's list.
  pub conflicted: Option<Vec<String>>,
}

impl Snapshot {
  /// What `loadChangedFiles` keeps of a workspace's diff.
  pub fn from_workspace_diff(diff: JjRevisionDiff, show_committed: bool) -> Self {
    let files = ChangedFile::parse(&diff.uncommitted_files);
    let conflicted = diff.conflicted_files.clone();
    let uncommitted: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let mut committed = diff.committed_files.clone();
    for path in &conflicted {
      if !uncommitted.contains(path.as_str()) && !committed.iter().any(|f| &f.path == path) {
        committed.push(JjFileChange {
          path: path.clone(),
          status: "C".into(),
          previous_path: None,
          changed_line_count: 0,
          diff_deferred: false,
        });
      }
    }
    let committed_hunks = diff
      .hunks_by_file
      .into_iter()
      .filter(|f| {
        show_committed || uncommitted.contains(f.path.as_str()) || conflicted.contains(&f.path)
      })
      .map(|f| (f.path, f.hunks))
      .collect();
    Snapshot {
      files,
      committed,
      committed_hunks,
      conflicted: Some(conflicted),
    }
  }

  pub fn from_home(files: &[JjFileChange]) -> Self {
    Snapshot {
      files: ChangedFile::parse(files),
      ..Snapshot::default()
    }
  }
}

/// `filterVisibleCommittedFiles`: committed files show when the toggle is
/// on, or when they are also dirty or conflicted.
pub fn visible_committed<'a>(
  committed: &'a [JjFileChange],
  show: bool,
  always: &HashSet<String>,
) -> Vec<&'a JjFileChange> {
  committed
    .iter()
    .filter(|f| show || always.contains(&f.path))
    .collect()
}

/// Which list a file click came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
  Changes,
  Selected,
}

/// `useFileStaging`: the files picked for the next commit ("Selected"),
/// and the rows highlighted in each list. All client-side: jj has no index.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Staging {
  pub staged: HashSet<String>,
  pub selected: HashSet<String>,
  /// Index into all files.
  pub anchor: Option<usize>,
  pub selected_staged: HashSet<String>,
  /// Index into the staged list.
  pub staged_anchor: Option<usize>,
}

impl Staging {
  pub fn unstaged<'a>(&self, files: &'a [ChangedFile]) -> Vec<&'a ChangedFile> {
    files
      .iter()
      .filter(|f| !self.staged.contains(&f.path))
      .collect()
  }

  pub fn staged_list<'a>(&self, files: &'a [ChangedFile]) -> Vec<&'a ChangedFile> {
    files
      .iter()
      .filter(|f| self.staged.contains(&f.path))
      .collect()
  }

  /// `handleFileSelect` / `handleStagedFileSelect`: shift picks the range
  /// from the anchor, cmd toggles, a plain click picks just this row.
  pub fn click(&mut self, list: List, files: &[ChangedFile], path: &str, shift: bool, meta: bool) {
    let paths: Vec<String> = match list {
      List::Changes => files.iter().map(|f| f.path.clone()).collect(),
      List::Selected => self
        .staged_list(files)
        .iter()
        .map(|f| f.path.clone())
        .collect(),
    };
    let Some(idx) = paths.iter().position(|p| p == path) else {
      return;
    };
    let (set, anchor) = match list {
      List::Changes => (&mut self.selected, &mut self.anchor),
      List::Selected => (&mut self.selected_staged, &mut self.staged_anchor),
    };
    match *anchor {
      Some(from) if shift => {
        set.clear();
        let (a, b) = (from.min(idx), from.max(idx));
        set.extend(paths[a..=b.min(paths.len() - 1)].iter().cloned());
      }
      _ if meta => {
        if !set.remove(path) {
          set.insert(path.to_string());
        }
      }
      _ => {
        if !(set.len() == 1 && set.contains(path)) {
          set.clear();
          set.insert(path.to_string());
        }
      }
    }
    *anchor = Some(idx);
  }

  /// `handleStageFile`: the whole selection when `path` is in it.
  pub fn stage(&mut self, path: &str) {
    if self.selected.len() > 1 && self.selected.contains(path) {
      let picked: Vec<String> = self.selected.drain().collect();
      self.staged.extend(picked);
    } else {
      self.staged.insert(path.to_string());
    }
    self.selected.clear();
  }

  pub fn unstage(&mut self, path: &str) {
    self.staged.remove(path);
    self.selected_staged.clear();
  }

  pub fn unstage_all(&mut self) {
    self.staged.clear();
    self.selected_staged.clear();
  }

  /// The "Changes" Plus button: all unstaged, or none when all already are.
  pub fn toggle_all_unstaged(&mut self, files: &[ChangedFile]) {
    let unstaged: Vec<String> = self
      .unstaged(files)
      .iter()
      .map(|f| f.path.clone())
      .collect();
    if !unstaged.is_empty() && self.selected.len() == unstaged.len() {
      self.selected.clear();
    } else {
      self.selected = unstaged.iter().cloned().collect();
    }
    if let Some(last) = unstaged.last() {
      self.anchor = files.iter().position(|f| &f.path == last);
    }
  }

  /// Cmd+A: every row of the list that has a selection, not a toggle.
  pub fn select_all(&mut self, files: &[ChangedFile]) {
    if !self.selected_staged.is_empty() {
      let staged: Vec<String> = self
        .staged_list(files)
        .iter()
        .map(|f| f.path.clone())
        .collect();
      self.staged_anchor = staged.len().checked_sub(1);
      self.selected_staged = staged.into_iter().collect();
    } else {
      let unstaged: Vec<String> = self
        .unstaged(files)
        .iter()
        .map(|f| f.path.clone())
        .collect();
      if let Some(last) = unstaged.last() {
        self.anchor = files.iter().position(|f| &f.path == last);
      }
      self.selected = unstaged.into_iter().collect();
    }
  }

  /// The paths a row action applies to: the selection when the row is in
  /// it, else the row (`filesFor`).
  pub fn targets(&self, path: &str) -> Vec<String> {
    if self.selected.contains(path) {
      let mut all: Vec<String> = self.selected.iter().cloned().collect();
      all.sort();
      all
    } else {
      vec![path.to_string()]
    }
  }

  /// Drops paths no longer listed.
  pub fn retain(&mut self, files: &[ChangedFile]) {
    let listed: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    self.staged.retain(|p| listed.contains(p.as_str()));
    self.selected.retain(|p| listed.contains(p.as_str()));
    self.selected_staged.retain(|p| listed.contains(p.as_str()));
  }
}

/// The "Changes" Plus button's tooltip.
pub fn select_all_label(selected: usize, files: usize) -> &'static str {
  if selected == files {
    "Deselect all"
  } else {
    "Select all"
  }
}

/// `commitLabel`.
pub fn commit_label(selected: usize) -> String {
  match selected {
    0 => "Commit".into(),
    1 => "Commit 1 file".into(),
    n => format!("Commit {n} files"),
  }
}

/// What a commit button does after committing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitAction {
  Commit,
  Push,
  Pr,
}

pub fn pending_label(action: CommitAction) -> &'static str {
  match action {
    CommitAction::Push => "Pushing…",
    CommitAction::Pr => "Creating PR…",
    CommitAction::Commit => "Committing…",
  }
}

/// `performCommit`'s checks: the message to commit, or the toast's text.
pub fn check_message(message: &str) -> Result<String, &'static str> {
  let message = message.trim();
  if message.is_empty() {
    Err("Enter a commit message.")
  } else if message.chars().count() > MAX_MESSAGE {
    Err("Please keep the message under 500 characters.")
  } else {
    Ok(message.to_string())
  }
}

/// A row's file name and the directory shown after it.
pub fn split_path(path: &str) -> (String, Option<String>) {
  let path = path.trim_end_matches('/');
  match path.rsplit_once('/') {
    Some((dir, name)) if !name.is_empty() => (name.to_string(), Some(dir.to_string())),
    _ => (path.to_string(), None),
  }
}

/// `isBinaryFile`.
pub fn is_binary(path: &str) -> bool {
  const EXTENSIONS: [&str; 25] = [
    ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".ico", ".svg", ".pdf", ".zip", ".tar", ".gz", ".rar",
    ".7z", ".exe", ".dll", ".so", ".dylib", ".mp3", ".mp4", ".avi", ".mov", ".wmv", ".woff",
    ".woff2", ".ttf",
  ];
  let lower = path.to_ascii_lowercase();
  EXTENSIONS.iter().any(|e| lower.ends_with(e)) || lower.ends_with(".eot")
}

/// The file's full path, for copying and opening.
pub fn full_path(workspace_path: &str, path: &str) -> String {
  if path.starts_with('/') {
    path.to_string()
  } else {
    format!("{}/{path}", workspace_path.trim_end_matches('/'))
  }
}

/// `computeHunksHash`: djb2 over each hunk's header and lines, in hex.
pub fn hunks_hash(hunks: &[JjDiffHunk]) -> String {
  let text = hunks
    .iter()
    .map(|h| format!("{}{}", h.header, h.lines.concat()))
    .collect::<Vec<_>>()
    .join("|");
  let mut hash: u64 = 5381;
  // JavaScript's `charCodeAt` counts UTF-16 units.
  for unit in text.encode_utf16() {
    hash = (hash * 33 + u64::from(unit)) % (1 << 32);
  }
  format!("{hash:x}")
}

/// Added and removed lines.
pub fn line_counts(hunks: &[JjDiffHunk]) -> (usize, usize) {
  hunks
    .iter()
    .flat_map(|h| &h.lines)
    .fold((0, 0), |(a, d), l| {
      if l.starts_with('+') {
        (a + 1, d)
      } else if l.starts_with('-') {
        (a, d + 1)
      } else {
        (a, d)
      }
    })
}

/// A file's diff as read.
#[derive(Debug, Clone, Default)]
pub struct FileHunks {
  pub hunks: Vec<JjDiffHunk>,
  pub loading: bool,
  pub error: Option<String>,
}

/// What a file card shows below its header (`fileDisplayState.body`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
  Collapsed,
  Binary,
  Deleted,
  Loading,
  Error,
  Empty,
  /// Changed lines past [`LARGE_FILE_LINES`].
  Large(usize),
  Hunks,
}

/// The inputs to [`body`].
pub struct BodyInput<'a> {
  pub path: &'a str,
  pub status: &'a str,
  pub renamed: bool,
  pub data: Option<&'a FileHunks>,
  pub collapsed: bool,
  pub comment_activity: bool,
  pub expanded_large: bool,
}

/// Whether the file card is shut: comments keep it open; binary files and
/// renames start shut.
pub fn is_collapsed(input: &BodyInput) -> bool {
  !input.comment_activity && (is_binary(input.path) || input.renamed || input.collapsed)
}

pub fn body(input: &BodyInput) -> Body {
  if is_collapsed(input) {
    return Body::Collapsed;
  }
  if is_binary(input.path) {
    return Body::Binary;
  }
  if input.status == "D" {
    return Body::Deleted;
  }
  let Some(data) = input.data.filter(|d| !d.loading) else {
    return Body::Loading;
  };
  if data.error.is_some() {
    return Body::Error;
  }
  if data.hunks.is_empty() {
    return Body::Empty;
  }
  let (a, d) = line_counts(&data.hunks);
  if a + d > LARGE_FILE_LINES && !input.expanded_large {
    return Body::Large(a + d);
  }
  Body::Hunks
}

/// Lines of every loaded working-copy diff, for the large-changeset gate.
pub fn total_lines<'a>(files: impl Iterator<Item = &'a FileHunks>) -> usize {
  files
    .filter(|f| !f.loading)
    .flat_map(|f| &f.hunks)
    .map(|h| h.lines.len())
    .sum()
}

/// The hunk's new-side start and length.
pub fn new_range(header: &str) -> (usize, usize) {
  let re = Regex::new(r"@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@").expect("hunk header regex");
  re.captures(header)
    .map(|c| {
      (
        c[1].parse().unwrap_or(1),
        c.get(2).map_or(1, |n| n.as_str().parse().unwrap_or(1)),
      )
    })
    .unwrap_or((1, 1))
}

/// The lines "Show more lines above" reads next, if any.
pub fn before_range(header: &str, loaded: usize) -> Option<(usize, usize)> {
  let (start, _) = new_range(header);
  let end = (start as i64) - 1 - loaded as i64;
  if end < 1 {
    return None;
  }
  let end = end as usize;
  Some(((end + 1).saturating_sub(CONTEXT_STEP).max(1), end))
}

/// The lines "Show more lines below" reads next.
pub fn after_range(header: &str, loaded: usize) -> (usize, usize) {
  let (start, count) = new_range(header);
  let first = start + count + loaded;
  (first, first + CONTEXT_STEP - 1)
}

/// Above a hunk there is more to show.
pub fn can_expand_before(header: &str, loaded: usize) -> bool {
  new_range(header).0 > 1 || loaded > 0
}

/// A conflict region as the tab numbers it (`useConflicts`).
#[derive(Debug, Clone)]
pub struct Conflict {
  pub id: String,
  pub number: usize,
  pub region: ConflictRegionView,
}

/// A file's conflict regions, de-duplicated (the backend repeats them on
/// every hunk) and numbered.
pub fn conflicts(path: &str, hunks: &[JjDiffHunk]) -> Vec<Conflict> {
  let mut seen = HashSet::new();
  let mut out = Vec::new();
  for (i, region) in hunks.iter().flat_map(|h| &h.conflict_regions).enumerate() {
    let number = if region.conflict_number > 0 {
      region.conflict_number
    } else {
      i + 1
    };
    let id = format!("{path}-conflict-{number}");
    if seen.insert(id.clone()) {
      out.push(Conflict {
        id,
        number,
        region: region.clone(),
      });
    }
  }
  out
}

/// The region each new-side line number falls in.
pub fn conflict_lines(conflicts: &[Conflict]) -> HashMap<usize, usize> {
  let mut map = HashMap::new();
  for (i, c) in conflicts.iter().enumerate() {
    for line in c.region.start_line..=c.region.end_line {
      map.entry(line).or_insert(i);
    }
  }
  map
}

/// `getConflictDeletedSides`: a side with no content lines is a deletion.
pub fn deleted_sides(region: &ConflictRegionView) -> (bool, bool) {
  let labelled = matches!(
    region.marker_style,
    ConflictStyle::GitMerge | ConflictStyle::GitDiff3 | ConflictStyle::JjGitDiff3
  );
  if !labelled {
    return (false, false);
  }
  let has = |role| {
    region
      .lines
      .iter()
      .any(|l| l.kind == ConflictLineKind::Content && l.role == role && !l.raw.trim().is_empty())
  };
  (!has(ConflictLineRole::Left), !has(ConflictLineRole::Right))
}

/// The section badge on a marker line.
pub fn section_label(region: &ConflictRegionView, role: ConflictLineRole) -> Option<&'static str> {
  if !matches!(
    region.marker_style,
    ConflictStyle::GitMerge | ConflictStyle::GitDiff3 | ConflictStyle::JjGitDiff3
  ) {
    return None;
  }
  match role {
    ConflictLineRole::Start => Some("Side #1"),
    ConflictLineRole::Base => Some("Base"),
    ConflictLineRole::Separator => Some("Side #2"),
    _ => None,
  }
}

/// The Discard toast's description.
pub fn discarded(paths: &[String]) -> String {
  match paths {
    [one] => format!("{one} discarded"),
    many => format!("{} files discarded", many.len()),
  }
}

/// The Stash toast's description.
pub fn stashed(entry: &StashEntry) -> String {
  let n = entry.files_changed.len();
  format!(
    "Stashed {n} file{} ({})",
    if n == 1 { "" } else { "s" },
    entry.short_commit_id
  )
}

/// `AGENT_REVIEW_TARGET_WORKSPACE_DIFF`.
pub const REVIEW_TARGET: &str = "workspace_diff";
/// `DEFAULT_REVIEW_PROMPT`, as `lib/agent-review-prompt.ts` writes it.
const DEFAULT_REVIEW_PROMPT: &str = include_str!("review_prompt.txt");

/// `workspaceDiffSummary`.
pub fn diff_summary(branch: Option<&str>, files: usize) -> String {
  let files = format!("{files} changed file{}", if files == 1 { "" } else { "s" });
  match branch {
    Some(branch) => format!("the {branch} workspace diff ({files})"),
    None => format!("the current workspace diff ({files})"),
  }
}

/// `buildReviewPrompt`: the repository's prompt when set, else the
/// built-in one, with its placeholders filled.
pub fn review_prompt(custom: Option<&str>, target_id: &str, summary: &str) -> String {
  let template = custom
    .filter(|c| !c.trim().is_empty())
    .unwrap_or(DEFAULT_REVIEW_PROMPT);
  template
    .replace("{target_type}", REVIEW_TARGET)
    .replace("{target_id}", target_id)
    .replace("{workspace_id}", target_id)
    .replace("{diff_summary}", summary)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn file(path: &str) -> ChangedFile {
    ChangedFile {
      path: path.into(),
      status: "M".into(),
      old_path: None,
    }
  }

  fn files(paths: &[&str]) -> Vec<ChangedFile> {
    paths.iter().map(|p| file(p)).collect()
  }

  fn hunk(header: &str, lines: &[&str]) -> JjDiffHunk {
    JjDiffHunk {
      id: "h".into(),
      header: header.into(),
      lines: lines.iter().map(|l| l.to_string()).collect(),
      patch: String::new(),
      conflict_style: Default::default(),
      conflict_regions: Vec::new(),
    }
  }

  fn set(paths: &[&str]) -> HashSet<String> {
    paths.iter().map(|p| p.to_string()).collect()
  }

  #[test]
  fn click_selects_only_the_row() {
    let fs = files(&["a", "b", "c"]);
    let mut s = Staging::default();
    s.click(List::Changes, &fs, "a", false, false);
    s.click(List::Changes, &fs, "b", false, false);
    assert_eq!(s.selected, set(&["b"]));
    assert_eq!(s.anchor, Some(1));
  }

  #[test]
  fn shift_click_selects_the_range_from_the_anchor() {
    let fs = files(&["a", "b", "c", "d"]);
    let mut s = Staging::default();
    s.click(List::Changes, &fs, "d", false, false);
    s.click(List::Changes, &fs, "b", true, false);
    assert_eq!(s.selected, set(&["b", "c", "d"]));
    assert_eq!(s.anchor, Some(1));
  }

  #[test]
  fn meta_click_toggles_the_row() {
    let fs = files(&["a", "b"]);
    let mut s = Staging::default();
    s.click(List::Changes, &fs, "a", false, false);
    s.click(List::Changes, &fs, "b", false, true);
    assert_eq!(s.selected, set(&["a", "b"]));
    s.click(List::Changes, &fs, "a", false, true);
    assert_eq!(s.selected, set(&["b"]));
  }

  #[test]
  fn stage_takes_the_whole_selection_when_the_row_is_in_it() {
    let fs = files(&["a", "b", "c"]);
    let mut s = Staging {
      selected: set(&["a", "b"]),
      ..Default::default()
    };
    s.stage("a");
    assert_eq!(s.staged, set(&["a", "b"]));
    assert!(s.selected.is_empty());
    assert_eq!(
      s.unstaged(&fs)
        .iter()
        .map(|f| f.path.as_str())
        .collect::<Vec<_>>(),
      ["c"]
    );
    s.stage("c");
    assert_eq!(s.staged, set(&["a", "b", "c"]));
  }

  #[test]
  fn toggle_all_unstaged_selects_then_clears() {
    let fs = files(&["a", "b", "c"]);
    let mut s = Staging {
      staged: set(&["b"]),
      ..Default::default()
    };
    s.toggle_all_unstaged(&fs);
    assert_eq!(s.selected, set(&["a", "c"]));
    assert_eq!(s.anchor, Some(2));
    s.toggle_all_unstaged(&fs);
    assert!(s.selected.is_empty());
  }

  #[test]
  fn select_all_prefers_the_selected_list() {
    let fs = files(&["a", "b", "c"]);
    let mut s = Staging {
      staged: set(&["a", "b"]),
      selected_staged: set(&["a"]),
      ..Default::default()
    };
    s.select_all(&fs);
    assert_eq!(s.selected_staged, set(&["a", "b"]));
    assert!(s.selected.is_empty());
  }

  #[test]
  fn labels_count_files() {
    assert_eq!(commit_label(0), "Commit");
    assert_eq!(commit_label(1), "Commit 1 file");
    assert_eq!(commit_label(3), "Commit 3 files");
    assert_eq!(select_all_label(2, 2), "Deselect all");
    assert_eq!(select_all_label(0, 2), "Select all");
  }

  #[test]
  fn check_message_rejects_empty_and_long_messages() {
    assert_eq!(check_message("  "), Err("Enter a commit message."));
    assert!(check_message(&"x".repeat(501)).is_err());
    assert_eq!(check_message(" fix it \n"), Ok("fix it".into()));
  }

  #[test]
  fn split_path_separates_name_and_directory() {
    assert_eq!(
      split_path("src/lib/a.ts"),
      ("a.ts".into(), Some("src/lib".into()))
    );
    assert_eq!(split_path("README.md"), ("README.md".into(), None));
  }

  #[test]
  fn hunks_hash_matches_djb2() {
    // "ab" under djb2: ((5381*33 + 97)*33 + 98) = 5863208, in hex.
    let h = hunk("a", &["b"]);
    assert_eq!(hunks_hash(&[h]), format!("{:x}", 5863208));
  }

  #[test]
  fn body_follows_the_display_rules() {
    let loaded = FileHunks {
      hunks: vec![hunk("@@ -1 +1 @@", &["+a"])],
      ..FileHunks::default()
    };
    let input = |path, data| BodyInput {
      path,
      status: "M",
      renamed: false,
      data,
      collapsed: false,
      comment_activity: false,
      expanded_large: false,
    };
    assert_eq!(body(&input("a.png", Some(&loaded))), Body::Collapsed);
    assert_eq!(body(&input("a.rs", None)), Body::Loading);
    assert_eq!(body(&input("a.rs", Some(&loaded))), Body::Hunks);
    let big = FileHunks {
      hunks: vec![hunk("@@ -1 +1,251 @@", &["+a"; 251])],
      ..FileHunks::default()
    };
    assert_eq!(body(&input("a.rs", Some(&big))), Body::Large(251));
    let deleted = BodyInput {
      status: "D",
      ..input("a.rs", Some(&loaded))
    };
    assert_eq!(body(&deleted), Body::Deleted);
  }

  #[test]
  fn context_ranges_step_twenty_lines() {
    assert_eq!(before_range("@@ -30,4 +30,4 @@", 0), Some((10, 29)));
    assert_eq!(before_range("@@ -30,4 +30,4 @@", 20), Some((1, 9)));
    assert_eq!(before_range("@@ -1,4 +1,4 @@", 0), None);
    assert_eq!(after_range("@@ -30,4 +30,4 @@", 0), (34, 53));
    assert_eq!(after_range("@@ -30,4 +30,4 @@", 20), (54, 73));
    assert!(!can_expand_before("@@ -1,4 +1,4 @@", 0));
  }

  #[test]
  fn workspace_diff_lists_conflicted_paths_as_committed() {
    let change = |path: &str, status: &str| JjFileChange {
      path: path.into(),
      status: status.into(),
      previous_path: None,
      changed_line_count: 1,
      diff_deferred: false,
    };
    let diff = JjRevisionDiff {
      committed_files: vec![change("done.rs", "M")],
      hunks_by_file: Vec::new(),
      uncommitted_files: vec![change("dirty.rs", "M")],
      conflicted_files: vec!["clash.rs".into(), "dirty.rs".into()],
      too_large_to_render: false,
      render_block_reason: None,
    };
    let snap = Snapshot::from_workspace_diff(diff, false);
    let committed: Vec<&str> = snap.committed.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(committed, ["done.rs", "clash.rs"]);
    assert_eq!(snap.committed[1].status, "C");
    let always = set(&["dirty.rs", "clash.rs"]);
    let visible: Vec<&str> = visible_committed(&snap.committed, false, &always)
      .iter()
      .map(|f| f.path.as_str())
      .collect();
    assert_eq!(visible, ["clash.rs"]);
  }

  #[test]
  fn review_prompt_fills_the_built_in_template() {
    let summary = diff_summary(Some("feat/a"), 2);
    assert_eq!(summary, "the feat/a workspace diff (2 changed files)");
    let prompt = review_prompt(None, "7", &summary);
    assert!(prompt.starts_with("You are reviewing the current changes in this repository."));
    assert!(prompt.contains("--target-type workspace_diff --target-id 7"));
    assert!(prompt.contains("What is under review: the feat/a workspace diff"));
    assert_eq!(
      review_prompt(Some("Look at {workspace_id}"), "7", ""),
      "Look at 7"
    );
    assert!(review_prompt(Some("  "), "7", "").starts_with("You are reviewing"));
  }
}
