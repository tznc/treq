//! What the New Workspaces dialog reads and does: which branch each line
//! names, whether it opens an existing branch or creates a new one, and the
//! backend calls that do it.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use super::{
  github_data::GithubHost, linear_data::LinearHost, workspace_dialog_data::WorkspaceDialogHost,
};
use crate::{
  core::{self, BranchWorkspaceRequest},
  github::GhPullRequest,
  gpui_shell::{native::Backend, remote_repo::Remote},
  linear::LinearIssue,
};

/// How many pull requests the dialog lists.
const PR_LIMIT: u32 = 50;

/// The dialog's route: the repository on screen and every repository the
/// sidebar shows, whose GitHub links the dialog accepts.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  pub repos: Vec<String>,
  /// The Linear integration is on, so the dialog offers the user's issues.
  #[serde(default)]
  pub linear: bool,
  /// The branch new workspaces stack on; the default branch when `None`.
  #[serde(default)]
  pub stack_on: Option<String>,
}

pub use crate::core::new_workspaces::{parse_lines, stack_parents, Line};

/// The dialog's footer: what submitting will create, from the checked lines
/// and the picked pull requests and issues.
pub fn preview(targets: &[Target]) -> String {
  let new: Vec<&Target> = targets.iter().filter(|t| !t.existing).collect();
  let open = targets.len() - new.len();
  match (new.as_slice(), open) {
    ([], 0) => "Type a line or pick an item to create workspaces.".to_string(),
    ([], 1) => format!("{} is already open", targets[0].branch),
    ([], n) => format!("{n} workspaces are already open"),
    ([one], 0) => match (&one.pr, &one.issue) {
      (Some(pr), _) => format!("Will create {} from PR #{pr}", one.branch),
      (None, Some(issue)) => format!("Will create {} from {}", one.branch, issue.identifier),
      (None, None) => format!("Will create {}", one.branch),
    },
    (new, open) => {
      let mut parts = vec![format!(
        "Will create {}",
        if new.len() == 1 {
          "1 workspace".to_string()
        } else {
          format!("{} workspaces", new.len())
        }
      )];
      // A line's base is set only when it stacks on the line above it.
      let stacked = new
        .iter()
        .filter(|t| t.pr.is_none() && t.base.is_some())
        .count();
      if stacked > 0 {
        parts.push(format!("{stacked} stacked"));
      }
      if open > 0 {
        parts.push(format!("{open} already open"));
      }
      parts.join(", ")
    }
  }
}

/// `checked` stacked on `parent`'s branch.
pub fn stack_on_line(checked: &Checked, parent: &Checked) -> Checked {
  let parent = parent
    .as_ref()
    .map_err(|_| "Stacks on a line that can't be opened".to_string())?;
  let mut target = checked.clone()?;
  if target.repo_path != parent.repo_path {
    return Err(format!(
      "Can't stack on {}, another repository's branch",
      parent.branch
    ));
  }
  target.base = Some(parent.branch.clone());
  Ok(target)
}

/// A workspace the dialog will open.
#[derive(Debug, Clone)]
pub struct Target {
  pub repo_path: String,
  pub branch: String,
  /// The rebase target: the pull request's base, or `None` for the branch
  /// the dialog stacks on.
  pub base: Option<String>,
  pub title: Option<String>,
  pub pr: Option<u64>,
  /// The Linear issue the workspace is for; its branch is made when missing.
  pub issue: Option<LinearIssue>,
  /// A workspace for the branch is open already.
  pub existing: bool,
  /// The branch does not exist yet, so the workspace makes it.
  pub create: bool,
}

impl Target {
  /// Opens the existing branch, stacked on its base or `stack_on`.
  pub fn request(&self, stack_on: &str) -> BranchWorkspaceRequest {
    BranchWorkspaceRequest {
      head_branch: self.branch.clone(),
      base_branch: self.base.clone().unwrap_or_else(|| stack_on.to_string()),
      title: self.title.clone(),
      description: None,
      pr_number: self.pr,
    }
  }

  /// A pull request of the repository on screen.
  pub fn from_pr(repo_path: &str, pr: &GhPullRequest, existing: bool) -> Self {
    Self {
      repo_path: repo_path.to_string(),
      branch: pr.head_ref_name.clone(),
      base: Some(pr.base_ref_name.clone()),
      title: Some(pr.title.clone()),
      pr: Some(pr.number),
      issue: None,
      existing,
      create: false,
    }
  }

  /// A Linear issue, opened on its branch in the repository on screen.
  pub fn from_issue(repo_path: &str, issue: &LinearIssue, existing: bool) -> Self {
    Self {
      repo_path: repo_path.to_string(),
      branch: issue.branch_name.clone(),
      base: None,
      title: Some(issue.title.clone()),
      pr: None,
      issue: Some(issue.clone()),
      existing,
      create: false,
    }
  }
}

/// The picked issue that `issue` is a sub-issue of.
fn picked_parent<'a>(issue: &LinearIssue, issues: &'a [LinearIssue]) -> Option<&'a LinearIssue> {
  issues.iter().find(|p| {
    issue.parent_id.as_deref() == Some(p.id.as_str())
      || issue.parent_identifier.as_deref() == Some(p.identifier.as_str())
  })
}

/// The workspaces for the picked Linear issues, a parent before its
/// sub-issues. A sub-issue stacks on its parent's workspace: the one the
/// dialog makes for a picked parent, or the open one in `linked` (branches
/// by Linear issue key).
pub fn issue_targets(
  repo_path: &str,
  issues: &[LinearIssue],
  open: &HashSet<String>,
  linked: &HashMap<String, String>,
) -> Vec<Target> {
  fn visit<'a>(
    issue: &'a LinearIssue,
    issues: &'a [LinearIssue],
    seen: &mut HashSet<&'a str>,
    order: &mut Vec<&'a LinearIssue>,
  ) {
    if !seen.insert(&issue.id) {
      return;
    }
    if let Some(parent) = picked_parent(issue, issues) {
      visit(parent, issues, seen, order);
    }
    order.push(issue);
  }
  let (mut seen, mut order) = (HashSet::new(), Vec::new());
  for issue in issues {
    visit(issue, issues, &mut seen, &mut order);
  }
  order
    .into_iter()
    .map(|issue| {
      let existing = open.contains(&issue.branch_name) || linked.contains_key(&issue.identifier);
      let mut target = Target::from_issue(repo_path, issue, existing);
      // Stacking only affects a workspace that is about to be made.
      if !existing {
        target.base = match picked_parent(issue, issues) {
          Some(parent) => Some(
            linked
              .get(&parent.identifier)
              .unwrap_or(&parent.branch_name)
              .clone(),
          ),
          None => issue
            .parent_identifier
            .as_ref()
            .and_then(|key| linked.get(key).cloned()),
        };
      }
      target
    })
    .collect()
}

/// What checking a line found.
pub type Checked = Result<Target, String>;

/// What the dialog asks of the backend; faked in tests. Every call blocks.
pub trait OpenWorkspacesHost: GithubHost + LinearHost + WorkspaceDialogHost {
  /// Fetches the repository's remote, so its branches can be checked.
  fn fetch(&self, repo_path: &str);
  /// Open pull requests, newest first: the user's own, or everyone's
  /// matching `query`.
  fn open_prs(&self, repo: &str, mine: bool, query: &str) -> Result<Vec<GhPullRequest>, String>;
  /// Opens or creates a workspace for each request: its id, and whether it
  /// was made.
  fn open_workspaces(
    &self,
    repo_path: &str,
    requests: &[BranchWorkspaceRequest],
  ) -> Vec<Result<(i64, bool), String>>;
  /// Opens or creates the workspace for a Linear issue, as the Linear view
  /// does: its id, and whether it was made.
  fn open_issue(&self, repo_path: &str, issue: &LinearIssue) -> Result<(i64, bool), String>;
}

impl OpenWorkspacesHost for Backend {
  fn fetch(&self, repo_path: &str) {
    if self.remote(repo_path).is_some() {
      return;
    }
    if let Err(err) = core::repo::fetch_repo(repo_path) {
      log::warn!("open workspaces: fetch {repo_path}: {err}");
    }
  }

  fn open_prs(&self, repo: &str, mine: bool, query: &str) -> Result<Vec<GhPullRequest>, String> {
    core::github_cli::search_open_prs(repo, mine, query, PR_LIMIT)
  }

  fn open_workspaces(
    &self,
    repo_path: &str,
    requests: &[BranchWorkspaceRequest],
  ) -> Vec<Result<(i64, bool), String>> {
    if let Some(remote) = self.remote(repo_path) {
      return requests
        .iter()
        .map(|request| open_remote_workspace(&remote, request))
        .collect();
    }
    core::open_or_create_workspaces(repo_path, requests)
      .into_iter()
      .zip(requests)
      .map(|(result, request)| {
        let (workspace, created) = result?;
        if created {
          crate::commands::workspace::spawn_setup_script_if_configured(&workspace);
          // The dialog's PRs carry no body; the poll fills it in.
          if request.pr_number.is_some() {
            crate::pr_status::global()
              .queue_branch_refresh(repo_path.to_string(), workspace.branch_name.clone());
          }
        }
        Ok((workspace.id, created))
      })
      .collect()
  }

  fn open_issue(&self, repo_path: &str, issue: &LinearIssue) -> Result<(i64, bool), String> {
    self.local_only(repo_path, "Opening a Linear issue's workspace")?;
    let opened = crate::gpui_shell::block_on(
      crate::commands::linear::open_or_create_workspace_from_linear_issue(repo_path, issue),
    )?;
    if opened.created {
      if let Ok(Some(workspace)) = core::find_workspace(repo_path, opened.workspace_id) {
        crate::commands::workspace::spawn_setup_script_if_configured(&workspace);
      }
    }
    Ok((opened.workspace_id, opened.created))
  }
}

/// Opens the workspace of an existing branch on the window's remote
/// repository, or makes one on its base, as `open_or_create_workspaces`
/// does locally.
fn open_remote_workspace(
  remote: &Remote,
  request: &BranchWorkspaceRequest,
) -> Result<(i64, bool), String> {
  let head = request.head_branch.trim();
  if let Some(existing) = remote
    .workspaces()?
    .into_iter()
    .find(|w| w.branch_name == head)
  {
    return Ok((existing.id, false));
  }
  let metadata = core::WorkspaceMetadata {
    title: request.title.clone().filter(|t| !t.trim().is_empty()),
    description: request.description.clone(),
    ..Default::default()
  };
  let metadata = serde_json::to_string(&metadata).map_err(|e| e.to_string())?;
  let id = remote.create_workspace(head, None, Some(metadata))?;
  let base = request.base_branch.trim();
  if !base.is_empty() {
    remote.update_workspace(id, Some(base), None, None)?;
  }
  Ok((id, true))
}

/// The repositories' `owner/name`, by path; one without a GitHub remote is
/// left out.
pub fn remotes(host: &dyn OpenWorkspacesHost, repos: &[String]) -> Vec<(String, String)> {
  repos
    .iter()
    .filter_map(|path| {
      let remote = host.remote(path).ok().flatten()?;
      Some((path.clone(), remote.full_name))
    })
    .collect()
}

/// Checks `line` against the backend: the repository it belongs to,
/// whether its branch exists or is made, and whether a workspace is open for
/// it. `pattern` names a title's branch.
pub fn check(
  host: &dyn OpenWorkspacesHost,
  repo_path: &str,
  remotes: &[(String, String)],
  pattern: &str,
  line: &Line,
) -> Checked {
  let repo_of = |full_name: &str| {
    remotes
      .iter()
      .find(|(_, name)| name.eq_ignore_ascii_case(full_name))
      .map(|(path, _)| path.clone())
      .ok_or_else(|| format!("{full_name} is not one of your repositories"))
  };
  match line {
    Line::Invalid(reason) => Err(reason.clone()),
    Line::Branch(branch) => {
      // `git branch -r` prints `origin/feat`.
      let short = branch.strip_prefix("origin/");
      if branch_exists(host, repo_path, branch)? {
        branch_target(host, repo_path, branch, None)
      } else if let Some(short) =
        short.filter(|s| branch_exists(host, repo_path, s).unwrap_or(false))
      {
        branch_target(host, repo_path, short, None)
      } else {
        new_target(host, repo_path, branch, None)
      }
    }
    Line::Title(title) => {
      let branch = super::workspace_dialog_data::branch_from_title(pattern, title);
      if branch_exists(host, repo_path, &branch)? {
        branch_target(host, repo_path, &branch, Some(title))
      } else {
        new_target(host, repo_path, &branch, Some(title))
      }
    }
    Line::RepoBranch { repo, branch } => {
      let path = repo_of(repo)?;
      if !branch_exists(host, &path, branch)? {
        return Err(format!("No branch '{branch}' in {repo}"));
      }
      branch_target(host, &path, branch, None)
    }
    Line::Pr { repo, number } => {
      let path = repo_of(repo)?;
      let pr = host.pr(repo, *number)?;
      if !pr.state.eq_ignore_ascii_case("open") {
        return Err(format!(
          "Pull request #{number} is {}",
          pr.state.to_lowercase()
        ));
      }
      if !branch_exists(host, &path, &pr.head_ref_name)? {
        return Err(format!(
          "Pull request #{number}'s branch '{}' is not on the remote",
          pr.head_ref_name
        ));
      }
      let existing = is_open(host, &path, &pr.head_ref_name);
      Ok(Target::from_pr(&path, &pr, existing))
    }
  }
}

fn branch_exists(
  host: &dyn OpenWorkspacesHost,
  repo_path: &str,
  branch: &str,
) -> Result<bool, String> {
  let status = host.branch_status(repo_path, branch)?;
  Ok(status.local_exists || status.remote_exists)
}

fn branch_target(
  host: &dyn OpenWorkspacesHost,
  repo_path: &str,
  branch: &str,
  title: Option<&str>,
) -> Checked {
  let default = WorkspaceDialogHost::default_branch(host, repo_path)?;
  if branch == default {
    return Err(format!("'{branch}' is the default branch"));
  }
  Ok(Target {
    repo_path: repo_path.to_string(),
    branch: branch.to_string(),
    base: None,
    title: title.map(str::to_string),
    pr: None,
    issue: None,
    existing: is_open(host, repo_path, branch),
    create: false,
  })
}

/// A branch no one has made yet, made for a new workspace.
fn new_target(
  host: &dyn OpenWorkspacesHost,
  repo_path: &str,
  branch: &str,
  title: Option<&str>,
) -> Checked {
  core::validate_branch_name(branch).map_err(|err| {
    let prefix = format!("Invalid branch name '{branch}': ");
    format!(
      "Not a valid branch name: {}",
      err.strip_prefix(&prefix).unwrap_or(&err)
    )
  })?;
  Ok(Target {
    create: true,
    ..branch_target(host, repo_path, branch, title)?
  })
}

/// The Linear issues to offer: with no search, the user's own that are not
/// done, or everyone's when they have none; with one, everyone's that match
/// by identifier or title. Says whether the list is the user's own.
pub fn issue_list<'a>(
  issues: &'a [LinearIssue],
  viewer: Option<&str>,
  query: &str,
) -> (Vec<&'a LinearIssue>, bool) {
  let query = query.trim().to_lowercase();
  let open = issues
    .iter()
    .filter(|i| !matches!(i.state.state_type.as_str(), "completed" | "canceled"));
  if !query.is_empty() {
    let matching = open
      .filter(|i| {
        i.identifier.to_lowercase().contains(&query) || i.title.to_lowercase().contains(&query)
      })
      .collect();
    return (matching, false);
  }
  let open: Vec<&LinearIssue> = open.collect();
  let mine: Vec<&LinearIssue> = open
    .iter()
    .copied()
    .filter(|i| viewer.is_some_and(|v| i.assignee.as_ref().is_some_and(|a| a.id == v)))
    .collect();
  if mine.is_empty() {
    (open, false)
  } else {
    (mine, true)
  }
}

/// Whether a workspace is open for `branch`.
pub fn is_open(host: &dyn OpenWorkspacesHost, repo_path: &str, branch: &str) -> bool {
  GithubHost::workspaces(host, repo_path)
    .map(|all| all.iter().any(|w| w.branch_name == branch))
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::new_workspaces::parse_line;

  fn branch(name: &str) -> Line {
    Line::Branch(name.into())
  }

  #[test]
  fn parse_line_reads_branch_names() {
    assert_eq!(parse_line("feat/login"), branch("feat/login"));
    assert_eq!(parse_line(" feat/login@origin "), branch("feat/login"));
    assert_eq!(parse_line("refs/heads/fix"), branch("fix"));
  }

  #[test]
  fn parse_line_reads_pull_request_links() {
    let pr = Line::Pr {
      repo: "acme/app".into(),
      number: 42,
    };
    assert_eq!(parse_line("https://github.com/acme/app/pull/42"), pr);
    assert_eq!(parse_line("github.com/acme/app/pull/42/files#diff"), pr);
    assert_eq!(
      parse_line("https://www.github.com/acme/app.git/pull/42"),
      pr
    );
  }

  #[test]
  fn parse_line_reads_branch_links() {
    assert_eq!(
      parse_line("https://github.com/acme/app/tree/feat/login?tab=readme"),
      Line::RepoBranch {
        repo: "acme/app".into(),
        branch: "feat/login".into()
      }
    );
  }

  #[test]
  fn parse_line_rejects_other_text() {
    for line in [
      "https://github.com/acme/app",
      "https://github.com/acme/app/issues/3",
      "https://github.com/acme/app/pull/abc",
      "https://gitlab.com/acme/app/pull/1",
    ] {
      assert!(matches!(parse_line(line), Line::Invalid(_)), "{line}");
    }
  }

  fn issue(id: &str, state: &str, assignee: Option<&str>) -> LinearIssue {
    serde_json::from_value(serde_json::json!({
      "id": id,
      "identifier": format!("ENG-{id}"),
      "title": format!("Issue {id}"),
      "description": null,
      "state": {"name": state, "type": state},
      "labels": [],
      "branch_name": format!("eng-{id}"),
      "parent_id": null,
      "sub_issue_ids": [],
      "url": "",
      "assignee": assignee.map(|a| serde_json::json!({"id": a, "name": a})),
      "priority": 0,
      "priority_label": "",
      "project": null,
    }))
    .expect("issue")
  }

  fn ids(list: &[&LinearIssue]) -> Vec<String> {
    list.iter().map(|i| i.id.clone()).collect()
  }

  fn sub_issue(id: &str, parent: &str) -> LinearIssue {
    let mut issue = issue(id, "unstarted", None);
    issue.parent_id = Some(parent.to_string());
    issue.parent_identifier = Some(format!("ENG-{parent}"));
    issue
  }

  fn bases(targets: &[Target]) -> Vec<(&str, Option<&str>, bool)> {
    targets
      .iter()
      .map(|t| (t.branch.as_str(), t.base.as_deref(), t.existing))
      .collect()
  }

  #[test]
  fn issue_targets_put_a_picked_parent_before_its_sub_issue() {
    let picked = [sub_issue("2", "1"), issue("1", "unstarted", None)];
    let targets = issue_targets("/repo", &picked, &HashSet::new(), &HashMap::new());
    assert_eq!(
      bases(&targets),
      vec![("eng-1", None, false), ("eng-2", Some("eng-1"), false)]
    );
  }

  #[test]
  fn issue_targets_stack_on_a_parent_workspace_that_is_open() {
    let linked = HashMap::from([("ENG-1".to_string(), "octocat/parent".to_string())]);
    let targets = issue_targets("/repo", &[sub_issue("2", "1")], &HashSet::new(), &linked);
    assert_eq!(
      bases(&targets),
      vec![("eng-2", Some("octocat/parent"), false)]
    );
  }

  #[test]
  fn issue_targets_keep_the_default_branch_without_a_parent_workspace() {
    let targets = issue_targets(
      "/repo",
      &[sub_issue("2", "1")],
      &HashSet::new(),
      &HashMap::new(),
    );
    assert_eq!(bases(&targets), vec![("eng-2", None, false)]);
  }

  #[test]
  fn issue_targets_leave_an_open_sub_issue_where_it_is() {
    let linked = HashMap::from([
      ("ENG-1".to_string(), "eng-1".to_string()),
      ("ENG-2".to_string(), "eng-2".to_string()),
    ]);
    let targets = issue_targets("/repo", &[sub_issue("2", "1")], &HashSet::new(), &linked);
    assert_eq!(bases(&targets), vec![("eng-2", None, true)]);
  }

  #[test]
  fn issue_list_shows_the_users_open_issues_first() {
    let issues = [
      issue("1", "started", Some("me")),
      issue("2", "completed", Some("me")),
      issue("3", "unstarted", Some("you")),
    ];
    let (list, mine) = issue_list(&issues, Some("me"), "");
    assert_eq!((ids(&list), mine), (vec!["1".to_string()], true));

    let (list, mine) = issue_list(&issues, Some("nobody"), "");
    assert_eq!((ids(&list), mine), (vec!["1".into(), "3".into()], false));

    let (list, mine) = issue_list(&issues, Some("me"), "eng-3");
    assert_eq!((ids(&list), mine), (vec!["3".to_string()], false));
  }

  #[test]
  fn parse_line_reads_titles() {
    assert_eq!(
      parse_line(" Fix the login flow "),
      Line::Title("Fix the login flow".into())
    );
  }

  #[test]
  fn parse_lines_skips_blanks_and_repeats() {
    let lines = parse_lines("feat/a\n\n  feat/b \nfeat/a\n");
    let names: Vec<&str> = lines.iter().map(|(raw, _)| raw.as_str()).collect();
    assert_eq!(names, ["feat/a", "feat/b"]);
  }
  #[test]
  fn indented_lines_stack_in_order_on_the_line_above() {
    let parents = stack_parents("feat/a\n  feat/b\n\n  Fix login\nfeat/c\n\tfeat/d\n  feat/a\n");
    let get = |line: &str| parents.get(line).map(String::as_str);
    assert_eq!(get("feat/a"), None);
    assert_eq!(get("feat/b"), Some("feat/a"));
    assert_eq!(get("Fix login"), Some("feat/b"));
    assert_eq!(get("feat/c"), None);
    assert_eq!(get("feat/d"), Some("feat/c"));
  }

  #[test]
  fn stacking_on_a_line_sets_its_base() {
    let target = |repo: &str, branch: &str| Target {
      repo_path: repo.into(),
      branch: branch.into(),
      base: None,
      title: None,
      pr: None,
      issue: None,
      existing: false,
      create: true,
    };
    let child = Ok(target("/r", "feat/b"));
    let stacked = stack_on_line(&child, &Ok(target("/r", "feat/a"))).unwrap();
    assert_eq!(stacked.base.as_deref(), Some("feat/a"));
    assert_eq!(
      stack_on_line(&child, &Err("bad".into())).unwrap_err(),
      "Stacks on a line that can't be opened"
    );
    assert!(stack_on_line(&child, &Ok(target("/other", "feat/a"))).is_err());
    assert_eq!(
      stack_on_line(&Err("x".into()), &Ok(target("/r", "a"))).unwrap_err(),
      "x"
    );
  }

  fn line(branch: &str) -> Target {
    Target {
      repo_path: "/r".into(),
      branch: branch.into(),
      base: None,
      title: None,
      pr: None,
      issue: None,
      existing: false,
      create: true,
    }
  }

  #[test]
  fn preview_says_nothing_is_picked_yet() {
    assert_eq!(
      preview(&[]),
      "Type a line or pick an item to create workspaces."
    );
  }

  #[test]
  fn preview_names_a_single_workspace_and_its_source() {
    assert_eq!(preview(&[line("feat/a")]), "Will create feat/a");
    let mut pr = line("feat/login-tests");
    pr.pr = Some(42);
    pr.base = Some("main".into());
    assert_eq!(preview(&[pr]), "Will create feat/login-tests from PR #42");
    let mut open = line("feat/c");
    open.existing = true;
    assert_eq!(preview(&[open]), "feat/c is already open");
  }

  #[test]
  fn preview_counts_stacked_and_open_workspaces() {
    let mut stacked = line("feat/b");
    stacked.base = Some("feat/a".into());
    assert_eq!(
      preview(&[line("feat/a"), stacked.clone()]),
      "Will create 2 workspaces, 1 stacked"
    );
    let mut open = line("feat/c");
    open.existing = true;
    assert_eq!(
      preview(&[line("feat/a"), stacked, open.clone()]),
      "Will create 2 workspaces, 1 stacked, 1 already open"
    );
    assert_eq!(
      preview(&[open.clone(), open]),
      "2 workspaces are already open"
    );
  }
}
