//! Merges everything that already comments on a diff — this tool's own local
//! review comments, the user's local draft comments, and the review threads on
//! the branch's GitHub PR — into one normalized list.
//!
//! A review agent reads this before it writes anything, so it can skip a
//! finding somebody has already raised. The GitHub half is best effort: with no
//! `gh` binary, no GitHub remote, or no open PR, the local comments still list.

use crate::github::{GhReviewThread, GitRemoteInfo};
use crate::local_db::{self, AgentReviewComment};

/// Source tag on a normalized comment: this tool's review agent.
pub const SOURCE_LOCAL_AGENT: &str = "local-agent";
/// Source tag on a normalized comment: the user's own local draft comment.
pub const SOURCE_LOCAL_HUMAN: &str = "local-human";
/// Source tag on a normalized comment: a review thread on the GitHub PR.
pub const SOURCE_GITHUB: &str = "github";

/// Target type that carries human and GitHub comments as well as agent ones.
const TARGET_WORKSPACE_DIFF: &str = "workspace_diff";

/// One review comment from any source, with the field names normalized so a
/// caller never has to know which source it came from.
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub struct NormalizedReviewComment {
  /// `"local-agent"`, `"local-human"` or `"github"`.
  pub source: String,
  /// Identifier within the source. Stable enough to resolve or delete a
  /// `local-agent` comment; opaque for the other two.
  pub id: String,
  /// Path relative to the repo root, as the source recorded it.
  pub file_path: String,
  pub start_line: i64,
  pub end_line: i64,
  /// `"old"` or `"new"` when the source records a side.
  pub side: Option<String>,
  /// The comment text itself.
  pub body: String,
  /// Raw suggested replacement, when the source carries one.
  pub suggested_replacement: Option<String>,
  /// True when the source says the comment is dealt with, false when it says
  /// it is not, and None when the source does not track resolution at all
  /// (local draft comments).
  pub resolved: Option<bool>,
}

/// The frontend's `LineComment`, as stored in `pending_reviews.comments`. Only
/// the fields needed for a normalized listing are read; the rest are ignored so
/// a newer frontend shape cannot break the CLI.
#[derive(serde::Deserialize)]
struct StoredLineComment {
  #[serde(default)]
  id: String,
  file_path: String,
  start_line: i64,
  #[serde(default)]
  end_line: Option<i64>,
  text: String,
  #[serde(default)]
  line_side: Option<String>,
}

/// Map this tool's own review comments into the normalized shape.
pub fn normalize_agent_comments(comments: Vec<AgentReviewComment>) -> Vec<NormalizedReviewComment> {
  comments
    .into_iter()
    .map(|comment| NormalizedReviewComment {
      source: SOURCE_LOCAL_AGENT.to_string(),
      id: comment.id,
      file_path: comment.file_path,
      start_line: comment.start_line,
      end_line: comment.end_line,
      side: comment.side,
      body: comment.comment_text,
      suggested_replacement: comment.suggested_replacement,
      resolved: Some(comment.status == "resolved"),
    })
    .collect()
}

/// Map the JSON array stored in `pending_reviews.comments` into the normalized
/// shape. Unparseable JSON yields an empty list rather than an error: a broken
/// draft review must not stop an agent from listing everything else.
pub fn normalize_human_comments(comments_json: &str) -> Vec<NormalizedReviewComment> {
  let Ok(stored) = serde_json::from_str::<Vec<StoredLineComment>>(comments_json) else {
    return Vec::new();
  };
  stored
    .into_iter()
    .map(|comment| NormalizedReviewComment {
      source: SOURCE_LOCAL_HUMAN.to_string(),
      id: comment.id,
      file_path: comment.file_path,
      start_line: comment.start_line,
      end_line: comment.end_line.unwrap_or(comment.start_line),
      side: comment.line_side,
      body: comment.text,
      suggested_replacement: None,
      // Draft line comments have no resolved state to report.
      resolved: None,
    })
    .collect()
}

/// Map GitHub review threads into the normalized shape, one entry per thread.
/// A thread's comments are joined so the agent reads the whole exchange, and
/// the thread's single `line` becomes both ends of the range.
pub fn normalize_github_threads(threads: Vec<GhReviewThread>) -> Vec<NormalizedReviewComment> {
  threads
    .into_iter()
    .map(|thread| {
      let line = thread.line.unwrap_or(0);
      let body = thread
        .comments
        .iter()
        .map(|comment| format!("@{}: {}", comment.author.login, comment.body))
        .collect::<Vec<_>>()
        .join("\n\n");
      NormalizedReviewComment {
        source: SOURCE_GITHUB.to_string(),
        id: thread.id,
        file_path: thread.path,
        start_line: line,
        end_line: line,
        side: Some(if thread.diff_side.eq_ignore_ascii_case("LEFT") {
          "old".to_string()
        } else {
          "new".to_string()
        }),
        body,
        suggested_replacement: None,
        resolved: Some(thread.is_resolved),
      }
    })
    .collect()
}

/// Review threads on the GitHub PR for a workspace's branch, or an empty list
/// when any step of the lookup does not apply — no `gh`, no GitHub remote, no
/// PR, or a `gh` call that fails. Never returns an error.
fn github_comments_best_effort(repo_path: &str, workspace_id: i64) -> Vec<NormalizedReviewComment> {
  let Ok(Some(workspace)) = local_db::get_workspace_by_id(repo_path, workspace_id) else {
    return Vec::new();
  };
  let Some(gh) =
    crate::binary_paths::get_binary_path("gh").or_else(|| crate::binary_paths::detect_binary("gh"))
  else {
    return Vec::new();
  };
  let extended_path = crate::binary_paths::get_extended_path();
  let dir = &workspace.workspace_path;

  let remote: Option<GitRemoteInfo> = crate::github::get_git_remote_url_impl(dir)
    .ok()
    .flatten()
    .or_else(|| {
      crate::github::get_git_remote_url_impl(repo_path)
        .ok()
        .flatten()
    });
  let Some(remote) = remote else {
    return Vec::new();
  };

  let pr = crate::github::get_pr_info_via_gh_impl(&gh, dir, &workspace.branch_name, &extended_path);
  let Ok(Some(pr)) = pr else {
    return Vec::new();
  };

  match crate::github::gh_list_pr_review_threads_impl(
    &gh,
    &remote.owner,
    &remote.repo,
    pr.number,
    &extended_path,
  ) {
    Ok(threads) => normalize_github_threads(threads),
    Err(_) => Vec::new(),
  }
}

/// Every review comment on one reviewed target, normalized and tagged by
/// source. For `workspace_diff` targets this is the agent's own comments plus
/// the user's local draft comments plus the GitHub PR threads; every other
/// target type has only agent comments today, so only those are returned.
pub fn aggregate_review_comments(
  repo_path: &str,
  target_type: &str,
  target_id: &str,
) -> Result<Vec<NormalizedReviewComment>, String> {
  let agent = local_db::list_agent_review_comments(repo_path, target_type, target_id)?;
  let mut comments = normalize_agent_comments(agent);

  if target_type != TARGET_WORKSPACE_DIFF {
    return Ok(comments);
  }

  let Ok(workspace_id) = target_id.trim().parse::<i64>() else {
    return Ok(comments);
  };

  if let Some(review) = local_db::get_pending_review(repo_path, workspace_id)? {
    comments.extend(normalize_human_comments(&review.comments));
  }
  comments.extend(github_comments_best_effort(repo_path, workspace_id));

  comments.sort_by(|a, b| {
    a.file_path
      .cmp(&b.file_path)
      .then(a.start_line.cmp(&b.start_line))
      .then(a.source.cmp(&b.source))
  });
  Ok(comments)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::github::{GhAuthor, GhReviewComment};

  fn agent_comment(id: &str, status: &str) -> AgentReviewComment {
    AgentReviewComment {
      id: id.to_string(),
      repo_path: "/repo".to_string(),
      target_type: "workspace_diff".to_string(),
      target_id: "1".to_string(),
      file_path: "src/main.rs".to_string(),
      hunk_id: None,
      start_line: 10,
      end_line: 12,
      side: Some("new".to_string()),
      comment_text: "unchecked unwrap".to_string(),
      suggested_replacement: Some("let x = y?;".to_string()),
      status: status.to_string(),
      source: "local-agent".to_string(),
      created_at: "2024-01-01T00:00:00Z".to_string(),
      resolved_at: None,
      quoted_text: None,
    }
  }

  #[test]
  fn normalizes_agent_comments_with_resolved_state() {
    let normalized = normalize_agent_comments(vec![
      agent_comment("a", "open"),
      agent_comment("b", "resolved"),
    ]);
    assert_eq!(normalized[0].source, SOURCE_LOCAL_AGENT);
    assert_eq!(normalized[0].resolved, Some(false));
    assert_eq!(normalized[1].resolved, Some(true));
    assert_eq!(normalized[0].body, "unchecked unwrap");
    assert_eq!(
      normalized[0].suggested_replacement.as_deref(),
      Some("let x = y?;")
    );
  }

  #[test]
  fn normalizes_stored_human_line_comments() {
    let json = r#"[
      {
        "id": "c1",
        "file_path": "src/lib.rs",
        "hunk_id": "h1",
        "start_line": 4,
        "end_line": 6,
        "line_content": ["a", "b", "c"],
        "text": "this loop is quadratic",
        "created_at": "2024-01-01T00:00:00Z",
        "line_side": "new"
      }
    ]"#;
    let normalized = normalize_human_comments(json);
    assert_eq!(normalized.len(), 1);
    assert_eq!(normalized[0].source, SOURCE_LOCAL_HUMAN);
    assert_eq!(normalized[0].file_path, "src/lib.rs");
    assert_eq!(normalized[0].start_line, 4);
    assert_eq!(normalized[0].end_line, 6);
    assert_eq!(normalized[0].body, "this loop is quadratic");
    assert_eq!(normalized[0].side.as_deref(), Some("new"));
    // Draft comments carry no resolution state.
    assert_eq!(normalized[0].resolved, None);
  }

  #[test]
  fn falls_back_to_start_line_when_end_line_is_absent() {
    let json = r#"[{"id":"c1","file_path":"a.ts","start_line":9,"text":"nit"}]"#;
    let normalized = normalize_human_comments(json);
    assert_eq!(normalized[0].end_line, 9);
  }

  #[test]
  fn treats_unparseable_human_comment_json_as_empty() {
    assert!(normalize_human_comments("not json").is_empty());
    assert!(normalize_human_comments("").is_empty());
  }

  #[test]
  fn normalizes_github_threads_including_resolved_state() {
    let thread = GhReviewThread {
      id: "T1".to_string(),
      is_resolved: true,
      is_outdated: false,
      path: "src/main.rs".to_string(),
      line: Some(10),
      diff_side: "RIGHT".to_string(),
      comments: vec![GhReviewComment {
        id: "C1".to_string(),
        body: "same unwrap problem".to_string(),
        author: GhAuthor {
          login: "octocat".to_string(),
          avatar_url: None,
        },
        created_at: "2024-01-01T00:00:00Z".to_string(),
        diff_hunk: String::new(),
        url: "https://github.com/o/r/pull/1#discussion_r1".to_string(),
      }],
    };
    let normalized = normalize_github_threads(vec![thread]);
    assert_eq!(normalized.len(), 1);
    assert_eq!(normalized[0].source, SOURCE_GITHUB);
    assert_eq!(normalized[0].start_line, 10);
    assert_eq!(normalized[0].end_line, 10);
    assert_eq!(normalized[0].side.as_deref(), Some("new"));
    assert_eq!(normalized[0].resolved, Some(true));
    assert!(normalized[0].body.contains("same unwrap problem"));
    assert!(normalized[0].body.contains("@octocat"));
  }

  #[test]
  fn maps_a_left_side_thread_to_the_old_side() {
    let thread = GhReviewThread {
      id: "T2".to_string(),
      is_resolved: false,
      is_outdated: false,
      path: "a.ts".to_string(),
      line: None,
      diff_side: "LEFT".to_string(),
      comments: vec![],
    };
    let normalized = normalize_github_threads(vec![thread]);
    assert_eq!(normalized[0].side.as_deref(), Some("old"));
    // A thread with no line still lists, anchored at 0.
    assert_eq!(normalized[0].start_line, 0);
  }

  #[test]
  fn github_lookup_returns_empty_when_the_workspace_is_unknown() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo_path = dir.path().to_str().unwrap();
    // No DB, no git remote, no PR: the lookup must stay silent, not error.
    assert!(github_comments_best_effort(repo_path, 4242).is_empty());
  }
}
