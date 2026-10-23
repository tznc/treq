//! Parses and applies unified diffs in-process, so applying a patch does not
//! shell out to `git apply`.
//!
//! Supports what `diff -u` and `git diff` emit for text files: `---`/`+++`
//! headers (with optional `a/`/`b/` prefixes and `/dev/null` for created or
//! deleted files), `@@ -a,b +c,d @@` hunks, and the `\ No newline at end of
//! file` marker. Like `git apply`, a hunk must match its context exactly but
//! may sit at a different line than its header says.

/// One file's changes from a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatch {
  /// Path before the change, `None` for a created file.
  pub old_path: Option<String>,
  /// Path after the change, `None` for a deleted file.
  pub new_path: Option<String>,
  hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
  old_start: usize,
  lines: Vec<HunkLine>,
  /// The old side's last line has no trailing newline.
  old_missing_newline: bool,
  /// The new side's last line has no trailing newline.
  new_missing_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HunkLine {
  Context(String),
  Remove(String),
  Add(String),
}

impl FilePatch {
  /// The path this patch writes to: the new path, or the old one when deleting.
  pub fn target_path(&self) -> Option<&str> {
    self.new_path.as_deref().or(self.old_path.as_deref())
  }
}

/// Parses every file patch in `patch`.
pub fn parse(patch: &str) -> Result<Vec<FilePatch>, String> {
  // Dropping the final newline leaves empty lines only for space-less context.
  let lines: Vec<&str> = patch
    .strip_suffix('\n')
    .unwrap_or(patch)
    .split('\n')
    .collect();
  let mut patches = Vec::new();
  let mut i = 0;
  while i < lines.len() {
    let Some(old_header) = lines[i].strip_prefix("--- ") else {
      // `diff --git`, `index`, mode lines and anything else between files.
      i += 1;
      continue;
    };
    let new_header = lines
      .get(i + 1)
      .and_then(|line| line.strip_prefix("+++ "))
      .ok_or_else(|| format!("line {}: '---' header without a '+++' header", i + 1))?;
    let old_path = header_path(old_header, "a/");
    let new_path = header_path(new_header, "b/");
    if old_path.is_none() && new_path.is_none() {
      return Err(format!("line {}: both sides are /dev/null", i + 1));
    }
    i += 2;
    let mut hunks = Vec::new();
    while let Some(header) = lines.get(i).filter(|line| line.starts_with("@@ ")) {
      let (old_start, old_len, new_len) = parse_hunk_header(header)
        .ok_or_else(|| format!("line {}: malformed hunk header '{header}'", i + 1))?;
      i += 1;
      let (hunk, next) = parse_hunk_body(&lines, i, old_start, old_len, new_len)?;
      hunks.push(hunk);
      i = next;
    }
    if hunks.is_empty() {
      return Err(format!(
        "no hunks for '{}'",
        new_path
          .as_deref()
          .or(old_path.as_deref())
          .unwrap_or_default()
      ));
    }
    patches.push(FilePatch {
      old_path,
      new_path,
      hunks,
    });
  }
  if patches.is_empty() {
    return Err("no file changes found in patch".to_string());
  }
  Ok(patches)
}

/// `a/src/x.rs\t2024-...` -> `Some("src/x.rs")`; `/dev/null` -> `None`.
fn header_path(header: &str, prefix: &str) -> Option<String> {
  let path = header
    .split('\t')
    .next()
    .unwrap_or(header)
    .trim_end_matches('\r');
  if path == "/dev/null" {
    return None;
  }
  Some(path.strip_prefix(prefix).unwrap_or(path).to_string())
}

/// `@@ -a,b +c,d @@ ...` -> `(a, b, d)`; a missing count means 1.
fn parse_hunk_header(header: &str) -> Option<(usize, usize, usize)> {
  let mut parts = header.strip_prefix("@@ ")?.split(' ');
  let range = |part: Option<&str>, sign: char| -> Option<(usize, usize)> {
    let range = part?.strip_prefix(sign)?;
    match range.split_once(',') {
      Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
      None => Some((range.parse().ok()?, 1)),
    }
  };
  let (old_start, old_len) = range(parts.next(), '-')?;
  let (_, new_len) = range(parts.next(), '+')?;
  (parts.next()? == "@@" || header.contains(" @@")).then_some((old_start, old_len, new_len))
}

fn parse_hunk_body(
  lines: &[&str],
  mut i: usize,
  old_start: usize,
  old_len: usize,
  new_len: usize,
) -> Result<(Hunk, usize), String> {
  let mut hunk = Hunk {
    old_start,
    lines: Vec::new(),
    old_missing_newline: false,
    new_missing_newline: false,
  };
  let (mut old_seen, mut new_seen) = (0, 0);
  while old_seen < old_len || new_seen < new_len {
    let Some(line) = lines.get(i) else {
      return Err(format!(
        "hunk at old line {old_start} ends early: expected {old_len} old and {new_len} new lines"
      ));
    };
    match line.chars().next() {
      Some(' ') | None => {
        hunk
          .lines
          .push(HunkLine::Context(line.get(1..).unwrap_or("").to_string()));
        old_seen += 1;
        new_seen += 1;
      }
      Some('-') => {
        hunk
          .lines
          .push(HunkLine::Remove(line.get(1..).unwrap_or("").to_string()));
        old_seen += 1;
      }
      Some('+') => {
        hunk
          .lines
          .push(HunkLine::Add(line.get(1..).unwrap_or("").to_string()));
        new_seen += 1;
      }
      Some('\\') => {}
      _ => return Err(format!("line {}: unexpected '{line}' inside a hunk", i + 1)),
    }
    i += 1;
    if lines.get(i).is_some_and(|next| next.starts_with('\\')) {
      match hunk.lines.last() {
        Some(HunkLine::Remove(_)) => hunk.old_missing_newline = true,
        Some(HunkLine::Add(_)) => hunk.new_missing_newline = true,
        Some(HunkLine::Context(_)) => {
          hunk.old_missing_newline = true;
          hunk.new_missing_newline = true;
        }
        None => {}
      }
      i += 1;
    }
  }
  if old_seen != old_len || new_seen != new_len {
    return Err(format!(
      "hunk at old line {old_start} has {old_seen}/{new_seen} lines, header says {old_len}/{new_len}"
    ));
  }
  Ok((hunk, i))
}

/// Applies `patch` to `original` (`None` when the file does not exist).
/// Returns the new content, or `None` when the patch deletes the file.
pub fn apply(original: Option<&str>, patch: &FilePatch) -> Result<Option<String>, String> {
  let name = patch.target_path().unwrap_or_default();
  match (original, &patch.old_path) {
    (Some(_), None) => return Err(format!("'{name}' already exists")),
    (None, Some(_)) => return Err(format!("'{name}' does not exist")),
    _ => {}
  }
  let original = original.unwrap_or("");
  let mut ends_with_newline = original.ends_with('\n');
  let body = original.strip_suffix('\n').unwrap_or(original);
  let mut lines: Vec<String> = if original.is_empty() {
    Vec::new()
  } else {
    body.split('\n').map(str::to_string).collect()
  };

  // Where the next hunk may start, and how far earlier hunks shifted lines.
  let (mut min_start, mut shift) = (0usize, 0isize);
  for hunk in &patch.hunks {
    let old: Vec<&str> = hunk
      .lines
      .iter()
      .filter_map(|line| match line {
        HunkLine::Context(text) | HunkLine::Remove(text) => Some(text.as_str()),
        HunkLine::Add(_) => None,
      })
      .collect();
    let new: Vec<String> = hunk
      .lines
      .iter()
      .filter_map(|line| match line {
        HunkLine::Context(text) | HunkLine::Add(text) => Some(text.clone()),
        HunkLine::Remove(_) => None,
      })
      .collect();
    // A zero-length old side's header names the line *after* which to insert.
    let header_index = if old.is_empty() {
      hunk.old_start
    } else {
      hunk.old_start.saturating_sub(1)
    };
    let expected = (header_index as isize + shift).max(min_start as isize) as usize;
    let at = find_hunk(&lines, &old, expected, min_start).ok_or_else(|| {
      format!(
        "hunk at old line {} does not apply to '{name}'",
        hunk.old_start
      )
    })?;
    let touches_end = at + old.len() == lines.len();
    lines.splice(at..at + old.len(), new.iter().cloned());
    if touches_end {
      if hunk.new_missing_newline {
        ends_with_newline = false;
      } else if hunk.old_missing_newline || !new.is_empty() {
        ends_with_newline = true;
      }
    }
    shift += new.len() as isize - old.len() as isize;
    min_start = at + new.len();
  }

  if patch.new_path.is_none() {
    if !lines.is_empty() {
      return Err(format!(
        "deleting '{name}' would drop lines the patch does not remove"
      ));
    }
    return Ok(None);
  }
  if lines.is_empty() {
    return Ok(Some(String::new()));
  }
  let mut content = lines.join("\n");
  if ends_with_newline {
    content.push('\n');
  }
  Ok(Some(content))
}

/// The first index at or after `min_start` where `old` matches `lines`,
/// searching outward from `expected`.
fn find_hunk(lines: &[String], old: &[&str], expected: usize, min_start: usize) -> Option<usize> {
  let matches = |at: usize| {
    at + old.len() <= lines.len()
      && lines[at..at + old.len()]
        .iter()
        .zip(old)
        .all(|(a, b)| a == b)
  };
  let last = lines.len().checked_sub(old.len())?;
  for distance in 0..=lines.len() {
    let candidates = [
      expected.checked_add(distance),
      expected.checked_sub(distance),
    ];
    for at in candidates.into_iter().flatten() {
      if at >= min_start && at <= last && matches(at) {
        return Some(at);
      }
    }
  }
  None
}

#[cfg(test)]
mod tests {
  use super::*;

  fn apply_one(original: Option<&str>, patch: &str) -> Result<Option<String>, String> {
    let patches = parse(patch)?;
    assert_eq!(patches.len(), 1, "{patches:?}");
    apply(original, &patches[0])
  }

  #[test]
  fn replaces_a_line() {
    let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-hi\n+patched\n";
    assert_eq!(
      apply_one(Some("hi\n"), patch).unwrap().as_deref(),
      Some("patched\n")
    );
  }

  #[test]
  fn reads_paths_without_git_prefixes_and_skips_git_headers() {
    let patch = "diff --git a/src/x.rs b/src/x.rs\nindex 1..2 100644\n--- a/src/x.rs\n+++ b/src/x.rs\n@@ -1 +1 @@\n-a\n+b\n";
    let patches = parse(patch).unwrap();
    assert_eq!(patches[0].old_path.as_deref(), Some("src/x.rs"));
    assert_eq!(patches[0].target_path(), Some("src/x.rs"));
  }

  #[test]
  fn applies_several_hunks_with_context() {
    let original = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
    let patch =
      "--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n 1\n-2\n+two\n 3\n@@ -8,3 +8,4 @@\n 8\n 9\n+9.5\n 10\n";
    assert_eq!(
      apply_one(Some(original), patch).unwrap().as_deref(),
      Some("1\ntwo\n3\n4\n5\n6\n7\n8\n9\n9.5\n10\n")
    );
  }

  #[test]
  fn finds_a_hunk_whose_header_line_is_off() {
    // Two lines were added above since the patch was made.
    let original = "x\ny\n1\n2\n3\n";
    let patch = "--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n 1\n-2\n+two\n 3\n";
    assert_eq!(
      apply_one(Some(original), patch).unwrap().as_deref(),
      Some("x\ny\n1\ntwo\n3\n")
    );
  }

  #[test]
  fn refuses_a_hunk_whose_context_does_not_match() {
    let patch = "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n";
    let err = apply_one(Some("a\nzzz\n"), patch).unwrap_err();
    assert!(err.contains("does not apply"), "{err}");
  }

  #[test]
  fn creates_and_deletes_files() {
    let create = "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n";
    let patches = parse(create).unwrap();
    assert_eq!(patches[0].old_path, None);
    assert_eq!(
      apply(None, &patches[0]).unwrap().as_deref(),
      Some("one\ntwo\n")
    );

    let delete = "--- a/old.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-gone\n";
    let patches = parse(delete).unwrap();
    assert_eq!(patches[0].target_path(), Some("old.txt"));
    assert_eq!(apply(Some("gone\n"), &patches[0]).unwrap(), None);
  }

  #[test]
  fn refuses_to_create_a_file_that_exists_or_patch_one_that_does_not() {
    let create = "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+one\n";
    assert!(apply_one(Some("already here\n"), create).is_err());
    let modify = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+b\n";
    assert!(apply_one(None, modify).is_err());
  }

  #[test]
  fn honours_no_newline_at_end_of_file_markers() {
    let add_newline = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-end\n\\ No newline at end of file\n+end\n";
    assert_eq!(
      apply_one(Some("end"), add_newline).unwrap().as_deref(),
      Some("end\n")
    );
    let drop_newline = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-end\n+end\n\\ No newline at end of file\n";
    assert_eq!(
      apply_one(Some("end\n"), drop_newline).unwrap().as_deref(),
      Some("end")
    );
  }

  #[test]
  fn parses_a_patch_that_touches_two_files() {
    let patch = "--- a/a\n+++ b/a\n@@ -1 +1 @@\n-1\n+2\n--- a/b\n+++ b/b\n@@ -1 +1 @@\n-3\n+4\n";
    let patches = parse(patch).unwrap();
    let targets: Vec<_> = patches.iter().map(|p| p.target_path()).collect();
    assert_eq!(targets, vec![Some("a"), Some("b")]);
  }

  #[test]
  fn rejects_malformed_patches() {
    assert!(parse("").is_err());
    assert!(parse("not a patch\n").is_err());
    // Header promises two old lines, body has one.
    assert!(parse("--- a/f\n+++ b/f\n@@ -1,2 +1 @@\n-a\n").is_err());
  }
}
