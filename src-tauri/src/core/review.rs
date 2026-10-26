//! A review of a diff: line selection, comments on lines and files,
//! conflict resolution notes, the pending review a host keeps, and the
//! markdown an agent gets (`useLineSelection`, `useComments`, `useReview`
//! and `lib/review.ts`). Shared by the desktop and mobile apps.

use std::{ops::Range, sync::LazyLock};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{jj::JjDiffHunk, local_db::PendingReview};

#[expect(clippy::expect_used, reason = "constant pattern")]
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

pub fn numbered(hunk: &JjDiffHunk) -> impl Iterator<Item = (Option<u32>, Option<u32>)> + '_ {
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

/// A place in a diff's code: its line, and a byte offset into the line's
/// text after the `+`/`-`/` ` marker.
pub type TextPoint = (LineRef, usize);

/// The two points in reading order.
fn ordered(a: TextPoint, b: TextPoint) -> (TextPoint, TextPoint) {
  if (a.0 .0, a.0 .1, a.1) <= (b.0 .0, b.0 .1, b.1) {
    (a, b)
  } else {
    (b, a)
  }
}

/// A diff line's code, after its marker.
fn code_of(hunks: &[JjDiffHunk], (h, l): LineRef) -> &str {
  hunks
    .get(h)
    .and_then(|h| h.lines.get(l))
    .and_then(|t| t.get(1..))
    .unwrap_or("")
}

/// The bytes of line `at`'s code that the selection from `a` to `b` covers,
/// `None` when it covers none of the line.
pub fn selected_range(
  hunks: &[JjDiffHunk],
  a: TextPoint,
  b: TextPoint,
  at: LineRef,
) -> Option<Range<usize>> {
  let (start, end) = ordered(a, b);
  if !selection(hunks, start.0, end.0).contains(&at) {
    return None;
  }
  let t = code_of(hunks, at);
  let clamp = |i: usize| {
    let mut i = i.min(t.len());
    while !t.is_char_boundary(i) {
      i -= 1;
    }
    i
  };
  let from = if at == start.0 { clamp(start.1) } else { 0 };
  let to = if at == end.0 { clamp(end.1) } else { t.len() };
  Some(from..to.max(from))
}

/// The code between two points, in either order, one line per diff line:
/// what copying a text selection gives, without markers or line numbers.
pub fn selected_text(hunks: &[JjDiffHunk], a: TextPoint, b: TextPoint) -> String {
  let (start, end) = ordered(a, b);
  selection(hunks, start.0, end.0)
    .into_iter()
    .map(|at| {
      let r = selected_range(hunks, a, b, at).unwrap_or(0..0);
      code_of(hunks, at).get(r).unwrap_or("")
    })
    .collect::<Vec<_>>()
    .join("\n")
}

/// The word around byte `at` for a double-click: a run of identifier
/// characters, a run of whitespace, or else the single character there.
pub fn word_at(text: &str, at: usize) -> Range<usize> {
  let Some((i, c)) = text.char_indices().find(|(i, c)| at < i + c.len_utf8()) else {
    return text.len()..text.len();
  };
  let class = |c: char| {
    if c.is_alphanumeric() || c == '_' {
      1
    } else if c.is_whitespace() {
      2
    } else {
      0
    }
  };
  let kind = class(c);
  if kind == 0 {
    return i..i + c.len_utf8();
  }
  let start = text
    .get(..i)
    .unwrap_or("")
    .char_indices()
    .rev()
    .take_while(|(_, c)| class(*c) == kind)
    .last()
    .map_or(i, |(j, _)| j);
  let end = text
    .get(i..)
    .unwrap_or("")
    .char_indices()
    .find(|(_, c)| class(*c) != kind)
    .map_or(text.len(), |(j, _)| i + j);
  start..end
}

/// `L12` or `L12-14`, as the comment box labels its lines.
pub fn line_label(start: u32, end: u32) -> String {
  if start == end {
    format!("L{start}")
  } else {
    format!("L{start}-{end}")
  }
}

/// `FILE_COMMENT_HUNK_ID`: a comment on the whole file.
pub const FILE_HUNK: &str = "__file__";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
  Old,
  New,
}

/// A pending comment, in the shape the backend stores (`ApiLineComment`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineComment {
  pub id: String,
  pub file_path: String,
  pub hunk_id: String,
  pub start_line: u32,
  pub end_line: u32,
  pub line_content: Vec<String>,
  pub text: String,
  pub created_at: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub line_side: Option<Side>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub source: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub github_author: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub github_avatar_url: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub github_comment_url: Option<String>,
}

impl LineComment {
  pub fn is_file(&self) -> bool {
    self.hunk_id == FILE_HUNK
  }
}

/// A conflict's resolution note (`ConflictCommentRecord`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictNote {
  pub id: String,
  pub conflict_id: String,
  pub file_path: String,
  pub conflict_number: usize,
  pub text: String,
  pub created_at: String,
}

/// Where the comment box is open, and what it will be about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
  pub file_path: String,
  /// The hunk's id, or `FILE_HUNK`.
  pub hunk_id: String,
  /// The box shows under this line; `None` for a file comment.
  pub at: Option<LineRef>,
  pub start_line: u32,
  pub end_line: u32,
  pub line_content: Vec<String>,
  pub side: Side,
  /// The GitHub review comment it quotes (`githubMeta`).
  pub quote: Option<Quote>,
}

/// A quoted GitHub review comment's author and link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
  pub author: String,
  pub avatar_url: Option<String>,
  pub url: String,
}

impl Pending {
  pub fn file(path: &str) -> Self {
    Self {
      file_path: path.into(),
      hunk_id: FILE_HUNK.into(),
      at: None,
      start_line: 0,
      end_line: 0,
      line_content: vec![],
      side: Side::New,
      quote: None,
    }
  }

  pub fn comment(&self, text: &str, id: String, now: String) -> LineComment {
    LineComment {
      id,
      file_path: self.file_path.clone(),
      hunk_id: self.hunk_id.clone(),
      start_line: self.start_line,
      end_line: self.end_line,
      line_content: self.line_content.clone(),
      text: text.into(),
      created_at: now,
      line_side: Some(self.side),
      source: self.quote.as_ref().map(|_| "github".into()),
      github_author: self.quote.as_ref().map(|q| q.author.clone()),
      github_avatar_url: self.quote.as_ref().and_then(|q| q.avatar_url.clone()),
      github_comment_url: self.quote.as_ref().map(|q| q.url.clone()),
    }
  }

  /// `CommentInput`'s `file:L{n}` label; none for a file comment.
  pub fn label(&self) -> Option<String> {
    (self.start_line > 0 && self.end_line > 0).then(|| {
      format!(
        "{}:{}",
        self.file_path,
        line_label(self.start_line, self.end_line)
      )
    })
  }
}

/// A line's number (`new ?? old ?? index+1`) and side.
pub fn line_at(hunk: &JjDiffHunk, l: usize) -> (u32, Side) {
  number_and_side(line_number(hunk, l), l)
}

/// `line_at` from line `l`'s `(old, new)` number.
pub fn number_and_side((old, new): (Option<u32>, Option<u32>), l: usize) -> (u32, Side) {
  let side = if old.is_some() && new.is_none() {
    Side::Old
  } else {
    Side::New
  };
  (new.or(old).unwrap_or(l as u32 + 1), side)
}

/// The + button on a line with no selection.
pub fn pending_for_line(path: &str, hunks: &[JjDiffHunk], at: LineRef) -> Option<Pending> {
  let hunk = hunks.get(at.0)?;
  let (number, side) = line_at(hunk, at.1);
  Some(Pending {
    file_path: path.into(),
    hunk_id: hunk.id.clone(),
    at: Some(at),
    start_line: number,
    end_line: number,
    line_content: vec![hunk.lines.get(at.1)?.clone()],
    side,
    quote: None,
  })
}

/// `handleAddCommentFromSelection`: `None` when the lines mix removed-only
/// lines with new ones, which one side cannot describe.
pub fn pending_for_selection(
  path: &str,
  hunks: &[JjDiffHunk],
  lines: &[LineRef],
) -> Option<Pending> {
  let (mut min, mut max) = (u32::MAX, 0);
  let (mut old, mut new) = (false, false);
  let mut content = Vec::new();
  for &(h, l) in lines {
    let hunk = hunks.get(h)?;
    let (number, side) = line_at(hunk, l);
    min = min.min(number);
    max = max.max(number);
    match side {
      Side::Old => old = true,
      Side::New => new = true,
    }
    content.push(hunk.lines.get(l)?.clone());
  }
  let &(h, l) = lines.last()?;
  if old && new {
    return None;
  }
  Some(Pending {
    file_path: path.into(),
    hunk_id: hunks[h].id.clone(),
    at: Some((h, l)),
    start_line: min,
    end_line: max,
    line_content: content,
    side: if old { Side::Old } else { Side::New },
    quote: None,
  })
}

/// `isCommentOutdated`: its hunk is gone, or no longer holds its lines.
/// `hunks` is `None` while the file is loading, when nothing is outdated.
pub fn is_outdated(comment: &LineComment, hunks: Option<&[JjDiffHunk]>) -> bool {
  if comment.is_file() {
    return false;
  }
  let Some(hunks) = hunks else {
    return false;
  };
  let Some(hunk) = hunks.iter().find(|h| h.id == comment.hunk_id) else {
    return true;
  };
  !numbered(hunk).enumerate().any(|(l, n)| {
    let (number, side) = number_and_side(n, l);
    (comment.start_line..=comment.end_line).contains(&number)
      && comment.line_side.is_none_or(|s| s == side)
  })
}

/// The comments drawn under line `l` of `hunk`: those that end on it.
pub fn comments_under<'a>(
  comments: &'a [LineComment],
  path: &str,
  hunk: &JjDiffHunk,
  (number, side): (u32, Side),
  outdated: &dyn Fn(&LineComment) -> bool,
) -> Vec<&'a LineComment> {
  let on_line: Vec<&LineComment> = comments
    .iter()
    .filter(|c| {
      c.file_path == path
        && c.hunk_id == hunk.id
        && (c.start_line..=c.end_line).contains(&number)
        && c.line_side.is_none_or(|s| s == side)
        && !outdated(c)
    })
    .collect();
  match on_line.first() {
    Some(first) if first.end_line == number => on_line,
    _ => vec![],
  }
}

/// Whether line `l` has any comment, for the gutter's icon.
pub fn line_has_comment(
  comments: &[LineComment],
  path: &str,
  hunk: &JjDiffHunk,
  (number, side): (u32, Side),
  outdated: &dyn Fn(&LineComment) -> bool,
) -> bool {
  comments.iter().any(|c| {
    c.file_path == path
      && c.hunk_id == hunk.id
      && (c.start_line..=c.end_line).contains(&number)
      && c.line_side.is_none_or(|s| s == side)
      && !outdated(c)
  })
}

/// `Line 3` or `Line 3-5`, on an outdated comment.
pub fn outdated_label(c: &LineComment) -> String {
  if c.start_line == c.end_line {
    format!("Line {}", c.start_line)
  } else {
    format!("Line {}-{}", c.start_line, c.end_line)
  }
}

/// The "Copy Outdated" markdown.
pub fn outdated_markdown(comments: &[&LineComment]) -> String {
  comments
    .iter()
    .map(|c| {
      let range = if c.start_line == c.end_line {
        c.start_line.to_string()
      } else {
        format!("{}-{}", c.start_line, c.end_line)
      };
      let mut out = format!("**{}:{range}**", c.file_path);
      if !c.line_content.is_empty() {
        out += &format!("\n```\n{}\n```\n", c.line_content.join("\n"));
      }
      out + &c.text
    })
    .collect::<Vec<_>>()
    .join("\n\n")
}

/// `formatReviewMarkdown`. `conflict_lines` gives a note's conflict's
/// first and last line, when its file is loaded.
pub fn review_markdown(
  comments: &[LineComment],
  summary: &str,
  notes: &[ConflictNote],
  conflict_lines: impl Fn(&ConflictNote) -> Option<(usize, usize)>,
  conflicted_files: &[String],
) -> String {
  let mut md = String::new();
  if !notes.is_empty() {
    md += "## Conflict Resolution\n\n";
    for note in notes.iter().filter(|n| !n.text.trim().is_empty()) {
      let mut header = format!("### {} - Conflict {}", note.file_path, note.conflict_number);
      if let Some((start, end)) = conflict_lines(note) {
        header += &format!(" (lines {start}-{end})");
      }
      md += &format!("{header}\n> {}\n\n", note.text);
    }
  }
  let summary = summary.trim();
  if comments.is_empty() && summary.is_empty() && conflicted_files.is_empty() {
    return md;
  }
  md += "## Code Review\n\n";
  if !conflicted_files.is_empty() {
    md += "### Conflicted Files\n\n";
    for path in conflicted_files {
      md += &format!("- {path}\n");
    }
    md += "\n";
  }
  if !summary.is_empty() {
    md += &format!("### Summary\n{summary}\n\n");
  }
  if comments.is_empty() {
    return md;
  }
  md += "### Comments\n\n";
  let mut files: Vec<&str> = Vec::new();
  for c in comments {
    if !files.contains(&c.file_path.as_str()) {
      files.push(&c.file_path);
    }
  }
  for path in files {
    for c in comments.iter().filter(|c| c.file_path == path) {
      if c.is_file() {
        md += &format!("{path}\n> {}\n\n", c.text);
        continue;
      }
      md += &if c.start_line == c.end_line {
        format!("{path}:{}\n", c.start_line)
      } else {
        format!("{path}:{}:{}\n", c.start_line, c.end_line)
      };
      if c.source.as_deref() == Some("github") {
        md += &format!(
          "> Quoting GitHub review comment by @{}:\n> {}\n\n",
          c.github_author.as_deref().unwrap_or_default(),
          c.line_content.join("\n> ")
        );
      } else {
        md += &format!("```\n{}\n```\n", c.line_content.join("\n"));
      }
      md += &format!("> {}\n\n", c.text);
    }
  }
  md
}

/// What the backend kept of a review.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stored {
  pub comments: Vec<LineComment>,
  pub summary: String,
  pub notes: Vec<ConflictNote>,
}

impl Stored {
  pub fn is_empty(&self) -> bool {
    self.comments.is_empty() && self.summary.trim().is_empty() && self.notes.is_empty()
  }

  pub fn parse(review: &PendingReview) -> Self {
    Self {
      comments: serde_json::from_str(&review.comments).unwrap_or_default(),
      summary: review.summary_text.clone().unwrap_or_default(),
      notes: review
        .conflict_comments
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default(),
    }
  }

  /// `save_pending_review`'s JSON arguments: comments, summary (trimmed,
  /// `None` when blank) and notes (`None` when there are none).
  pub fn to_save(&self) -> (String, Option<String>, Option<String>) {
    let summary = Some(self.summary.trim().to_string()).filter(|s| !s.is_empty());
    let notes =
      (!self.notes.is_empty()).then(|| serde_json::to_string(&self.notes).unwrap_or_default());
    (
      serde_json::to_string(&self.comments).unwrap_or_default(),
      summary,
      notes,
    )
  }
}

/// `{n} comment(s)`.
pub fn count_label(n: usize, word: &str) -> String {
  format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The stale banner's file list: the first three names, then `+n more`.
pub fn stale_names(paths: &[String]) -> String {
  let names: Vec<&str> = paths
    .iter()
    .take(3)
    .map(|p| p.rsplit('/').next().unwrap_or(p))
    .collect();
  let more = paths.len().saturating_sub(3);
  format!(
    "({}{})",
    names.join(", "),
    if more > 0 {
      format!(" +{more} more")
    } else {
      String::new()
    }
  )
}

/// The `file:N` or `file:a-b` the line menu copies.
pub fn line_location(pending: &Pending) -> String {
  if pending.start_line == pending.end_line {
    format!("{}:{}", pending.file_path, pending.start_line)
  } else {
    format!(
      "{}:{}-{}",
      pending.file_path, pending.start_line, pending.end_line
    )
  }
}

/// The diff line after (`down`) or before `at`, across hunks; `None` at
/// the file's end.
pub fn step_line(hunks: &[JjDiffHunk], (h, l): LineRef, down: bool) -> Option<LineRef> {
  if down {
    if l + 1 < hunks.get(h)?.lines.len() {
      return Some((h, l + 1));
    }
    (h + 1..hunks.len())
      .find(|&n| !hunks[n].lines.is_empty())
      .map(|n| (n, 0))
  } else if l > 0 {
    Some((h, l - 1))
  } else {
    (0..h.min(hunks.len()))
      .rev()
      .find(|&n| !hunks[n].lines.is_empty())
      .map(|n| (n, hunks[n].lines.len() - 1))
  }
}

/// Where the cursor enters a file: its first line going down, its last
/// going up.
pub fn edge_line(hunks: &[JjDiffHunk], down: bool) -> Option<LineRef> {
  if down {
    hunks
      .iter()
      .position(|h| !h.lines.is_empty())
      .map(|n| (n, 0))
  } else {
    hunks
      .iter()
      .rposition(|h| !h.lines.is_empty())
      .map(|n| (n, hunks[n].lines.len() - 1))
  }
}

/// The commented lines' text, without their diff markers.
pub fn commented_text(line_content: &[String]) -> Vec<String> {
  line_content
    .iter()
    .map(|l| l.get(1..).unwrap_or("").to_string())
    .collect()
}

/// The ```` ```suggestion ```` block "Suggest change" starts a comment with:
/// the lines as they are, to edit. Only lines on the new side can be
/// replaced.
pub fn suggestion_block(pending: &Pending) -> Option<String> {
  if pending.side != Side::New || pending.at.is_none() || pending.line_content.is_empty() {
    return None;
  }
  Some(format!(
    "```suggestion\n{}\n```\n",
    commented_text(&pending.line_content).join("\n")
  ))
}

/// A comment's text around its first suggestion block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
  pub before: String,
  pub suggested: String,
  pub after: String,
}

/// The first ```` ```suggestion ```` block of `text`, as GitHub reads it.
#[expect(
  clippy::string_slice,
  reason = "offsets come from finding ASCII fence markers and newlines"
)]
pub fn parse_suggestion(text: &str) -> Option<Suggestion> {
  const OPEN: &str = "```suggestion";
  let start = text.find(OPEN)?;
  let body_start = start + OPEN.len();
  let body_start = body_start + text[body_start..].find('\n')? + 1;
  let close = text[body_start..]
    .match_indices("```")
    .map(|(i, _)| body_start + i)
    .find(|&i| i == body_start || text[..i].ends_with('\n'))?;
  Some(Suggestion {
    before: text[..start].trim().to_string(),
    suggested: text[body_start..close]
      .strip_suffix('\n')
      .unwrap_or(&text[body_start..close])
      .to_string(),
    after: text[close + 3..].trim().to_string(),
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn text_hunks() -> Vec<JjDiffHunk> {
    vec![
      hunk(
        "a",
        "@@ -1,2 +1,2 @@",
        &[" let a = 1;", "-let b = 2;", "+let b = 3;"],
      ),
      hunk("b", "@@ -9 +9 @@", &["+fn main() {}"]),
    ]
  }

  #[test]
  fn selected_text_within_one_line_drops_the_marker() {
    let h = text_hunks();
    assert_eq!(selected_text(&h, ((0, 0), 4), ((0, 0), 9)), "a = 1");
  }

  #[test]
  fn selected_text_spans_lines_and_hunks_in_either_order() {
    let h = text_hunks();
    let want = "= 1;\nlet b = 2;\nlet b = 3;\nfn main";
    assert_eq!(selected_text(&h, ((0, 0), 6), ((1, 0), 7)), want);
    assert_eq!(selected_text(&h, ((1, 0), 7), ((0, 0), 6)), want);
  }

  #[test]
  fn selected_text_clamps_offsets_past_the_line_end() {
    let h = text_hunks();
    assert_eq!(selected_text(&h, ((0, 2), 4), ((0, 2), 99)), "b = 3;");
  }

  #[test]
  fn selected_range_gives_each_line_its_part() {
    let h = text_hunks();
    let (a, b) = (((0, 0), 6), ((0, 2), 3));
    assert_eq!(selected_range(&h, b, a, (0, 0)), Some(6..10));
    assert_eq!(selected_range(&h, a, b, (0, 1)), Some(0..10));
    assert_eq!(selected_range(&h, a, b, (0, 2)), Some(0..3));
    assert_eq!(selected_range(&h, a, b, (1, 0)), None);
  }

  #[test]
  fn word_at_finds_identifiers_and_single_symbols() {
    assert_eq!(word_at("let foo_bar = 1;", 6), 4..11);
    assert_eq!(word_at("let foo_bar = 1;", 12), 12..13);
    assert_eq!(word_at("a  b", 2), 1..3);
    assert_eq!(word_at("", 0), 0..0);
  }

  fn hunk(id: &str, header: &str, lines: &[&str]) -> JjDiffHunk {
    serde_json::from_value(serde_json::json!({
      "id": id, "header": header, "lines": lines, "patch": ""
    }))
    .unwrap()
  }

  fn comment(hunk_id: &str, start: u32, end: u32, side: Side) -> LineComment {
    Pending {
      file_path: "a.rs".into(),
      hunk_id: hunk_id.into(),
      at: None,
      start_line: start,
      end_line: end,
      line_content: vec!["+x".into()],
      side,
      quote: None,
    }
    .comment("fix", "1".into(), "now".into())
  }

  #[test]
  fn a_line_comment_takes_its_number_and_side() {
    let hunks = [hunk("h", "@@ -1,2 +1,2 @@", &[" a", "-b", "+c"])];
    let p = pending_for_line("a.rs", &hunks, (0, 1)).unwrap();
    assert_eq!((p.start_line, p.side), (2, Side::Old));
    let p = pending_for_line("a.rs", &hunks, (0, 2)).unwrap();
    assert_eq!((p.start_line, p.side), (2, Side::New));
    assert_eq!(p.label().as_deref(), Some("a.rs:L2"));
  }

  #[test]
  fn a_selection_spans_its_lines_but_not_both_sides() {
    let hunks = [hunk("h", "@@ -1,2 +1,3 @@", &[" a", "+b", "+c", "-d"])];
    let p = pending_for_selection("a.rs", &hunks, &[(0, 0), (0, 1), (0, 2)]).unwrap();
    assert_eq!((p.start_line, p.end_line, p.at), (1, 3, Some((0, 2))));
    assert_eq!(p.line_content, [" a", "+b", "+c"]);
    assert_eq!(
      pending_for_selection("a.rs", &hunks, &[(0, 2), (0, 3)]),
      None
    );
  }

  #[test]
  fn comments_go_outdated_with_their_lines() {
    let hunks = [hunk("h", "@@ -1,1 +1,2 @@", &[" a", "+b"])];
    assert!(!is_outdated(&comment("h", 2, 2, Side::New), Some(&hunks)));
    assert!(is_outdated(&comment("h", 9, 9, Side::New), Some(&hunks)));
    assert!(is_outdated(&comment("gone", 2, 2, Side::New), Some(&hunks)));
    assert!(!is_outdated(&comment("gone", 2, 2, Side::New), None));
  }

  #[test]
  fn a_comment_shows_under_its_last_line() {
    let hunks = [hunk("h", "@@ -1,1 +1,3 @@", &[" a", "+b", "+c"])];
    let comments = [comment("h", 1, 3, Side::New)];
    let at = |l| line_at(&hunks[0], l);
    let outdated = |c: &LineComment| is_outdated(c, Some(&hunks));
    assert!(comments_under(&comments, "a.rs", &hunks[0], at(0), &outdated).is_empty());
    assert_eq!(
      comments_under(&comments, "a.rs", &hunks[0], at(2), &outdated).len(),
      1
    );
    assert!(line_has_comment(
      &comments,
      "a.rs",
      &hunks[0],
      at(1),
      &outdated
    ));
  }

  #[test]
  fn review_markdown_matches_react() {
    let mut range = comment("h", 2, 3, Side::New);
    range.line_content = vec!["+b".into(), "+c".into()];
    let mut file = Pending::file("b.rs").comment("whole", "2".into(), "now".into());
    file.text = "whole".into();
    let md = review_markdown(&[range, file], " tidy ", &[], |_| None, &["c.rs".into()]);
    assert_eq!(
      md,
      "## Code Review\n\n### Conflicted Files\n\n- c.rs\n\n### Summary\ntidy\n\n### Comments\n\n\
       a.rs:2:3\n```\n+b\n+c\n```\n> fix\n\nb.rs\n> whole\n\n"
    );
  }

  #[test]
  fn stored_reviews_round_trip() {
    let stored = Stored {
      comments: vec![comment("h", 1, 1, Side::New)],
      summary: " s ".into(),
      notes: vec![],
    };
    let (comments, summary, notes) = stored.to_save();
    assert_eq!(summary.as_deref(), Some("s"));
    assert_eq!(notes, None);
    let review = PendingReview {
      id: 1,
      workspace_id: 7,
      comments,
      viewed_files: None,
      summary_text: summary,
      conflict_comments: notes,
      created_at: String::new(),
      updated_at: String::new(),
    };
    let back = Stored::parse(&review);
    assert_eq!(back.comments, stored.comments);
    assert_eq!(back.summary, "s");
  }

  #[test]
  fn the_stale_banner_names_three_files() {
    let paths: Vec<String> = ["a/x.rs", "y.rs", "z.rs", "w.rs"]
      .iter()
      .map(|s| s.to_string())
      .collect();
    assert_eq!(stale_names(&paths), "(x.rs, y.rs, z.rs +1 more)");
  }

  #[test]
  fn the_line_cursor_steps_across_hunks() {
    let hunks = vec![
      hunk("a", "@@ -1,2 +1,2 @@", &[" a", "-b"]),
      hunk("e", "@@ -9,0 +9,0 @@", &[]),
      hunk("c", "@@ -20,1 +20,1 @@", &["+c"]),
    ];
    assert_eq!(step_line(&hunks, (0, 0), true), Some((0, 1)));
    assert_eq!(step_line(&hunks, (0, 1), true), Some((2, 0)));
    assert_eq!(step_line(&hunks, (2, 0), true), None);
    assert_eq!(step_line(&hunks, (2, 0), false), Some((0, 1)));
    assert_eq!(step_line(&hunks, (0, 0), false), None);
    assert_eq!(edge_line(&hunks, true), Some((0, 0)));
    assert_eq!(edge_line(&hunks, false), Some((2, 0)));
    assert_eq!(edge_line(&[], true), None);
  }

  #[test]
  fn suggest_change_quotes_the_new_side_lines() {
    let hunks = vec![hunk("h", "@@ -1,2 +1,3 @@", &[" a", "+b", "+c"])];
    let pending = pending_for_selection("a.rs", &hunks, &[(0, 1), (0, 2)]).unwrap();
    assert_eq!(
      suggestion_block(&pending).as_deref(),
      Some("```suggestion\nb\nc\n```\n")
    );
    let removed = vec![hunk("h", "@@ -1,1 +1,0 @@", &["-gone"])];
    let pending = pending_for_line("a.rs", &removed, (0, 0)).unwrap();
    assert_eq!(
      suggestion_block(&pending),
      None,
      "old lines cannot be replaced"
    );
    assert_eq!(suggestion_block(&Pending::file("a.rs")), None);
  }

  #[test]
  fn reads_the_suggestion_out_of_a_comment() {
    let s = parse_suggestion("Rename it\n```suggestion\nlet x = 2;\n```\nthanks").unwrap();
    assert_eq!(s.before, "Rename it");
    assert_eq!(s.suggested, "let x = 2;");
    assert_eq!(s.after, "thanks");
    let empty = parse_suggestion("```suggestion\n```").unwrap();
    assert_eq!(empty.suggested, "", "an empty block deletes the lines");
    assert_eq!(parse_suggestion("no block"), None);
    assert_eq!(parse_suggestion("```suggestion\nunclosed"), None);
  }
}
