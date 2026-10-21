use crate::local_db;

/// Replace `start_line..=end_line` of the comment's file with its suggested
/// text. Deliberately not the generic patch-file path: a suggestion already
/// carries an exact line range, so applying it is a splice, not a diff apply.
pub fn apply_agent_review_suggestion(repo_path: String, comment_id: String) -> Result<(), String> {
  let comment = local_db::get_agent_review_comment(&repo_path, &comment_id)?
    .ok_or_else(|| format!("Review comment '{}' not found", comment_id))?;

  let replacement = comment
    .suggested_replacement
    .as_deref()
    .ok_or_else(|| "Review comment has no suggested change".to_string())?;

  splice_lines(
    &repo_path,
    &comment.file_path,
    comment.start_line,
    comment.end_line,
    None,
    replacement,
  )?;
  local_db::resolve_agent_review_comment(&repo_path, &comment.id)
}

/// Replaces lines `start_line..=end_line` (1-based) of `file_path` (relative
/// to `root` unless absolute) with `replacement`. With `expected`, refuses
/// when those lines no longer read so: the file changed since.
pub fn splice_lines(
  root: &str,
  file_path: &str,
  start_line: i64,
  end_line: i64,
  expected: Option<&[String]>,
  replacement: &str,
) -> Result<(), String> {
  let path = std::path::Path::new(file_path);
  let path = if path.is_absolute() {
    path.to_path_buf()
  } else {
    std::path::Path::new(root).join(path)
  };

  let original = std::fs::read_to_string(&path)
    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
  let had_trailing_newline = original.ends_with('\n');
  let mut lines: Vec<String> = original.lines().map(|line| line.to_string()).collect();

  if start_line < 1 || end_line < start_line {
    return Err(format!(
      "Invalid suggestion range {}..{}",
      start_line, end_line
    ));
  }
  let start = (start_line - 1) as usize;
  let end = end_line as usize;
  if end > lines.len() {
    return Err(format!(
      "Suggestion range {}..{} is outside {} ({} lines)",
      start_line,
      end_line,
      path.display(),
      lines.len()
    ));
  }
  if expected.is_some_and(|expected| lines[start..end] != *expected) {
    return Err(format!(
      "{} changed since the suggestion was written",
      path.display()
    ));
  }

  let replacement_lines: Vec<String> = match replacement.strip_suffix('\n').unwrap_or(replacement) {
    "" if expected.is_some() => Vec::new(),
    text => text.split('\n').map(|line| line.to_string()).collect(),
  };
  lines.splice(start..end, replacement_lines);

  let mut updated = lines.join("\n");
  if had_trailing_newline {
    updated.push('\n');
  }
  std::fs::write(&path, updated).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn splices_lines_only_while_they_still_read_as_expected() {
    let dir = std::env::temp_dir().join(format!("treq-splice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.to_str().unwrap();
    std::fs::write(dir.join("a.rs"), "a\nb\nc\n").unwrap();
    let expected = vec!["b".to_string(), "c".to_string()];
    splice_lines(root, "a.rs", 2, 3, Some(&expected), "B\n").unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.rs")).unwrap(), "a\nB\n");
    let err = splice_lines(root, "a.rs", 2, 2, Some(&expected[..1]), "x").unwrap_err();
    assert!(err.contains("changed since"), "{err}");
    splice_lines(root, "a.rs", 2, 2, Some(&["B".to_string()]), "").unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.rs")).unwrap(), "a\n");
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
