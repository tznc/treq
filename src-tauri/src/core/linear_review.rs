//! Agent review of Linear content: an issue, a project or a document, plus the
//! comments on it.
//!
//! A review agent cannot reach Linear itself (the app holds the credentials),
//! so the app writes what is being reviewed into a snapshot directory first:
//! `body.md` holds the entity's own text and `comments/<id>.md` holds each
//! Linear comment. The agent anchors its review comments on line ranges of
//! those files, the same way it anchors them on a diff.
//!
//! Line numbers only hold for that one snapshot, so each review comment also
//! stores the exact text it covers (`quoted_text`). Applying a suggestion
//! finds that text in the live Linear content and replaces it, which still
//! works after other edits have moved it around.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const TARGET_LINEAR_ISSUE: &str = "linear_issue";
pub const TARGET_LINEAR_PROJECT: &str = "linear_project";
pub const TARGET_LINEAR_DOCUMENT: &str = "linear_document";
pub const LINEAR_TARGET_TYPES: [&str; 3] = [
  TARGET_LINEAR_ISSUE,
  TARGET_LINEAR_PROJECT,
  TARGET_LINEAR_DOCUMENT,
];

/// Snapshot file holding the entity's own text.
pub const BODY_FILE: &str = "body.md";
/// Snapshot file describing the other files; not itself reviewable.
pub const INDEX_FILE: &str = "INDEX.md";

pub fn is_linear_target(target_type: &str) -> bool {
  LINEAR_TARGET_TYPES.contains(&target_type)
}

/// What the app shows the agent: the entity's text and its comments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearReviewSnapshot {
  pub title: String,
  pub url: String,
  pub body: String,
  #[serde(default)]
  pub comments: Vec<LinearReviewSnapshotComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearReviewSnapshotComment {
  pub id: String,
  #[serde(default)]
  pub author: Option<String>,
  #[serde(default)]
  pub created_at: Option<String>,
  /// The excerpt of the body this comment is anchored to, when it has one.
  #[serde(default)]
  pub quoted_text: Option<String>,
  pub body: String,
}

/// Which Linear text a review comment's `file_path` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinearTextRef {
  /// The issue description, project description or document content.
  Body,
  /// The body of one Linear comment.
  Comment(String),
}

/// Linear ids are UUIDs or short slugs; anything else could escape the
/// snapshot directory once used as a path segment.
fn is_safe_id(id: &str) -> bool {
  !id.is_empty()
    && id.len() <= 128
    && id
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parses a snapshot-relative `--file` into the Linear text it names.
pub fn parse_text_ref(file: &str) -> Option<LinearTextRef> {
  let file = file.trim().trim_start_matches("./");
  if file == BODY_FILE {
    return Some(LinearTextRef::Body);
  }
  let id = file.strip_prefix("comments/")?.strip_suffix(".md")?;
  is_safe_id(id).then(|| LinearTextRef::Comment(id.to_string()))
}

fn entity_label(target_type: &str) -> &'static str {
  match target_type {
    TARGET_LINEAR_ISSUE => "issue description",
    TARGET_LINEAR_PROJECT => "project description",
    _ => "document content",
  }
}

/// `<repo>/.treq/linear-review/<target_type>/<target_id>`.
pub fn snapshot_dir(
  repo_path: &str,
  target_type: &str,
  target_id: &str,
) -> Result<PathBuf, String> {
  if !is_linear_target(target_type) {
    return Err(format!(
      "invalid_arguments: '{target_type}' is not a Linear review target. Expected one of: {}",
      LINEAR_TARGET_TYPES.join(", ")
    ));
  }
  if !is_safe_id(target_id) {
    return Err(format!(
      "invalid_arguments: '{target_id}' is not a Linear id"
    ));
  }
  Ok(
    Path::new(repo_path)
      .join(".treq")
      .join("linear-review")
      .join(target_type)
      .join(target_id),
  )
}

fn index_markdown(target_type: &str, snapshot: &LinearReviewSnapshot) -> String {
  let mut out = format!("# {}\n\n{}\n\n", snapshot.title, snapshot.url);
  out.push_str(
    "Line numbers in the files below are what `--start-line` and `--end-line` refer to.\n\n",
  );
  out.push_str(&format!(
    "- `{BODY_FILE}`: the {}\n",
    entity_label(target_type)
  ));
  for comment in snapshot.comments.iter().filter(|c| is_safe_id(&c.id)) {
    let author = comment.author.as_deref().unwrap_or("Unknown");
    let mut line = format!("- `comments/{}.md`: comment by {author}", comment.id);
    if let Some(at) = &comment.created_at {
      line.push_str(&format!(" at {at}"));
    }
    if let Some(quote) = comment
      .quoted_text
      .as_deref()
      .filter(|q| !q.trim().is_empty())
    {
      line.push_str(&format!(", on the passage \"{}\"", quote.trim()));
    }
    out.push_str(&line);
    out.push('\n');
  }
  out
}

/// Replaces the target's snapshot with `snapshot` and returns its directory.
/// Comments whose id is not a plain Linear id are left out.
pub fn write_snapshot(
  repo_path: &str,
  target_type: &str,
  target_id: &str,
  snapshot: &LinearReviewSnapshot,
) -> Result<PathBuf, String> {
  let dir = snapshot_dir(repo_path, target_type, target_id)?;
  if dir.exists() {
    std::fs::remove_dir_all(&dir).map_err(|e| format!("Failed to clear {}: {e}", dir.display()))?;
  }
  let comments_dir = dir.join("comments");
  std::fs::create_dir_all(&comments_dir)
    .map_err(|e| format!("Failed to create {}: {e}", comments_dir.display()))?;
  let write = |path: PathBuf, text: &str| {
    std::fs::write(&path, text).map_err(|e| format!("Failed to write {}: {e}", path.display()))
  };
  write(dir.join(BODY_FILE), &snapshot.body)?;
  for comment in snapshot.comments.iter().filter(|c| is_safe_id(&c.id)) {
    write(
      comments_dir.join(format!("{}.md", comment.id)),
      &comment.body,
    )?;
  }
  write(dir.join(INDEX_FILE), &index_markdown(target_type, snapshot))?;
  Ok(dir)
}

/// Returns the exact text of `start_line..=end_line` of one snapshot file,
/// rejecting a file the snapshot does not have or a range past its end.
pub fn quote_snapshot_lines(
  repo_path: &str,
  target_type: &str,
  target_id: &str,
  file: &str,
  start_line: i64,
  end_line: i64,
) -> Result<String, String> {
  let dir = snapshot_dir(repo_path, target_type, target_id)?;
  if parse_text_ref(file).is_none()
    || !Path::new(file)
      .components()
      .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
  {
    return Err(format!(
      "invalid_arguments: --file must be {BODY_FILE} or comments/<id>.md, got '{file}'"
    ));
  }
  let path = dir.join(file);
  let content = std::fs::read_to_string(&path).map_err(|_| {
    format!(
      "invalid_arguments: no snapshot file '{file}' for {target_type} {target_id}. Start the review from treq so it writes the snapshot"
    )
  })?;
  let lines: Vec<&str> = content.lines().collect();
  if start_line < 1 || end_line < start_line || end_line as usize > lines.len() {
    return Err(format!(
      "invalid_arguments: lines {start_line}..{end_line} are outside {file} ({} lines)",
      lines.len()
    ));
  }
  Ok(lines[(start_line - 1) as usize..end_line as usize].join("\n"))
}

/// Replaces the one occurrence of `quoted` in `live` with `replacement`.
/// Fails rather than guessing when the text is gone or appears more than once.
pub fn splice_quoted(live: &str, quoted: &str, replacement: &str) -> Result<String, String> {
  if quoted.is_empty() {
    return Err("This review comment has no reviewed text to replace".to_string());
  }
  // Linear may hand back CRLF line endings; compare on LF.
  let live = live.replace("\r\n", "\n");
  match live.matches(quoted).count() {
    0 => Err(
      "The reviewed text is no longer in the Linear content; it changed after the review".to_string(),
    ),
    1 => Ok(live.replacen(quoted, replacement, 1)),
    n => Err(format!(
      "The reviewed text appears {n} times in the Linear content, so the suggestion cannot be placed"
    )),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn snapshot() -> LinearReviewSnapshot {
    LinearReviewSnapshot {
      title: "Ship search".into(),
      url: "https://linear.app/x/issue/ENG-1".into(),
      body: "Line one\nLine two\nLine three\n".into(),
      comments: vec![
        LinearReviewSnapshotComment {
          id: "c-1".into(),
          author: Some("Ada".into()),
          created_at: Some("2026-01-01T00:00:00Z".into()),
          quoted_text: Some("Line two".into()),
          body: "Is this right?".into(),
        },
        LinearReviewSnapshotComment {
          id: "../escape".into(),
          author: None,
          created_at: None,
          quoted_text: None,
          body: "nope".into(),
        },
      ],
    }
  }

  #[test]
  fn writes_body_comments_and_index() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_str().unwrap();
    let out = write_snapshot(repo, TARGET_LINEAR_ISSUE, "issue-1", &snapshot()).unwrap();
    assert_eq!(
      std::fs::read_to_string(out.join("body.md")).unwrap(),
      "Line one\nLine two\nLine three\n"
    );
    assert_eq!(
      std::fs::read_to_string(out.join("comments/c-1.md")).unwrap(),
      "Is this right?"
    );
    let index = std::fs::read_to_string(out.join("INDEX.md")).unwrap();
    assert!(
      index.contains("`body.md`: the issue description"),
      "{index}"
    );
    assert!(index.contains("comment by Ada"), "{index}");
    assert!(index.contains("\"Line two\""), "{index}");
    assert!(!index.contains("escape"), "{index}");
    assert!(!dir
      .path()
      .join(".treq/linear-review/linear_issue/escape.md")
      .exists());
  }

  #[test]
  fn rewriting_a_snapshot_drops_stale_comment_files() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_str().unwrap();
    write_snapshot(repo, TARGET_LINEAR_DOCUMENT, "d", &snapshot()).unwrap();
    let mut next = snapshot();
    next.comments.clear();
    let out = write_snapshot(repo, TARGET_LINEAR_DOCUMENT, "d", &next).unwrap();
    assert!(!out.join("comments/c-1.md").exists());
  }

  #[test]
  fn snapshot_dir_rejects_unknown_types_and_unsafe_ids() {
    assert!(snapshot_dir("/r", "workspace_diff", "a").is_err());
    assert!(snapshot_dir("/r", TARGET_LINEAR_ISSUE, "../x").is_err());
    assert!(snapshot_dir("/r", TARGET_LINEAR_ISSUE, "").is_err());
    assert!(snapshot_dir("/r", TARGET_LINEAR_PROJECT, "3f2a-b").is_ok());
  }

  #[test]
  fn parses_text_refs() {
    assert_eq!(parse_text_ref("body.md"), Some(LinearTextRef::Body));
    assert_eq!(parse_text_ref("./body.md"), Some(LinearTextRef::Body));
    assert_eq!(
      parse_text_ref("comments/c-1.md"),
      Some(LinearTextRef::Comment("c-1".into()))
    );
    for bad in [
      "INDEX.md",
      "comments/../body.md",
      "comments/a/b.md",
      "other.md",
    ] {
      assert_eq!(parse_text_ref(bad), None, "{bad}");
    }
  }

  #[test]
  fn quotes_snapshot_lines_and_rejects_bad_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_str().unwrap();
    write_snapshot(repo, TARGET_LINEAR_ISSUE, "issue-1", &snapshot()).unwrap();
    let quote = |file: &str, start, end| {
      quote_snapshot_lines(repo, TARGET_LINEAR_ISSUE, "issue-1", file, start, end)
    };
    assert_eq!(quote("body.md", 2, 3).unwrap(), "Line two\nLine three");
    assert_eq!(quote("comments/c-1.md", 1, 1).unwrap(), "Is this right?");
    assert!(quote("body.md", 3, 4).is_err());
    assert!(quote("body.md", 0, 1).is_err());
    assert!(quote("INDEX.md", 1, 1).is_err());
    assert!(quote("comments/missing.md", 1, 1).is_err());
    assert!(
      quote_snapshot_lines(repo, TARGET_LINEAR_ISSUE, "other", "body.md", 1, 1)
        .unwrap_err()
        .contains("Start the review from treq")
    );
  }

  #[test]
  fn splices_a_unique_quote() {
    assert_eq!(
      splice_quoted("a\nteh fix\nc", "teh fix", "the fix").unwrap(),
      "a\nthe fix\nc"
    );
    assert_eq!(splice_quoted("a\r\nb\r\n", "a\nb", "x").unwrap(), "x\n");
    assert!(splice_quoted("abc", "zzz", "y")
      .unwrap_err()
      .contains("changed"));
    assert!(splice_quoted("ab ab", "ab", "y")
      .unwrap_err()
      .contains("2 times"));
    assert!(splice_quoted("ab", "", "y").is_err());
  }
}
