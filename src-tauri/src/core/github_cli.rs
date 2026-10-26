//! The GitHub reads and changes GPUI screens make through the `gh` CLI. Each
//! resolves the `gh` binary (and the PATH it runs with) itself, so a caller
//! only names the repository, as `owner/name` unless it says otherwise.

use crate::binary_paths::{detect_binary_cached, get_binary_path, get_extended_path};
use crate::github::{
  self, GhDraftReviewComment, GhIssue, GhListPage, GhPullRequest, GhReviewThread, GitRemoteInfo,
  PrCiStatus, GH_LIST_PAGE_SIZE,
};

/// Why `gh` was not found, naming where Treq looked.
fn gh_not_found(search_path: &str) -> String {
  let sep = if cfg!(windows) { ';' } else { ':' };
  let dirs: Vec<&str> = search_path.split(sep).filter(|d| !d.is_empty()).collect();
  format!(
    "gh CLI not found. Install it (https://cli.github.com) or make sure it is on your login shell's PATH. Treq looked in: {}",
    dirs.join(", ")
  )
}

/// The `gh` binary: the configured path if it still exists, else one found
/// on the system.
fn resolve_gh_with(
  configured: impl FnOnce() -> Option<String>,
  exists: impl Fn(&str) -> bool,
  detected: impl FnOnce() -> Option<String>,
  search_path: impl FnOnce() -> String,
) -> Result<String, String> {
  configured()
    .filter(|path| exists(path))
    .or_else(detected)
    .ok_or_else(|| gh_not_found(&search_path()))
}

/// The `gh` binary's path.
pub fn gh_path() -> Result<String, String> {
  resolve_gh_with(
    || get_binary_path("gh"),
    |path| std::path::Path::new(path).is_file(),
    || detect_binary_cached("gh"),
    get_extended_path,
  )
}

/// The `gh` binary and the PATH to run it with.
fn gh() -> Result<(String, String), String> {
  Ok((gh_path()?, get_extended_path()))
}

/// The repository's GitHub remote, when it has one.
pub fn remote(repo_path: &str) -> Result<Option<GitRemoteInfo>, String> {
  github::get_git_remote_url_impl(repo_path)
}

pub fn list_issues(
  repo: &str,
  state: &str,
  after: Option<&str>,
) -> Result<GhListPage<GhIssue>, String> {
  let (gh, path) = gh()?;
  github::gh_list_issues_impl(&gh, repo, state, GH_LIST_PAGE_SIZE, after, &path)
}

pub fn list_prs(repo: &str, state: &str, page: u32) -> Result<GhListPage<GhPullRequest>, String> {
  let (gh, path) = gh()?;
  github::gh_list_prs_impl(&gh, repo, state, GH_LIST_PAGE_SIZE, page, &path)
}

/// The open pull requests matching `query`, up to `limit`; with `mine`, only
/// the signed-in user's.
pub fn search_open_prs(
  repo: &str,
  mine: bool,
  query: &str,
  limit: u32,
) -> Result<Vec<GhPullRequest>, String> {
  let (gh, path) = gh()?;
  github::gh_search_open_prs_impl(&gh, repo, mine, query, limit, &path)
}

/// An issue with its comments' reactions filled in.
pub fn issue(repo: &str, number: u64) -> Result<GhIssue, String> {
  let (gh, path) = gh()?;
  let mut issue = github::gh_view_issue_impl(&gh, repo, number, &path)?;
  github::fill_comment_reactions(&gh, issue.comments.as_mut(), &path);
  Ok(issue)
}

/// A pull request with its comments' reactions filled in.
pub fn pr(repo: &str, number: u64) -> Result<GhPullRequest, String> {
  let (gh, path) = gh()?;
  let mut pr = github::gh_view_pr_impl(&gh, repo, number, &path)?;
  github::fill_comment_reactions(&gh, pr.comments.as_mut(), &path);
  Ok(pr)
}

/// A pull request as `gh` reports it, without the reactions.
pub fn view_pr(repo: &str, number: u64) -> Result<GhPullRequest, String> {
  let (gh, path) = gh()?;
  github::gh_view_pr_impl(&gh, repo, number, &path)
}

pub fn pr_diff(repo: &str, number: u64) -> Result<String, String> {
  let (gh, path) = gh()?;
  github::gh_pr_diff_impl(&gh, repo, number, &path)
}

pub fn pr_checks(repo: &str, number: u64) -> Result<Option<PrCiStatus>, String> {
  let (gh, path) = gh()?;
  github::get_pr_checks_for_pr_impl(&gh, repo, number, &path)
}

/// The signed-in user's login.
pub fn current_user() -> Result<String, String> {
  // `gh api user` is a network round trip and the login rarely changes; a
  // switch of `gh auth` account shows up after the TTL.
  const TTL: std::time::Duration = std::time::Duration::from_secs(600);
  static CACHE: std::sync::Mutex<Option<(String, std::time::Instant)>> =
    std::sync::Mutex::new(None);
  if let Some((login, at)) = CACHE.lock().ok().and_then(|cache| cache.clone()) {
    if at.elapsed() < TTL {
      return Ok(login);
    }
  }
  let (gh, path) = gh()?;
  let login = github::gh_current_user_impl(&gh, &path)?;
  if let Ok(mut cache) = CACHE.lock() {
    *cache = Some((login.clone(), std::time::Instant::now()));
  }
  Ok(login)
}

/// The review threads of pull request `number` in `full_name`; none when the
/// name is not `owner/name`.
pub fn pr_review_threads(full_name: &str, number: u64) -> Result<Vec<GhReviewThread>, String> {
  let Some((owner, repo)) = full_name.split_once('/') else {
    return Ok(Vec::new());
  };
  let (gh, path) = gh()?;
  github::gh_list_pr_review_threads_impl(&gh, owner, repo, number, &path)
}

/// The review threads of the open pull request for `branch`; none when the
/// repository has no GitHub remote or the branch no cached pull request.
#[cfg(feature = "gui")]
pub fn branch_review_threads(repo_path: &str, branch: &str) -> Result<Vec<GhReviewThread>, String> {
  let Some(remote) = remote(repo_path)? else {
    return Ok(Vec::new());
  };
  let pr = crate::pr_status::global()
    .get_cached(repo_path, branch)
    .flatten();
  let Some(pr) = pr else {
    return Ok(Vec::new());
  };
  let (gh, path) = gh()?;
  github::gh_list_pr_review_threads_impl(&gh, &remote.owner, &remote.repo, pr.number, &path)
}

pub fn reply_to_review_thread(thread_id: &str, body: &str) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_reply_pr_review_thread_impl(&gh, thread_id, body, &path)
}

pub fn submit_pr_review(
  repo: &str,
  number: u64,
  event: &str,
  body: &str,
  comments: &[GhDraftReviewComment],
) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_submit_pr_review_impl(&gh, repo, number, event, body, comments, &path)
}

/// Turns the reaction `content` on or off for a comment.
pub fn set_reaction(comment_id: &str, content: &str, on: bool) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_set_reaction_impl(&gh, comment_id, content, on, &path)
}

pub fn create_issue(repo: &str, title: &str, body: &str) -> Result<u64, String> {
  let (gh, path) = gh()?;
  github::gh_create_issue_impl(&gh, repo, title, body, &path)
}

pub fn comment_on_issue(repo: &str, number: u64, body: &str) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_create_issue_comment_impl(&gh, repo, number, body, &path)
}

pub fn close_issue(repo: &str, number: u64, reason: &str) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_close_issue_impl(&gh, repo, number, Some(reason), &path)
}

pub fn reopen_issue(repo: &str, number: u64) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_reopen_issue_impl(&gh, repo, number, &path)
}

pub fn edit_issue(repo: &str, number: u64, title: &str, body: &str) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_edit_issue_impl(&gh, repo, number, title, body, &path)
}

pub fn delete_issue(repo: &str, number: u64) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_delete_issue_impl(&gh, repo, number, &path)
}

pub fn create_pr(
  repo: &str,
  title: &str,
  body: &str,
  base: &str,
  head: &str,
  draft: bool,
) -> Result<u64, String> {
  let (gh, path) = gh()?;
  github::gh_create_pr_impl(&gh, repo, title, body, base, head, draft, &path)
}

pub fn comment_on_pr(repo: &str, number: u64, body: &str) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_create_pr_comment_impl(&gh, repo, number, body, &path)
}

/// Replaces PR `number`'s title and/or body; a `None` field stays.
pub fn edit_pr(
  repo: &str,
  number: u64,
  title: Option<&str>,
  body: Option<&str>,
) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_edit_pr_impl(&gh, repo, number, title, body, &path)
}

pub fn close_pr(repo: &str, number: u64) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_close_pr_impl(&gh, repo, number, &path)
}

pub fn reopen_pr(repo: &str, number: u64) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_reopen_pr_impl(&gh, repo, number, &path)
}

pub fn set_pr_draft(repo: &str, number: u64, draft: bool) -> Result<(), String> {
  let (gh, path) = gh()?;
  github::gh_set_pr_draft_impl(&gh, repo, number, draft, &path)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn path() -> String {
    "/usr/bin".into()
  }

  #[test]
  fn prefers_the_configured_gh_over_a_detected_one() {
    let found = resolve_gh_with(
      || Some("/cfg/gh".into()),
      |_| true,
      || Some("/usr/bin/gh".into()),
      path,
    );
    assert_eq!(found.as_deref(), Ok("/cfg/gh"));
  }

  #[test]
  fn falls_back_to_the_detected_gh() {
    let found = resolve_gh_with(|| None, |_| true, || Some("/usr/bin/gh".into()), path);
    assert_eq!(found.as_deref(), Ok("/usr/bin/gh"));
  }

  #[test]
  fn skips_a_configured_gh_that_no_longer_exists() {
    let found = resolve_gh_with(
      || Some("/gone/gh".into()),
      |_| false,
      || Some("/opt/homebrew/bin/gh".into()),
      path,
    );
    assert_eq!(found.as_deref(), Ok("/opt/homebrew/bin/gh"));
  }

  #[test]
  fn reports_where_it_looked_when_gh_is_not_found() {
    let err = resolve_gh_with(
      || None,
      |_| true,
      || None,
      || {
        if cfg!(windows) {
          "C:\\a;C:\\b"
        } else {
          "/usr/bin:/opt/homebrew/bin"
        }
        .into()
      },
    )
    .unwrap_err();
    assert!(err.starts_with("gh CLI not found"), "{err}");
    let listed = if cfg!(windows) {
      "C:\\a, C:\\b"
    } else {
      "/usr/bin, /opt/homebrew/bin"
    };
    assert!(err.ends_with(listed), "{err}");
  }

  #[test]
  fn lists_no_review_threads_for_a_name_without_an_owner() {
    assert!(pr_review_threads("no-slash", 1).unwrap().is_empty());
  }
}
