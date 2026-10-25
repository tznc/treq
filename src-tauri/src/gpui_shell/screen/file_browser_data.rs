//! What the Code tab's file browser (`FileBrowser`) reads from and writes
//! to the backend, and the rules React applies to it: which entries the
//! tree shows and in what order, which files show as text, the change
//! markers beside lines, and the review comments kept while browsing.

use std::{
  cmp::Ordering,
  collections::{HashMap, HashSet},
  sync::LazyLock,
};

use regex::Regex;

use super::{
  changes_data::{ChangedFile, ChangesHost},
  changes_review::LineComment,
};
use crate::{
  core::{self, WorkspaceEntry},
  gpui_shell::native::Backend,
  jj::JjDiffHunk,
  local_db::FileBrowserPendingReview,
};

/// Files longer than this (in UTF-16 code units, as `content.length`)
/// are not shown.
pub const MAX_FILE_LEN: usize = 1024 * 1024;
/// `FILE_BROWSER_COMMENT_HUNK_ID`: there is no diff hunk to anchor to.
pub const COMMENT_HUNK: &str = "browser";

/// What the file browser asks of the backend; faked in tests. Everything
/// blocks, so it is called off the main thread.
pub trait FileBrowserHost: ChangesHost {
  /// `list_directory`: a directory's children, minus ignored files.
  fn list_directory(&self, path: &str) -> Result<Vec<WorkspaceEntry>, String>;
  fn read_file(&self, path: &str) -> Result<String, String>;
  /// `get_file_modified_at`.
  fn modified_at(&self, path: &str) -> Option<String>;
  /// The file browser's own review (`load_file_browser_review`).
  fn load_browser_review(
    &self,
    repo_path: &str,
    workspace_id: i64,
  ) -> Result<Option<FileBrowserPendingReview>, String>;
  /// `save_file_browser_review`: the comments as JSON.
  fn save_browser_review(
    &self,
    repo_path: &str,
    workspace_id: i64,
    comments: &str,
    summary: Option<&str>,
  ) -> Result<(), String>;
  fn clear_browser_review(&self, repo_path: &str, workspace_id: i64) -> Result<(), String>;
}

impl FileBrowserHost for Backend {
  fn list_directory(&self, path: &str) -> Result<Vec<WorkspaceEntry>, String> {
    core::list_directory(path)
  }

  fn read_file(&self, path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())
  }

  fn modified_at(&self, path: &str) -> Option<String> {
    core::file_modified_at(path)
  }

  fn load_browser_review(
    &self,
    repo_path: &str,
    workspace_id: i64,
  ) -> Result<Option<FileBrowserPendingReview>, String> {
    core::review_drafts::file_browser_review(repo_path, workspace_id)
  }

  fn save_browser_review(
    &self,
    repo_path: &str,
    workspace_id: i64,
    comments: &str,
    summary: Option<&str>,
  ) -> Result<(), String> {
    core::review_drafts::save_file_browser_review(repo_path, workspace_id, comments, summary)
  }

  fn clear_browser_review(&self, repo_path: &str, workspace_id: i64) -> Result<(), String> {
    core::review_drafts::clear_file_browser_review(repo_path, workspace_id)
  }
}

/// `filterHiddenEntries`: hidden files stay, hidden directories go.
pub fn filter_hidden(entries: Vec<WorkspaceEntry>) -> Vec<WorkspaceEntry> {
  entries
    .into_iter()
    .filter(|e| !e.is_directory || !e.name.starts_with('.'))
    .collect()
}

/// Directories first, then by name as `localeCompare` orders them: case
/// folded, lowercase first on a tie.
pub fn compare_entries(a: &WorkspaceEntry, b: &WorkspaceEntry) -> Ordering {
  // Folded char by char, not into new strings: a sort makes thousands of
  // comparisons.
  fn fold(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().flat_map(char::to_lowercase)
  }
  b.is_directory.cmp(&a.is_directory).then_with(|| {
    fold(&a.name)
      .cmp(fold(&b.name))
      .then_with(|| b.name.cmp(&a.name))
  })
}

/// A directory's entries as the tree lists them, sorted off the main
/// thread once, so [`flatten`]'s sort finds each level in order.
pub fn sorted(mut entries: Vec<WorkspaceEntry>) -> Vec<WorkspaceEntry> {
  entries.sort_by(compare_entries);
  entries
}

/// A row of the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeRow {
  pub entry: WorkspaceEntry,
  pub depth: usize,
}

/// `flattenExpandedTree`: every entry under an expanded directory, sorted
/// at each level, with its depth.
pub fn flatten(
  roots: &[WorkspaceEntry],
  expanded: &HashSet<String>,
  cache: &HashMap<String, Vec<WorkspaceEntry>>,
) -> Vec<TreeRow> {
  fn append(
    entries: &[WorkspaceEntry],
    depth: usize,
    expanded: &HashSet<String>,
    cache: &HashMap<String, Vec<WorkspaceEntry>>,
    rows: &mut Vec<TreeRow>,
  ) {
    let mut sorted: Vec<&WorkspaceEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| compare_entries(a, b));
    for entry in sorted {
      rows.push(TreeRow {
        entry: entry.clone(),
        depth,
      });
      if entry.is_directory && expanded.contains(&entry.path) {
        let children = cache.get(&entry.path).map(Vec::as_slice).unwrap_or(&[]);
        append(children, depth + 1, expanded, cache, rows);
      }
    }
  }
  let mut rows = Vec::new();
  append(roots, 0, expanded, cache, &mut rows);
  rows
}

/// `isBinaryFile`: by extension.
pub fn is_binary(path: &str) -> bool {
  const BINARY: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".ico", ".svg", ".pdf", ".zip", ".tar", ".gz", ".rar",
    ".7z", ".exe", ".dll", ".so", ".dylib", ".mp3", ".mp4", ".avi", ".mov", ".wmv", ".woff",
    ".woff2", ".ttf", ".eot",
  ];
  let lower = path.to_lowercase();
  BINARY.iter().any(|ext| lower.ends_with(ext))
}

/// `.md` and `.markdown` files offer a preview.
pub fn is_markdown(path: &str) -> bool {
  let lower = path.to_lowercase();
  lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// `getRelativePath`: `full` inside `base`, else `full` itself.
pub fn relative(base: &str, full: &str) -> String {
  if base.is_empty() {
    return full.to_string();
  }
  full
    .strip_prefix(&format!("{base}/"))
    .unwrap_or(full)
    .to_string()
}

/// The last component of `path`.
pub fn file_name(path: &str) -> &str {
  path.rsplit('/').next().unwrap_or(path)
}

/// The root's README the browser opens first (`readme.md`, any case).
pub fn readme(entries: &[WorkspaceEntry]) -> Option<&WorkspaceEntry> {
  entries
    .iter()
    .find(|e| !e.is_directory && e.name.to_lowercase() == "readme.md")
}

/// Whether React would refuse to show `content` (`content.length > 1MB`).
pub fn too_large(content: &str) -> bool {
  content.len() > MAX_FILE_LEN && content.encode_utf16().count() > MAX_FILE_LEN
}

/// Line markers from a file's working-copy hunks: lines added, and lines
/// after which lines were deleted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineMarks {
  pub added: HashSet<u32>,
  pub deleted_after: HashSet<u32>,
}

/// `handleFileClick`'s hunk parse.
#[expect(clippy::expect_used, reason = "constant pattern")]
pub fn line_marks(hunks: &[JjDiffHunk]) -> LineMarks {
  static HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@").expect("regex"));
  let header = &*HEADER;
  let mut marks = LineMarks::default();
  for hunk in hunks {
    let Some(start) = header
      .captures(&hunk.header)
      .and_then(|c| c[1].parse::<u32>().ok())
    else {
      continue;
    };
    let mut line = start;
    for text in &hunk.lines {
      if text.starts_with('+') {
        marks.added.insert(line);
        line += 1;
      } else if text.starts_with('-') {
        if line > 1 {
          marks.deleted_after.insert(line - 1);
        }
      } else if text.starts_with(' ') {
        line += 1;
      }
    }
  }
  marks
}

/// `getDirectoryChangeStatus`: the status of the first changed file under
/// `rel_dir`.
pub fn dir_status<'a>(changed: &'a [ChangedFile], rel_dir: &str) -> Option<&'a str> {
  let prefix = format!("{rel_dir}/");
  changed
    .iter()
    .find(|f| f.path.starts_with(&prefix))
    .map(|f| f.status.as_str())
}

/// The status dot's title.
pub fn status_title(status: &str) -> &'static str {
  match status {
    "M" => "Modified",
    "A" => "Added",
    "D" => "Deleted",
    _ => "Changed",
  }
}

/// The line number column's width in characters.
pub fn line_number_width(lines: usize) -> usize {
  lines.to_string().len().max(4)
}

/// `handleAddComment`: what a comment covers. `line` is the + button's line;
/// `None` comments on the selection.
pub fn comment_target(
  lines: &[String],
  selection: Option<(u32, u32)>,
  line: Option<u32>,
) -> Option<(u32, u32, Vec<String>)> {
  let slice = |start: u32, end: u32| {
    lines
      .iter()
      .skip(start.saturating_sub(1) as usize)
      .take((end + 1).saturating_sub(start) as usize)
      .cloned()
      .collect::<Vec<_>>()
  };
  match (line, selection) {
    (Some(n), Some((start, end))) if n >= start && n <= end && start != end => {
      Some((start, end, slice(start, end)))
    }
    (Some(n), _) => Some((
      n,
      n,
      vec![lines.get(n as usize - 1).cloned().unwrap_or_default()],
    )),
    (None, Some((start, end))) => Some((start, end, slice(start, end))),
    (None, None) => None,
  }
}

/// Pending comments per file, for the tree's counts.
pub fn comment_counts(comments: &[LineComment]) -> HashMap<String, usize> {
  let mut counts = HashMap::new();
  for c in comments {
    *counts.entry(c.file_path.clone()).or_default() += 1;
  }
  counts
}

/// The kept review's comments and summary.
pub fn parse_review(review: &FileBrowserPendingReview) -> (Vec<LineComment>, String) {
  (
    serde_json::from_str(&review.comments).unwrap_or_default(),
    review.summary_text.clone().unwrap_or_default(),
  )
}

/// `saveFileBrowserReview`'s arguments: the comments as JSON, and the
/// summary trimmed, `None` when blank.
pub fn review_to_save(comments: &[LineComment], summary: &str) -> (String, Option<String>) {
  let summary = Some(summary.trim().to_string()).filter(|s| !s.is_empty());
  (serde_json::to_string(comments).unwrap_or_default(), summary)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn entry(name: &str, is_directory: bool) -> WorkspaceEntry {
    WorkspaceEntry {
      name: name.into(),
      path: format!("/ws/{name}"),
      is_directory,
      modified_at: None,
      submodule_pin: None,
      submodule_synced: None,
      status: None,
    }
  }

  fn hunk(header: &str, lines: &[&str]) -> JjDiffHunk {
    serde_json::from_value(serde_json::json!({
      "id": "h",
      "header": header,
      "lines": lines,
      "patch": "",
    }))
    .unwrap()
  }

  #[test]
  fn hidden_directories_are_left_out_but_hidden_files_stay() {
    let kept = filter_hidden(vec![
      entry(".git", true),
      entry(".env", false),
      entry("src", true),
    ]);
    let names: Vec<_> = kept.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, [".env", "src"]);
  }

  #[test]
  fn flatten_lists_expanded_directories_children_under_them() {
    let roots = vec![
      entry("b.txt", false),
      entry("src", true),
      entry("A.md", false),
    ];
    let mut cache = HashMap::new();
    cache.insert(
      "/ws/src".to_string(),
      vec![WorkspaceEntry {
        path: "/ws/src/lib.rs".into(),
        ..entry("lib.rs", false)
      }],
    );
    let rows = |expanded: &[&str]| {
      let expanded: HashSet<String> = expanded.iter().map(|s| s.to_string()).collect();
      flatten(&roots, &expanded, &cache)
        .into_iter()
        .map(|r| (r.entry.name, r.depth))
        .collect::<Vec<_>>()
    };
    assert_eq!(
      rows(&[]),
      [("src".into(), 0), ("A.md".into(), 0), ("b.txt".into(), 0)]
    );
    assert_eq!(
      rows(&["/ws/src"]),
      [
        ("src".into(), 0),
        ("lib.rs".into(), 1),
        ("A.md".into(), 0),
        ("b.txt".into(), 0)
      ]
    );
  }

  #[test]
  fn entries_sort_directories_first_then_case_folded() {
    let names = |entries: Vec<WorkspaceEntry>| {
      sorted(entries)
        .into_iter()
        .map(|e| e.name)
        .collect::<Vec<_>>()
    };
    assert_eq!(
      names(vec![
        entry("B", false),
        entry("a", false),
        entry("b", false),
        entry("Z", true),
      ]),
      ["Z", "a", "b", "B"]
    );
  }

  #[test]
  fn binary_files_go_by_extension() {
    assert!(is_binary("/ws/logo.PNG"));
    assert!(is_binary("/ws/font.woff2"));
    assert!(!is_binary("/ws/main.rs"));
  }

  #[test]
  fn relative_paths_drop_the_base() {
    assert_eq!(relative("/ws", "/ws/src/a.rs"), "src/a.rs");
    assert_eq!(relative("/ws", "/other/a.rs"), "/other/a.rs");
    assert_eq!(relative("", "/ws/a.rs"), "/ws/a.rs");
  }

  #[test]
  fn readme_is_found_in_any_case() {
    let entries = vec![entry("docs", true), entry("ReadMe.MD", false)];
    assert_eq!(readme(&entries).map(|e| e.name.as_str()), Some("ReadMe.MD"));
    assert!(readme(&[entry("README.md", true)]).is_none());
  }

  #[test]
  fn hunks_mark_added_lines_and_deletions() {
    let marks = line_marks(&[hunk("@@ -1,4 +1,4 @@", &[" a", "-b", "+B", " c", "-d"])]);
    assert_eq!(marks.added, HashSet::from([2]));
    assert_eq!(marks.deleted_after, HashSet::from([1, 3]));
  }

  #[test]
  fn directories_take_their_first_changed_file_status() {
    let changed = vec![
      ChangedFile {
        path: "src/a.rs".into(),
        status: "A".into(),
        old_path: None,
      },
      ChangedFile {
        path: "src/b.rs".into(),
        status: "M".into(),
        old_path: None,
      },
    ];
    assert_eq!(dir_status(&changed, "src"), Some("A"));
    assert_eq!(dir_status(&changed, "sr"), None);
  }

  #[test]
  fn status_titles_match_react() {
    assert_eq!(status_title("M"), "Modified");
    assert_eq!(status_title("R"), "Changed");
  }

  #[test]
  fn line_numbers_are_at_least_four_wide() {
    assert_eq!(line_number_width(12), 4);
    assert_eq!(line_number_width(12345), 5);
  }

  #[test]
  fn comments_cover_the_selection_or_the_line() {
    let lines: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
    assert_eq!(
      comment_target(&lines, Some((1, 3)), Some(2)),
      Some((1, 3, lines.clone()))
    );
    assert_eq!(
      comment_target(&lines, Some((1, 3)), None),
      Some((1, 3, lines.clone()))
    );
    assert_eq!(
      comment_target(&lines, Some((2, 2)), Some(3)),
      Some((3, 3, vec!["c".into()]))
    );
    assert_eq!(comment_target(&lines, None, None), None);
  }

  #[test]
  fn large_files_count_utf16_units() {
    assert!(!too_large("abc"));
    assert!(too_large(&"a".repeat(MAX_FILE_LEN + 1)));
    // Three bytes, one unit each: not over the limit by units.
    assert!(!too_large(&"€".repeat(MAX_FILE_LEN / 2)));
  }

  #[test]
  fn reviews_round_trip() {
    let comment = LineComment {
      id: "1".into(),
      file_path: "a.rs".into(),
      hunk_id: COMMENT_HUNK.into(),
      start_line: 2,
      end_line: 2,
      line_content: vec!["b".into()],
      text: "why".into(),
      created_at: "t".into(),
      line_side: None,
      source: None,
      github_author: None,
      github_avatar_url: None,
      github_comment_url: None,
    };
    let (json, summary) = review_to_save(std::slice::from_ref(&comment), "  ");
    assert_eq!(summary, None);
    let review = FileBrowserPendingReview {
      id: 1,
      workspace_id: 7,
      comments: json,
      summary_text: Some("ok".into()),
      created_at: String::new(),
      updated_at: String::new(),
    };
    assert_eq!(parse_review(&review), (vec![comment], "ok".into()));
    assert_eq!(comment_counts(&parse_review(&review).0)["a.rs"], 1);
  }
}
