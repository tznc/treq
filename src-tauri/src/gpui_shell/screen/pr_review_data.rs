//! What the pull request review screen (`PrReviewPage`) reads from and
//! writes to the backend, and the rules of `lib/pr-review.ts`: the PR's
//! unified diff as files and hunks, review drafts kept in the local
//! database as line comments, and drafts back to a GitHub review.

use std::{
  collections::HashMap,
  sync::{Arc, LazyLock},
};

use super::{
  changes_data::{ChangedFile, ChangesHost},
  changes_review::{LineComment, Side, FILE_HUNK},
  commits_data::line_numbers,
};
use crate::{
  core,
  github::{GhDraftReviewComment, GhPullRequest, GhReviewThread},
  gpui_shell::native::Backend,
  jj::JjDiffHunk,
  local_db::AgentReviewComment,
};

/// `AGENT_REVIEW_TARGET_GITHUB_PR`: drafts are kept under the PR number.
pub const TARGET: &str = "github_pr";
/// Never a real hunk id: the diff lists such a comment as outdated.
pub const OUTDATED_HUNK: &str = "__outdated__";

/// A draft the user wrote (`ReviewDraftInput`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDraft {
  pub file_path: String,
  pub start_line: i64,
  pub end_line: i64,
  pub side: Side,
  pub text: String,
  pub line_hash: Option<String>,
  /// The commented diff lines, shown if the draft goes outdated.
  pub quoted_text: Option<String>,
}

/// What the PR review asks of the backend; faked in tests. Everything
/// blocks, so it is called off the main thread.
pub trait PrReviewHost: ChangesHost {
  /// The repository's GitHub `owner/repo` (`useGitRemoteInfo`).
  fn remote_full_name(&self, repo_path: &str) -> Option<String>;
  fn view_pr(&self, full_name: &str, number: u64) -> Result<GhPullRequest, String>;
  /// `gh pr diff`.
  fn pr_diff(&self, full_name: &str, number: u64) -> Result<String, String>;
  /// The login `gh` is signed in as.
  fn current_user(&self) -> Result<String, String>;
  fn pr_threads(&self, full_name: &str, number: u64) -> Result<Vec<GhReviewThread>, String>;
  fn reply_thread(&self, thread_id: &str, body: &str) -> Result<(), String>;
  fn submit_review(
    &self,
    full_name: &str,
    number: u64,
    event: &str,
    body: &str,
    comments: &[GhDraftReviewComment],
  ) -> Result<(), String>;
  /// The drafts kept for the PR, the user's and the review agent's.
  fn drafts(&self, repo_path: &str, number: u64) -> Result<Vec<AgentReviewComment>, String>;
  fn create_draft(&self, repo_path: &str, number: u64, draft: &NewDraft) -> Result<(), String>;
  /// Edits a draft's text and/or records the hash and text of its lines.
  fn update_draft(
    &self,
    repo_path: &str,
    id: &str,
    text: Option<&str>,
    line_hash: Option<&str>,
    quoted_text: Option<&str>,
  ) -> Result<(), String>;
}

impl PrReviewHost for Backend {
  fn remote_full_name(&self, repo_path: &str) -> Option<String> {
    core::github_cli::remote(repo_path)
      .ok()
      .flatten()
      .map(|r| r.full_name)
  }

  fn view_pr(&self, full_name: &str, number: u64) -> Result<GhPullRequest, String> {
    core::github_cli::view_pr(full_name, number)
  }

  fn pr_diff(&self, full_name: &str, number: u64) -> Result<String, String> {
    core::github_cli::pr_diff(full_name, number)
  }

  fn current_user(&self) -> Result<String, String> {
    core::github_cli::current_user()
  }

  fn pr_threads(&self, full_name: &str, number: u64) -> Result<Vec<GhReviewThread>, String> {
    core::github_cli::pr_review_threads(full_name, number)
  }

  fn reply_thread(&self, thread_id: &str, body: &str) -> Result<(), String> {
    core::github_cli::reply_to_review_thread(thread_id, body)
  }

  fn submit_review(
    &self,
    full_name: &str,
    number: u64,
    event: &str,
    body: &str,
    comments: &[GhDraftReviewComment],
  ) -> Result<(), String> {
    core::github_cli::submit_pr_review(full_name, number, event, body, comments)
  }

  fn drafts(&self, repo_path: &str, number: u64) -> Result<Vec<AgentReviewComment>, String> {
    core::agent_review::list_comments(repo_path, TARGET, &number.to_string())
  }

  fn create_draft(&self, repo_path: &str, number: u64, draft: &NewDraft) -> Result<(), String> {
    let draft = crate::core::review_drafts::NewReviewDraft {
      target_type: TARGET.into(),
      target_id: number.to_string(),
      file_path: draft.file_path.clone(),
      start_line: draft.start_line,
      end_line: draft.end_line,
      side: Some(side_name(draft.side).into()),
      comment_text: draft.text.clone(),
      line_hash: draft.line_hash.clone(),
      quoted_text: draft.quoted_text.clone(),
    };
    crate::core::review_drafts::create_review_draft(repo_path, &draft).map(|_| ())
  }

  fn update_draft(
    &self,
    repo_path: &str,
    id: &str,
    text: Option<&str>,
    line_hash: Option<&str>,
    quoted_text: Option<&str>,
  ) -> Result<(), String> {
    core::review_drafts::update_review_draft(repo_path, id, text, line_hash, quoted_text)
  }
}

pub fn side_name(side: Side) -> &'static str {
  match side {
    Side::Old => "old",
    Side::New => "new",
  }
}

fn strip_prefix(path: &str) -> &str {
  path
    .strip_prefix("a/")
    .or_else(|| path.strip_prefix("b/"))
    .unwrap_or(path)
}

/// The PR's files, in diff order, and each file's hunks.
#[derive(Debug, Clone, Default)]
pub struct ParsedDiff {
  pub files: Vec<ChangedFile>,
  pub hunks: HashMap<String, Vec<JjDiffHunk>>,
}

/// `parseUnifiedDiff`: `gh pr diff` output as changed files and hunks.
#[allow(clippy::expect_used, reason = "constant pattern")]
pub fn parse_unified_diff(diff: &str) -> ParsedDiff {
  let mut out = ParsedDiff::default();
  let mut file: Option<ChangedFile> = None;
  let mut hunks: Vec<JjDiffHunk> = Vec::new();
  let mut in_hunk = false;
  let mut flush = |file: &mut Option<ChangedFile>, hunks: &mut Vec<JjDiffHunk>| {
    if let Some(f) = file.take() {
      out.hunks.insert(f.path.clone(), std::mem::take(hunks));
      out.files.push(f);
    }
  };
  static GIT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^diff --git a/(.*) b/(.*)$").expect("diff header regex"));
  let git = &*GIT;
  for line in diff.split('\n') {
    if let Some(rest) = line.strip_prefix("diff --git ") {
      flush(&mut file, &mut hunks);
      let path = git
        .captures(line)
        .map_or_else(|| rest.to_string(), |c| c[2].to_string());
      file = Some(ChangedFile {
        path,
        status: "M".into(),
        old_path: None,
      });
      in_hunk = false;
      continue;
    }
    let Some(f) = file.as_mut() else {
      continue;
    };
    if line.starts_with("@@") {
      hunks.push(JjDiffHunk {
        id: format!("{}#{}", f.path, hunks.len()),
        header: line.to_string(),
        lines: Vec::new(),
        patch: String::new(),
        conflict_style: Default::default(),
        conflict_regions: Vec::new(),
      });
      in_hunk = true;
    } else if in_hunk {
      if line.starts_with([' ', '+', '-']) {
        if let Some(h) = hunks.last_mut() {
          h.lines.push(line.to_string());
        }
      }
    } else if line.starts_with("new file mode") {
      f.status = "A".into();
    } else if line.starts_with("deleted file mode") {
      f.status = "D".into();
    } else if let Some(old) = line.strip_prefix("rename from ") {
      f.status = "R".into();
      f.old_path = Some(old.to_string());
    } else if let Some(new) = line.strip_prefix("rename to ") {
      f.path = new.to_string();
    } else if let Some(old) = line
      .strip_prefix("--- ")
      .filter(|_| line != "--- /dev/null")
    {
      if f.status == "D" {
        f.path = strip_prefix(old).to_string();
      }
    } else if let Some(new) = line
      .strip_prefix("+++ ")
      .filter(|_| line != "+++ /dev/null")
    {
      f.path = strip_prefix(new).to_string();
    }
  }
  flush(&mut file, &mut hunks);
  out
}

/// `hashDiffLines`: djb2 over the lines joined by newlines, as 8 hex digits.
pub fn hash_diff_lines(lines: &[String]) -> String {
  let text = lines.join("\n");
  let mut hash: u64 = 5381;
  for unit in text.encode_utf16() {
    hash = (hash * 33 + u64::from(unit)) % (1 << 32);
  }
  format!("{hash:08x}")
}

/// A kept draft as the diff shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
  pub row: AgentReviewComment,
  pub comment: LineComment,
  /// The hash of the lines it covers now, `None` when they are gone.
  pub line_hash: Option<String>,
  /// The lines it covers now, joined, `None` when they are gone.
  pub quoted_text: Option<String>,
  pub outdated: bool,
}

/// A file's hunks, shared (`Arc`) or owned.
pub trait HunkList {
  fn hunks(&self) -> &[JjDiffHunk];
}

impl HunkList for Vec<JjDiffHunk> {
  fn hunks(&self) -> &[JjDiffHunk] {
    self
  }
}

impl HunkList for Arc<Vec<JjDiffHunk>> {
  fn hunks(&self) -> &[JjDiffHunk] {
    self
  }
}

type LineNumbers = Vec<(Option<u32>, Option<u32>)>;

/// The diff lines of one side of a hunk, as `(line index, line number)`.
/// Consecutive lines on one side of a hunk have consecutive numbers.
type SideLines = Vec<(usize, i64)>;

/// djb2 as `hashDiffLines` computes it, over UTF-16 units, mod 2^32.
const DJB2_SEED: u32 = 5381;

fn pow33(mut exp: usize) -> u32 {
  let (mut base, mut out) = (33u32, 1u32);
  while exp > 0 {
    if exp & 1 == 1 {
      out = out.wrapping_mul(base);
    }
    base = base.wrapping_mul(base);
    exp >>= 1;
  }
  out
}

/// `33 * inverse == 1 (mod 2^32)`: 33 is odd, so it has one.
fn inverse33() -> u32 {
  let mut x = 1u32;
  for _ in 0..5 {
    x = x.wrapping_mul(2u32.wrapping_sub(33u32.wrapping_mul(x)));
  }
  x
}

/// The hash of every `size` consecutive lines of `lines`, as
/// [`hash_diff_lines`] gives them as a number, from one pass over the lines
/// instead of joining each window.
fn window_hashes<'a>(lines: impl Iterator<Item = &'a str>, size: usize) -> Vec<u32> {
  // `prefix[k]` is the polynomial of the first `k` lines each followed by a
  // newline, `end[k]` how many UTF-16 units that is.
  let mut prefix = vec![0u32];
  let mut end = vec![0usize];
  for line in lines {
    let (mut poly, mut units) = (0u32, 0usize);
    for unit in line.encode_utf16() {
      poly = poly.wrapping_mul(33).wrapping_add(u32::from(unit));
      units += 1;
    }
    let (last, at) = (prefix[prefix.len() - 1], end[end.len() - 1]);
    // The line and its newline, after what came before.
    let with_newline = poly.wrapping_mul(33).wrapping_add(10);
    prefix.push(
      last
        .wrapping_mul(pow33(units + 1))
        .wrapping_add(with_newline),
    );
    end.push(at + units + 1);
  }
  let inverse = inverse33();
  (0..prefix.len().saturating_sub(size))
    .map(|a| {
      let b = a + size;
      let span = end[b] - end[a];
      let blocks = prefix[b].wrapping_sub(prefix[a].wrapping_mul(pow33(span)));
      // The window ends at its last line, not at a newline.
      let poly = blocks.wrapping_sub(10).wrapping_mul(inverse);
      DJB2_SEED.wrapping_mul(pow33(span - 1)).wrapping_add(poly)
    })
    .collect()
}

/// Places drafts on a diff. What one pass learns about a file (its line
/// numbers, the lines of each side, the hash of every window of a size) is
/// kept for the drafts after it.
/// Window hash to the windows with it.
type WindowIndex = HashMap<u32, Vec<(usize, usize)>>;

struct Placer<'a, H> {
  hunks: &'a HashMap<String, H>,
  numbers: HashMap<String, Vec<LineNumbers>>,
  sides: HashMap<(String, bool), Vec<SideLines>>,
  /// Window hash to the windows with it, as `(hunk, first index in the
  /// side's lines)`, in diff order.
  windows: HashMap<(String, bool, usize), WindowIndex>,
}

impl<'a, H: HunkList> Placer<'a, H> {
  fn new(hunks: &'a HashMap<String, H>) -> Self {
    Self {
      hunks,
      numbers: HashMap::new(),
      sides: HashMap::new(),
      windows: HashMap::new(),
    }
  }

  fn numbers(&mut self, file: &str) -> &[LineNumbers] {
    let hunks = self.hunks;
    self.numbers.entry(file.to_string()).or_insert_with(|| {
      hunks
        .get(file)
        .map_or(&[][..], HunkList::hunks)
        .iter()
        .map(line_numbers)
        .collect()
    })
  }

  /// The hunk and lines `start..=end` on the draft's side cover, if all are
  /// in the diff (`findCommentedLines`).
  fn commented_lines(&mut self, draft: &AgentReviewComment) -> Option<(String, Vec<String>)> {
    let side = if draft.side.as_deref() == Some("old") {
      Side::Old
    } else {
      Side::New
    };
    let all = self.hunks;
    let hunks = all.get(&draft.file_path)?.hunks();
    self.numbers(&draft.file_path);
    let numbers = &self.numbers[&draft.file_path];
    for (hunk, numbers) in hunks.iter().zip(numbers) {
      let lines: Vec<String> = hunk
        .lines
        .iter()
        .zip(numbers)
        .filter(|(_, (old, new))| {
          let line_side = if old.is_some() && new.is_none() {
            Side::Old
          } else {
            Side::New
          };
          let n = match side {
            Side::Old => *old,
            Side::New => *new,
          };
          line_side == side
            && n.is_some_and(|n| i64::from(n) >= draft.start_line && i64::from(n) <= draft.end_line)
        })
        .map(|(l, _)| l.clone())
        .collect();
      if lines.len() as i64 == draft.end_line - draft.start_line + 1 {
        return Some((hunk.id.clone(), lines));
      }
    }
    None
  }

  /// Where a draft's lines moved to (`findMovedLines`): the run of diff
  /// lines on its side that hashes to `line_hash`, nearest its old start.
  /// Only lines in the diff can be found.
  fn moved_lines(
    &mut self,
    draft: &AgentReviewComment,
    line_hash: &str,
  ) -> Option<(String, Vec<String>, i64)> {
    let old_side = draft.side.as_deref() == Some("old");
    let size = usize::try_from(draft.end_line - draft.start_line + 1)
      .ok()
      .filter(|&size| size > 0)?;
    // `hash_diff_lines` writes eight lowercase digits; nothing else equals it.
    let target = u32::from_str_radix(line_hash, 16)
      .ok()
      .filter(|h| format!("{h:08x}") == line_hash)?;
    let file = draft.file_path.as_str();
    let all = self.hunks;
    let hunks = all.get(file)?.hunks();
    self.numbers(file);
    let side_key = (file.to_string(), old_side);
    if !self.sides.contains_key(&side_key) {
      let sides = hunks
        .iter()
        .zip(&self.numbers[file])
        .map(|(hunk, numbers)| {
          (0..hunk.lines.len())
            .filter_map(|i| match (old_side, numbers[i]) {
              (true, (Some(o), None)) => Some((i, i64::from(o))),
              (false, (_, Some(n))) => Some((i, i64::from(n))),
              _ => None,
            })
            .collect()
        })
        .collect();
      self.sides.insert(side_key.clone(), sides);
    }
    let sides = &self.sides[&side_key];
    let windows = self
      .windows
      .entry((file.to_string(), old_side, size))
      .or_insert_with(|| {
        let mut index: HashMap<u32, Vec<(usize, usize)>> = HashMap::new();
        for (h, (hunk, side)) in hunks.iter().zip(sides).enumerate() {
          let lines = side.iter().map(|&(i, _)| hunk.lines[i].as_str());
          for (at, hash) in window_hashes(lines, size).into_iter().enumerate() {
            index.entry(hash).or_default().push((h, at));
          }
        }
        index
      });
    let mut best: Option<(String, Vec<String>, i64)> = None;
    for &(h, at) in windows.get(&target).map_or(&[][..], Vec::as_slice) {
      let (hunk, side) = (&hunks[h], &sides[h]);
      let lines: Vec<String> = side[at..at + size]
        .iter()
        .map(|&(i, _)| hunk.lines[i].clone())
        .collect();
      // The index keys on a 32-bit hash: confirm it.
      if hash_diff_lines(&lines) != line_hash {
        continue;
      }
      let start = side[at].1;
      let nearer = best
        .as_ref()
        .is_none_or(|(_, _, b)| (start - draft.start_line).abs() < (b - draft.start_line).abs());
      if nearer {
        best = Some((hunk.id.clone(), lines, start));
      }
    }
    best
  }

  /// `draftToLineComment`: follows its lines if a PR update moved them
  /// unchanged; outdated when they left the diff or no longer hash as when
  /// it was written.
  fn place(&mut self, row: &AgentReviewComment) -> Draft {
    let mut found = self.commented_lines(row);
    let mut start_line = row.start_line;
    if let Some(stored) = row.line_hash.as_deref() {
      let unchanged = found
        .as_ref()
        .is_some_and(|(_, lines)| hash_diff_lines(lines) == stored);
      if !unchanged {
        if let Some((id, lines, start)) = self.moved_lines(row, stored) {
          found = Some((id, lines));
          start_line = start;
        }
      }
    }
    let end_line = start_line + (row.end_line - row.start_line);
    let line_hash = found.as_ref().map(|(_, lines)| hash_diff_lines(lines));
    let outdated = found.is_none()
      || row
        .line_hash
        .as_ref()
        .is_some_and(|h| Some(h) != line_hash.as_ref());
    let text = match &row.suggested_replacement {
      Some(s) if !row.comment_text.contains("```suggestion") => {
        format!("{}\n\n```suggestion\n{s}\n```", row.comment_text)
      }
      _ => row.comment_text.clone(),
    };
    let (hunk_id, line_content) = match (&found, outdated) {
      (Some((id, lines)), false) => (id.clone(), lines.clone()),
      _ => (
        OUTDATED_HUNK.to_string(),
        row
          .quoted_text
          .as_deref()
          .map(|q| q.split('\n').map(str::to_string).collect())
          .unwrap_or_default(),
      ),
    };
    let quoted_text = found.as_ref().map(|(_, lines)| lines.join("\n"));
    let n = |v: i64| u32::try_from(v).unwrap_or(0);
    Draft {
      comment: LineComment {
        id: row.id.clone(),
        file_path: row.file_path.clone(),
        hunk_id,
        start_line: n(start_line),
        end_line: n(end_line),
        line_content,
        text,
        created_at: row.created_at.clone(),
        line_side: Some(if row.side.as_deref() == Some("old") {
          Side::Old
        } else {
          Side::New
        }),
        source: None,
        github_author: None,
        github_avatar_url: None,
        github_comment_url: None,
      },
      row: row.clone(),
      line_hash,
      quoted_text,
      outdated,
    }
  }
}

/// `draftToLineComment` for one draft.
pub fn draft_to_comment<H: HunkList>(
  row: &AgentReviewComment,
  hunks: &HashMap<String, H>,
) -> Draft {
  Placer::new(hunks).place(row)
}

/// The drafts shown: every one not resolved, once the diff was read.
pub fn drafts<H: HunkList>(rows: &[AgentReviewComment], hunks: &HashMap<String, H>) -> Vec<Draft> {
  let mut placer = Placer::new(hunks);
  rows
    .iter()
    .filter(|r| r.status != "resolved")
    .map(|r| placer.place(r))
    .collect()
}

/// `commentsToGhReview`: line comments become inline comments; file
/// comments have no line, so they go in the review body.
pub fn comments_to_review(comments: &[LineComment]) -> (Vec<GhDraftReviewComment>, String) {
  let mut inline = Vec::new();
  let mut files = Vec::new();
  for c in comments {
    if c.hunk_id == FILE_HUNK {
      files.push(format!("**{}**: {}", c.file_path, c.text));
      continue;
    }
    inline.push(GhDraftReviewComment {
      path: c.file_path.clone(),
      line: u64::from(c.end_line),
      side: if c.line_side == Some(Side::Old) {
        "LEFT".into()
      } else {
        "RIGHT".into()
      },
      start_line: (c.start_line < c.end_line).then_some(u64::from(c.start_line)),
      body: c.text.clone(),
    });
  }
  (inline, files.join("\n\n"))
}

/// The review's verdict (`GhReviewEvent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Event {
  #[default]
  Comment,
  Approve,
  RequestChanges,
}

impl Event {
  pub const ALL: [Event; 3] = [Event::Comment, Event::Approve, Event::RequestChanges];

  pub fn value(self) -> &'static str {
    match self {
      Event::Comment => "COMMENT",
      Event::Approve => "APPROVE",
      Event::RequestChanges => "REQUEST_CHANGES",
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      Event::Comment => "Comment",
      Event::Approve => "Approve",
      Event::RequestChanges => "Request changes",
    }
  }

  pub fn hint(self) -> &'static str {
    match self {
      Event::Comment => "Submit general feedback without explicit approval.",
      Event::Approve => "Submit feedback and approve merging these changes.",
      Event::RequestChanges => "Submit feedback that must be addressed before merging.",
    }
  }
}

/// The hint under an option the PR's author cannot pick.
pub const OWN_PR_HINT: &str =
  "Pull request authors can't approve or request changes on their own pull request.";

/// Requesting changes needs a summary or a comment.
pub fn can_submit(event: Event, summary: &str, postable: usize) -> bool {
  event != Event::RequestChanges || !summary.trim().is_empty() || postable > 0
}

/// The popover's line under "Finish your review".
pub fn finish_text(postable: usize, outdated: usize) -> String {
  let mut text = format!(
    "{postable} pending comment{} will be posted to GitHub.",
    if postable == 1 { "" } else { "s" }
  );
  if outdated > 0 {
    text.push_str(&format!(
      " {outdated} outdated draft{}.",
      if outdated == 1 {
        " stays as a draft"
      } else {
        "s stay as drafts"
      }
    ));
  }
  text
}

/// `N file(s)`.
pub fn files_label(n: usize) -> String {
  format!("{n} file{}", if n == 1 { "" } else { "s" })
}

/// `githubPrReviewSummary`: what is under review, and where the agent reads
/// the diff, as the PR is usually not checked out.
pub fn review_summary(full_name: &str, number: u64, title: &str, files: usize) -> String {
  let files = format!("{files} changed file{}", if files == 1 { "" } else { "s" });
  format!(
    "GitHub pull request #{number} \"{title}\" ({files}). The PR is not checked out locally: read \
     its diff with `gh pr diff {number} --repo {full_name}` and, for full file contents, `gh api \
     repos/{full_name}/contents/<path>?ref=<head sha>`. Use paths and new-side line numbers from \
     that diff in --file and --start-line/--end-line (--side old for removed lines)"
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs
index 1..2 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,3 @@
 one
+two
 three
\\ No newline at end of file
diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1 @@
+hi
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
diff --git a/old.rs b/moved.rs
similarity index 100%
rename from old.rs
rename to moved.rs
";

  fn row(start: i64, end: i64, side: &str, hash: Option<&str>) -> AgentReviewComment {
    serde_json::from_value(serde_json::json!({
      "id": "d1",
      "repo_path": "/repo",
      "target_type": TARGET,
      "target_id": "5",
      "file_path": "src/a.rs",
      "hunk_id": null,
      "start_line": start,
      "end_line": end,
      "side": side,
      "comment_text": "Nit",
      "suggested_replacement": null,
      "status": "open",
      "source": "local-human",
      "created_at": "2026-01-01T00:00:00Z",
      "resolved_at": null,
      "line_hash": hash,
    }))
    .expect("draft row")
  }

  #[test]
  fn parses_files_and_their_hunks() {
    let parsed = parse_unified_diff(DIFF);
    let files: Vec<(&str, &str, Option<&str>)> = parsed
      .files
      .iter()
      .map(|f| (f.path.as_str(), f.status.as_str(), f.old_path.as_deref()))
      .collect();
    assert_eq!(
      files,
      [
        ("src/a.rs", "M", None),
        ("new.txt", "A", None),
        ("gone.txt", "D", None),
        ("moved.rs", "R", Some("old.rs")),
      ]
    );
    let hunks = &parsed.hunks["src/a.rs"];
    assert_eq!(hunks[0].id, "src/a.rs#0");
    assert_eq!(hunks[0].lines, [" one", "+two", " three"]);
    assert_eq!(parsed.hunks["gone.txt"][0].lines, ["-bye"]);
    assert!(parsed.hunks["moved.rs"].is_empty());
  }

  #[test]
  fn hashes_lines_as_padded_djb2() {
    // "a" under djb2: 5381*33 + 97.
    assert_eq!(
      hash_diff_lines(&["a".into()]),
      format!("{:08x}", 5381 * 33 + 97)
    );
    assert_eq!(hash_diff_lines(&[]), "00001505");
  }

  #[test]
  fn draft_on_present_lines_takes_their_hunk_and_hash() {
    let hunks = parse_unified_diff(DIFF).hunks;
    let draft = draft_to_comment(&row(2, 3, "new", None), &hunks);
    assert!(!draft.outdated);
    assert_eq!(draft.comment.hunk_id, "src/a.rs#0");
    assert_eq!(draft.comment.line_content, ["+two", " three"]);
    assert_eq!(
      draft.line_hash,
      Some(hash_diff_lines(&["+two".into(), " three".into()]))
    );
  }

  #[test]
  fn draft_is_outdated_when_its_lines_changed_or_left() {
    let hunks = parse_unified_diff(DIFF).hunks;
    let changed = draft_to_comment(&row(2, 2, "new", Some("deadbeef")), &hunks);
    assert!(changed.outdated);
    assert_eq!(changed.comment.hunk_id, OUTDATED_HUNK);
    assert!(changed.comment.line_content.is_empty());
    let gone = draft_to_comment(&row(40, 41, "new", None), &hunks);
    assert!(gone.outdated);
    assert_eq!(gone.line_hash, None);
  }

  #[test]
  fn outdated_draft_shows_the_lines_it_was_written_on() {
    let hunks = parse_unified_diff(DIFF).hunks;
    let mut r = row(2, 2, "new", Some("deadbeef"));
    r.quoted_text = Some("-older\n+old".into());
    let stale = draft_to_comment(&r, &hunks);
    assert!(stale.outdated);
    assert_eq!(stale.comment.line_content, ["-older", "+old"]);
  }

  #[test]
  fn draft_reports_its_current_lines_to_store() {
    let hunks = parse_unified_diff(DIFF).hunks;
    let draft = draft_to_comment(&row(2, 3, "new", None), &hunks);
    assert_eq!(draft.quoted_text.as_deref(), Some("+two\n three"));
    assert_eq!(
      draft_to_comment(&row(40, 41, "new", None), &hunks).quoted_text,
      None
    );
  }

  #[test]
  fn draft_follows_lines_that_moved_unchanged() {
    let hunks = parse_unified_diff(DIFF).hunks;
    let hash = hash_diff_lines(&["+two".into(), " three".into()]);
    let moved = draft_to_comment(&row(7, 8, "new", Some(&hash)), &hunks);
    assert!(!moved.outdated);
    assert_eq!(moved.comment.hunk_id, "src/a.rs#0");
    assert_eq!((moved.comment.start_line, moved.comment.end_line), (2, 3));
    assert_eq!(moved.comment.line_content, ["+two", " three"]);
  }

  #[test]
  fn moved_draft_takes_the_copy_nearest_its_old_line() {
    let hunks = parse_unified_diff(
      "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -0,0 +1,9 @@\n+x\n+a\n+b\n+c\n+d\n+e\n+f\n+x\n+g\n",
    )
    .hunks;
    let hash = hash_diff_lines(&["+x".into()]);
    let moved = draft_to_comment(&row(7, 7, "new", Some(&hash)), &hunks);
    assert_eq!(moved.comment.start_line, 8);
  }

  #[test]
  fn window_hashes_equal_the_hash_of_the_joined_lines() {
    let lines: Vec<String> = [
      "",
      "a",
      "+x\u{e9}",
      "\u{1f600} smile",
      " ",
      "same",
      "same",
      "z",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    for size in 1..=lines.len() {
      let hashes = window_hashes(lines.iter().map(String::as_str), size);
      assert_eq!(hashes.len(), lines.len() - size + 1);
      for (at, hash) in hashes.iter().enumerate() {
        assert_eq!(
          format!("{hash:08x}"),
          hash_diff_lines(&lines[at..at + size]),
          "size {size} at {at}"
        );
      }
    }
  }

  #[test]
  fn many_moved_drafts_each_follow_their_own_lines() {
    let body: String = (0..200).map(|i| format!("+line {i}\n")).collect();
    let hunks = parse_unified_diff(&format!(
      "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -0,0 +1,200 @@\n{body}"
    ))
    .hunks;
    let rows: Vec<_> = (0..20)
      .map(|i| {
        let hash = hash_diff_lines(&[format!("+line {}", i * 10), format!("+line {}", i * 10 + 1)]);
        let mut r = row(500, 501, "new", Some(&hash));
        r.file_path = "f".into();
        r.id = format!("d{i}");
        r
      })
      .collect();
    for (i, draft) in drafts(&rows, &hunks).iter().enumerate() {
      assert!(!draft.outdated, "draft {i}");
      assert_eq!(draft.comment.start_line as usize, i * 10 + 1);
    }
  }

  #[test]
  fn resolved_drafts_are_not_shown() {
    let hunks = parse_unified_diff(DIFF).hunks;
    let mut done = row(2, 2, "new", None);
    done.status = "resolved".into();
    assert!(drafts(&[done, row(1, 1, "new", None)], &hunks).len() == 1);
  }

  #[test]
  fn suggestions_are_fenced_into_the_text() {
    let hunks = parse_unified_diff(DIFF).hunks;
    let mut r = row(2, 2, "new", None);
    r.suggested_replacement = Some("2".into());
    assert_eq!(
      draft_to_comment(&r, &hunks).comment.text,
      "Nit\n\n```suggestion\n2\n```"
    );
  }

  #[test]
  fn comments_become_inline_review_comments() {
    let hunks = parse_unified_diff(DIFF).hunks;
    let one = draft_to_comment(&row(2, 3, "new", None), &hunks).comment;
    let mut file = one.clone();
    file.hunk_id = FILE_HUNK.into();
    file.text = "Split this".into();
    let (inline, body) = comments_to_review(&[one, file]);
    assert_eq!(inline.len(), 1);
    assert_eq!(inline[0].line, 3);
    assert_eq!(inline[0].start_line, Some(2));
    assert_eq!(inline[0].side, "RIGHT");
    assert_eq!(body, "**src/a.rs**: Split this");
  }

  #[test]
  fn request_changes_needs_a_summary_or_a_comment() {
    assert!(can_submit(Event::Comment, "", 0));
    assert!(!can_submit(Event::RequestChanges, "  ", 0));
    assert!(can_submit(Event::RequestChanges, "Fix", 0));
    assert!(can_submit(Event::RequestChanges, "", 1));
  }

  #[test]
  fn finish_text_counts_postable_and_outdated_drafts() {
    assert_eq!(
      finish_text(1, 0),
      "1 pending comment will be posted to GitHub."
    );
    assert_eq!(
      finish_text(2, 1),
      "2 pending comments will be posted to GitHub. 1 outdated draft stays as a draft."
    );
    assert_eq!(
      finish_text(0, 3),
      "0 pending comments will be posted to GitHub. 3 outdated drafts stay as drafts."
    );
  }

  #[test]
  fn summary_tells_the_agent_where_the_diff_is() {
    let s = review_summary("o/r", 5, "Fix", 1);
    assert!(s.starts_with("GitHub pull request #5 \"Fix\" (1 changed file). The PR"));
    assert!(s.contains("`gh pr diff 5 --repo o/r`"));
  }
}
