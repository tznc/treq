//! Draft comments the user writes on a reviewed target (a PR review).

use crate::local_db::{self, AgentReviewComment, FileBrowserPendingReview, PendingReview};

/// A draft comment on lines `start_line..=end_line` of a file.
#[derive(Debug, Clone, Default)]
pub struct NewReviewDraft {
  pub target_type: String,
  pub target_id: String,
  pub file_path: String,
  pub start_line: i64,
  pub end_line: i64,
  pub side: Option<String>,
  pub comment_text: String,
  /// The hash of the commented lines, so the draft follows them.
  pub line_hash: Option<String>,
  /// The commented diff lines, shown if the draft goes outdated.
  pub quoted_text: Option<String>,
}

/// Saves `draft` as the user's own comment, with its line hash.
pub fn create_review_draft(
  repo_path: &str,
  draft: &NewReviewDraft,
) -> Result<AgentReviewComment, String> {
  let mut comment = local_db::create_agent_review_comment(
    repo_path,
    &draft.target_type,
    &draft.target_id,
    &draft.file_path,
    None,
    draft.start_line,
    draft.end_line,
    draft.side.as_deref(),
    &draft.comment_text,
    None,
    crate::review_aggregate::SOURCE_LOCAL_HUMAN,
    draft.quoted_text.as_deref(),
  )?;
  if let Some(hash) = draft.line_hash.as_deref() {
    local_db::update_agent_review_comment(repo_path, &comment.id, None, Some(hash), None)?;
    comment.line_hash = draft.line_hash.clone();
  }
  Ok(comment)
}

/// Edits a draft's text and/or the hash and text of the lines it comments on.
pub fn update_review_draft(
  repo_path: &str,
  id: &str,
  text: Option<&str>,
  line_hash: Option<&str>,
  quoted_text: Option<&str>,
) -> Result<(), String> {
  local_db::update_agent_review_comment(repo_path, id, text, line_hash, quoted_text)
}

/// The review a workspace's changes page left unsent.
pub fn pending_review(repo_path: &str, workspace_id: i64) -> Result<Option<PendingReview>, String> {
  local_db::get_pending_review(repo_path, workspace_id)
}

pub fn save_pending_review(
  repo_path: &str,
  workspace_id: i64,
  comments: &str,
  summary: Option<&str>,
  notes: Option<&str>,
) -> Result<(), String> {
  local_db::save_pending_review(repo_path, workspace_id, comments, None, summary, notes).map(|_| ())
}

pub fn clear_pending_review(repo_path: &str, workspace_id: i64) -> Result<(), String> {
  local_db::clear_pending_review(repo_path, workspace_id)
}

/// The review a workspace's file browser left unsent.
pub fn file_browser_review(
  repo_path: &str,
  workspace_id: i64,
) -> Result<Option<FileBrowserPendingReview>, String> {
  local_db::get_file_browser_review(repo_path, workspace_id)
}

pub fn save_file_browser_review(
  repo_path: &str,
  workspace_id: i64,
  comments: &str,
  summary: Option<&str>,
) -> Result<(), String> {
  local_db::save_file_browser_review(repo_path, workspace_id, comments, summary).map(|_| ())
}

pub fn clear_file_browser_review(repo_path: &str, workspace_id: i64) -> Result<(), String> {
  local_db::clear_file_browser_review(repo_path, workspace_id)
}

#[cfg(test)]
mod tests {
  use tempfile::TempDir;

  use super::*;

  fn draft(line_hash: Option<&str>) -> NewReviewDraft {
    NewReviewDraft {
      target_type: "github_pr".into(),
      target_id: "42".into(),
      file_path: "src/search.ts".into(),
      start_line: 3,
      end_line: 4,
      side: Some("new".into()),
      comment_text: "Trim first.".into(),
      line_hash: line_hash.map(str::to_string),
      quoted_text: Some("+a\n+b".into()),
    }
  }

  #[test]
  fn creates_a_human_draft_with_its_line_hash() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();

    let created = create_review_draft(repo, &draft(Some("abc123"))).unwrap();

    assert_eq!(created.line_hash.as_deref(), Some("abc123"));
    let listed = local_db::list_agent_review_comments(repo, "github_pr", "42").unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);
    assert_eq!(listed[0].line_hash.as_deref(), Some("abc123"));
    assert_eq!(listed[0].quoted_text.as_deref(), Some("+a\n+b"));
    assert_eq!(listed[0].comment_text, "Trim first.");
    assert_eq!((listed[0].start_line, listed[0].end_line), (3, 4));
    assert_eq!(listed[0].side.as_deref(), Some("new"));
    assert_eq!(
      listed[0].source,
      crate::review_aggregate::SOURCE_LOCAL_HUMAN
    );
  }

  #[test]
  fn records_the_lines_an_agent_draft_covers() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let mut new = draft(None);
    new.quoted_text = None;
    let created = create_review_draft(repo, &new).unwrap();

    update_review_draft(repo, &created.id, None, Some("h1"), Some("+x")).unwrap();

    let listed = local_db::list_agent_review_comments(repo, "github_pr", "42").unwrap();
    assert_eq!(listed[0].line_hash.as_deref(), Some("h1"));
    assert_eq!(listed[0].quoted_text.as_deref(), Some("+x"));
  }

  #[test]
  fn creates_a_draft_without_a_line_hash() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();

    let created = create_review_draft(repo, &draft(None)).unwrap();

    assert_eq!(created.line_hash, None);
    let listed = local_db::list_agent_review_comments(repo, "github_pr", "42").unwrap();
    assert_eq!(listed[0].line_hash, None);
  }
}
