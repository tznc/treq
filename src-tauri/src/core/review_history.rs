//! Review history: what happened in each code review over the last 30 days,
//! for the Logs tab and for an agent that tunes the review prompt from it.
//! Rows are written by `local_db` as review comments are created and
//! decided, and here as reviews are sent.

use std::path::PathBuf;

use crate::{
  core::checks_logs::{severity_number_for_text, severity_text_for_level, LogQuery, LogRecordView},
  local_db::{self, ReviewHistoryEntry},
};

/// Rows the Logs tab and the export read at most.
const MAX_ROWS: usize = 5000;

/// Keeps a review the user sent to an agent.
pub fn record_sent_review(
  repo_path: &str,
  workspace_id: Option<i64>,
  scope: &str,
  markdown: &str,
  mode: &str,
) -> Result<(), String> {
  local_db::add_user_review_history(repo_path, workspace_id, scope, markdown, mode)
}

/// The level a row shows at: a dismissed agent comment is the signal a
/// prompt should stop asking for, so it stands out.
fn level(entry: &ReviewHistoryEntry) -> &'static str {
  match entry.outcome.as_str() {
    "dismissed" => "warning",
    _ => "info",
  }
}

/// `file:start-end`, or the scope for a sent review.
fn location(entry: &ReviewHistoryEntry) -> String {
  match (&entry.file_path, entry.start_line, entry.end_line) {
    (Some(file), Some(start), Some(end)) if start != end => format!("{file}:{start}-{end}"),
    (Some(file), Some(start), _) => format!("{file}:{start}"),
    (Some(file), None, _) => file.clone(),
    (None, ..) => entry.target_type.clone(),
  }
}

/// One Logs line per row: the outcome in the Outcome column, where it is
/// in the File column.
pub fn to_log_record(entry: &ReviewHistoryEntry) -> LogRecordView {
  let severity = severity_text_for_level(level(entry));
  let who = if entry.kind == "user_review" {
    "You"
  } else {
    "Agent"
  };
  let mut body = format!("{who}: {}", entry.body);
  if let Some(suggestion) = &entry.suggestion {
    body.push_str(&format!("\nSuggested:\n{suggestion}"));
  }
  LogRecordView {
    timestamp: entry.created_at.clone(),
    severity_number: severity_number_for_text(severity),
    severity_text: severity.to_string(),
    body,
    trace_id: String::new(),
    span_id: String::new(),
    run_id: entry.id,
    job_id: location(entry),
    step_index: 0,
    step_name: entry.outcome.clone(),
    stream: entry.kind.clone(),
  }
}

/// The Logs tab's Reviews lines, newest first, filtered by `query`'s
/// levels and search.
pub fn log_records(repo_path: &str, query: &LogQuery) -> Result<Vec<LogRecordView>, String> {
  let levels: Vec<&str> = query
    .levels
    .iter()
    .flatten()
    .map(|l| severity_text_for_level(l))
    .collect();
  let search = query
    .search
    .as_deref()
    .filter(|s| !s.is_empty())
    .map(str::to_lowercase);
  Ok(
    local_db::list_review_history(repo_path, MAX_ROWS)?
      .iter()
      .map(to_log_record)
      .filter(|r| levels.is_empty() || levels.contains(&r.severity_text.as_str()))
      .filter(|r| {
        search.as_ref().is_none_or(|s| {
          [&r.body, &r.job_id, &r.step_name]
            .iter()
            .any(|field| field.to_lowercase().contains(s))
        })
      })
      .collect(),
  )
}

/// Writes the history to `.treq/review-history.json` for the tuning agent
/// to read, and returns that path.
pub fn export(repo_path: &str) -> Result<PathBuf, String> {
  let entries = local_db::list_review_history(repo_path, MAX_ROWS)?;
  let dir = PathBuf::from(repo_path).join(".treq");
  std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create .treq directory: {e}"))?;
  let path = dir.join("review-history.json");
  let json = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
  std::fs::write(&path, json).map_err(|e| format!("Failed to write review history: {e}"))?;
  Ok(path)
}

/// What the tuning agent is asked: read the history, propose a revised
/// review prompt, and leave applying it to the user.
pub fn tune_prompt(current: Option<&str>, history_path: &str) -> String {
  let current = match current.map(str::trim).filter(|p| !p.is_empty()) {
    Some(prompt) => format!("The current custom review prompt:\n\n```\n{prompt}\n```"),
    None => "There is no custom review prompt yet; the built-in one is used.".to_string(),
  };
  format!(
    "Help me tune my code review agent's prompt from how I reacted to its reviews.\n\n\
     {current}\n\n\
     {history_path} holds the last 30 days of code review history as JSON. Rows with \
     kind \"agent_comment\" are the review agent's comments, and their outcome is what I \
     did: \"applied\" (I applied its suggestion), \"resolved\" (I agreed), \"dismissed\" (I \
     deleted it) or \"open\" (no decision yet). Rows with kind \"user_review\" are reviews I \
     wrote myself and sent to an agent: they show what the review agent missed.\n\n\
     1. Find patterns: which kinds of comments I dismiss, which I act on, and what I keep \
     having to point out myself. Quote a few examples for each.\n\
     2. Propose a revised review prompt that asks for more of what I act on, less of what \
     I dismiss, and covers what I raise myself. Keep the {{target_type}}, {{target_id}} and \
     {{diff_summary}} placeholders.\n\
     3. Show the change as a diff against the current prompt, then the full new prompt in \
     one fenced block.\n\n\
     Do not edit any settings or files. I will paste the prompt into Settings → Code Review \
     myself."
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn entry(kind: &str, outcome: &str) -> ReviewHistoryEntry {
    ReviewHistoryEntry {
      id: 1,
      kind: kind.into(),
      comment_id: None,
      source: None,
      workspace_id: Some(7),
      target_type: "changes".into(),
      target_id: None,
      file_path: None,
      start_line: None,
      end_line: None,
      body: "Rename this".into(),
      suggestion: None,
      quoted_text: None,
      mode: None,
      outcome: outcome.into(),
      created_at: "2026-10-26T00:00:00Z".into(),
      decided_at: None,
    }
  }

  #[test]
  fn a_log_record_shows_who_where_and_the_outcome() {
    let mut comment = entry("agent_comment", "dismissed");
    comment.file_path = Some("a.rs".into());
    comment.start_line = Some(2);
    comment.end_line = Some(4);
    comment.suggestion = Some("x?".into());
    let record = to_log_record(&comment);
    assert_eq!(record.job_id, "a.rs:2-4");
    assert_eq!(record.step_name, "dismissed");
    assert_eq!(record.severity_text, severity_text_for_level("warning"));
    assert_eq!(record.body, "Agent: Rename this\nSuggested:\nx?");

    let sent = to_log_record(&entry("user_review", "sent"));
    assert_eq!(sent.job_id, "changes");
    assert_eq!(sent.body, "You: Rename this");
    assert_eq!(sent.severity_text, severity_text_for_level("info"));
  }

  #[test]
  fn log_records_filter_by_search_and_level() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    record_sent_review(repo, None, "changes", "Split this function", "plan").unwrap();
    record_sent_review(repo, None, "files", "Add a test", "plan").unwrap();
    let query = |search: &str, levels: Option<Vec<String>>| LogQuery {
      search: Some(search.into()),
      levels,
      ..LogQuery::default()
    };
    let found = log_records(repo, &query("split", None)).unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].body.contains("Split this function"));
    assert!(log_records(repo, &query("", Some(vec!["warning".into()])))
      .unwrap()
      .is_empty());
  }

  #[test]
  fn export_writes_the_history_for_the_tuning_agent() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    record_sent_review(repo, Some(3), "changes", "Nit", "plan").unwrap();
    let path = export(repo).unwrap();
    let rows: Vec<ReviewHistoryEntry> =
      serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].body, "Nit");
  }

  #[test]
  fn tune_prompt_names_the_history_and_the_current_prompt() {
    let prompt = tune_prompt(Some("Be strict"), "/r/.treq/review-history.json");
    assert!(prompt.contains("/r/.treq/review-history.json"));
    assert!(prompt.contains("```\nBe strict\n```"));
    assert!(prompt.contains("{target_type}"));
    assert!(tune_prompt(Some("  "), "p").contains("built-in one is used"));
  }
}
