//! What others left on the Changes tab's diff: the pull request's GitHub
//! review threads (`useGithubReviewThreads`, `placeGithubReviewThreads`)
//! and the review agent's local comments (`useAgentReviewComments`), with
//! a suggestion drawn as a diff (`suggestionDiff.ts`).

use std::collections::HashMap;

use super::{
  changes_review::{number_and_side, side_version, Placement, Side},
  commits_data::{line_numbers, numbered},
};
use crate::{github::GhReviewThread, jj::JjDiffHunk, local_db::AgentReviewComment};

/// `AGENT_REVIEW_TARGET_WORKSPACE_DIFF`.
#[cfg(test)]
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

/// Agent comments on their line, and the outdated ones by file, as indices
/// into the comments placed.
#[derive(Default, Clone)]
pub struct AgentPlaced {
  pub by_line: HashMap<(String, u32, Side), Vec<usize>>,
  pub outdated: HashMap<String, Vec<usize>>,
  pub off_diff: HashMap<String, Vec<usize>>,
}

/// Open agent comments, and resolved sent ones, by the last line of their
/// range and side, as GitHub places a thread. One carried to the version of
/// its file `content_ids` names stands on its lines alone, off the diff when
/// its hunks in `maps` (the working copy's, then the committed ones) no
/// longer show them; any other goes outdated once those hunks no longer
/// read as it quoted them.
pub fn place_agent(
  comments: &[AgentReviewComment],
  maps: &[&HashMap<String, &[JjDiffHunk]>],
  content_ids: &HashMap<String, String>,
) -> AgentPlaced {
  let mut placed = AgentPlaced::default();
  for (i, c) in comments
    .iter()
    .enumerate()
    // A resolved comment the user sent stays, folded.
    .filter(|(_, c)| {
      c.status != "resolved" || c.source == crate::core::agent_review::SENT_REVIEW_SOURCE
    })
  {
    let hunks = maps.iter().find_map(|m| m.get(&c.file_path));
    let shown = content_ids
      .get(&c.file_path)
      .and_then(|id| side_version(id, agent_side(c)));
    let carried = c.version.is_some() && c.version.as_deref() == shown;
    let at = if c.outdated {
      Placement::Outdated
    } else if carried && hunks.is_some_and(|h| code_on(c, h).is_none()) {
      Placement::OffDiff
    } else if !carried && hunks.is_some_and(|h| agent_outdated(c, h)) {
      Placement::Outdated
    } else {
      Placement::Line
    };
    let file = || c.file_path.clone();
    match at {
      Placement::Outdated => placed.outdated.entry(file()).or_default().push(i),
      Placement::OffDiff => placed.off_diff.entry(file()).or_default().push(i),
      Placement::Line => {
        if let Ok(end) = u32::try_from(c.end_line) {
          placed
            .by_line
            .entry((file(), end, agent_side(c)))
            .or_default()
            .push(i);
        }
      }
    }
  }
  placed
}

/// What carrying an agent comment to its file's shown version needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCarry {
  /// Diff the version it kept against the shown one (`review::carry_lines`).
  Read { from: String, to: String },
  /// It kept no version, but its lines still read as it quoted them: it
  /// follows the file from the shown version on.
  Adopt(String),
  /// It kept no version, and its lines changed.
  Outdate,
}

/// What carrying `comment` to its file at `content_id`, whose diff is
/// `hunks`, needs; `None` when nothing.
pub fn agent_carry(
  comment: &AgentReviewComment,
  hunks: &[JjDiffHunk],
  content_id: &str,
) -> Option<AgentCarry> {
  if comment.outdated {
    return None;
  }
  let to = side_version(content_id, agent_side(comment))?;
  match &comment.version {
    Some(from) if from == to => None,
    Some(from) => Some(AgentCarry::Read {
      from: from.clone(),
      to: to.to_string(),
    }),
    None if comment.quoted_text.is_none() => None,
    None if agent_outdated(comment, hunks) => Some(AgentCarry::Outdate),
    None => Some(AgentCarry::Adopt(to.to_string())),
  }
}

fn agent_side(comment: &AgentReviewComment) -> Side {
  if comment.side.as_deref() == Some("old") {
    Side::Old
  } else {
    Side::New
  }
}

/// GitHub's outdated rule for a comment that quoted its lines: their code
/// is no longer what it quoted, or the diff no longer has them. A sent
/// review quotes diff lines, marker first; the review agent quotes the
/// file. One that quoted nothing stays on its line.
pub fn agent_outdated(comment: &AgentReviewComment, hunks: &[JjDiffHunk]) -> bool {
  let Some(quoted) = &comment.quoted_text else {
    return false;
  };
  let sent = comment.source == crate::core::agent_review::SENT_REVIEW_SOURCE;
  let quoted = quoted
    .split('\n')
    .map(|l| if sent { l.get(1..).unwrap_or("") } else { l });
  code_on(comment, hunks).is_none_or(|code| !code.iter().map(String::as_str).eq(quoted))
}

/// The code of the comment's range on its side, from any of `hunks`;
/// `None` when a line of it is not there.
fn code_on(comment: &AgentReviewComment, hunks: &[JjDiffHunk]) -> Option<Vec<String>> {
  let old_side = agent_side(comment) == Side::Old;
  let text_of = |n: i64| {
    hunks.iter().find_map(|hunk| {
      hunk
        .lines
        .iter()
        .zip(line_numbers(hunk))
        .find_map(|(line, (old, new))| {
          let at = if old_side { old } else { new };
          (at.map(i64::from) == Some(n)).then(|| line.get(1..).unwrap_or("").to_string())
        })
    })
  };
  (comment.start_line..=comment.end_line)
    .map(text_of)
    .collect()
}

/// `originalTextForComment`: the range's current text from `hunk`, or
/// `None` when a line of it is not there.
pub fn original_text(comment: &AgentReviewComment, hunk: &JjDiffHunk) -> Option<String> {
  code_on(comment, std::slice::from_ref(hunk)).map(|lines| lines.join("\n"))
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

/// How many lines a review thread's comment covers: `startLine..=line`.
pub fn suggestion_span(thread: &GhReviewThread) -> usize {
  match (thread.start_line, thread.line) {
    (Some(start), Some(end)) if end >= start => usize::try_from(end - start + 1).unwrap_or(1),
    _ => 1,
  }
}

/// The `lines` a GitHub suggestion replaces: the last lines of the
/// comment's `diffHunk`, which GitHub ends on the commented line.
pub fn github_suggestion_original(diff_hunk: &str, lines: usize) -> Option<String> {
  let body: Vec<&str> = diff_hunk.lines().filter(|l| !l.starts_with("@@")).collect();
  let start = body.len().checked_sub(lines.max(1))?;
  Some(
    body[start..]
      .iter()
      .map(|l| l.get(1..).unwrap_or(""))
      .collect::<Vec<_>>()
      .join("\n"),
  )
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
      start_line: None,
      diff_side: side.into(),
      comments: vec![GhReviewComment {
        author_id: None,
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
    let by_line = place_agent(
      &[
        agent("a.rs", 1, 3, None, "open"),
        agent("a.rs", 2, 2, Some("old"), "open"),
        agent("a.rs", 4, 4, None, "resolved"),
      ],
      &[],
      &HashMap::new(),
    )
    .by_line;
    assert!(by_line.contains_key(&("a.rs".into(), 3, Side::New)));
    assert!(by_line.contains_key(&("a.rs".into(), 2, Side::Old)));
    assert_eq!(by_line.len(), 2);
  }

  #[test]
  fn resolved_sent_review_comments_stay_on_their_line() {
    let mut sent = agent("a.rs", 4, 4, None, "resolved");
    sent.source = crate::core::agent_review::SENT_REVIEW_SOURCE.into();
    let by_line = place_agent(&[sent], &[], &HashMap::new()).by_line;
    assert!(by_line.contains_key(&("a.rs".into(), 4, Side::New)));
  }

  #[test]
  fn an_agent_comment_on_other_code_is_outdated() {
    let hunks = [hunk("h", "@@ -1,1 +1,3 @@", &[" a", "+b", "+c"])];
    let mut c = agent("a.rs", 2, 3, None, "open");
    // Quoting nothing, it stays on its line.
    assert!(!agent_outdated(&c, &hunks));
    // The review agent quotes the file's text.
    c.quoted_text = Some("b\nc".into());
    assert!(!agent_outdated(&c, &hunks));
    c.quoted_text = Some("b\nx".into());
    assert!(agent_outdated(&c, &hunks));
    // A sent review quotes its diff lines.
    c.source = crate::core::agent_review::SENT_REVIEW_SOURCE.into();
    c.quoted_text = Some("+b\n+c".into());
    assert!(!agent_outdated(&c, &hunks));
    // Lines the diff no longer has.
    c.end_line = 9;
    assert!(agent_outdated(&c, &hunks));
  }

  #[test]
  fn outdated_agent_comments_leave_their_line() {
    let hunks = [hunk("h", "@@ -1,1 +1,3 @@", &[" a", "+b", "+c"])];
    let files = HashMap::from([("a.rs".to_string(), &hunks[..])]);
    let mut moved = agent("a.rs", 2, 2, None, "open");
    moved.quoted_text = Some("x".into());
    let placed = place_agent(
      &[agent("a.rs", 3, 3, None, "open"), moved],
      &[&files],
      &HashMap::new(),
    );
    assert!(placed.by_line.contains_key(&("a.rs".into(), 3, Side::New)));
    assert!(!placed.by_line.contains_key(&("a.rs".into(), 2, Side::New)));
    assert_eq!(placed.outdated["a.rs"], [1]);
  }

  #[test]
  fn an_agent_comment_that_followed_its_file_stands_on_its_lines_alone() {
    let hunks = [hunk("h", "@@ -1,1 +1,3 @@", &[" a", "+b", "+c"])];
    let files = HashMap::from([("a.rs".to_string(), &hunks[..])]);
    let ids = HashMap::from([("a.rs".to_string(), "fbase>fv1:git".to_string())]);
    let at = |line: i64, version: &str, outdated: bool| {
      let mut c = agent("a.rs", line, line, None, "open");
      // A quote that no longer matches: the version decides.
      c.quoted_text = Some("x".into());
      c.version = Some(version.into());
      c.outdated = outdated;
      place_agent(&[c], &[&files], &ids)
    };
    assert!(at(2, "fv1", false)
      .by_line
      .contains_key(&("a.rs".into(), 2, Side::New)));
    assert_eq!(at(9, "fv1", false).off_diff["a.rs"], [0]);
    assert_eq!(at(2, "fv1", true).outdated["a.rs"], [0]);
    // Not carried to the shown version yet: its quote decides.
    assert_eq!(at(2, "fv0", false).outdated["a.rs"], [0]);
  }

  #[test]
  fn what_carrying_an_agent_comment_needs() {
    let hunks = [hunk("h", "@@ -1,1 +1,3 @@", &[" a", "+b", "+c"])];
    let due = |c: &AgentReviewComment| agent_carry(c, &hunks, "fbase>fv2:git");
    let mut c = agent("a.rs", 2, 2, None, "open");
    // Nothing tells where its lines were.
    assert_eq!(due(&c), None);
    // Its quote still matches: it follows the file from here.
    c.quoted_text = Some("b".into());
    assert_eq!(due(&c), Some(AgentCarry::Adopt("fv2".into())));
    c.quoted_text = Some("x".into());
    assert_eq!(due(&c), Some(AgentCarry::Outdate));
    c.version = Some("fv1".into());
    let read = AgentCarry::Read {
      from: "fv1".into(),
      to: "fv2".into(),
    };
    assert_eq!(due(&c), Some(read));
    c.version = Some("fv2".into());
    assert_eq!(due(&c), None);
    (c.version, c.outdated) = (Some("fv1".into()), true);
    assert_eq!(due(&c), None);
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
    let rows = suggestion_diff(Some("na\u{ef}ve code"), "na\u{ee}ve code").expect("rows");
    assert_eq!(changed(&rows[0]), vec!["\u{ef}"]);
    assert_eq!(changed(&rows[1]), vec!["\u{ee}"]);
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
  fn github_suggestion_original_is_the_hunks_last_line() {
    let hunk = "@@ -1,2 +1,3 @@\n context\n+No scrape endpoint.";
    assert_eq!(
      github_suggestion_original(hunk, 1).as_deref(),
      Some("No scrape endpoint.")
    );
    assert_eq!(github_suggestion_original("", 1), None);
  }

  #[test]
  fn github_suggestion_original_spans_a_multi_line_comment() {
    let hunk = "@@ -1,3 +1,3 @@\n a\n+b\n+c";
    assert_eq!(github_suggestion_original(hunk, 2).as_deref(), Some("b\nc"));
    assert_eq!(github_suggestion_original(hunk, 4), None);
  }

  #[test]
  fn suggestion_span_counts_start_line_to_line() {
    let mut t = thread("a.rs", Some(4), "RIGHT");
    assert_eq!(suggestion_span(&t), 1);
    t.start_line = Some(2);
    assert_eq!(suggestion_span(&t), 3);
    t.start_line = Some(9);
    assert_eq!(suggestion_span(&t), 1);
  }

  #[test]
  fn an_empty_suggestion_removes_the_line() {
    let rows = suggestion_diff(Some("No scrape endpoint."), "").expect("rows");
    assert_eq!(rows, vec![plain(RowKind::Removed, "No scrape endpoint.")]);
  }

  #[test]
  fn dates_drop_this_year() {
    assert_eq!(comment_date("2026-03-04T10:00:00Z", 2026), "Mar 4");
    assert_eq!(comment_date("2025-12-31T10:00:00Z", 2026), "Dec 31, 2025");
    assert_eq!(preview(" a\n  b "), "a b");
  }
}
