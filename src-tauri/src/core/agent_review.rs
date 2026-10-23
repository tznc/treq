use crate::result_ext::ResultExt;
use std::path::{Path, PathBuf};

use crate::local_db::{self, AgentReviewComment};

/// The comments left on a reviewed target (a PR, a workspace diff, a Linear
/// issue), ordered by file and line.
pub fn list_comments(
  repo_path: &str,
  target_type: &str,
  target_id: &str,
) -> Result<Vec<AgentReviewComment>, String> {
  local_db::list_agent_review_comments(repo_path, target_type, target_id)
}

pub fn resolve_comment(repo_path: &str, id: &str) -> Result<(), String> {
  local_db::resolve_agent_review_comment(repo_path, id)
}

pub fn delete_comment(repo_path: &str, id: &str) -> Result<(), String> {
  local_db::delete_agent_review_comment(repo_path, id)
}

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

/// Replaces lines `start_line..=end_line` (1-based) of `file_path` with
/// `replacement`. `file_path` is relative to `root`, or absolute inside it; a
/// path that leaves `root` (through `..`, an absolute path or a symlink) or
/// names `.jj`/`.git` is refused. With `expected`, refuses when those lines no
/// longer read so: the file changed since. Keeps the file's line endings and
/// writes through a temporary file, so a failed write leaves it intact.
pub fn splice_lines(
  root: &str,
  file_path: &str,
  start_line: i64,
  end_line: i64,
  expected: Option<&[String]>,
  replacement: &str,
) -> Result<(), String> {
  let path = contained_path(Path::new(root), file_path)?;

  let original = std::fs::read_to_string(&path)
    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
  let had_trailing_newline = original.ends_with('\n');
  let newline = if original.contains("\r\n") {
    "\r\n"
  } else {
    "\n"
  };
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
    text => text
      .split('\n')
      .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
      .collect(),
  };
  lines.splice(start..end, replacement_lines);

  let mut updated = lines.join(newline);
  if had_trailing_newline {
    updated.push_str(newline);
  }
  write_atomically(&path, &updated)
}

/// Resolves `file_path` under `root`, refusing anything that ends up outside it.
fn contained_path(root: &Path, file_path: &str) -> Result<PathBuf, String> {
  let root = root
    .canonicalize()
    .map_err(|e| format!("Failed to resolve {}: {}", root.display(), e))?;
  let requested = Path::new(file_path);
  let relative = if requested.is_absolute() {
    requested
      .strip_prefix(&root)
      .map_err(|_| format!("'{file_path}' is outside the workspace"))?
      .to_string_lossy()
      .into_owned()
  } else {
    file_path.to_string()
  };
  let repo_path = crate::jj::workspace_file_path(&relative)?;
  let path = repo_path
    .to_fs_path(&root)
    .map_err(|e| format!("'{file_path}': {e}"))?;
  // A symlink inside the workspace can still point out of it.
  let resolved = path
    .canonicalize()
    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
  if !resolved.starts_with(&root) {
    return Err(format!("'{file_path}' is outside the workspace"));
  }
  Ok(resolved)
}

fn write_atomically(path: &Path, contents: &str) -> Result<(), String> {
  let write_err = |e: std::io::Error| format!("Failed to write {}: {}", path.display(), e);
  let file_name = path.file_name().unwrap_or_default().to_string_lossy();
  let temp = path.with_file_name(format!(".{file_name}.treq-{}", std::process::id()));
  let result = std::fs::write(&temp, contents)
    .and_then(|()| {
      let permissions = std::fs::metadata(path)?.permissions();
      std::fs::set_permissions(&temp, permissions)
    })
    .and_then(|()| std::fs::rename(&temp, path));
  if result.is_err() {
    std::fs::remove_file(&temp).best_effort();
  }
  result.map_err(write_err)
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

  #[test]
  fn refuses_paths_outside_the_workspace() {
    let root = tempfile::TempDir::new().unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    std::fs::write(outside.path().join("x.rs"), "x\n").unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    std::fs::write(root.path().join(".git/config"), "c\n").unwrap();
    let root_str = root.path().to_str().unwrap();
    let escape = format!(
      "../{}/x.rs",
      outside.path().file_name().unwrap().to_string_lossy()
    );
    let absolute = outside.path().join("x.rs");

    for bad in [escape.as_str(), absolute.to_str().unwrap(), ".git/config"] {
      assert!(
        splice_lines(root_str, bad, 1, 1, None, "y").is_err(),
        "'{bad}' should be refused"
      );
    }
    assert_eq!(std::fs::read_to_string(&absolute).unwrap(), "x\n");
    assert_eq!(
      std::fs::read_to_string(root.path().join(".git/config")).unwrap(),
      "c\n"
    );
  }

  #[cfg(unix)]
  #[test]
  fn refuses_a_symlink_that_leaves_the_workspace() {
    let root = tempfile::TempDir::new().unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    std::fs::write(outside.path().join("x.rs"), "x\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();

    let result = splice_lines(root.path().to_str().unwrap(), "link/x.rs", 1, 1, None, "y");

    assert!(result.is_err());
    assert_eq!(
      std::fs::read_to_string(outside.path().join("x.rs")).unwrap(),
      "x\n"
    );
  }

  #[test]
  fn accepts_an_absolute_path_inside_the_workspace() {
    let root = tempfile::TempDir::new().unwrap();
    let file = root.path().join("a.rs");
    std::fs::write(&file, "a\n").unwrap();

    splice_lines(
      root.path().to_str().unwrap(),
      file.to_str().unwrap(),
      1,
      1,
      None,
      "A",
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "A\n");
  }

  #[test]
  fn keeps_crlf_line_endings() {
    let root = tempfile::TempDir::new().unwrap();
    let file = root.path().join("a.rs");
    std::fs::write(&file, "a\r\nb\r\nc\r\n").unwrap();
    let expected = vec!["b".to_string()];

    splice_lines(
      root.path().to_str().unwrap(),
      "a.rs",
      2,
      2,
      Some(&expected),
      "B1\nB2",
    )
    .unwrap();

    assert_eq!(
      std::fs::read_to_string(&file).unwrap(),
      "a\r\nB1\r\nB2\r\nc\r\n"
    );
  }
}
