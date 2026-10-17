use crate::binary_paths::{detect_binary, get_binary_path, get_extended_path};
pub use crate::github::{
  GhIssue, GhListPage, GhPullRequest, GhReviewThread, GitRemoteInfo, PrCiStatus, PrInfo,
};

fn gh_bin() -> Result<String, String> {
  get_binary_path("gh")
    .or_else(|| detect_binary("gh"))
    .ok_or_else(|| "gh CLI not found".to_string())
}

/// Run `gh pr view` for the given branch in the given repo directory.
/// Returns None if no PR exists for the branch. Returns Err if gh is
/// missing, not authenticated, or fails otherwise.
/// Also updates the background PR-status cache so sidebar reads stay fresh.
/// Runs off the UI/command thread via `spawn_blocking`.
#[tauri::command]
pub async fn get_pr_info_via_gh(
  repo_path: String,
  branch_name: String,
) -> Result<Option<PrInfo>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let gh = get_binary_path("gh")
      .or_else(|| detect_binary("gh"))
      .ok_or_else(|| "gh CLI not found".to_string())?;

    let info =
      crate::github::get_pr_info_via_gh_impl(&gh, &repo_path, &branch_name, &get_extended_path())?;
    crate::pr_status::global().put_cached(&repo_path, &branch_name, info.clone());
    Ok(info)
  })
  .await
  .map_err(|e| format!("Failed to join get_pr_info_via_gh task: {e}"))?
}

/// Start background PR-status polling for every workspace branch in `repo_path`.
/// Safe to call repeatedly; the first call also starts the poller thread.
#[tauri::command]
pub fn start_pr_status_polling(repo_path: String) -> Result<(), String> {
  crate::pr_status::global().watch_repo(&repo_path);
  Ok(())
}

/// Stop background PR-status polling for `repo_path`.
#[tauri::command]
pub fn stop_pr_status_polling(repo_path: String) -> Result<(), String> {
  crate::pr_status::global().unwatch_repo(&repo_path);
  Ok(())
}

/// Return the cached PR statuses for a repo (`branch_name -> Option<PrInfo>`).
/// Never shells out to `gh` — the background poller owns that work.
#[tauri::command]
pub fn list_cached_pr_statuses(
  repo_path: String,
) -> Result<std::collections::HashMap<String, Option<PrInfo>>, String> {
  Ok(crate::pr_status::global().list_cached(&repo_path))
}

/// Return a single cached PR. `None` means not yet polled or no PR (callers
/// that need to distinguish should use `list_cached_pr_statuses`).
#[tauri::command]
pub fn get_cached_pr_info(
  repo_path: String,
  branch_name: String,
) -> Result<Option<PrInfo>, String> {
  Ok(
    crate::pr_status::global()
      .get_cached(&repo_path, &branch_name)
      .flatten(),
  )
}

/// Return the cached CI statuses for a repo (`branch_name -> Option<PrCiStatus>`).
/// Never shells out to `gh` — the background poller owns that work.
#[tauri::command]
pub fn list_cached_pr_ci_statuses(
  repo_path: String,
) -> Result<std::collections::HashMap<String, Option<PrCiStatus>>, String> {
  Ok(crate::pr_status::global().list_cached_ci(&repo_path))
}

/// Return a single cached CI rollup. `None` means not yet polled, no PR, or
/// no checks reported yet.
#[tauri::command]
pub fn get_cached_pr_ci_status(
  repo_path: String,
  branch_name: String,
) -> Result<Option<PrCiStatus>, String> {
  Ok(
    crate::pr_status::global()
      .get_cached_ci(&repo_path, &branch_name)
      .flatten(),
  )
}

/// Force a refresh of all workspace PR statuses for a repo.
/// Used after creating a PR so the sidebar updates immediately.
/// Runs off the UI/command thread via `spawn_blocking`.
#[tauri::command]
pub async fn refresh_pr_statuses(repo_path: String) -> Result<(), String> {
  tauri::async_runtime::spawn_blocking(move || {
    // Ensure the repo is watched so subsequent background polls continue.
    crate::pr_status::global().watch_repo(&repo_path);
    crate::pr_status::global().refresh_repo_now(&repo_path);
  })
  .await
  .map_err(|e| format!("Failed to join refresh_pr_statuses task: {e}"))?;
  Ok(())
}

/// Queue an out-of-band PR info + CI refresh for one branch.
/// Returns immediately; the background poller fetches `gh pr view` and
/// (when a PR exists) `gh pr checks`, updates the cache, and emits
/// `pr-statuses-updated`. Used when the user opens a workspace so they
/// do not wait for the next poll tick to see fresh data.
#[tauri::command]
pub fn refresh_pr_branch_status(repo_path: String, branch_name: String) -> Result<(), String> {
  crate::pr_status::global().watch_repo(&repo_path);
  crate::pr_status::global().queue_branch_refresh(repo_path, branch_name);
  Ok(())
}

/// Run `gh pr checks` for the given branch in the given repo directory and
/// roll the individual check runs up into an overall CI status.
/// Returns None if there is no PR or the PR has no checks reported yet.
/// Returns Err if gh is missing, not authenticated, or fails otherwise.
/// Prefer cache reads for UI polling; this force-fetches and warms the cache.
/// Runs off the UI/command thread via `spawn_blocking`.
#[tauri::command]
pub async fn get_pr_checks_via_gh(
  repo_path: String,
  branch_name: String,
) -> Result<Option<PrCiStatus>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let gh = get_binary_path("gh")
      .or_else(|| detect_binary("gh"))
      .ok_or_else(|| "gh CLI not found".to_string())?;

    let status = crate::github::get_pr_checks_via_gh_impl(
      &gh,
      &repo_path,
      &branch_name,
      &get_extended_path(),
    )?;
    crate::pr_status::global().put_cached_ci(&repo_path, &branch_name, status.clone());
    Ok(status)
  })
  .await
  .map_err(|e| format!("Failed to join get_pr_checks_via_gh task: {e}"))?
}

/// Run `gh pr checks` for a specific PR number in a repo (not necessarily
/// checked out locally), for the GitHub panel's PR detail view. Shares the
/// same rollup as `get_pr_checks_via_gh` so both surfaces agree.
/// Runs off the UI/command thread via `spawn_blocking`.
#[tauri::command]
pub async fn get_pr_checks_for_pr(
  repo_full_name: String,
  pr_number: u64,
) -> Result<Option<PrCiStatus>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let gh = gh_bin()?;
    crate::github::get_pr_checks_for_pr_impl(&gh, &repo_full_name, pr_number, &get_extended_path())
  })
  .await
  .map_err(|e| format!("Failed to join get_pr_checks_for_pr task: {e}"))?
}

/// Read the GitHub remote URL from .git/config and parse owner/repo.
/// Returns None if no GitHub remote is found.
#[tauri::command]
pub fn get_git_remote_url(repo_path: String) -> Result<Option<GitRemoteInfo>, String> {
  crate::github::get_git_remote_url_impl(&repo_path)
}

/// Run a `gh` subprocess on the blocking pool. Sync Tauri commands execute on
/// the main thread, so a network call there freezes the window until `gh`
/// returns.
async fn run_gh_blocking<T, F>(task: &'static str, f: F) -> Result<T, String>
where
  T: Send + 'static,
  F: FnOnce(&str, &str) -> Result<T, String> + Send + 'static,
{
  let gh = gh_bin()?;
  let extended_path = get_extended_path();
  tauri::async_runtime::spawn_blocking(move || f(&gh, &extended_path))
    .await
    .map_err(|e| format!("Failed to join {task} task: {e}"))?
}

#[tauri::command]
pub async fn gh_list_issues(
  repo_full_name: String,
  state: String,
  limit: Option<u32>,
  after: Option<String>,
) -> Result<GhListPage<GhIssue>, String> {
  run_gh_blocking("gh_list_issues", move |gh, path| {
    crate::github::gh_list_issues_impl(
      gh,
      &repo_full_name,
      &state,
      limit.unwrap_or(crate::github::GH_LIST_PAGE_SIZE),
      after.as_deref(),
      path,
    )
  })
  .await
}

#[tauri::command]
pub async fn gh_view_issue(repo_full_name: String, issue_number: u64) -> Result<GhIssue, String> {
  run_gh_blocking("gh_view_issue", move |gh, path| {
    crate::github::gh_view_issue_impl(gh, &repo_full_name, issue_number, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_create_issue(
  repo_full_name: String,
  title: String,
  body: String,
) -> Result<u64, String> {
  run_gh_blocking("gh_create_issue", move |gh, path| {
    crate::github::gh_create_issue_impl(gh, &repo_full_name, &title, &body, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_create_issue_comment(
  repo_full_name: String,
  issue_number: u64,
  body: String,
) -> Result<(), String> {
  run_gh_blocking("gh_create_issue_comment", move |gh, path| {
    crate::github::gh_create_issue_comment_impl(gh, &repo_full_name, issue_number, &body, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_close_issue(
  repo_full_name: String,
  issue_number: u64,
  reason: Option<String>,
) -> Result<(), String> {
  run_gh_blocking("gh_close_issue", move |gh, path| {
    crate::github::gh_close_issue_impl(gh, &repo_full_name, issue_number, reason.as_deref(), path)
  })
  .await
}

#[tauri::command]
pub async fn gh_edit_issue(
  repo_full_name: String,
  issue_number: u64,
  title: String,
  body: String,
) -> Result<(), String> {
  run_gh_blocking("gh_edit_issue", move |gh, path| {
    crate::github::gh_edit_issue_impl(gh, &repo_full_name, issue_number, &title, &body, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_delete_issue(repo_full_name: String, issue_number: u64) -> Result<(), String> {
  run_gh_blocking("gh_delete_issue", move |gh, path| {
    crate::github::gh_delete_issue_impl(gh, &repo_full_name, issue_number, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_reopen_issue(repo_full_name: String, issue_number: u64) -> Result<(), String> {
  run_gh_blocking("gh_reopen_issue", move |gh, path| {
    crate::github::gh_reopen_issue_impl(gh, &repo_full_name, issue_number, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_list_prs(
  repo_full_name: String,
  state: String,
  limit: Option<u32>,
  page: Option<u32>,
) -> Result<GhListPage<GhPullRequest>, String> {
  run_gh_blocking("gh_list_prs", move |gh, path| {
    crate::github::gh_list_prs_impl(
      gh,
      &repo_full_name,
      &state,
      limit.unwrap_or(crate::github::GH_LIST_PAGE_SIZE),
      page.unwrap_or(1),
      path,
    )
  })
  .await
}

#[tauri::command]
pub async fn gh_view_pr(repo_full_name: String, pr_number: u64) -> Result<GhPullRequest, String> {
  run_gh_blocking("gh_view_pr", move |gh, path| {
    crate::github::gh_view_pr_impl(gh, &repo_full_name, pr_number, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_create_pr_comment(
  repo_full_name: String,
  pr_number: u64,
  body: String,
) -> Result<(), String> {
  run_gh_blocking("gh_create_pr_comment", move |gh, path| {
    crate::github::gh_create_pr_comment_impl(gh, &repo_full_name, pr_number, &body, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_close_pr(repo_full_name: String, pr_number: u64) -> Result<(), String> {
  run_gh_blocking("gh_close_pr", move |gh, path| {
    crate::github::gh_close_pr_impl(gh, &repo_full_name, pr_number, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_repo_merge_settings(
  repo_full_name: String,
) -> Result<crate::github::GhRepoMergeSettings, String> {
  run_gh_blocking("gh_repo_merge_settings", move |gh, path| {
    crate::github::gh_repo_merge_settings_impl(gh, &repo_full_name, path)
  })
  .await
}

/// Merge a workspace's PR on GitHub, sync locally, move stacked child PRs
/// onto the new base, and refresh the PR status of every branch involved.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn merge_workspace_on_github(
  repo_path: String,
  workspace_id: i64,
  repo_full_name: String,
  pr_number: u64,
  method: crate::github::GhMergeMethod,
  subject: Option<String>,
  body: Option<String>,
) -> Result<crate::core::MergeResult, String> {
  run_gh_blocking("merge_workspace_on_github", move |gh, path| {
    let plan = crate::core::merge_workspace_on_github(
      &repo_path,
      workspace_id,
      gh,
      path,
      &repo_full_name,
      pr_number,
      method,
      subject.as_deref(),
      body.as_deref(),
    )?;
    Ok(finish_merge(&repo_path, plan, true))
  })
  .await
}

/// For a PR already merged on GitHub: sync the target and stack locally.
#[tauri::command]
pub async fn sync_workspace_after_remote_merge(
  repo_path: String,
  workspace_id: i64,
) -> Result<crate::core::MergeResult, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let plan = crate::core::sync_after_remote_merge(&repo_path, workspace_id)?;
    Ok(finish_merge(&repo_path, plan, true))
  })
  .await
  .map_err(|e| format!("Failed to join sync_workspace_after_remote_merge task: {e}"))?
}

/// Shared tail of every merge path: optionally move open child PRs onto the
/// new base (best-effort, needs `gh` and a GitHub remote), then queue a
/// PR+CI refresh for every branch the merge landed or retargeted.
pub fn finish_merge(
  repo_path: &str,
  plan: crate::core::MergeLandingPlan,
  retarget_prs: bool,
) -> crate::core::MergeResult {
  let mut notes = Vec::new();
  if retarget_prs && !plan.retargeted.is_empty() {
    let remote = crate::github::get_git_remote_url_impl(repo_path)
      .ok()
      .flatten();
    if let (Ok(gh), Some(remote)) = (gh_bin(), remote) {
      let retargets: Vec<(String, String)> = plan
        .retargeted
        .iter()
        .map(|r| (r.branch_name.clone(), r.to_branch.clone()))
        .collect();
      notes = crate::github::gh_retarget_open_prs_impl(
        &gh,
        repo_path,
        &remote.full_name,
        &retargets,
        &get_extended_path(),
      );
    }
  }
  let pr_status = crate::pr_status::global();
  for branch in plan
    .landed_branches
    .iter()
    .chain(plan.retargeted.iter().map(|r| &r.branch_name))
  {
    pr_status.queue_branch_refresh(repo_path.to_string(), branch.clone());
  }
  crate::core::MergeResult { plan, notes }
}

#[tauri::command]
pub async fn gh_reopen_pr(repo_full_name: String, pr_number: u64) -> Result<(), String> {
  run_gh_blocking("gh_reopen_pr", move |gh, path| {
    crate::github::gh_reopen_pr_impl(gh, &repo_full_name, pr_number, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_set_pr_draft(
  repo_full_name: String,
  pr_number: u64,
  draft: bool,
) -> Result<(), String> {
  run_gh_blocking("gh_set_pr_draft", move |gh, path| {
    crate::github::gh_set_pr_draft_impl(gh, &repo_full_name, pr_number, draft, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_current_user() -> Result<String, String> {
  run_gh_blocking("gh_current_user", crate::github::gh_current_user_impl).await
}

#[tauri::command]
pub async fn gh_pr_diff(repo_full_name: String, pr_number: u64) -> Result<String, String> {
  run_gh_blocking("gh_pr_diff", move |gh, path| {
    crate::github::gh_pr_diff_impl(gh, &repo_full_name, pr_number, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_reply_pr_review_thread(thread_id: String, body: String) -> Result<(), String> {
  run_gh_blocking("gh_reply_pr_review_thread", move |gh, path| {
    crate::github::gh_reply_pr_review_thread_impl(gh, &thread_id, &body, path)
  })
  .await
}

#[tauri::command]
pub async fn gh_submit_pr_review(
  repo_full_name: String,
  pr_number: u64,
  event: String,
  body: String,
  comments: Vec<crate::github::GhDraftReviewComment>,
) -> Result<(), String> {
  run_gh_blocking("gh_submit_pr_review", move |gh, path| {
    crate::github::gh_submit_pr_review_impl(
      gh,
      &repo_full_name,
      pr_number,
      &event,
      &body,
      &comments,
      path,
    )
  })
  .await
}

/// List every review-comment thread on a PR, including resolved/outdated
/// state. Read-only: no replies, resolves, or other mutations are issued.
/// Runs off the UI/command thread via `spawn_blocking`.
#[tauri::command]
pub async fn gh_list_pr_review_threads(
  owner: String,
  repo: String,
  pr_number: u64,
) -> Result<Vec<GhReviewThread>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let gh = gh_bin()?;
    crate::github::gh_list_pr_review_threads_impl(
      &gh,
      &owner,
      &repo,
      pr_number,
      &get_extended_path(),
    )
  })
  .await
  .map_err(|e| format!("Failed to join gh_list_pr_review_threads task: {e}"))?
}

/// Create a GitHub PR via `gh`. Runs on the blocking pool so the network
/// subprocess cannot stall the async runtime / UI IPC path (same pattern as
/// `push_workspace_to_remote` and other long-running workspace commands).
#[tauri::command]
pub async fn gh_create_pr(
  repo_full_name: String,
  title: String,
  body: String,
  base_branch: String,
  head_branch: String,
  draft: Option<bool>,
) -> Result<u64, String> {
  let gh = gh_bin()?;
  let extended_path = get_extended_path();
  let draft = draft.unwrap_or(false);
  tauri::async_runtime::spawn_blocking(move || {
    crate::github::gh_create_pr_impl(
      &gh,
      &repo_full_name,
      &title,
      &body,
      &base_branch,
      &head_branch,
      draft,
      &extended_path,
    )
  })
  .await
  .map_err(|e| format!("Failed to join gh_create_pr task: {}", e))?
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::io::Write;
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::sync::Arc;
  use std::time::{Duration, Instant};
  use tempfile::TempDir;

  #[cfg(unix)]
  fn write_fake_gh(dir: &TempDir, script_body: &str) -> String {
    let path = dir.path().join("gh");
    let mut f = fs::File::create(&path).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    write!(f, "{script_body}").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.to_str().unwrap().to_string()
  }

  /// `gh_create_pr` must schedule the `gh` subprocess on the blocking pool
  /// and return a Future the caller can await — never run the network call
  /// synchronously on the invoking thread.
  #[test]
  #[cfg(unix)]
  fn gh_create_pr_runs_on_blocking_pool_not_caller_thread() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      "sleep 0.2\necho 'https://github.com/owner/repo/pull/11'",
    );

    let started = Arc::new(AtomicBool::new(false));
    let started_flag = Arc::clone(&started);

    let join = std::thread::spawn(move || {
      tauri::async_runtime::block_on(async move {
        tauri::async_runtime::spawn_blocking(move || {
          started_flag.store(true, Ordering::SeqCst);
          crate::github::gh_create_pr_impl(
            &gh_path,
            "owner/repo",
            "T",
            "B",
            "main",
            "feat",
            false,
            "/usr/bin:/bin",
          )
        })
        .await
        .expect("join create_pr task")
      })
    });

    // The invoking path returns a join handle immediately; work happens on
    // the blocking pool. We must observe the subprocess start without the
    // caller thread being stuck inside `Command::output`.
    let deadline = Instant::now() + Duration::from_secs(2);
    while !started.load(Ordering::SeqCst) {
      assert!(
        Instant::now() < deadline,
        "gh_create_pr blocking task never started"
      );
      std::thread::sleep(Duration::from_millis(10));
    }

    let number = join.join().expect("worker thread").unwrap();
    assert_eq!(number, 11);
  }

  #[test]
  #[cfg(unix)]
  fn gh_create_pr_command_returns_pr_number_via_spawn_blocking() {
    let bin_dir = TempDir::new().unwrap();
    let gh_path = write_fake_gh(
      &bin_dir,
      r#"test "$*" = "pr create --repo owner/repo --title T --body-file - --base main --head feat" || exit 9
echo 'https://github.com/owner/repo/pull/42'"#,
    );

    // Exercise the same spawn_blocking join path the Tauri command uses,
    // without depending on the process-wide gh binary cache.
    let number = tauri::async_runtime::block_on(async move {
      tauri::async_runtime::spawn_blocking(move || {
        crate::github::gh_create_pr_impl(
          &gh_path,
          "owner/repo",
          "T",
          "B",
          "main",
          "feat",
          false,
          "/usr/bin:/bin",
        )
      })
      .await
      .expect("join")
    })
    .unwrap();

    assert_eq!(number, 42);
  }
}

#[derive(serde::Serialize)]
pub struct GithubIssueKickoffResult {
  pub workspace_id: i64,
  pub created: bool,
}

/// Opens the workspace for a GitHub issue, creating it (branch
/// `github-<number>-<slug>`) when it does not exist yet.
#[tauri::command]
pub async fn github_open_or_create_workspace_from_issue(
  repo_path: String,
  number: u64,
  title: String,
  url: String,
) -> Result<GithubIssueKickoffResult, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::core::github_issues::open_or_create_workspace_from_github_issue(
      &repo_path, number, &title, &url,
    )
    .map(|(workspace, created)| GithubIssueKickoffResult {
      workspace_id: workspace.id,
      created,
    })
  })
  .await
  .map_err(|e| format!("Failed to join GitHub issue kickoff: {e}"))?
}
