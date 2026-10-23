//! What the workspace Commits tab (`CommitDiffViewer`) reads from and writes
//! to the backend, and the rules it applies: which commits show, which can
//! be resolved, diff line numbers and the comments sent to an agent.

use std::sync::{Arc, LazyLock};

use regex::Regex;

use super::code_data::CodeHost;
use crate::{
  core::{self, ResolveConflictsSession},
  gpui_shell::{native::Backend, remote_repo::unsupported},
  jj::{JjDiffHunk, JjFileDiff, JjLogCommit, JjLogResult, JjRevisionDiff},
};

/// Target-branch commits listed before "Load more commits" (`targetBranchLimit`).
pub const TARGET_PAGE: usize = 10;
/// Home-repository commits listed before "Load more commits" (`homeRepoLimit`).
pub const HOME_PAGE: usize = 15;

/// How the timestamp dialog moves a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeShift {
  /// Signed offsets.
  By { days: i64, hours: i64, minutes: i64 },
  /// To a `YYYY-MM-DD` day.
  ToDay(String),
}

/// What the Commits tab asks of the backend; faked in tests. Everything
/// blocks, so it is called off the main thread.
pub trait CommitsHost: CodeHost {
  /// `list_commits` with target-branch history; `None` limits keep the
  /// backend's defaults.
  fn log(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    target_limit: Option<usize>,
    home_limit: Option<usize>,
  ) -> Result<JjLogResult, String>;
  fn commit_diff(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    revision: &str,
  ) -> Result<JjRevisionDiff, String>;
  fn commit_file_diff(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    revision: &str,
    file_path: &str,
  ) -> Result<JjFileDiff, String>;
  /// Returns the operation id, for undo.
  fn abandon(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_id: &str,
  ) -> Result<String, String>;
  fn undo_operation(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    operation_id: &str,
  ) -> Result<(), String>;
  /// Returns the stash entry's short commit id.
  fn stash(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_id: &str,
  ) -> Result<String, String>;
  fn undo_commit(&self, repo_path: &str, workspace_id: i64, change_id: &str) -> Result<(), String>;
  fn revert(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_id: &str,
  ) -> Result<(), String>;
  fn shift_to_now(&self, repo_path: &str, workspace_id: i64) -> Result<(), String>;
  fn commit_description(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_id: &str,
  ) -> Result<String, String>;
  fn describe(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_id: &str,
    description: &str,
  ) -> Result<(), String>;
  fn shift_timestamp(
    &self,
    repo_path: &str,
    workspace_id: i64,
    change_id: &str,
    shift: &TimeShift,
  ) -> Result<(), String>;
  fn start_resolve(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_ids: Option<Vec<String>>,
  ) -> Result<ResolveConflictsSession, String>;
  fn resolve_prompt(&self, prompt: &str, session: &ResolveConflictsSession) -> String;
  /// `jj_snapshot_working_copy`: the snapshot's id.
  fn snapshot_working_copy(&self, workspace_path: &str) -> Result<String, String>;
  fn discard_all(&self, workspace_path: &str) -> Result<(), String>;
  fn restore_snapshot(&self, workspace_path: &str, snapshot_id: &str) -> Result<(), String>;
}

/// Remote hosts only run these against a workspace, not the home repository.
fn remote_workspace(workspace_id: Option<i64>) -> Result<i64, String> {
  workspace_id.ok_or_else(|| unsupported("Rewriting commits in a remote home repository"))
}

impl CommitsHost for Backend {
  fn log(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    target_limit: Option<usize>,
    home_limit: Option<usize>,
  ) -> Result<JjLogResult, String> {
    if let Some(remote) = self.remote_containing(repo_path) {
      return remote.commits(workspace_id);
    }
    let home_limit = home_limit.filter(|_| workspace_id.is_none());
    core::list_commits(repo_path, workspace_id, true, target_limit, home_limit)
  }

  fn commit_diff(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    revision: &str,
  ) -> Result<JjRevisionDiff, String> {
    if let Some(remote) = self.remote_containing(repo_path) {
      return remote.commit_diff(workspace_id, revision);
    }
    let state = tauri::Manager::state::<crate::AppState>(self.app());
    core::get_commit_diff(&state.db, repo_path, workspace_id, revision)
  }

  fn commit_file_diff(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    revision: &str,
    file_path: &str,
  ) -> Result<JjFileDiff, String> {
    if let Some(remote) = self.remote_containing(repo_path) {
      return remote.commit_file_diff(workspace_id, revision, file_path);
    }
    let state = tauri::Manager::state::<crate::AppState>(self.app());
    core::get_commit_file_diff(&state.db, repo_path, workspace_id, revision, file_path)
  }

  fn abandon(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_id: &str,
  ) -> Result<String, String> {
    match self.remote_containing(repo_path) {
      Some(remote) => remote.abandon(remote_workspace(workspace_id)?, change_id),
      None => core::abandon_commit(repo_path, workspace_id, change_id),
    }
  }

  fn undo_operation(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    operation_id: &str,
  ) -> Result<(), String> {
    match self.remote_containing(repo_path) {
      Some(remote) => remote.undo_operation(workspace_id, operation_id),
      None => core::undo_repo_operation(repo_path, workspace_id, operation_id).map(|_| ()),
    }
  }

  fn stash(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_id: &str,
  ) -> Result<String, String> {
    if self.remote_containing(repo_path).is_some() {
      return Err(unsupported("Stashing"));
    }
    core::stash_commit(repo_path, workspace_id, change_id).map(|entry| entry.short_commit_id)
  }

  fn undo_commit(&self, repo_path: &str, workspace_id: i64, change_id: &str) -> Result<(), String> {
    match self.remote_containing(repo_path) {
      Some(remote) => remote.undo_commit(workspace_id, change_id),
      None => core::undo_commit(repo_path, workspace_id, change_id),
    }
  }

  fn revert(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_id: &str,
  ) -> Result<(), String> {
    match self.remote_containing(repo_path) {
      Some(remote) => remote.revert(remote_workspace(workspace_id)?, change_id),
      None => core::revert_commit(repo_path, workspace_id, change_id),
    }
  }

  fn shift_to_now(&self, repo_path: &str, workspace_id: i64) -> Result<(), String> {
    if self.remote_containing(repo_path).is_some() {
      return Err(unsupported("Shifting commit timestamps"));
    }
    core::shift_mutable_commits_to_now(repo_path, workspace_id)
  }

  fn commit_description(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_id: &str,
  ) -> Result<String, String> {
    match self.remote_containing(repo_path) {
      Some(remote) => remote.commit_description(remote_workspace(workspace_id)?, change_id),
      None => core::get_commit_description(repo_path, workspace_id, change_id),
    }
  }

  fn describe(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_id: &str,
    description: &str,
  ) -> Result<(), String> {
    match self.remote_containing(repo_path) {
      Some(remote) => remote.describe(remote_workspace(workspace_id)?, change_id, description),
      None => core::describe_commit(repo_path, workspace_id, change_id, description),
    }
  }

  fn shift_timestamp(
    &self,
    repo_path: &str,
    workspace_id: i64,
    change_id: &str,
    shift: &TimeShift,
  ) -> Result<(), String> {
    if self.remote_containing(repo_path).is_some() {
      return Err(unsupported("Shifting commit timestamps"));
    }
    match shift {
      TimeShift::By {
        days,
        hours,
        minutes,
      } => core::shift_commit_timestamp(
        repo_path,
        workspace_id,
        change_id,
        *days,
        *hours,
        *minutes,
        None,
      ),
      TimeShift::ToDay(day) => {
        core::shift_commit_timestamp(repo_path, workspace_id, change_id, 0, 0, 0, Some(day))
      }
    }
  }

  fn start_resolve(
    &self,
    repo_path: &str,
    workspace_id: Option<i64>,
    change_ids: Option<Vec<String>>,
  ) -> Result<ResolveConflictsSession, String> {
    if self.remote_containing(repo_path).is_some() {
      return Err(unsupported("Starting a conflict resolution session"));
    }
    core::start_resolve_conflicts(repo_path, workspace_id, change_ids)
  }

  fn resolve_prompt(&self, prompt: &str, session: &ResolveConflictsSession) -> String {
    core::build_resolve_agent_prompt(prompt, session)
  }

  fn snapshot_working_copy(&self, workspace_path: &str) -> Result<String, String> {
    match self.remote_containing(workspace_path) {
      Some(remote) => remote.snapshot(workspace_path),
      None => core::snapshot_working_copy(workspace_path),
    }
  }

  fn discard_all(&self, workspace_path: &str) -> Result<(), String> {
    match self.remote_containing(workspace_path) {
      Some(remote) => remote.restore_all(workspace_path),
      None => core::discard_all_changes(workspace_path).map(|_| ()),
    }
  }

  fn restore_snapshot(&self, workspace_path: &str, snapshot_id: &str) -> Result<(), String> {
    match self.remote_containing(workspace_path) {
      Some(remote) => remote.restore_snapshot(workspace_path, snapshot_id),
      None => core::restore_working_copy_snapshot(workspace_path, snapshot_id).map(|_| ()),
    }
  }
}

/// A shared handle to the Commits tab's backend.
pub type Host = Arc<dyn CommitsHost>;

/// `normalizeCommits`: drops an empty working-copy placeholder on top.
pub fn normalize(commits: Vec<JjLogCommit>) -> Vec<JjLogCommit> {
  let placeholder = match commits.as_slice() {
    [first, second, ..] => {
      first.description == "(no description)"
        && first.bookmarks.is_empty()
        && !first.is_immutable
        && first.parent_ids.contains(&second.commit_id)
    }
    _ => false,
  };
  if placeholder {
    commits.into_iter().skip(1).collect()
  } else {
    commits
  }
}

/// The commits the tab lists, as `CommitDiffViewer` derives them.
#[derive(Debug, Clone, Default)]
pub struct History {
  /// The branch's own commits, newest first.
  pub own: Vec<JjLogCommit>,
  /// The target branch's commits the branch lacks.
  pub target: Vec<JjLogCommit>,
  pub tentative: Option<(JjLogCommit, String)>,
  pub target_branch: String,
  /// A workspace on the default branch shows no target section.
  pub default_workspace: bool,
}

impl History {
  pub fn new(log: JjLogResult, home: bool) -> Self {
    let (target, own): (Vec<_>, Vec<_>) = log.commits.into_iter().partition(|c| c.on_target_only);
    let default_workspace = !home
      && !log.workspace_branch.is_empty()
      && !log.target_branch.is_empty()
      && log.workspace_branch == log.target_branch;
    Self {
      own: normalize(own),
      target,
      tentative: if home {
        None
      } else {
        log
          .tentative_working_copy
          .map(|wc| (wc.commit, wc.workspace_label))
      },
      target_branch: log.target_branch,
      default_workspace,
    }
  }

  pub fn shows_target(&self) -> bool {
    !self.default_workspace && !self.target.is_empty()
  }

  pub fn is_empty(&self) -> bool {
    self.own.is_empty() && !self.shows_target() && self.tentative.is_none()
  }

  /// "Shift to now" shows while a commit can still be rewritten.
  pub fn has_mutable(&self) -> bool {
    self
      .own
      .iter()
      .any(|c| !c.is_immutable && !c.on_target_only && !c.is_working_copy)
  }

  /// `resolvableConflictedCommits`.
  pub fn resolvable(&self, home: bool) -> Vec<JjLogCommit> {
    let distinct_branch = !home && !self.default_workspace;
    let conflicted: Vec<&JjLogCommit> = self
      .own
      .iter()
      .chain(&self.target)
      .filter(|c| c.has_conflicts && !c.is_working_copy)
      .collect();
    let all_on_branch =
      distinct_branch && !conflicted.is_empty() && conflicted.iter().all(|c| !c.on_target_only);
    conflicted
      .into_iter()
      .filter(|c| !c.on_target_only && (!c.is_immutable || all_on_branch))
      .cloned()
      .collect()
  }
}

/// `getCommitHeadline`.
pub fn headline(commit: &JjLogCommit) -> String {
  match commit.description.lines().next().map(str::trim) {
    Some(line) if !line.is_empty() && line != "(no description)" => line.to_string(),
    _ => "Working copy".into(),
  }
}

/// A description's first line for confirmations (`|| "(no message)"`).
pub fn first_line(commit: &JjLogCommit) -> String {
  match commit.description.split('\n').next() {
    Some(line) if !line.is_empty() => line.to_string(),
    _ => "(no message)".into(),
  }
}

/// The revisions a diff is read at: the commit id, then the change id.
pub fn revisions(commit_id: &str, change_id: Option<&str>) -> Vec<String> {
  let mut out = vec![commit_id.to_string()];
  if let Some(change) = change_id.filter(|c| !c.is_empty() && *c != commit_id) {
    out.push(change.to_string());
  }
  out
}

/// `loadCommitDiff`: the first revision whose diff has something in it,
/// else an empty diff, or the last error when every read failed.
pub fn first_diff(
  revisions: &[String],
  read: impl Fn(&str) -> Result<JjRevisionDiff, String>,
) -> Result<JjRevisionDiff, String> {
  let mut error = None;
  for revision in revisions {
    match read(revision) {
      Ok(diff)
        if diff.too_large_to_render
          || !diff.committed_files.is_empty()
          || !diff.hunks_by_file.is_empty() =>
      {
        return Ok(diff)
      }
      Ok(_) => {}
      Err(err) => error = Some(err),
    }
  }
  match error {
    Some(err) => Err(err),
    None => Ok(empty_diff()),
  }
}

pub fn empty_diff() -> JjRevisionDiff {
  JjRevisionDiff {
    committed_files: Vec::new(),
    hunks_by_file: Vec::new(),
    uncommitted_files: Vec::new(),
    conflicted_files: Vec::new(),
    too_large_to_render: false,
    render_block_reason: None,
  }
}

/// `loadCommitFileDiff`: the first revision that reads.
pub fn first_file_diff(
  revisions: &[String],
  read: impl Fn(&str) -> Result<JjFileDiff, String>,
) -> Result<JjFileDiff, String> {
  let mut error = None;
  for revision in revisions {
    match read(revision) {
      Ok(diff) => return Ok(diff),
      Err(err) => error = Some(err),
    }
  }
  Err(error.unwrap_or_else(|| "Failed to load commit file diff".into()))
}

#[allow(clippy::expect_used, reason = "constant pattern")]
static HUNK_HEADER: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@").expect("hunk header regex")
});

/// `parseHunkHeader`: `(old_start, new_start)`.
pub fn hunk_starts(header: &str) -> (u32, u32) {
  HUNK_HEADER
    .captures(header)
    .map(|c| (c[1].parse().unwrap_or(1), c[2].parse().unwrap_or(1)))
    .unwrap_or((1, 1))
}

/// `computeHunkLineNumbers`: each line's `(old, new)` number.
pub fn line_numbers(hunk: &JjDiffHunk) -> Vec<(Option<u32>, Option<u32>)> {
  numbered(hunk).collect()
}

/// Line `l`'s `(old, new)` number, counting only the lines before it.
pub fn line_number(hunk: &JjDiffHunk, l: usize) -> (Option<u32>, Option<u32>) {
  numbered(hunk).nth(l).unwrap_or((None, None))
}

fn numbered(hunk: &JjDiffHunk) -> impl Iterator<Item = (Option<u32>, Option<u32>)> + '_ {
  let (mut old, mut new) = hunk_starts(&hunk.header);
  hunk.lines.iter().map(move |line| {
    if line.starts_with('+') {
      new += 1;
      (None, Some(new - 1))
    } else if line.starts_with('-') {
      old += 1;
      (Some(old - 1), None)
    } else {
      old += 1;
      new += 1;
      (Some(old - 1), Some(new - 1))
    }
  })
}

/// A diff line's kind, from its prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
  Added,
  Removed,
  Context,
}

pub fn line_kind(line: &str) -> LineKind {
  if line.starts_with('+') {
    LineKind::Added
  } else if line.starts_with('-') {
    LineKind::Removed
  } else {
    LineKind::Context
  }
}

/// A line of a file's diff: its hunk and index in it.
pub type LineRef = (usize, usize);

/// The lines from `from` to `to`, inclusive and in order, across hunks.
pub fn selection(hunks: &[JjDiffHunk], from: LineRef, to: LineRef) -> Vec<LineRef> {
  let global = |(h, l): LineRef| {
    hunks[..h.min(hunks.len())]
      .iter()
      .map(|h| h.lines.len())
      .sum::<usize>()
      + l
  };
  let (start, end) = {
    let (a, b) = (global(from), global(to));
    (a.min(b), a.max(b))
  };
  let mut out = Vec::new();
  let mut index = 0;
  for (h, hunk) in hunks.iter().enumerate() {
    for l in 0..hunk.lines.len() {
      if (start..=end).contains(&index) {
        out.push((h, l));
      }
      index += 1;
    }
  }
  out
}

/// The lines a comment is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentTarget {
  pub file_path: String,
  /// Where the comment box shows: after this line.
  pub at: LineRef,
  pub start_line: u32,
  pub end_line: u32,
  pub lines: Vec<String>,
}

/// `handleAddCommentFromSelection`: the selected lines' range and text.
pub fn comment_target(file: &JjFileDiff, lines: &[LineRef]) -> Option<CommentTarget> {
  let mut target: Option<CommentTarget> = None;
  for &(h, l) in lines {
    let Some(hunk) = file.hunks.get(h) else {
      continue;
    };
    let numbers = line_numbers(hunk);
    let number = numbers
      .get(l)
      .and_then(|(old, new)| new.or(*old))
      .unwrap_or(l as u32 + 1);
    let content = hunk.lines.get(l).cloned().unwrap_or_default();
    let t = target.get_or_insert_with(|| CommentTarget {
      file_path: file.path.clone(),
      at: (h, l),
      start_line: number,
      end_line: number,
      lines: Vec::new(),
    });
    t.at = (h, l);
    t.start_line = t.start_line.min(number);
    t.end_line = t.end_line.max(number);
    t.lines.push(content);
  }
  target
}

/// `L12` or `L12-14`, as the comment box labels its lines.
pub fn line_label(start: u32, end: u32) -> String {
  if start == end {
    format!("L{start}")
  } else {
    format!("L{start}-{end}")
  }
}

/// `handleCreateAgentWithComment`'s prompt: the reference, the lines and
/// the comment. `file_path` is relative to the checkout.
pub fn comment_prompt(target: &CommentTarget, comment: &str, commit_short_id: &str) -> String {
  let range = if target.start_line == target.end_line {
    target.start_line.to_string()
  } else {
    format!("{}-{}", target.start_line, target.end_line)
  };
  format!(
    "{}:{range} (commit {commit_short_id})\n```\n{}\n```\n> {comment}\n",
    target.file_path,
    target.lines.join("\n")
  )
}

/// The timestamp dialog's fields, as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShiftForm {
  pub to_day: bool,
  pub back: bool,
  pub days: String,
  pub hours: String,
  pub minutes: String,
  pub day: String,
}

impl Default for ShiftForm {
  fn default() -> Self {
    Self {
      to_day: false,
      back: false,
      days: "0".into(),
      hours: "0".into(),
      minutes: "0".into(),
      day: String::new(),
    }
  }
}

impl ShiftForm {
  /// The shift to apply, or `None` while the form can't be submitted.
  #[allow(clippy::expect_used, reason = "constant pattern")]
  pub fn shift(&self) -> Option<TimeShift> {
    if self.to_day {
      static DAY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").expect("day regex"));
      return DAY
        .is_match(&self.day)
        .then(|| TimeShift::ToDay(self.day.clone()));
    }
    // `Number("")` is 0, as an emptied field is.
    let parse = |s: &str| match s.trim() {
      "" => Some(0),
      s => s.parse::<i64>().ok().filter(|n| *n >= 0),
    };
    let (d, h, m) = (
      parse(&self.days)?,
      parse(&self.hours)?,
      parse(&self.minutes)?,
    );
    if d + h + m == 0 {
      return None;
    }
    let sign = if self.back { -1 } else { 1 };
    Some(TimeShift::By {
      days: sign * d,
      hours: sign * h,
      minutes: sign * m,
    })
  }
}

/// The resolve session's name (`ResolveConflictsDialog`).
pub fn resolve_session_name(session: &ResolveConflictsSession) -> Option<String> {
  let primary = session.targets.first()?;
  Some(if session.targets.len() > 1 {
    "Resolve conflicts".into()
  } else {
    format!(
      "Resolve {}",
      primary.change_id.chars().take(8).collect::<String>()
    )
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn commit(id: &str) -> JjLogCommit {
    serde_json::from_value(serde_json::json!({
      "commit_id": id, "short_id": id, "change_id": format!("ch-{id}"),
      "description": format!("{id} work\n\nbody"), "author_name": "Zn",
      "timestamp": "2026-10-19T00:00:00Z", "parent_ids": [],
      "is_working_copy": false, "bookmarks": [], "is_immutable": false,
      "insertions": 1, "deletions": 0
    }))
    .expect("commit")
  }

  fn hunk(header: &str, lines: &[&str]) -> JjDiffHunk {
    serde_json::from_value(serde_json::json!({
      "id": header, "header": header, "patch": "",
      "lines": lines,
    }))
    .expect("hunk")
  }

  #[test]
  fn drops_an_empty_working_copy_on_top() {
    let mut wc = commit("wc");
    wc.description = "(no description)".into();
    wc.parent_ids = vec!["a".into()];
    let ids = |v: Vec<JjLogCommit>| v.into_iter().map(|c| c.commit_id).collect::<Vec<_>>();
    assert_eq!(ids(normalize(vec![wc.clone(), commit("a")])), ["a"]);
    wc.bookmarks = vec!["main".into()];
    assert_eq!(ids(normalize(vec![wc, commit("a")])), ["wc", "a"]);
  }

  #[test]
  fn splits_own_and_target_commits() {
    let mut t = commit("t");
    t.on_target_only = true;
    let log: JjLogResult = serde_json::from_value(serde_json::json!({
      "commits": [commit("a"), t],
      "target_branch": "main",
      "workspace_branch": "feat"
    }))
    .expect("log");
    let history = History::new(log.clone(), false);
    assert_eq!(history.own.len(), 1);
    assert!(history.shows_target() && history.has_mutable());
    let mut default = log;
    default.workspace_branch = "main".into();
    assert!(!History::new(default, false).shows_target());
  }

  #[test]
  fn resolvable_conflicts_match_react() {
    let mut a = commit("a");
    a.has_conflicts = true;
    let mut b = commit("b");
    b.has_conflicts = true;
    b.is_immutable = true;
    let history = History {
      own: vec![a, b],
      ..Default::default()
    };
    // Every conflict is on the workspace branch: immutable ones count too.
    assert_eq!(history.resolvable(false).len(), 2);
    // On the home branch only mutable ones do.
    assert_eq!(history.resolvable(true).len(), 1);
  }

  #[test]
  fn headlines_and_first_lines() {
    let mut c = commit("a");
    assert_eq!(headline(&c), "a work");
    c.description = "(no description)".into();
    assert_eq!(headline(&c), "Working copy");
    c.description = String::new();
    assert_eq!(first_line(&c), "(no message)");
  }

  #[test]
  fn numbers_hunk_lines() {
    let h = hunk("@@ -10,3 +20,4 @@", &[" a", "-b", "+c", "+d", " e"]);
    assert_eq!(
      line_numbers(&h),
      [
        (Some(10), Some(20)),
        (Some(11), None),
        (None, Some(21)),
        (None, Some(22)),
        (Some(12), Some(23)),
      ]
    );
    assert_eq!(hunk_starts("bogus"), (1, 1));
  }

  #[test]
  fn selects_across_hunks_in_order() {
    let hunks = [
      hunk("@@ -1 +1 @@", &[" a", "+b"]),
      hunk("@@ -9 +9 @@", &[" c"]),
    ];
    assert_eq!(selection(&hunks, (1, 0), (0, 1)), [(0, 1), (1, 0)]);
  }

  #[test]
  fn formats_the_agent_comment() {
    let file = JjFileDiff {
      path: "src/a.rs".into(),
      hunks: vec![hunk("@@ -1,2 +1,3 @@", &[" a", "+b", "+c"])],
      conflict_style: Default::default(),
      conflict_regions: vec![],
    };
    let target = comment_target(&file, &[(0, 1), (0, 2)]).expect("target");
    assert_eq!(
      (target.start_line, target.end_line, target.at),
      (2, 3, (0, 2))
    );
    assert_eq!(line_label(2, 3), "L2-3");
    assert_eq!(
      comment_prompt(&target, "why?", "abc"),
      "src/a.rs:2-3 (commit abc)\n```\n+b\n+c\n```\n> why?\n"
    );
  }

  #[test]
  fn timestamp_form_validates_like_react() {
    let mut form = ShiftForm::default();
    assert_eq!(form.shift(), None);
    form.hours = "2".into();
    form.back = true;
    assert_eq!(
      form.shift(),
      Some(TimeShift::By {
        days: 0,
        hours: -2,
        minutes: 0
      })
    );
    form.minutes = "-1".into();
    assert_eq!(form.shift(), None);
    form.minutes = String::new();
    assert!(form.shift().is_some());
    form.to_day = true;
    form.day = "2026-10-01".into();
    assert_eq!(form.shift(), Some(TimeShift::ToDay("2026-10-01".into())));
  }

  #[test]
  fn revisions_try_the_commit_then_the_change() {
    assert_eq!(revisions("c1", Some("ch")), ["c1", "ch"]);
    assert_eq!(revisions("c1", Some("c1")), ["c1"]);
  }

  #[test]
  fn diffs_fall_back_to_the_change_id() {
    let revs = revisions("c1", Some("ch"));
    let mut found = empty_diff();
    found.too_large_to_render = true;
    let diff = first_diff(&revs, |r| {
      if r == "ch" {
        Ok(found.clone())
      } else {
        Err("no such revision".into())
      }
    })
    .expect("diff");
    assert!(diff.too_large_to_render);
    assert!(first_diff(&revs, |_| Ok(empty_diff()))
      .expect("empty")
      .committed_files
      .is_empty());
    assert_eq!(
      first_diff(&revs, |_| Err("boom".to_string())).map(|_| ()),
      Err("boom".to_string())
    );
    assert!(first_file_diff(&[], |_| Err("x".to_string())).is_err());
  }
}
