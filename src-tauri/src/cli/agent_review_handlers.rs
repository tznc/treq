use std::path::{Component, Path, PathBuf};

use tauri_plugin_cli::Matches;

use crate::core;
use crate::core::linear_review;
use crate::local_db;
use crate::review_aggregate;

use super::{detect_repo_path, get_arg_value, print_json, OutputFormat};

/// Comment rows written by `treq agent review` always carry this source, so a
/// later local producer can be told apart from the review agent's output.
const LOCAL_AGENT_SOURCE: &str = "local-agent";

/// Default target type when the caller does not pass `--target-type`.
const DEFAULT_TARGET_TYPE: &str = "workspace_diff";

/// Human-readable resolution state for a normalized comment. Sources that do
/// not track resolution print `unknown` rather than claiming a state.
fn describe_state(resolved: Option<bool>) -> &'static str {
  match resolved {
    Some(true) => "resolved",
    Some(false) => "open",
    None => "unknown",
  }
}

fn require_value(matches: &Matches, name: &str) -> Result<String, String> {
  get_arg_value(matches, name).ok_or_else(|| format!("--{name} is required"))
}

fn parse_line_number(value: &str, name: &str) -> Result<i64, String> {
  value
    .trim()
    .parse::<i64>()
    .ok()
    .filter(|line| *line > 0)
    .ok_or_else(|| format!("--{name} must be a positive line number, got '{value}'"))
}

fn parse_side(value: Option<&str>) -> Result<Option<String>, String> {
  match value.map(str::trim) {
    None | Some("") => Ok(None),
    Some("old") => Ok(Some("old".to_string())),
    Some("new") => Ok(Some("new".to_string())),
    Some(other) => Err(format!("invalid side '{other}'. Expected one of: old, new")),
  }
}

/// Strip a GitHub-style ```suggestion fence, so the stored replacement is the
/// raw text the apply command splices into the file. Text without a fence is
/// returned unchanged.
pub(super) fn strip_suggestion_fence(text: &str) -> String {
  let trimmed = text.trim();
  let Some(rest) = trimmed.strip_prefix("```suggestion") else {
    return text.to_string();
  };
  let body = rest.strip_prefix('\n').unwrap_or(rest).trim_end();
  let body = body.strip_suffix("```").unwrap_or(body);
  body.trim_end_matches('\n').to_string()
}

/// `treq agent-review <action>` — writes local-only review comments straight to
/// the repo's local DB. No running GUI instance is needed: nothing here touches
/// a live session, it only records what the review agent found.
///
/// `list` is the exception to "local only": it reports everything that already
/// comments on the target, so an agent can avoid repeating a finding. Its JSON
/// output is an array of `NormalizedReviewComment` — one object shape for all
/// three sources, tagged `source: "local-agent" | "local-human" | "github"`,
/// with `id`, `file_path`, `start_line`, `end_line`, `side`, `body`,
/// `suggested_replacement` and `resolved` (null when the source does not track
/// resolution).
pub(super) fn handle_agent_review_command(
  matches: &Matches,
  format: OutputFormat,
) -> Result<(), String> {
  let action = get_arg_value(matches, "action")
    .ok_or_else(|| "agent review action is required".to_string())?;
  let repo_path = detect_repo_path()?;
  core::init(&repo_path).map_err(|e| format!("Failed to initialize repo: {e}"))?;

  match action.as_str() {
    "add" => {
      let start_line = parse_line_number(&require_value(matches, "start-line")?, "start-line")?;
      let end_line = match get_arg_value(matches, "end-line") {
        Some(value) => parse_line_number(&value, "end-line")?,
        None => start_line,
      };
      if start_line < 1 || end_line < start_line {
        return Err(format!(
          "invalid line range {start_line}..{end_line}: expected 1 <= start-line <= end-line"
        ));
      }
      let suggestion = get_arg_value(matches, "suggestion").map(|s| strip_suggestion_fence(&s));
      let target_type =
        get_arg_value(matches, "target-type").unwrap_or_else(|| DEFAULT_TARGET_TYPE.to_string());
      let target_id = require_value(matches, "target-id")?;
      let file = require_value(matches, "file")?;
      let side = parse_side(get_arg_value(matches, "side").as_deref())?;
      // Linear content is reviewed from a snapshot; keep the exact text the
      // comment covers so a suggestion can be applied to the live content.
      let quoted_text = if linear_review::is_linear_target(&target_type) {
        Some(linear_review::quote_snapshot_lines(
          &repo_path,
          &target_type,
          &target_id,
          &file,
          start_line,
          end_line,
        )?)
      } else {
        validate_add_target(
          &repo_path,
          &target_type,
          &target_id,
          &file,
          end_line,
          side.as_deref(),
        )?;
        None
      };
      let comment = local_db::create_agent_review_comment(
        &repo_path,
        &target_type,
        &target_id,
        &file,
        None,
        start_line,
        end_line,
        side.as_deref(),
        &require_value(matches, "comment")?,
        suggestion.as_deref(),
        LOCAL_AGENT_SOURCE,
        quoted_text.as_deref(),
      )?;
      match format {
        OutputFormat::Json => print_json(&comment),
        OutputFormat::Human => {
          println!(
            "Added review comment {} on {}:{}-{}",
            comment.id, comment.file_path, comment.start_line, comment.end_line
          );
          Ok(())
        }
      }
    }
    "list" => {
      let comments = review_aggregate::aggregate_review_comments(
        &repo_path,
        &get_arg_value(matches, "target-type").unwrap_or_else(|| DEFAULT_TARGET_TYPE.to_string()),
        &require_value(matches, "target-id")?,
      )?;
      match format {
        OutputFormat::Json => print_json(&comments),
        OutputFormat::Human => {
          if comments.is_empty() {
            println!("No review comments");
          }
          for comment in &comments {
            println!(
              "{} [{}] [{}] {}:{}-{} {}",
              comment.id,
              comment.source,
              describe_state(comment.resolved),
              comment.file_path,
              comment.start_line,
              comment.end_line,
              comment.body
            );
          }
          Ok(())
        }
      }
    }
    "resolve" => {
      let comment_id = require_value(matches, "comment-id")?;
      resolve_comment(&repo_path, &comment_id)?;
      match format {
        OutputFormat::Json => print_json(&serde_json::json!({ "resolved": comment_id })),
        OutputFormat::Human => {
          println!("Resolved review comment {comment_id}");
          Ok(())
        }
      }
    }
    "delete" => {
      let comment_id = require_value(matches, "comment-id")?;
      delete_comment(&repo_path, &comment_id)?;
      match format {
        OutputFormat::Json => print_json(&serde_json::json!({ "deleted": comment_id })),
        OutputFormat::Human => {
          println!("Deleted review comment {comment_id}");
          Ok(())
        }
      }
    }
    other => Err(format!("unknown agent review action '{other}'")),
  }
}

/// Target types the app renders review comments for. The Linear ones are
/// validated against their snapshot instead (`linear_review`).
const KNOWN_TARGET_TYPES: &[&str] = &[
  DEFAULT_TARGET_TYPE,
  "file_browser_file",
  linear_review::TARGET_LINEAR_ISSUE,
  linear_review::TARGET_LINEAR_PROJECT,
  linear_review::TARGET_LINEAR_DOCUMENT,
];

/// Rejects an `add` that nothing could display: an unknown target type, a
/// target that does not exist, a `--file` that is not relative, or (on the
/// new side, which is the file on disk) lines past the end of the file.
fn validate_add_target(
  repo_path: &str,
  target_type: &str,
  target_id: &str,
  file: &str,
  end_line: i64,
  side: Option<&str>,
) -> Result<(), String> {
  // The directory `--file` is relative to.
  let root = match target_type {
    DEFAULT_TARGET_TYPE => {
      let id = target_id.trim().parse::<i64>().ok().filter(|id| *id > 0);
      let id = id.ok_or_else(|| {
        format!("invalid_arguments: --target-id must be a workspace id, got '{target_id}'")
      })?;
      let workspace_dir = core::changes::resolve_workspace_dir(repo_path, Some(id))
        .map_err(|_| format!("workspace_not_found: workspace {id} not found"))?;
      PathBuf::from(workspace_dir)
    }
    "file_browser_file" => PathBuf::from(repo_path),
    other => {
      return Err(format!(
        "invalid_arguments: unknown --target-type '{other}'. Expected one of: {}",
        KNOWN_TARGET_TYPES.join(", ")
      ))
    }
  };
  let relative = Path::new(file);
  if !relative
    .components()
    .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
  {
    return Err(format!(
      "invalid_arguments: --file must be a relative path inside the repository, got '{file}'"
    ));
  }
  // Canonicalize so a symlink cannot point the line check outside the root.
  let resolved = match (root.join(relative).canonicalize(), root.canonicalize()) {
    (Ok(path), Ok(root)) if path.starts_with(&root) && path.is_file() => Some(path),
    _ => None,
  };
  if target_type == "file_browser_file"
    && (resolved.is_none() || resolved != Path::new(target_id).canonicalize().ok())
  {
    return Err(format!(
      "invalid_arguments: --target-id '{target_id}' must be the repository file --file '{file}' names"
    ));
  }
  if side == Some("old") {
    return Ok(());
  }
  let content = resolved
    .and_then(|path| std::fs::read(path).ok())
    .ok_or_else(|| {
      format!("invalid_arguments: --file '{file}' does not exist in the repository")
    })?;
  let line_count = String::from_utf8_lossy(&content).lines().count();
  if end_line as usize > line_count {
    return Err(format!(
      "invalid_arguments: line {end_line} is past the end of {file} ({line_count} lines)"
    ));
  }
  Ok(())
}

fn require_comment(repo_path: &str, comment_id: &str) -> Result<(), String> {
  match local_db::get_agent_review_comment(repo_path, comment_id)? {
    Some(_) => Ok(()),
    None => Err(format!("review comment '{comment_id}' not found")),
  }
}

/// The local_db writes are no-ops for an unknown id; the CLI must not report
/// success for a comment that does not exist.
fn resolve_comment(repo_path: &str, comment_id: &str) -> Result<(), String> {
  require_comment(repo_path, comment_id)?;
  local_db::resolve_agent_review_comment(repo_path, comment_id)
}

fn delete_comment(repo_path: &str, comment_id: &str) -> Result<(), String> {
  require_comment(repo_path, comment_id)?;
  local_db::delete_agent_review_comment(repo_path, comment_id)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn strips_a_suggestion_fence() {
    assert_eq!(
      strip_suggestion_fence("```suggestion\nlet x = 1;\nlet y = 2;\n```"),
      "let x = 1;\nlet y = 2;"
    );
  }

  #[test]
  fn resolving_or_deleting_a_missing_comment_fails() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_str().unwrap();
    assert!(resolve_comment(repo, "nope")
      .unwrap_err()
      .contains("not found"));
    assert!(delete_comment(repo, "nope")
      .unwrap_err()
      .contains("not found"));

    let comment = local_db::create_agent_review_comment(
      repo,
      DEFAULT_TARGET_TYPE,
      "feat/x",
      "a.rs",
      None,
      1,
      1,
      None,
      "body",
      None,
      LOCAL_AGENT_SOURCE,
      None,
    )
    .unwrap();
    resolve_comment(repo, &comment.id).unwrap();
    delete_comment(repo, &comment.id).unwrap();
    assert!(delete_comment(repo, &comment.id).is_err());
  }

  #[test]
  fn leaves_unfenced_text_alone() {
    assert_eq!(strip_suggestion_fence("let x = 1;"), "let x = 1;");
  }

  #[test]
  fn keeps_an_empty_suggestion_empty() {
    assert_eq!(strip_suggestion_fence("```suggestion\n```"), "");
  }

  #[test]
  fn add_target_must_be_real_and_inside_the_repo() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_str().unwrap();
    let id = local_db::add_workspace(
      repo,
      "feat".into(),
      "feat".into(),
      "feat".into(),
      None,
      None,
      None,
    )
    .unwrap()
    .to_string();
    let workspace = dir.path().join(".treq/workspaces/feat");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("a.rs"), "1\n2\n3\n").unwrap();
    std::fs::write(dir.path().join("b.rs"), "1\n").unwrap();
    let b = dir.path().join("b.rs");
    let b = b.to_str().unwrap();
    std::fs::write(dir.path().join("big.rs"), "1\n2\n3\n").unwrap();
    let big = dir.path().join("big.rs");
    let big = big.to_str().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("o.rs"), "1\n2\n3\n4\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path().join("o.rs"), workspace.join("link.rs")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), workspace.join("dir")).unwrap();
    let check = |target_type: &str, target_id: &str, file: &str, end: i64, side: Option<&str>| {
      validate_add_target(repo, target_type, target_id, file, end, side)
    };

    check(DEFAULT_TARGET_TYPE, &id, "a.rs", 3, None).unwrap();
    check(DEFAULT_TARGET_TYPE, &id, "a.rs", 99, Some("old")).unwrap();
    check("file_browser_file", b, "b.rs", 1, None).unwrap();
    for (target_type, target_id, file, end) in [
      ("bogus", id.as_str(), "a.rs", 1),
      (DEFAULT_TARGET_TYPE, "999", "a.rs", 1),
      (DEFAULT_TARGET_TYPE, "feat", "a.rs", 1),
      (DEFAULT_TARGET_TYPE, id.as_str(), "/etc/passwd", 1),
      (DEFAULT_TARGET_TYPE, id.as_str(), "../../../b.rs", 1),
      (DEFAULT_TARGET_TYPE, id.as_str(), "missing.rs", 1),
      (DEFAULT_TARGET_TYPE, id.as_str(), "a.rs", 4),
      ("file_browser_file", "/etc/passwd", "b.rs", 1),
      ("file_browser_file", b, "b.rs", 2),
      ("file_browser_file", big, "b.rs", 3),
      ("file_browser_file", b, "missing.rs", 1),
      (DEFAULT_TARGET_TYPE, id.as_str(), "link.rs", 1),
      (DEFAULT_TARGET_TYPE, id.as_str(), "dir/o.rs", 1),
    ] {
      assert!(
        check(target_type, target_id, file, end, None).is_err(),
        "{target_type} {target_id} {file}:{end} was accepted"
      );
    }
  }

  #[test]
  fn line_numbers_must_be_positive() {
    assert_eq!(parse_line_number("7", "start-line"), Ok(7));
    for bad in ["0", "-1"] {
      let error = parse_line_number(bad, "start-line").unwrap_err();
      assert!(error.contains("must be a positive line number"), "{error}");
    }
  }

  #[test]
  fn rejects_an_unknown_side() {
    assert!(parse_side(Some("both")).is_err());
    assert_eq!(parse_side(Some("old")).unwrap().as_deref(), Some("old"));
    assert_eq!(parse_side(None).unwrap(), None);
  }
}
