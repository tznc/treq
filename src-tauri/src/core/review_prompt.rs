//! Tuning the review prompt with an agent, reviewed like code.
//!
//! "Tune with agent" writes the prompt in use into a snapshot directory,
//! `.treq/review-prompt/`, next to the last 30 days of review history. The
//! agent records each edit it proposes as a local review comment on
//! `body.md` with a suggestion, through the same `treq agent-review add` it
//! uses on a diff. Settings lists those comments with the review cards;
//! applying one replaces the exact text it quotes in the saved prompt, so it
//! still lands after other edits have moved lines around.

use std::path::{Component, Path, PathBuf};

use crate::{
  core::{agent_launch, linear_review::splice_quoted_in, review_history},
  local_db::{self, AgentReviewComment},
};

/// The review comment target of a proposed prompt edit.
pub const TARGET_TYPE: &str = "review_prompt";
/// A repository has one review prompt, so one target id.
pub const TARGET_ID: &str = "repo";
/// Snapshot file holding the prompt being tuned.
pub const BODY_FILE: &str = "body.md";
/// Snapshot file holding the review history.
pub const HISTORY_FILE: &str = "review-history.json";

pub fn is_target(target_type: &str) -> bool {
  target_type == TARGET_TYPE
}

/// `<repo>/.treq/review-prompt`.
pub fn snapshot_dir(repo_path: &str) -> PathBuf {
  Path::new(repo_path).join(".treq").join("review-prompt")
}

/// Writes the prompt in use (`custom`, else the built-in one) and the
/// review history into the snapshot directory, and returns it.
pub fn write_snapshot(repo_path: &str, custom: Option<&str>) -> Result<PathBuf, String> {
  let dir = snapshot_dir(repo_path);
  std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
  let write = |name: &str, text: &str| {
    let path = dir.join(name);
    std::fs::write(&path, text).map_err(|e| format!("Failed to write {}: {e}", path.display()))
  };
  write(BODY_FILE, agent_launch::review_template(custom))?;
  write(HISTORY_FILE, &review_history::export_json(repo_path)?)?;
  Ok(dir)
}

/// The exact text of `start_line..=end_line` of `body.md`, kept on the
/// review comment so applying it can find that text again.
pub fn quote_lines(
  repo_path: &str,
  file: &str,
  start_line: i64,
  end_line: i64,
) -> Result<String, String> {
  let file = file.trim().trim_start_matches("./");
  if file != BODY_FILE
    || !Path::new(file)
      .components()
      .all(|c| matches!(c, Component::Normal(_)))
  {
    return Err(format!(
      "invalid_arguments: --file must be {BODY_FILE} for a review prompt, got '{file}'"
    ));
  }
  let content = std::fs::read_to_string(snapshot_dir(repo_path).join(BODY_FILE)).map_err(|_| {
    "invalid_arguments: no review prompt snapshot. Start tuning from Settings so treq writes it"
      .to_string()
  })?;
  let lines: Vec<&str> = content.lines().collect();
  if start_line < 1 || end_line < start_line || end_line as usize > lines.len() {
    return Err(format!(
      "invalid_arguments: lines {start_line}..{end_line} are outside {BODY_FILE} ({} lines)",
      lines.len()
    ));
  }
  Ok(lines[(start_line - 1) as usize..end_line as usize].join("\n"))
}

/// The proposed edits not applied, resolved or deleted yet.
pub fn suggestions(repo_path: &str) -> Result<Vec<AgentReviewComment>, String> {
  Ok(
    local_db::list_agent_review_comments(repo_path, TARGET_TYPE, TARGET_ID)?
      .into_iter()
      .filter(|c| c.status == "open")
      .collect(),
  )
}

/// Applies proposed edit `id` to the prompt in use (`custom`, else the
/// built-in one), resolves it, and returns the new prompt to save.
pub fn apply(repo_path: &str, id: &str, custom: Option<&str>) -> Result<String, String> {
  let comment = local_db::get_agent_review_comment(repo_path, id)?
    .filter(|c| is_target(&c.target_type))
    .ok_or_else(|| format!("Review prompt suggestion '{id}' not found"))?;
  let replacement = comment
    .suggested_replacement
    .as_deref()
    .ok_or_else(|| "This comment has no suggested change".to_string())?;
  let quoted = comment.quoted_text.as_deref().unwrap_or_default();
  let replacement = replacement.strip_suffix('\n').unwrap_or(replacement);
  let prompt = splice_quoted_in(
    agent_launch::review_template(custom),
    quoted,
    replacement,
    "the review prompt",
  )?;
  local_db::resolve_agent_review_comment(repo_path, id)?;
  Ok(prompt)
}

/// What the tuning agent is asked: find patterns in the history and record
/// each proposed edit as a review comment with a suggestion.
pub fn tune_prompt(snapshot_dir: &str) -> String {
  format!(
    "Help me tune my code review agent's prompt from how I reacted to its reviews.\n\n\
     treq wrote two files into {snapshot_dir}:\n\
     - {BODY_FILE}: the review prompt in use. Line numbers in it are what --start-line and \
     --end-line refer to.\n\
     - {HISTORY_FILE}: the last 30 days of code review history. Rows with kind \
     \"agent_comment\" are the review agent's comments, and their outcome is what I did: \
     \"applied\" (I applied its suggestion), \"resolved\" (I agreed), \"dismissed\" (I deleted \
     it) or \"open\" (no decision yet). Rows with kind \"user_review\" are reviews I wrote \
     myself and sent to an agent: they show what the review agent missed.\n\n\
     Work through these steps in order.\n\n\
     1. List the edits already proposed, so you do not repeat one:\n\n\
     \x20    treq agent-review list --target-type {TARGET_TYPE} --target-id {TARGET_ID}\n\n\
     2. Find patterns in the history: which kinds of comments I dismiss, which I act on, \
     and what I keep having to point out myself.\n\n\
     3. Record each edit to {BODY_FILE} that follows from a pattern as its own comment, \
     saying which pattern it answers and quoting an example from the history:\n\n\
     \x20 treq agent-review add \\\n\
     \x20   --target-type {TARGET_TYPE} \\\n\
     \x20   --target-id {TARGET_ID} \\\n\
     \x20   --file {BODY_FILE} \\\n\
     \x20   --start-line <first line> --end-line <last line> \\\n\
     \x20   --comment \"<the pattern and why this edit answers it>\" \\\n\
     \x20   --suggestion \"<the replacement text for those lines>\"\n\n\
     To add text, replace the line it should follow with that line plus the new text. Keep \
     the {{target_type}}, {{target_id}} and {{diff_summary}} placeholders.\n\n\
     Do not edit {BODY_FILE}, settings or any other file. I review each edit in Settings → \
     Code Review and apply the ones I want."
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn propose(repo: &str, start: i64, end: i64, suggestion: &str) -> AgentReviewComment {
    let quoted = quote_lines(repo, BODY_FILE, start, end).unwrap();
    local_db::create_agent_review_comment(
      repo,
      TARGET_TYPE,
      TARGET_ID,
      BODY_FILE,
      None,
      start,
      end,
      None,
      "You dismiss style nits",
      Some(suggestion),
      "local-agent",
      Some(&quoted),
    )
    .unwrap()
  }

  #[test]
  fn the_snapshot_holds_the_prompt_in_use_and_the_history() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    review_history::record_sent_review(repo, None, "changes", "Nit", "plan").unwrap();
    let snapshot = write_snapshot(repo, Some("Be strict\nFlag bugs")).unwrap();
    assert_eq!(
      std::fs::read_to_string(snapshot.join(BODY_FILE)).unwrap(),
      "Be strict\nFlag bugs"
    );
    let history = std::fs::read_to_string(snapshot.join(HISTORY_FILE)).unwrap();
    assert!(history.contains("\"Nit\""), "{history}");

    write_snapshot(repo, Some("  ")).unwrap();
    let built_in = std::fs::read_to_string(snapshot.join(BODY_FILE)).unwrap();
    assert!(built_in.starts_with("You are reviewing"), "{built_in}");
  }

  #[test]
  fn quoting_takes_lines_of_the_body_only() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    assert!(
      quote_lines(repo, BODY_FILE, 1, 1).is_err(),
      "no snapshot yet"
    );
    write_snapshot(repo, Some("a\nb\nc")).unwrap();
    assert_eq!(quote_lines(repo, "./body.md", 2, 3).unwrap(), "b\nc");
    assert!(quote_lines(repo, BODY_FILE, 3, 4).is_err());
    assert!(quote_lines(repo, "../config.yaml", 1, 1).is_err());
    assert!(quote_lines(repo, HISTORY_FILE, 1, 1).is_err());
  }

  #[test]
  fn applying_a_suggestion_rewrites_the_quoted_text_and_resolves_it() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    write_snapshot(repo, Some("Be strict\nFlag style\nFlag bugs")).unwrap();
    let edit = propose(repo, 2, 2, "Skip style nits\n");
    let other = propose(repo, 3, 3, "Flag bugs and missing tests");
    assert_eq!(suggestions(repo).unwrap().len(), 2);

    // The user edited the first line since; the quoted text still lands.
    let saved = "Be very strict\nFlag style\nFlag bugs";
    let prompt = apply(repo, &edit.id, Some(saved)).unwrap();
    assert_eq!(prompt, "Be very strict\nSkip style nits\nFlag bugs");
    let open = suggestions(repo).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, other.id);

    let err = apply(repo, &other.id, Some("Something else")).unwrap_err();
    assert!(err.contains("no longer in the review prompt"), "{err}");
  }

  #[test]
  fn tuning_edits_stay_out_of_the_code_review_history() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    write_snapshot(repo, Some("a")).unwrap();
    propose(repo, 1, 1, "b");
    assert!(local_db::list_review_history(repo, 10).unwrap().is_empty());
  }

  #[test]
  fn the_tuning_prompt_points_the_agent_at_the_snapshot_and_the_command() {
    let prompt = tune_prompt("/r/.treq/review-prompt");
    assert!(prompt.contains("/r/.treq/review-prompt"));
    assert!(prompt.contains("--target-type review_prompt"));
    assert!(prompt.contains("--target-id repo"));
    assert!(prompt.contains("--file body.md"));
    assert!(prompt.contains("{target_type}"));
  }
}
