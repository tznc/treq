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

  let path = std::path::Path::new(&comment.file_path);
  let path = if path.is_absolute() {
    path.to_path_buf()
  } else {
    std::path::Path::new(&repo_path).join(path)
  };

  let original = std::fs::read_to_string(&path)
    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
  let had_trailing_newline = original.ends_with('\n');
  let mut lines: Vec<String> = original.lines().map(|line| line.to_string()).collect();

  if comment.start_line < 1 || comment.end_line < comment.start_line {
    return Err(format!(
      "Invalid suggestion range {}..{}",
      comment.start_line, comment.end_line
    ));
  }
  let start = (comment.start_line - 1) as usize;
  let end = comment.end_line as usize;
  if end > lines.len() {
    return Err(format!(
      "Suggestion range {}..{} is outside {} ({} lines)",
      comment.start_line,
      comment.end_line,
      path.display(),
      lines.len()
    ));
  }

  let replacement_lines: Vec<String> = replacement
    .strip_suffix('\n')
    .unwrap_or(replacement)
    .split('\n')
    .map(|line| line.to_string())
    .collect();
  lines.splice(start..end, replacement_lines);

  let mut updated = lines.join("\n");
  if had_trailing_newline {
    updated.push('\n');
  }
  std::fs::write(&path, updated)
    .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;

  local_db::resolve_agent_review_comment(&repo_path, &comment.id)
}
