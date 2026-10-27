use super::{resolve_workspace_path, Deserialize, Serialize};

/// [`TreqCommandRequest::WorkspacePullRequest`]'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemotePullRequestStatus {
  /// The repository's GitHub `owner/name`; absent when it has no GitHub
  /// remote, so no pull request can be made.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub github_repo: Option<String>,
  /// The branch's pull request, when it has one.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub pull_request: Option<crate::github::PrInfo>,
  /// Its checks, once any are reported.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub checks: Option<crate::github::PrCiStatus>,
}

/// [`TreqCommandRequest::CreatePullRequest`]'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedPullRequest {
  pub number: u64,
  pub url: String,
}

pub(super) fn workspace_record(repo: &str, id: i64) -> Result<crate::local_db::Workspace, String> {
  crate::local_db::get_workspace_by_id(repo, id)?
    .ok_or_else(|| format!("workspace_not_found: Workspace {id} was not found"))
}

/// [`TreqCommandRequest::WorkspacePullRequest`]. A repository without a
/// GitHub remote reads as no pull request, without running `gh`.
pub(super) fn workspace_pull_request(
  repo: &str,
  id: i64,
) -> Result<RemotePullRequestStatus, String> {
  let workspace = workspace_record(repo, id)?;
  let Some(remote) = crate::github::get_git_remote_url_impl(repo)? else {
    return Ok(RemotePullRequestStatus {
      github_repo: None,
      pull_request: None,
      checks: None,
    });
  };
  let dir = resolve_workspace_path(repo, Some(id))?;
  let gh = crate::core::github_cli::gh_path()?;
  let path = crate::binary_paths::get_extended_path();
  let branch = &workspace.branch_name;
  let pull_request = crate::github::get_pr_info_via_gh_impl(&gh, &dir, branch, &path)?;
  let checks = match &pull_request {
    Some(_) => crate::github::get_pr_checks_via_gh_impl(&gh, &dir, branch, &path)?,
    None => None,
  };
  Ok(RemotePullRequestStatus {
    github_repo: Some(remote.full_name),
    pull_request,
    checks,
  })
}

/// [`TreqCommandRequest::CreatePullRequest`]: pushes first, since GitHub
/// needs the branch, then opens the pull request into the target branch.
pub(super) fn create_pull_request(
  repo: &str,
  id: i64,
  title: &str,
  body: &str,
  draft: bool,
) -> Result<CreatedPullRequest, String> {
  let title = title.trim();
  if title.is_empty() {
    return Err("invalid_arguments: a pull request needs a title".into());
  }
  let workspace = workspace_record(repo, id)?;
  let remote = crate::github::get_git_remote_url_impl(repo)?
    .ok_or("invalid_arguments: this repository has no GitHub remote")?;
  let gh = crate::core::github_cli::gh_path()?;
  crate::core::workspaces::push_workspace_to_remote(repo, Some(id))?;
  let base = workspace.target_branch.as_deref().unwrap_or("main");
  let number = crate::github::gh_create_pr_impl(
    &gh,
    &remote.full_name,
    title,
    body,
    base,
    &workspace.branch_name,
    draft,
    &crate::binary_paths::get_extended_path(),
  )?;
  Ok(CreatedPullRequest {
    number,
    url: format!("https://github.com/{}/pull/{number}", remote.full_name),
  })
}

/// What [`TreqCommandRequest::GithubRead`] reads; each matches the
/// [`crate::core::github_cli`] function of the same name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GithubQuery {
  /// The repository's `owner/name` and the signed-in user, as a
  /// [`GithubContext`].
  Context,
  ListIssues {
    state: String,
    #[serde(default)]
    after: Option<String>,
  },
  ListPrs {
    state: String,
    page: u32,
  },
  SearchOpenPrs {
    mine: bool,
    query: String,
    limit: u32,
  },
  Issue {
    number: u64,
  },
  Pr {
    number: u64,
  },
  PrDiff {
    number: u64,
  },
  PrChecks {
    number: u64,
  },
  PrReviewThreads {
    number: u64,
  },
}

/// What [`TreqCommandRequest::GithubWrite`] changes; each matches the
/// [`crate::core::github_cli`] function of the same name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GithubAction {
  CreateIssue {
    title: String,
    body: String,
  },
  EditIssue {
    number: u64,
    title: String,
    body: String,
  },
  CommentOnIssue {
    number: u64,
    body: String,
  },
  CloseIssue {
    number: u64,
    reason: String,
  },
  ReopenIssue {
    number: u64,
  },
  DeleteIssue {
    number: u64,
  },
  CommentOnPr {
    number: u64,
    body: String,
  },
  ClosePr {
    number: u64,
  },
  ReopenPr {
    number: u64,
  },
  SetPrDraft {
    number: u64,
    draft: bool,
  },
  ReplyToReviewThread {
    thread_id: String,
    body: String,
  },
  SubmitPrReview {
    number: u64,
    event: String,
    body: String,
    #[serde(default)]
    comments: Vec<crate::github::GhDraftReviewComment>,
  },
  SetReaction {
    comment_id: String,
    content: String,
    on: bool,
  },
}

/// [`GithubQuery::Context`]'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubContext {
  pub full_name: String,
  pub login: String,
}

/// The repository's GitHub `owner/name`.
pub(super) fn github_full_name(repo: &str) -> Result<String, String> {
  crate::github::get_git_remote_url_impl(repo)
    .ok()
    .flatten()
    .map(|remote| remote.full_name)
    .ok_or_else(|| "invalid_arguments: this repository has no GitHub remote".to_string())
}

pub(super) fn require_text(text: &str, what: &str) -> Result<(), String> {
  if text.trim().is_empty() {
    return Err(format!("invalid_arguments: {what} is empty"));
  }
  Ok(())
}

/// [`TreqCommandRequest::GithubRead`].
pub(super) fn github_read(repo: &str, query: GithubQuery) -> Result<serde_json::Value, String> {
  use crate::core::github_cli as gh;
  fn json<T: Serialize>(result: Result<T, String>) -> Result<serde_json::Value, String> {
    serde_json::to_value(result?).map_err(|error| error.to_string())
  }
  let name = github_full_name(repo)?;
  let name = name.as_str();
  match query {
    GithubQuery::Context => json(gh::current_user().map(|login| GithubContext {
      full_name: name.to_string(),
      login,
    })),
    GithubQuery::ListIssues { state, after } => {
      json(gh::list_issues(name, &state, after.as_deref()))
    }
    GithubQuery::ListPrs { state, page } => json(gh::list_prs(name, &state, page)),
    GithubQuery::SearchOpenPrs { mine, query, limit } => {
      json(gh::search_open_prs(name, mine, &query, limit))
    }
    GithubQuery::Issue { number } => json(gh::issue(name, number)),
    GithubQuery::Pr { number } => json(gh::pr(name, number)),
    GithubQuery::PrDiff { number } => json(gh::pr_diff(name, number)),
    GithubQuery::PrChecks { number } => json(gh::pr_checks(name, number)),
    GithubQuery::PrReviewThreads { number } => json(gh::pr_review_threads(name, number)),
  }
}

/// [`TreqCommandRequest::GithubWrite`]. Blank text fails before the
/// repository is looked at.
pub(super) fn github_write(repo: &str, action: GithubAction) -> Result<serde_json::Value, String> {
  use crate::core::github_cli as gh;
  match &action {
    GithubAction::CreateIssue { title, .. } | GithubAction::EditIssue { title, .. } => {
      require_text(title, "the title")?
    }
    GithubAction::CommentOnIssue { body, .. }
    | GithubAction::CommentOnPr { body, .. }
    | GithubAction::ReplyToReviewThread { body, .. } => require_text(body, "the comment")?,
    _ => {}
  }
  fn done(result: Result<(), String>) -> Result<serde_json::Value, String> {
    result.map(|()| serde_json::Value::Null)
  }
  let name = github_full_name(repo)?;
  let name = name.as_str();
  match action {
    GithubAction::CreateIssue { title, body } => {
      gh::create_issue(name, &title, &body).map(|number| serde_json::json!({ "number": number }))
    }
    GithubAction::EditIssue {
      number,
      title,
      body,
    } => done(gh::edit_issue(name, number, &title, &body)),
    GithubAction::CommentOnIssue { number, body } => {
      done(gh::comment_on_issue(name, number, &body))
    }
    GithubAction::CloseIssue { number, reason } => done(gh::close_issue(name, number, &reason)),
    GithubAction::ReopenIssue { number } => done(gh::reopen_issue(name, number)),
    GithubAction::DeleteIssue { number } => done(gh::delete_issue(name, number)),
    GithubAction::CommentOnPr { number, body } => done(gh::comment_on_pr(name, number, &body)),
    GithubAction::ClosePr { number } => done(gh::close_pr(name, number)),
    GithubAction::ReopenPr { number } => done(gh::reopen_pr(name, number)),
    GithubAction::SetPrDraft { number, draft } => done(gh::set_pr_draft(name, number, draft)),
    GithubAction::ReplyToReviewThread { thread_id, body } => {
      done(gh::reply_to_review_thread(&thread_id, &body))
    }
    GithubAction::SubmitPrReview {
      number,
      event,
      body,
      comments,
    } => done(gh::submit_pr_review(name, number, &event, &body, &comments)),
    GithubAction::SetReaction {
      comment_id,
      content,
      on,
    } => done(gh::set_reaction(&comment_id, &content, on)),
  }
}
