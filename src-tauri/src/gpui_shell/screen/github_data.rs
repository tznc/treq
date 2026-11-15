//! What the GitHub view (`GitHubPanel`) reads from and does through the
//! backend, and the list rules it shares with React (`github-pr-filters`,
//! `deriveConventionalPrTitle`, `ci-status`).
//!
//! React keeps the view's place in the URL (tab, state filter, the selected
//! issue or pull request, the create form) and pushes it as a [`Route`];
//! GPUI reads and changes everything else through `gh` and the local
//! database itself.

use std::{
  collections::{HashMap, HashSet},
  sync::LazyLock,
};

use serde::{Deserialize, Serialize};

use super::workspace_data::WorkspaceHost;
pub use crate::github_filters::*;
use crate::{
  core::{self, RunSummary, SetupScriptStatus, WorkflowInfo},
  github::{GhIssue, GhListPage, GhPullRequest, GitRemoteInfo, PrCiStatus, PrInfo},
  gpui_shell::native::{Backend, NativeEvent},
  local_db::Workspace,
};

/// `GitHubTab`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tab {
  Issues,
  Prs,
}

/// `GitHubStateFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum StateFilter {
  Draft,
  #[default]
  Open,
  Closed,
  All,
}

impl StateFilter {
  /// The `state` argument `gh_list_issues` and `gh_list_prs` take.
  pub fn value(self) -> &'static str {
    match self {
      StateFilter::Draft => "draft",
      StateFilter::Open => "open",
      StateFilter::Closed => "closed",
      StateFilter::All => "all",
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      StateFilter::Draft => "Draft",
      StateFilter::Open => "Open",
      StateFilter::Closed => "Closed",
      StateFilter::All => "All",
    }
  }

  /// `ISSUE_FILTERS` and `PR_FILTERS`.
  pub fn options(tab: Tab) -> &'static [StateFilter] {
    match tab {
      Tab::Issues => &[StateFilter::Open, StateFilter::Closed, StateFilter::All],
      Tab::Prs => &[
        StateFilter::Draft,
        StateFilter::Open,
        StateFilter::Closed,
        StateFilter::All,
      ],
    }
  }
}

/// What React sends: where in the GitHub view the URL points.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  pub tab: Tab,
  pub filter: StateFilter,
  /// The issue or pull request whose detail shows.
  pub selected: Option<u64>,
  /// The new issue form or new pull request dialog is open (`/new`).
  pub creating: bool,
  /// Workflow checks running from this window (`useRunningChecks` keys:
  /// `<workspace id>:<file>[:<job>]`).
  #[serde(default)]
  pub running_checks: Vec<String>,
  /// Bumped to read the selected issue or PR again (a review was submitted).
  #[serde(default)]
  pub reload: u64,
}

/// A change to one issue.
#[derive(Debug, Clone, PartialEq)]
pub enum IssueChange {
  Comment(String),
  /// `gh issue close --reason`: "completed" or "not planned".
  Close(String),
  Reopen,
  Edit {
    title: String,
    body: String,
  },
  Delete,
}

/// A change to one pull request.
#[derive(Debug, Clone, PartialEq)]
pub enum PrChange {
  Comment(String),
  Close,
  Reopen,
  Draft(bool),
}

/// `useWorkspaceChecks`: the Treq workflow checks of a PR's workspace.
#[derive(Debug, Clone, Default)]
pub struct TreqChecks {
  pub trusted: bool,
  pub workflows: Vec<WorkflowInfo>,
  /// The latest run of each workflow that has run, by file.
  pub latest: HashMap<String, RunSummary>,
  pub setup: Option<SetupScriptStatus>,
}

impl TreqChecks {
  /// Whether the repository declares any workflow or a setup script.
  pub fn configured(&self) -> bool {
    !self.workflows.is_empty() || self.setup.as_ref().is_some_and(|s| s.configured)
  }
}

/// One line of a job's logs (`getRunLogs`).
#[derive(Debug, Clone, PartialEq)]
pub struct LogLine {
  pub stream: String,
  pub body: String,
}

/// What the GitHub view asks of the backend; faked in tests. All but
/// `notify`, `open_url` and `refresh_pr_status` block, so they are called
/// off the main thread.
pub trait GithubHost: WorkspaceHost {
  /// The image at `url`: a reviewer's avatar.
  fn avatar(&self, url: &str) -> Result<Vec<u8>, String>;
  fn remote(&self, repo_path: &str) -> Result<Option<GitRemoteInfo>, String>;
  fn list_issues(
    &self,
    repo: &str,
    state: &str,
    after: Option<&str>,
  ) -> Result<GhListPage<GhIssue>, String>;
  fn list_prs(
    &self,
    repo: &str,
    state: &str,
    page: u32,
  ) -> Result<GhListPage<GhPullRequest>, String>;
  fn issue(&self, repo: &str, number: u64) -> Result<GhIssue, String>;
  fn pr(&self, repo: &str, number: u64) -> Result<GhPullRequest, String>;
  fn pr_checks(&self, repo: &str, number: u64) -> Result<Option<PrCiStatus>, String>;
  fn change_issue(&self, repo: &str, number: u64, change: &IssueChange) -> Result<(), String>;
  fn change_pr(&self, repo: &str, number: u64, change: &PrChange) -> Result<(), String>;
  /// Adds (`on`) or removes the user's `content` reaction on a comment.
  fn react(&self, comment_id: &str, content: &str, on: bool) -> Result<(), String>;
  fn create_issue(&self, repo: &str, title: &str, body: &str) -> Result<u64, String>;
  fn create_pr(
    &self,
    repo: &str,
    title: &str,
    body: &str,
    base: &str,
    head: &str,
    draft: bool,
  ) -> Result<u64, String>;
  fn workspaces(&self, repo_path: &str) -> Result<Vec<Workspace>, String>;
  /// The PR-status poller's cache, by branch.
  fn pr_statuses(&self, repo_path: &str) -> HashMap<String, Option<PrInfo>>;
  fn default_branch(&self, repo_path: &str) -> Result<String, String>;
  fn push_workspace(&self, repo_path: &str, workspace_id: i64) -> Result<(), String>;
  /// The workspace for a PR's head branch, made when there is none: its
  /// id, and whether it was made.
  fn open_pr_workspace(&self, repo_path: &str, pr: &GhPullRequest) -> Result<(i64, bool), String>;
  /// Has the poller read `branch`'s pull request again (`invalidatePrStatuses`).
  fn refresh_pr_status(&self, repo_path: &str, branch: &str);
  /// The Logs preview is on.
  fn logs_enabled(&self) -> bool;
  fn treq_checks(&self, repo_path: &str, workspace_id: i64) -> TreqChecks;
  fn run_history(
    &self,
    repo_path: &str,
    workspace_id: i64,
    filename: &str,
  ) -> Result<Vec<RunSummary>, String>;
  fn run_logs(&self, repo_path: &str, run_id: i64, job_id: &str) -> Result<Vec<LogLine>, String>;
  fn trust_repo(&self, repo_path: &str) -> Result<(), String>;
  /// Whether GPUI runs the repository's actions itself: false for a remote
  /// repository, whose flows React still runs.
  fn acts_locally(&self, repo_path: &str) -> bool;
  /// Runs a workflow, one of its jobs, or the setup script, to the end.
  fn run_checks(
    &self,
    target: &crate::core::running_checks::ChecksTarget,
    run: &crate::core::running_checks::CheckRun,
  ) -> Result<Vec<crate::core::checks::JobResult>, String>;
}

impl GithubHost for Backend {
  fn avatar(&self, url: &str) -> Result<Vec<u8>, String> {
    super::settings_data::SettingsHost::fetch_image(self, url)
  }

  fn remote(&self, repo_path: &str) -> Result<Option<GitRemoteInfo>, String> {
    core::github_cli::remote(repo_path)
  }

  fn list_issues(
    &self,
    repo: &str,
    state: &str,
    after: Option<&str>,
  ) -> Result<GhListPage<GhIssue>, String> {
    core::github_cli::list_issues(repo, state, after)
  }

  fn list_prs(
    &self,
    repo: &str,
    state: &str,
    page: u32,
  ) -> Result<GhListPage<GhPullRequest>, String> {
    core::github_cli::list_prs(repo, state, page)
  }

  fn issue(&self, repo: &str, number: u64) -> Result<GhIssue, String> {
    core::github_cli::issue(repo, number)
  }

  fn pr(&self, repo: &str, number: u64) -> Result<GhPullRequest, String> {
    core::github_cli::pr(repo, number)
  }

  fn pr_checks(&self, repo: &str, number: u64) -> Result<Option<PrCiStatus>, String> {
    core::github_cli::pr_checks(repo, number)
  }

  fn change_issue(&self, repo: &str, number: u64, change: &IssueChange) -> Result<(), String> {
    use core::github_cli as gh;
    match change {
      IssueChange::Comment(body) => gh::comment_on_issue(repo, number, body),
      IssueChange::Close(reason) => gh::close_issue(repo, number, reason),
      IssueChange::Reopen => gh::reopen_issue(repo, number),
      IssueChange::Edit { title, body } => gh::edit_issue(repo, number, title, body),
      IssueChange::Delete => gh::delete_issue(repo, number),
    }
  }

  fn change_pr(&self, repo: &str, number: u64, change: &PrChange) -> Result<(), String> {
    use core::github_cli as gh;
    match change {
      PrChange::Comment(body) => gh::comment_on_pr(repo, number, body),
      PrChange::Close => gh::close_pr(repo, number),
      PrChange::Reopen => gh::reopen_pr(repo, number),
      PrChange::Draft(draft) => gh::set_pr_draft(repo, number, *draft),
    }
  }

  fn react(&self, comment_id: &str, content: &str, on: bool) -> Result<(), String> {
    core::github_cli::set_reaction(comment_id, content, on)
  }

  fn create_issue(&self, repo: &str, title: &str, body: &str) -> Result<u64, String> {
    core::github_cli::create_issue(repo, title, body)
  }

  fn create_pr(
    &self,
    repo: &str,
    title: &str,
    body: &str,
    base: &str,
    head: &str,
    draft: bool,
  ) -> Result<u64, String> {
    core::github_cli::create_pr(repo, title, body, base, head, draft)
  }

  fn workspaces(&self, repo_path: &str) -> Result<Vec<Workspace>, String> {
    core::list_workspaces(repo_path)
  }

  fn pr_statuses(&self, repo_path: &str) -> HashMap<String, Option<PrInfo>> {
    crate::pr_status::global().list_cached(repo_path)
  }

  fn default_branch(&self, repo_path: &str) -> Result<String, String> {
    core::get_repo_default_branch(repo_path)
  }

  fn push_workspace(&self, repo_path: &str, workspace_id: i64) -> Result<(), String> {
    core::push_workspace_to_remote(repo_path, Some(workspace_id)).map(|_| ())
  }

  fn open_pr_workspace(&self, repo_path: &str, pr: &GhPullRequest) -> Result<(i64, bool), String> {
    let body = pr.body.as_deref().map(str::trim).filter(|b| !b.is_empty());
    let (workspace, created) = core::open_or_create_workspace_from_pr(
      repo_path,
      &pr.head_ref_name,
      &pr.base_ref_name,
      Some(&pr.title),
      body,
      Some(pr.number),
    )?;
    if created {
      crate::commands::workspace::spawn_setup_script_if_configured(&workspace);
      // The list's PRs carry no body; the poll fills it in.
      crate::pr_status::global()
        .queue_branch_refresh(repo_path.to_string(), pr.head_ref_name.clone());
    }
    Ok((workspace.id, created))
  }

  fn refresh_pr_status(&self, repo_path: &str, branch: &str) {
    let poller = crate::pr_status::global();
    poller.watch_repo(repo_path);
    poller.queue_branch_refresh(repo_path.to_string(), branch.to_string());
  }

  fn logs_enabled(&self) -> bool {
    true
  }

  fn treq_checks(&self, repo_path: &str, workspace_id: i64) -> TreqChecks {
    let workflows = core::list_workflows_sync(repo_path).unwrap_or_else(|err| {
      log::warn!("github panel: workflows: {err}");
      Vec::new()
    });
    let latest = workflows
      .iter()
      .filter_map(|w| {
        let run = core::list_workflow_runs_sync(repo_path, workspace_id, &w.filename, 1)
          .ok()?
          .into_iter()
          .next()?;
        Some((w.filename.clone(), run))
      })
      .collect();
    TreqChecks {
      trusted: core::repo::is_repo_trusted(repo_path),
      workflows,
      latest,
      setup: core::get_setup_script_status_sync(repo_path, workspace_id).ok(),
    }
  }

  fn run_history(
    &self,
    repo_path: &str,
    workspace_id: i64,
    filename: &str,
  ) -> Result<Vec<RunSummary>, String> {
    core::list_workflow_runs_sync(repo_path, workspace_id, filename, 20)
  }

  fn run_logs(&self, repo_path: &str, run_id: i64, job_id: &str) -> Result<Vec<LogLine>, String> {
    let query = core::checks_logs::LogQuery {
      levels: None,
      search: None,
      step_index: None,
      workflows: None,
      limit: None,
      offset: None,
    };
    Ok(
      core::get_run_logs_sync(repo_path, run_id, job_id, &query)?
        .into_iter()
        .map(|record| LogLine {
          stream: record.stream,
          body: record.body,
        })
        .collect(),
    )
  }

  fn trust_repo(&self, repo_path: &str) -> Result<(), String> {
    core::repo::trust_repo(repo_path)?;
    WorkspaceHost::notify(
      self,
      NativeEvent::WorkspaceChanged {
        repo_path: repo_path.into(),
      },
    );
    Ok(())
  }

  fn acts_locally(&self, repo_path: &str) -> bool {
    self.remote_containing(repo_path).is_none()
  }

  fn run_checks(
    &self,
    target: &crate::core::running_checks::ChecksTarget,
    run: &crate::core::running_checks::CheckRun,
  ) -> Result<Vec<crate::core::checks::JobResult>, String> {
    crate::core::running_checks::run(target, run)
  }
}

/// `uniqueByNumber`: pages can overlap when items move between requests;
/// the first copy stays.
pub fn unique_by_number<T>(items: Vec<T>, number: impl Fn(&T) -> u64) -> Vec<T> {
  let mut seen = HashSet::new();
  items
    .into_iter()
    .filter(|item| seen.insert(number(item)))
    .collect()
}

/// `CONVENTIONAL_TYPE_ALIASES`.
fn conventional_type(word: &str) -> Option<&'static str> {
  Some(match word.to_lowercase().as_str() {
    "feat" | "feature" => "feat",
    "fix" | "bugfix" | "hotfix" => "fix",
    "chore" => "chore",
    "docs" | "doc" | "documentation" => "docs",
    "refactor" => "refactor",
    "perf" | "performance" => "perf",
    "test" | "tests" => "test",
    "style" => "style",
    "build" => "build",
    "ci" => "ci",
    "revert" => "revert",
    _ => return None,
  })
}

#[expect(clippy::expect_used, reason = "constant pattern")]
fn humanize(slug: &str) -> String {
  static SEPARATORS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[-_]+").expect("regex"));
  SEPARATORS.replace_all(slug, " ").trim().to_string()
}

/// `buildGitHubComparePrUrl`: GitHub's "Open a pull request" form for
/// `head` into `base`, filled in.
pub fn compare_pr_url(
  owner: &str,
  repo: &str,
  base: &str,
  head: &str,
  title: &str,
  body: &str,
) -> String {
  // `encodeURIComponent` leaves `!'()*` as they are.
  let component = |s: &str| {
    urlencoding::encode(s)
      .replace("%21", "!")
      .replace("%27", "'")
      .replace("%28", "(")
      .replace("%29", ")")
      .replace("%2A", "*")
      // A fork head is `owner:branch`.
      .replace("%3A", ":")
  };
  let query = url::form_urlencoded::Serializer::new(String::new())
    .append_pair("expand", "1")
    .append_pair("title", title)
    .append_pair("body", body)
    .finish();
  format!(
    "https://github.com/{owner}/{repo}/compare/{}...{}?{query}",
    component(base),
    component(head)
  )
}

/// `deriveConventionalPrTitle`.
#[expect(clippy::expect_used, reason = "constant pattern")]
pub fn conventional_pr_title(title: &str, branch: &str) -> String {
  let fallback = if title.is_empty() { branch } else { title };
  static CONVENTIONAL: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)^([a-z]+)(\([^)]+\))?(!)?:\s*(.+)$").expect("regex"));
  if let Some(caps) = CONVENTIONAL.captures(fallback) {
    if let Some(kind) = conventional_type(&caps[1]) {
      let scope = caps.get(2).map_or("", |m| m.as_str());
      let breaking = caps.get(3).map_or("", |m| m.as_str());
      return format!("{kind}{scope}{breaking}: {}", &caps[4]);
    }
  }
  let custom = !title.is_empty() && title != branch;
  let segments: Vec<&str> = branch.split('/').filter(|s| !s.is_empty()).collect();
  for (i, segment) in segments.iter().enumerate() {
    if let Some(kind) = conventional_type(segment) {
      let description = if custom {
        title.to_string()
      } else {
        humanize(&segments[i + 1..].join(" "))
      };
      if !description.is_empty() {
        return format!("{kind}: {description}");
      }
      continue;
    }
    let mut parts = segment.split('-');
    let first = parts.next().unwrap_or_default();
    let rest: Vec<&str> = parts.collect();
    if let Some(kind) = conventional_type(first).filter(|_| !rest.is_empty()) {
      let description = if custom {
        title.to_string()
      } else {
        humanize(&rest.join("-"))
      };
      if !description.is_empty() {
        return format!("{kind}: {description}");
      }
    }
  }
  fallback.to_string()
}

/// `useWorkspacesWithoutOpenPr`: not archived, not the default branch, no
/// open pull request.
pub fn workspaces_without_open_pr(
  workspaces: Vec<Workspace>,
  statuses: &HashMap<String, Option<PrInfo>>,
  default_branch: Option<&str>,
) -> Vec<Workspace> {
  workspaces
    .into_iter()
    .filter(|ws| {
      !ws.archived
        && Some(ws.branch_name.as_str()) != default_branch
        && statuses
          .get(&ws.branch_name)
          .and_then(Option::as_ref)
          .is_none_or(|pr| pr.state != "OPEN")
    })
    .collect()
}

/// `CiState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiState {
  Success,
  Failure,
  Pending,
  /// A status that reports nothing about the code (the WIP app's): listed,
  /// muted, never spinning.
  Inactive,
}

impl CiState {
  /// `ciStatusStyle`: unknown states show as pending.
  pub fn parse(state: &str) -> Self {
    match state {
      "success" => CiState::Success,
      "failure" => CiState::Failure,
      _ => CiState::Pending,
    }
  }

  /// `ciStateForBucket`.
  pub fn of_bucket(bucket: &str) -> Self {
    match bucket {
      "pass" | "skipping" => CiState::Success,
      "fail" | "cancel" => CiState::Failure,
      crate::github::INACTIVE => CiState::Inactive,
      _ => CiState::Pending,
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      CiState::Success => "CI passed",
      CiState::Failure => "CI failed",
      CiState::Pending => "CI running",
      CiState::Inactive => "CI inactive",
    }
  }

  fn severity(self) -> u8 {
    match self {
      CiState::Failure => 0,
      CiState::Pending => 1,
      CiState::Success => 2,
      CiState::Inactive => 3,
    }
  }
}

/// The checks failed first, then pending, then passed
/// (`compareCiChecksBySeverity`, a stable sort).
pub fn by_severity(checks: &[crate::github::PrCheckEntry]) -> Vec<&crate::github::PrCheckEntry> {
  let mut sorted: Vec<_> = checks.iter().collect();
  sorted.sort_by_key(|c| CiState::of_bucket(&c.bucket).severity());
  sorted
}

/// `formatCheckDuration`.
pub fn check_duration(secs: u64) -> String {
  let (hours, minutes, seconds) = (secs / 3600, (secs % 3600) / 60, secs % 60);
  if hours > 0 {
    format!("{hours}h {minutes}m {seconds}s")
  } else if minutes > 0 {
    format!("{minutes}m {seconds}s")
  } else {
    format!("{seconds}s")
  }
}

pub use crate::core::running_checks::run_key;

/// How tall the expanded checks list grows before it scrolls.
pub const CHECKS_MAX_HEIGHT: f32 = 240.;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keeps_the_first_copy_of_each_number() {
    let items = vec![(1, "a"), (2, "b"), (1, "c")];
    assert_eq!(unique_by_number(items, |i| i.0), [(1, "a"), (2, "b")]);
  }

  #[test]
  fn derives_conventional_titles_as_react_does() {
    let t = conventional_pr_title;
    assert_eq!(t("Feature(ui)!: dark mode", "x"), "feat(ui)!: dark mode");
    assert_eq!(t("", "fix/login-bug"), "fix: login bug");
    assert_eq!(t("Repair login", "fix/login-bug"), "fix: Repair login");
    assert_eq!(t("", "bugfix-login-crash"), "fix: login crash");
    assert_eq!(t("", "zn/hotfix/a_b"), "fix: a b");
    assert_eq!(t("Plain title", "topic"), "Plain title");
    assert_eq!(t("", "fix"), "fix");
    assert_eq!(t("wip: thing", "wip"), "wip: thing");
  }

  fn workspace(branch: &str, archived: bool) -> Workspace {
    let mut ws: Workspace = serde_json::from_value(serde_json::json!({
      "id": 1, "repo_path": "/r", "workspace_name": branch, "workspace_path": branch,
      "branch_name": branch, "created_at": "", "refreshed_at": null, "metadata": null,
      "target_branch": null, "title": "", "description": null, "moved_files": null,
      "not_on_remote": false, "sparse_patterns": null,
    }))
    .expect("workspace");
    ws.archived = archived;
    ws
  }

  #[test]
  fn offers_workspaces_that_can_take_a_pr() {
    let info = |state: &str| PrInfo {
      number: 1,
      title: String::new(),
      state: state.into(),
      url: String::new(),
      head_ref_name: String::new(),
      base_ref_name: String::new(),
      merge_state_status: None,
      is_draft: false,
      body: String::new(),
    };
    let statuses = HashMap::from([
      ("open".to_string(), Some(info("OPEN"))),
      ("merged".to_string(), Some(info("MERGED"))),
      ("none".to_string(), None),
    ]);
    let all = vec![
      workspace("main", false),
      workspace("open", false),
      workspace("merged", false),
      workspace("none", false),
      workspace("old", true),
      workspace("new", false),
    ];
    let offered: Vec<_> = workspaces_without_open_pr(all, &statuses, Some("main"))
      .into_iter()
      .map(|w| w.branch_name)
      .collect();
    assert_eq!(offered, ["merged", "none", "new"]);
  }

  #[test]
  fn ci_states_and_durations_match_react() {
    assert_eq!(CiState::of_bucket("skipping"), CiState::Success);
    assert_eq!(CiState::of_bucket("cancel"), CiState::Failure);
    assert_eq!(CiState::of_bucket("queued"), CiState::Pending);
    assert_eq!(
      CiState::of_bucket(crate::github::INACTIVE),
      CiState::Inactive
    );
    assert_eq!(CiState::parse("weird"), CiState::Pending);
    assert_eq!(check_duration(5), "5s");
    assert_eq!(check_duration(65), "1m 5s");
    assert_eq!(check_duration(3725), "1h 2m 5s");
    let check = |name: &str, bucket: &str| crate::github::PrCheckEntry {
      name: name.into(),
      bucket: bucket.into(),
      link: String::new(),
      duration_secs: None,
    };
    let checks = [
      check("a", "pass"),
      check("b", "pending"),
      check("c", "fail"),
      check("d", crate::github::INACTIVE),
    ];
    let names: Vec<_> = by_severity(&checks)
      .iter()
      .map(|c| c.name.as_str())
      .collect();
    assert_eq!(names, ["c", "b", "a", "d"]);
  }
}
