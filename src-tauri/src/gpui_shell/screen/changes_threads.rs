//! What others left on the Changes tab's diff: the pull request's GitHub
//! review threads (`useGithubReviewThreads`, `placeGithubReviewThreads`)
//! and the review agent's local comments (`useAgentReviewComments`), with
//! a suggestion drawn as a diff (`suggestionDiff.ts`).

use std::collections::HashMap;

use super::{
  changes_review::{number_and_side, Side},
  commits_data::{line_numbers, numbered},
};
use crate::{github::GhReviewThread, jj::JjDiffHunk, local_db::AgentReviewComment};

/// `AGENT_REVIEW_TARGET_WORKSPACE_DIFF`.
pub const WORKSPACE_DIFF: &str = "workspace_diff";

/// A thread's or comment's line: file, hunk id, number and side.
pub type LineKey = (String, String, u32, Side);

/// Threads anchored on a line of the diff, and those that are not, as
/// indices into the threads placed.
#[derive(Default, Clone)]
pub struct Placed {
  pub by_line: HashMap<LineKey, Vec<usize>>,
  pub unplaced: HashMap<String, Vec<usize>>,
}

/// The first hunk of a file that has a line, by number and side.
type LineIndex = HashMap<(u32, Side), usize>;

fn line_index(hunks: &[JjDiffHunk]) -> LineIndex {
  let mut index = LineIndex::new();
  for (h, hunk) in hunks.iter().enumerate() {
    for (l, n) in numbered(hunk).enumerate() {
      index.entry(number_and_side(n, l)).or_insert(h);
    }
  }
  index
}

/// `placeGithubReviewThreads`: a thread sits on the first hunk that has
/// its line on its side, from the working copy's hunks then the committed
/// ones; GitHub's `is_outdated` is ignored.
pub fn place_threads(
  threads: &[GhReviewThread],
  maps: &[&HashMap<String, &[JjDiffHunk]>],
) -> Placed {
  let mut placed = Placed::default();
  // A file's lines are numbered once for all the threads on it.
  let mut indexes: HashMap<(usize, &str), LineIndex> = HashMap::new();
  for (i, thread) in threads.iter().enumerate() {
    match place(thread, maps, &mut indexes) {
      Some(key) => placed.by_line.entry(key).or_default().push(i),
      None => placed
        .unplaced
        .entry(thread.path.clone())
        .or_default()
        .push(i),
    }
  }
  placed
}

fn place<'a>(
  thread: &'a GhReviewThread,
  maps: &[&HashMap<String, &[JjDiffHunk]>],
  indexes: &mut HashMap<(usize, &'a str), LineIndex>,
) -> Option<LineKey> {
  let line = u32::try_from(thread.line?).ok()?;
  let side = if thread.diff_side == "LEFT" {
    Side::Old
  } else {
    Side::New
  };
  for (m, map) in maps.iter().enumerate() {
    let Some(hunks) = map.get(&thread.path) else {
      continue;
    };
    let index = indexes
      .entry((m, thread.path.as_str()))
      .or_insert_with(|| line_index(hunks));
    if let Some(&h) = index.get(&(line, side)) {
      return Some((thread.path.clone(), hunks[h].id.clone(), line, side));
    }
  }
  None
}

/// Open agent comments by the last line of their range and side, as
/// indices into `comments`.
pub fn agent_by_line(comments: &[AgentReviewComment]) -> HashMap<(String, u32, Side), Vec<usize>> {
  let mut out: HashMap<_, Vec<_>> = HashMap::new();
  for (i, c) in comments
    .iter()
    .enumerate()
    .filter(|(_, c)| c.status != "resolved")
  {
    let side = if c.side.as_deref() == Some("old") {
      Side::Old
    } else {
      Side::New
    };
    let Ok(end) = u32::try_from(c.end_line) else {
      continue;
    };
    out
      .entry((c.file_path.clone(), end, side))
      .or_default()
      .push(i);
  }
  out
}

/// `originalTextForComment`: the range's current text from `hunk`, or
/// `None` when a line of it is not there.
pub fn original_text(comment: &AgentReviewComment, hunk: &JjDiffHunk) -> Option<String> {
  let old_side = comment.side.as_deref() == Some("old");
  let numbers = line_numbers(hunk);
  let text_of = |n: i64| {
    hunk
      .lines
      .iter()
      .zip(&numbers)
      .find_map(|(line, (old, new))| {
        let at = if old_side { *old } else { *new };
        (at.map(i64::from) == Some(n)).then(|| line.get(1..).unwrap_or("").to_string())
      })
  };
  (comment.start_line..=comment.end_line)
    .map(text_of)
    .collect::<Option<Vec<_>>>()
    .map(|lines| lines.join("\n"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
  Unchanged,
  Removed,
  Added,
}

/// One run of a suggestion row; `changed` marks the words that differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
  pub text: String,
  pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
  pub kind: RowKind,
  pub segments: Vec<Segment>,
}

fn plain(kind: RowKind, text: &str) -> Row {
  Row {
    kind,
    segments: vec![Segment {
      text: text.into(),
      changed: kind != RowKind::Unchanged,
    }],
  }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChangeTag {
  Equal,
  Delete,
  Insert,
}

/// A longest-common-subsequence diff of two token lists; suggestions are a
/// few lines, so the quadratic table is fine.
fn diff_tokens<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(ChangeTag, &'a str)> {
  let (n, m) = (old.len(), new.len());
  let mut lcs = vec![vec![0usize; m + 1]; n + 1];
  for i in (0..n).rev() {
    for j in (0..m).rev() {
      lcs[i][j] = if old[i] == new[j] {
        lcs[i + 1][j + 1] + 1
      } else {
        lcs[i + 1][j].max(lcs[i][j + 1])
      };
    }
  }
  let (mut i, mut j, mut out) = (0, 0, Vec::new());
  while i < n || j < m {
    if i < n && j < m && old[i] == new[j] {
      out.push((ChangeTag::Equal, old[i]));
      i += 1;
      j += 1;
    } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
      out.push((ChangeTag::Insert, new[j]));
      j += 1;
    } else {
      out.push((ChangeTag::Delete, old[i]));
      i += 1;
    }
  }
  // Deletions before insertions within each changed run, as `diff` reports.
  let mut k = 0;
  while k < out.len() {
    if out[k].0 == ChangeTag::Equal {
      k += 1;
      continue;
    }
    let start = k;
    while k < out.len() && out[k].0 != ChangeTag::Equal {
      k += 1;
    }
    out[start..k].sort_by_key(|(tag, _)| *tag != ChangeTag::Delete);
  }
  out
}

fn split_lines(text: &str) -> Vec<&str> {
  let mut lines: Vec<&str> = text.split('\n').collect();
  if lines.len() > 1 && lines.last() == Some(&"") {
    lines.pop();
  }
  lines
}

/// A removed and an added line with the characters they changed marked,
/// as the diff view marks them.
#[expect(clippy::string_slice, reason = "ranges come from char_indices()")]
fn char_rows(removed: &str, added: &str) -> Option<[Row; 2]> {
  let (old, new) = super::intraline::line_marks(removed, added)?;
  let segments = |text: &str, marks: &[std::ops::Range<usize>]| {
    let mut out = Vec::new();
    let mut at = 0;
    for r in marks {
      if r.start > at {
        out.push(Segment {
          text: text[at..r.start].to_string(),
          changed: false,
        });
      }
      out.push(Segment {
        text: text[r.clone()].to_string(),
        changed: true,
      });
      at = r.end;
    }
    if at < text.len() || out.is_empty() {
      out.push(Segment {
        text: text[at..].to_string(),
        changed: false,
      });
    }
    out
  };
  Some([
    Row {
      kind: RowKind::Removed,
      segments: segments(removed, &old),
    },
    Row {
      kind: RowKind::Added,
      segments: segments(added, &new),
    },
  ])
}

/// `buildSuggestionDiff`: `suggested` as a +/- diff against `original`,
/// replaced lines paired with their changed words marked; `None` when
/// there is nothing to compare against or nothing changed.
pub fn suggestion_diff(original: Option<&str>, suggested: &str) -> Option<Vec<Row>> {
  let original = original.filter(|o| !o.is_empty())?;
  if original == suggested {
    return None;
  }
  // Blocks of consecutive changes, as `diffLines` reports them.
  let old: Vec<&str> = original.split_inclusive('\n').collect();
  let new: Vec<&str> = suggested.split_inclusive('\n').collect();
  let mut parts: Vec<(ChangeTag, String)> = Vec::new();
  for (change, value) in diff_tokens(&old, &new) {
    match parts.last_mut() {
      Some((tag, text)) if *tag == change => text.push_str(value),
      _ => parts.push((change, value.to_string())),
    }
  }
  let mut rows = Vec::new();
  let mut i = 0;
  while i < parts.len() {
    let (tag, text) = &parts[i];
    let lines = split_lines(text);
    match tag {
      ChangeTag::Equal => rows.extend(lines.iter().map(|l| plain(RowKind::Unchanged, l))),
      ChangeTag::Delete => {
        let added = parts
          .get(i + 1)
          .filter(|(t, _)| *t == ChangeTag::Insert)
          .map(|(_, t)| split_lines(t));
        match added {
          // Lines pair up by place in the block, as in the diff view.
          Some(added) => {
            for (old, new) in lines.iter().zip(&added) {
              match char_rows(old, new) {
                Some(pair) => rows.extend(pair),
                None => {
                  rows.push(plain(RowKind::Removed, old));
                  rows.push(plain(RowKind::Added, new));
                }
              }
            }
            let paired = lines.len().min(added.len());
            rows.extend(lines[paired..].iter().map(|l| plain(RowKind::Removed, l)));
            rows.extend(added[paired..].iter().map(|l| plain(RowKind::Added, l)));
            i += 1;
          }
          None => rows.extend(lines.iter().map(|l| plain(RowKind::Removed, l))),
        }
      }
      ChangeTag::Insert => rows.extend(lines.iter().map(|l| plain(RowKind::Added, l))),
    }
    i += 1;
  }
  (!rows.is_empty()).then_some(rows)
}

/// `formatGithubCommentDate`: "Mar 4", with the year when not this one.
pub fn comment_date(iso: &str, this_year: i32) -> String {
  use chrono::Datelike;
  match chrono::DateTime::parse_from_rfc3339(iso) {
    Ok(date) if date.year() == this_year => date.format("%b %-d").to_string(),
    Ok(date) => date.format("%b %-d, %Y").to_string(),
    Err(_) => iso.to_string(),
  }
}

/// A collapsed thread's one-line preview of its first comment, with
/// shortcodes as emoji.
pub fn preview(body: &str) -> String {
  super::code_data::emojify(body)
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::github::{GhAuthor, GhReviewComment};

  fn hunk(id: &str, header: &str, lines: &[&str]) -> JjDiffHunk {
    serde_json::from_value(serde_json::json!({
      "id": id, "header": header, "lines": lines, "patch": ""
    }))
    .expect("hunk")
  }

  fn thread(path: &str, line: Option<i64>, side: &str) -> GhReviewThread {
    GhReviewThread {
      id: format!("{path}-{line:?}-{side}"),
      is_resolved: false,
      is_outdated: false,
      path: path.into(),
      line,
      diff_side: side.into(),
      comments: vec![GhReviewComment {
        id: "c".into(),
        body: "Hm".into(),
        author: GhAuthor {
          login: "zn".into(),
          avatar_url: None,
        },
        created_at: "2026-03-04T00:00:00Z".into(),
        diff_hunk: String::new(),
        url: String::new(),
        reactions: Vec::new(),
      }],
    }
  }

  fn agent(
    path: &str,
    start: i64,
    end: i64,
    side: Option<&str>,
    status: &str,
  ) -> AgentReviewComment {
    serde_json::from_value(serde_json::json!({
      "id": format!("{path}{start}{end}"), "repo_path": "/r", "target_type": WORKSPACE_DIFF,
      "target_id": "7", "file_path": path, "hunk_id": null, "start_line": start,
      "end_line": end, "side": side, "comment_text": "Fix", "suggested_replacement": null,
      "status": status, "source": "local-agent", "created_at": "", "resolved_at": null
    }))
    .expect("agent comment")
  }

  #[test]
  fn threads_sit_on_their_line_and_side() {
    let hunks = [hunk("h1", "@@ -1,2 +1,2 @@", &[" a", "-b", "+c"])];
    let working = HashMap::from([("a.rs".to_string(), &hunks[..])]);
    let committed = HashMap::new();
    let threads = [
      thread("a.rs", Some(2), "RIGHT"),
      thread("a.rs", Some(2), "LEFT"),
      thread("a.rs", Some(9), "RIGHT"),
      thread("z.rs", None, "RIGHT"),
    ];
    let placed = place_threads(&threads, &[&working, &committed]);
    let at = |n: u32, side: Side| {
      placed
        .by_line
        .get(&("a.rs".into(), "h1".into(), n, side))
        .map_or(0, Vec::len)
    };
    assert_eq!(at(2, Side::New), 1);
    assert_eq!(at(2, Side::Old), 1);
    assert_eq!(placed.unplaced["a.rs"].len(), 1);
    assert_eq!(placed.unplaced["z.rs"].len(), 1);
  }

  #[test]
  fn agent_comments_sit_on_their_last_line_unless_resolved() {
    let by_line = agent_by_line(&[
      agent("a.rs", 1, 3, None, "open"),
      agent("a.rs", 2, 2, Some("old"), "open"),
      agent("a.rs", 4, 4, None, "resolved"),
    ]);
    assert!(by_line.contains_key(&("a.rs".into(), 3, Side::New)));
    assert!(by_line.contains_key(&("a.rs".into(), 2, Side::Old)));
    assert_eq!(by_line.len(), 2);
  }

  #[test]
  fn original_text_reads_the_range_from_the_hunk() {
    let h = hunk("h", "@@ -1,2 +1,3 @@", &[" a", "-b", "+c", "+d"]);
    assert_eq!(
      original_text(&agent("a.rs", 2, 3, None, "open"), &h).as_deref(),
      Some("c\nd")
    );
    assert_eq!(
      original_text(&agent("a.rs", 2, 2, Some("old"), "open"), &h).as_deref(),
      Some("b")
    );
    assert_eq!(original_text(&agent("a.rs", 3, 5, None, "open"), &h), None);
  }

  #[test]
  fn suggestions_mark_the_characters_that_changed() {
    let rows = suggestion_diff(Some("let a = 1;\nkeep"), "let a = 2;\nkeep").expect("rows");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].kind, RowKind::Removed);
    assert_eq!(rows[1].kind, RowKind::Added);
    let changed: Vec<&str> = rows[1]
      .segments
      .iter()
      .filter(|s| s.changed)
      .map(|s| s.text.as_str())
      .collect();
    assert_eq!(changed, vec!["2"]);
    assert_eq!(rows[2], plain(RowKind::Unchanged, "keep"));
  }

  fn changed(row: &Row) -> Vec<&str> {
    row
      .segments
      .iter()
      .filter(|s| s.changed)
      .map(|s| s.text.as_str())
      .collect()
  }

  #[test]
  fn suggestions_mark_characters_inside_a_word() {
    let rows = suggestion_diff(
      Some("defmodule Logflare.ShutdownDrain do"),
      "defmodule Logflare.ShutdownDrainWorker do",
    )
    .expect("rows");
    assert_eq!(rows.len(), 2);
    assert!(changed(&rows[0]).is_empty(), "nothing was removed");
    assert_eq!(changed(&rows[1]), vec!["Worker"]);
  }

  #[test]
  fn suggestions_mark_whitespace_and_unicode_changes() {
    let rows = suggestion_diff(Some("a  = 1"), "a = 1").expect("rows");
    assert_eq!(changed(&rows[0]), vec![" "]);
    let rows = suggestion_diff(Some("caf\u{e9} au lait"), "caf\u{e8} au lait").expect("rows");
    assert_eq!(changed(&rows[0]), vec!["\u{e9}"]);
    assert_eq!(changed(&rows[1]), vec!["\u{e8}"]);
  }

  #[test]
  fn suggestions_pair_lines_of_uneven_blocks_in_place() {
    let rows =
      suggestion_diff(Some("let a = 1;\nlet b = 2;\nend"), "let a = 3;\nend").expect("rows");
    assert_eq!(rows.len(), 4);
    assert_eq!(changed(&rows[0]), vec!["1"]);
    assert_eq!(changed(&rows[1]), vec!["3"]);
    assert_eq!(rows[2], plain(RowKind::Removed, "let b = 2;"));
    assert_eq!(rows[3], plain(RowKind::Unchanged, "end"));
  }

  #[test]
  fn rewritten_lines_are_a_plain_pair() {
    let rows = suggestion_diff(Some("alpha beta"), "gamma delta").expect("rows");
    assert_eq!(
      rows,
      vec![
        plain(RowKind::Removed, "alpha beta"),
        plain(RowKind::Added, "gamma delta")
      ]
    );
    assert_eq!(suggestion_diff(None, "x"), None);
    assert_eq!(suggestion_diff(Some("x"), "x"), None);
  }

  #[test]
  fn dates_drop_this_year() {
    assert_eq!(comment_date("2026-03-04T10:00:00Z", 2026), "Mar 4");
    assert_eq!(comment_date("2025-12-31T10:00:00Z", 2026), "Dec 31, 2025");
    assert_eq!(preview(" a\n  b "), "a b");
  }
}
