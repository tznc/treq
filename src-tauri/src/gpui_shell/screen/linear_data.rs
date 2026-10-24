//! What the Linear view (`LinearPanel`) reads from the backend, and the
//! filtering, grouping and highlighting it does on what it read.
//!
//! The Linear client lives in the backend (`crate::linear`), with the repo's
//! `linear_api_key` or the OAuth proxy session React pushes. GPUI calls it
//! directly; React only starts the sessions it owns (an issue's kickoff, an
//! agent review).

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use super::changes_data::ChangesHost;
use crate::{
  core::{
    feature_preview::{self, PreviewFeature},
    linear_review::{self, LinearReviewSnapshot, LinearReviewSnapshotComment},
  },
  gpui_shell::native::Backend,
  linear::{LinearComment, LinearDocument, LinearIssue, LinearProject, LinearTeam, LinearUser},
  local_db::AgentReviewComment,
};

/// What React sends: the repository whose Linear settings apply.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
}

/// Whose comments to read.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Commented {
  Issue(String),
  Project(String),
  Document(String),
}

impl Commented {
  /// The agent review target type of this text.
  pub fn target_type(&self) -> &'static str {
    match self {
      Commented::Issue(_) => linear_review::TARGET_LINEAR_ISSUE,
      Commented::Project(_) => linear_review::TARGET_LINEAR_PROJECT,
      Commented::Document(_) => linear_review::TARGET_LINEAR_DOCUMENT,
    }
  }

  pub fn id(&self) -> &str {
    match self {
      Commented::Issue(id) | Commented::Project(id) | Commented::Document(id) => id,
    }
  }

  /// `LINEAR_KIND_LABEL`.
  fn kind_label(&self) -> &'static str {
    match self {
      Commented::Issue(_) => "issue",
      Commented::Project(_) => "project",
      Commented::Document(_) => "document",
    }
  }
}

/// What the Linear view asks of the backend; faked in tests. Everything
/// blocks, so it is called off the main thread.
pub trait LinearHost: ChangesHost {
  fn linear_teams(&self, repo_path: &str) -> Result<Vec<LinearTeam>, String>;
  fn linear_issues(&self, repo_path: &str, team: Option<&str>) -> Result<Vec<LinearIssue>, String>;
  fn linear_viewer(&self, repo_path: &str) -> Result<LinearUser, String>;
  fn linear_projects(&self, repo_path: &str) -> Result<Vec<LinearProject>, String>;
  fn linear_documents(
    &self,
    repo_path: &str,
    project_id: &str,
  ) -> Result<Vec<LinearDocument>, String>;
  fn linear_comments(&self, repo_path: &str, of: &Commented) -> Result<Vec<LinearComment>, String>;
  /// Writes the review snapshot (`linear_prepare_review`); returns its directory.
  fn linear_prepare_review(
    &self,
    repo_path: &str,
    of: &Commented,
    snapshot: &LinearReviewSnapshot,
  ) -> Result<String, String>;
  /// The review agent's comments on `of` (`list_agent_review_comments`).
  fn linear_review_comments(
    &self,
    repo_path: &str,
    of: &Commented,
  ) -> Result<Vec<AgentReviewComment>, String>;
  /// Replaces the reviewed passage in Linear with the comment's suggestion.
  fn linear_apply_suggestion(&self, repo_path: &str, comment_id: &str) -> Result<(), String>;
}

impl Backend {
  /// The repo's Linear client, as the `linear_*` commands resolve it.
  fn linear_client(&self, repo_path: &str) -> Result<crate::linear::LinearClientSource, String> {
    self.with_db(|db| {
      feature_preview::require(db, PreviewFeature::LinearIntegration)?;
      crate::linear::resolve_linear_client(repo_path, db)
    })
  }
}

impl LinearHost for Backend {
  fn linear_teams(&self, repo_path: &str) -> Result<Vec<LinearTeam>, String> {
    let client = self.linear_client(repo_path)?;
    crate::gpui_shell::block_on(crate::linear::linear_list_teams_impl(&client))
  }

  fn linear_issues(&self, repo_path: &str, team: Option<&str>) -> Result<Vec<LinearIssue>, String> {
    let client = self.linear_client(repo_path)?;
    crate::gpui_shell::block_on(crate::linear::linear_list_issues_impl(&client, team))
  }

  fn linear_viewer(&self, repo_path: &str) -> Result<LinearUser, String> {
    let client = self.linear_client(repo_path)?;
    crate::gpui_shell::block_on(crate::linear::linear_get_viewer_impl(&client))
  }

  fn linear_projects(&self, repo_path: &str) -> Result<Vec<LinearProject>, String> {
    let client = self.linear_client(repo_path)?;
    crate::gpui_shell::block_on(crate::linear::linear_list_projects_impl(&client))
  }

  fn linear_documents(
    &self,
    repo_path: &str,
    project_id: &str,
  ) -> Result<Vec<LinearDocument>, String> {
    let client = self.linear_client(repo_path)?;
    crate::gpui_shell::block_on(crate::linear::linear_list_project_documents_impl(
      &client, project_id,
    ))
  }

  fn linear_comments(&self, repo_path: &str, of: &Commented) -> Result<Vec<LinearComment>, String> {
    let client = self.linear_client(repo_path)?;
    crate::gpui_shell::block_on(async {
      match of {
        Commented::Issue(id) => crate::linear::linear_list_issue_comments_impl(&client, id).await,
        Commented::Project(id) => {
          crate::linear::linear_list_project_comments_impl(&client, id).await
        }
        Commented::Document(id) => {
          crate::linear::linear_list_document_comments_impl(&client, id).await
        }
      }
    })
  }

  fn linear_prepare_review(
    &self,
    repo_path: &str,
    of: &Commented,
    snapshot: &LinearReviewSnapshot,
  ) -> Result<String, String> {
    linear_review::write_snapshot(repo_path, of.target_type(), of.id(), snapshot)
      .map(|dir| dir.to_string_lossy().into_owned())
  }

  fn linear_review_comments(
    &self,
    repo_path: &str,
    of: &Commented,
  ) -> Result<Vec<AgentReviewComment>, String> {
    crate::core::agent_review::list_comments(repo_path, of.target_type(), of.id())
  }

  fn linear_apply_suggestion(&self, repo_path: &str, comment_id: &str) -> Result<(), String> {
    let client = self.linear_client(repo_path)?;
    crate::gpui_shell::block_on(crate::linear::linear_apply_review_suggestion_impl(
      &client, repo_path, comment_id,
    ))
  }
}

/// The issue list's preset views (`STANDARD_VIEWS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IssueView {
  #[default]
  All,
  Active,
  MyIssues,
  Backlog,
}

impl IssueView {
  pub const ALL: [IssueView; 4] = [
    IssueView::All,
    IssueView::Active,
    IssueView::MyIssues,
    IssueView::Backlog,
  ];

  pub fn value(self) -> &'static str {
    match self {
      IssueView::All => "all",
      IssueView::Active => "active",
      IssueView::MyIssues => "my-issues",
      IssueView::Backlog => "backlog",
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      IssueView::All => "All Issues",
      IssueView::Active => "Active",
      IssueView::MyIssues => "My Issues",
      IssueView::Backlog => "Backlog",
    }
  }
}

/// `applyStandardView`.
pub fn issue_view<'a>(
  issues: &'a [LinearIssue],
  view: IssueView,
  viewer: Option<&str>,
) -> Vec<&'a LinearIssue> {
  issues
    .iter()
    .filter(|i| match view {
      IssueView::All => true,
      IssueView::Active => matches!(i.state.state_type.as_str(), "started" | "unstarted"),
      IssueView::Backlog => i.state.state_type == "backlog",
      IssueView::MyIssues => viewer.is_some_and(|v| i.assignee.as_ref().is_some_and(|a| a.id == v)),
    })
    .collect()
}

/// The issue filters; `None` is "Any".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueFilters {
  pub assignee: Option<String>,
  pub priority: Option<i32>,
  pub label: Option<String>,
  pub project: Option<String>,
}

/// One filter value: what it matches, and what the menu shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterOption {
  pub value: String,
  pub label: String,
}

impl FilterOption {
  fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
    Self {
      value: value.into(),
      label: label.into(),
    }
  }
}

/// `{id, name}` pairs in first-seen order, the last name winning, as a JS
/// `Map` keeps them.
fn ordered_pairs<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<FilterOption> {
  let mut out: Vec<FilterOption> = Vec::new();
  for (id, name) in pairs {
    match out.iter_mut().find(|o| o.value == id) {
      Some(found) => found.label = name.to_string(),
      None => out.push(FilterOption::new(id, name)),
    }
  }
  out
}

/// What each issue filter offers (`deriveFilterOptions`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueFilterOptions {
  pub assignees: Vec<FilterOption>,
  /// By priority value, ascending.
  pub priorities: Vec<FilterOption>,
  /// Sorted.
  pub labels: Vec<FilterOption>,
  pub projects: Vec<FilterOption>,
}

pub fn issue_filter_options(issues: &[&LinearIssue]) -> IssueFilterOptions {
  let assignees = ordered_pairs(issues.iter().filter_map(|i| {
    i.assignee
      .as_ref()
      .map(|a| (a.id.as_str(), a.name.as_str()))
  }));
  let mut priorities: Vec<(i32, String)> = Vec::new();
  for issue in issues {
    let label = if issue.priority_label.is_empty() {
      issue.priority.to_string()
    } else {
      issue.priority_label.clone()
    };
    match priorities.iter_mut().find(|(p, _)| *p == issue.priority) {
      Some(found) => found.1 = label,
      None => priorities.push((issue.priority, label)),
    }
  }
  priorities.sort_by_key(|(p, _)| *p);
  let labels: std::collections::BTreeSet<&str> = issues
    .iter()
    .flat_map(|i| i.labels.iter().map(String::as_str))
    .collect();
  let projects = ordered_pairs(
    issues
      .iter()
      .filter_map(|i| i.project.as_ref().map(|p| (p.id.as_str(), p.name.as_str()))),
  );
  IssueFilterOptions {
    assignees,
    priorities: priorities
      .into_iter()
      .map(|(p, label)| FilterOption::new(p.to_string(), label))
      .collect(),
    labels: labels
      .into_iter()
      .map(|l| FilterOption::new(l, l))
      .collect(),
    projects,
  }
}

/// `applyIssueFilters`.
pub fn filter_issues<'a>(issues: &[&'a LinearIssue], f: &IssueFilters) -> Vec<&'a LinearIssue> {
  issues
    .iter()
    .copied()
    .filter(|i| {
      f.assignee
        .as_ref()
        .is_none_or(|id| i.assignee.as_ref().is_some_and(|a| &a.id == id))
        && f.priority.is_none_or(|p| i.priority == p)
        && f.label.as_ref().is_none_or(|l| i.labels.contains(l))
        && f
          .project
          .as_ref()
          .is_none_or(|id| i.project.as_ref().is_some_and(|p| &p.id == id))
    })
    .collect()
}

/// `splitRootIssues`: a sub-issue nests under its parent only when the
/// parent is shown too; otherwise it is a root, so filters never hide it.
pub fn split_roots<'a>(
  issues: &[&'a LinearIssue],
) -> (Vec<&'a LinearIssue>, HashMap<String, Vec<&'a LinearIssue>>) {
  let shown: HashSet<&str> = issues.iter().map(|i| i.id.as_str()).collect();
  let mut roots = Vec::new();
  let mut subs: HashMap<String, Vec<&LinearIssue>> = HashMap::new();
  for issue in issues {
    match issue.parent_id.as_deref().filter(|p| shown.contains(p)) {
      Some(parent) => subs.entry(parent.to_string()).or_default().push(issue),
      None => roots.push(*issue),
    }
  }
  (roots, subs)
}

/// One Kanban column: a workflow state and its issues.
#[derive(Debug, Clone)]
pub struct KanbanColumn<'a> {
  pub name: String,
  pub state_type: String,
  pub issues: Vec<&'a LinearIssue>,
}

/// Linear's workflow order; unknown state types sort last.
const STATE_TYPE_ORDER: [&str; 6] = [
  "triage",
  "backlog",
  "unstarted",
  "started",
  "completed",
  "canceled",
];

fn state_rank(state_type: &str) -> usize {
  STATE_TYPE_ORDER
    .iter()
    .position(|t| *t == state_type)
    .unwrap_or(STATE_TYPE_ORDER.len())
}

/// `localeCompare` for names: case-insensitive first.
fn locale_cmp(a: &str, b: &str) -> std::cmp::Ordering {
  a.to_lowercase()
    .cmp(&b.to_lowercase())
    .then_with(|| b.cmp(a))
}

/// `groupByState`: every issue, sub-issues too, in its own state's column.
pub fn group_by_state<'a>(issues: &[&'a LinearIssue]) -> Vec<KanbanColumn<'a>> {
  let mut columns: Vec<KanbanColumn> = Vec::new();
  for issue in issues {
    match columns.iter_mut().find(|c| c.name == issue.state.name) {
      Some(column) => column.issues.push(issue),
      None => columns.push(KanbanColumn {
        name: issue.state.name.clone(),
        state_type: issue.state.state_type.clone(),
        issues: vec![issue],
      }),
    }
  }
  columns.sort_by(|a, b| {
    state_rank(&a.state_type)
      .cmp(&state_rank(&b.state_type))
      .then_with(|| locale_cmp(&a.name, &b.name))
  });
  columns
}

/// The project list's preset views (`PROJECT_STANDARD_VIEWS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectView {
  #[default]
  All,
  Mine,
  Active,
  Backlog,
}

impl ProjectView {
  pub const ALL: [ProjectView; 4] = [
    ProjectView::All,
    ProjectView::Mine,
    ProjectView::Active,
    ProjectView::Backlog,
  ];

  pub fn value(self) -> &'static str {
    match self {
      ProjectView::All => "all",
      ProjectView::Mine => "my-projects",
      ProjectView::Active => "active",
      ProjectView::Backlog => "backlog",
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      ProjectView::All => "All",
      ProjectView::Mine => "Mine",
      ProjectView::Active => "Active",
      ProjectView::Backlog => "Backlog",
    }
  }
}

/// `applyProjectStandardView`.
pub fn project_view<'a>(
  projects: &'a [LinearProject],
  view: ProjectView,
  viewer: Option<&str>,
) -> Vec<&'a LinearProject> {
  projects
    .iter()
    .filter(|p| match view {
      ProjectView::All => true,
      ProjectView::Active => p.state == "started",
      // Not started yet: Linear's `backlog` and `planned` project states.
      ProjectView::Backlog => matches!(p.state.as_str(), "backlog" | "planned"),
      ProjectView::Mine => viewer.is_some_and(|v| p.lead.as_ref().is_some_and(|l| l.id == v)),
    })
    .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectFilters {
  pub state: Option<String>,
  pub lead: Option<String>,
}

/// What the project filters offer (`deriveProjectFilterOptions`): states
/// sorted, leads in first-seen order.
pub fn project_filter_options(
  projects: &[&LinearProject],
) -> (Vec<FilterOption>, Vec<FilterOption>) {
  let states: std::collections::BTreeSet<&str> =
    projects.iter().map(|p| p.state.as_str()).collect();
  let leads = ordered_pairs(
    projects
      .iter()
      .filter_map(|p| p.lead.as_ref().map(|l| (l.id.as_str(), l.name.as_str()))),
  );
  (
    states
      .into_iter()
      .map(|s| FilterOption::new(s, s))
      .collect(),
    leads,
  )
}

/// `applyProjectFilters`.
pub fn filter_projects<'a>(
  projects: &[&'a LinearProject],
  f: &ProjectFilters,
) -> Vec<&'a LinearProject> {
  projects
    .iter()
    .copied()
    .filter(|p| {
      f.state.as_ref().is_none_or(|s| &p.state == s)
        && f
          .lead
          .as_ref()
          .is_none_or(|id| p.lead.as_ref().is_some_and(|l| &l.id == id))
    })
    .collect()
}

/// `Math.round(progress * 100)`.
pub fn percent(progress: f64) -> i64 {
  (progress * 100.).round() as i64
}

/// A quoted passage to highlight, in a `<mark>` of `color` (`#rrggbbaa`).
pub struct Anchor<'a> {
  pub quote: Option<&'a str>,
  pub color: &'static str,
}

/// `highlightQuotedText`: wraps each anchor's first match in a `<mark>`,
/// so the passage lines up with its card in the comments column.
#[allow(
  clippy::string_slice,
  reason = "offsets come from find() of the quote and its own length"
)]
pub fn highlight_quoted(content: &str, anchors: &[Anchor]) -> String {
  let mut result = content.to_string();
  for anchor in anchors {
    let Some(quote) = anchor.quote.map(str::trim).filter(|q| !q.is_empty()) else {
      continue;
    };
    if let Some(index) = result.find(quote) {
      let after = index + quote.len();
      result = format!(
        "{}<mark color=\"{}\">{quote}</mark>{}",
        &result[..index],
        anchor.color,
        &result[after..]
      );
    }
  }
  result
}

/// Linear's own anchors: `bg-yellow-200/70 dark:bg-yellow-500/30`.
pub fn comment_mark(dark: bool) -> &'static str {
  if dark {
    "#eab3084d"
  } else {
    "#fef08ab3"
  }
}

/// Agent findings on the body: `bg-violet-200/70 dark:bg-violet-500/30`.
pub fn agent_mark(dark: bool) -> &'static str {
  if dark {
    "#8b5cf64d"
  } else {
    "#ddd6feb3"
  }
}

/// What the review agent is shown: the text and its comments.
pub fn review_snapshot(
  title: &str,
  url: &str,
  body: &str,
  comments: &[LinearComment],
) -> LinearReviewSnapshot {
  LinearReviewSnapshot {
    title: title.into(),
    url: url.into(),
    body: body.into(),
    comments: comments
      .iter()
      .map(|c| LinearReviewSnapshotComment {
        id: c.id.clone(),
        author: c.user.as_ref().map(|u| u.name.clone()),
        created_at: Some(c.created_at.clone()),
        quoted_text: c.quoted_text.clone(),
        body: c.body.clone(),
      })
      .collect(),
  }
}

/// The prompt a Linear review starts from.
const LINEAR_REVIEW_PROMPT: &str = include_str!("linear_review_prompt.txt");

/// `prepareLinearReview`'s prompt (`buildLinearReviewPrompt`).
pub fn review_prompt(of: &Commented, title: &str, comments: usize, snapshot_dir: &str) -> String {
  let summary = format!(
    "the Linear {} \"{title}\" and its {comments} comment{}",
    of.kind_label(),
    if comments == 1 { "" } else { "s" }
  );
  LINEAR_REVIEW_PROMPT
    .replace("{target_type}", of.target_type())
    .replace("{target_id}", of.id())
    .replace("{workspace_id}", of.id())
    .replace("{diff_summary}", &summary)
    .replace("{snapshot_dir}", snapshot_dir)
}

/// Snapshot file of one Linear comment (`linearReviewCommentFile`).
pub fn comment_file(comment_id: &str) -> String {
  format!("comments/{comment_id}.md")
}

/// `n agent review comment(s)`.
pub fn agent_count_label(n: usize) -> String {
  format!("{n} agent review comment{}", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
  use super::*;

  pub(crate) fn issue(id: &str, state: (&str, &str)) -> LinearIssue {
    serde_json::from_value(serde_json::json!({
      "id": id,
      "identifier": format!("ENG-{id}"),
      "title": format!("Issue {id}"),
      "description": null,
      "state": {"name": state.0, "type": state.1},
      "labels": [],
      "branch_name": format!("eng-{id}"),
      "parent_id": null,
      "sub_issue_ids": [],
      "url": format!("https://linear.app/x/ENG-{id}"),
      "assignee": null,
      "priority": 0,
      "priority_label": "No priority",
      "project": null,
    }))
    .expect("issue")
  }

  fn project(id: &str, state: &str, lead: Option<(&str, &str)>) -> LinearProject {
    serde_json::from_value(serde_json::json!({
      "id": id,
      "name": format!("Project {id}"),
      "description": null,
      "state": state,
      "target_date": null,
      "progress": 0.5,
      "url": "https://linear.app/p",
      "lead": lead.map(|(id, name)| serde_json::json!({"id": id, "name": name})),
    }))
    .expect("project")
  }

  fn user(id: &str, name: &str) -> Option<LinearUser> {
    Some(LinearUser {
      id: id.into(),
      name: name.into(),
    })
  }

  fn ids(issues: &[&LinearIssue]) -> Vec<String> {
    issues.iter().map(|i| i.id.clone()).collect()
  }

  #[test]
  fn standard_views_pick_by_state_and_assignee() {
    let mut mine = issue("1", ("In Progress", "started"));
    mine.assignee = user("me", "Me");
    let issues = [
      mine,
      issue("2", ("Todo", "unstarted")),
      issue("3", ("Backlog", "backlog")),
      issue("4", ("Done", "completed")),
    ];
    assert_eq!(ids(&issue_view(&issues, IssueView::All, None)).len(), 4);
    assert_eq!(
      ids(&issue_view(&issues, IssueView::Active, None)),
      ["1", "2"]
    );
    assert_eq!(ids(&issue_view(&issues, IssueView::Backlog, None)), ["3"]);
    assert_eq!(
      ids(&issue_view(&issues, IssueView::MyIssues, Some("me"))),
      ["1"]
    );
    // Without a viewer, "My Issues" is empty.
    assert!(issue_view(&issues, IssueView::MyIssues, None).is_empty());
  }

  #[test]
  fn filter_options_follow_the_shown_issues() {
    let mut a = issue("1", ("Todo", "unstarted"));
    a.assignee = user("u2", "Bo");
    a.priority = 2;
    a.priority_label = "High".into();
    a.labels = vec!["bug".into(), "api".into()];
    let mut b = issue("2", ("Todo", "unstarted"));
    b.assignee = user("u1", "Ada");
    b.priority = 1;
    b.priority_label = String::new();
    b.labels = vec!["bug".into()];
    b.project = Some(crate::linear::LinearProjectRef {
      id: "p1".into(),
      name: "Search".into(),
    });
    let issues = [&a, &b];
    let options = issue_filter_options(&issues);
    let labels = |o: &[FilterOption]| o.iter().map(|o| o.label.clone()).collect::<Vec<_>>();
    assert_eq!(labels(&options.assignees), ["Bo", "Ada"]);
    // Sorted by value; a priority without a label shows its number.
    assert_eq!(labels(&options.priorities), ["1", "High"]);
    assert_eq!(labels(&options.labels), ["api", "bug"]);
    assert_eq!(labels(&options.projects), ["Search"]);

    let only = |f: IssueFilters| ids(&filter_issues(&issues, &f));
    assert_eq!(
      only(IssueFilters {
        assignee: Some("u1".into()),
        ..Default::default()
      }),
      ["2"]
    );
    assert_eq!(
      only(IssueFilters {
        priority: Some(2),
        ..Default::default()
      }),
      ["1"]
    );
    assert_eq!(
      only(IssueFilters {
        label: Some("api".into()),
        ..Default::default()
      }),
      ["1"]
    );
    assert_eq!(
      only(IssueFilters {
        project: Some("p1".into()),
        ..Default::default()
      }),
      ["2"]
    );
    assert_eq!(only(IssueFilters::default()), ["1", "2"]);
  }

  #[test]
  fn sub_issues_nest_only_under_a_shown_parent() {
    let parent = issue("1", ("Todo", "unstarted"));
    let mut child = issue("2", ("Todo", "unstarted"));
    child.parent_id = Some("1".into());
    let mut orphan = issue("3", ("Todo", "unstarted"));
    orphan.parent_id = Some("9".into());
    let (roots, subs) = split_roots(&[&parent, &child, &orphan]);
    assert_eq!(ids(&roots), ["1", "3"]);
    assert_eq!(ids(&subs["1"]), ["2"]);
    // Filtered out, the parent leaves its child a root.
    let (roots, subs) = split_roots(&[&child]);
    assert_eq!(ids(&roots), ["2"]);
    assert!(subs.is_empty());
  }

  #[test]
  fn kanban_columns_follow_the_workflow() {
    let issues = [
      issue("1", ("Done", "completed")),
      issue("2", ("Todo", "unstarted")),
      issue("3", ("Weird", "custom")),
      issue("4", ("Backlog", "backlog")),
      issue("5", ("Ready", "unstarted")),
      issue("6", ("Todo", "unstarted")),
    ];
    let refs: Vec<&LinearIssue> = issues.iter().collect();
    let columns = group_by_state(&refs);
    let names: Vec<_> = columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Backlog", "Ready", "Todo", "Done", "Weird"]);
    assert_eq!(ids(&columns[2].issues), ["2", "6"]);
  }

  #[test]
  fn project_views_and_filters() {
    let projects = [
      project("1", "started", Some(("me", "Me"))),
      project("2", "planned", Some(("u2", "Bo"))),
      project("3", "completed", None),
      project("4", "backlog", None),
    ];
    let pick = |v, viewer| {
      project_view(&projects, v, viewer)
        .iter()
        .map(|p| p.id.clone())
        .collect::<Vec<_>>()
    };
    assert_eq!(pick(ProjectView::Active, None), ["1"]);
    assert_eq!(pick(ProjectView::Backlog, None), ["2", "4"]);
    assert_eq!(pick(ProjectView::Mine, Some("me")), ["1"]);
    assert!(pick(ProjectView::Mine, None).is_empty());

    let all = project_view(&projects, ProjectView::All, None);
    let (states, leads) = project_filter_options(&all);
    assert_eq!(
      states.iter().map(|s| s.value.as_str()).collect::<Vec<_>>(),
      ["completed", "planned", "started"]
    );
    assert_eq!(
      leads.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(),
      ["Me", "Bo"]
    );
    let shown = filter_projects(
      &all,
      &ProjectFilters {
        lead: Some("u2".into()),
        ..Default::default()
      },
    );
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].id, "2");
    assert_eq!(percent(0.505), 51);
    assert_eq!(percent(0.), 0);
  }

  #[test]
  fn highlights_each_quote_once() {
    let text = "Ship the API. Then ship the API docs.";
    let out = highlight_quoted(
      text,
      &[
        Anchor {
          quote: Some(" the API "),
          color: "#fef08ab3",
        },
        Anchor {
          quote: Some(""),
          color: "#000",
        },
        Anchor {
          quote: Some("missing"),
          color: "#000",
        },
        Anchor {
          quote: None,
          color: "#000",
        },
      ],
    );
    assert_eq!(
      out,
      "Ship <mark color=\"#fef08ab3\">the API</mark>. Then ship the API docs."
    );
  }

  #[test]
  fn review_prompt_names_the_target_and_snapshot() {
    let of = Commented::Project("p1".into());
    let prompt = review_prompt(&of, "Search", 1, "/repo/.treq/linear-review/x");
    assert!(prompt.starts_with(
      "You are reviewing Linear content: the Linear project \"Search\" and its 1 comment.\n"
    ));
    assert!(prompt.contains("treq copied the text under review into /repo/.treq/linear-review/x:"));
    assert!(prompt.contains("treq agent-review list --target-type linear_project --target-id p1"));
    assert!(!prompt.contains('{'), "no placeholder is left: {prompt}");
    let prompt = review_prompt(&Commented::Issue("i".into()), "ENG-1 Fix", 2, "/d");
    assert!(prompt.contains("the Linear issue \"ENG-1 Fix\" and its 2 comments."));
  }

  #[test]
  fn snapshots_carry_each_comment() {
    let comments = [LinearComment {
      id: "c1".into(),
      body: "Why?".into(),
      user: user("u", "Ada"),
      created_at: "2026-10-19T00:00:00Z".into(),
      quoted_text: Some("the API".into()),
    }];
    let snap = review_snapshot("T", "https://l", "Body", &comments);
    assert_eq!(snap.comments[0].author.as_deref(), Some("Ada"));
    assert_eq!(snap.comments[0].quoted_text.as_deref(), Some("the API"));
    assert_eq!(comment_file("c1"), "comments/c1.md");
    assert_eq!(agent_count_label(1), "1 agent review comment");
    assert_eq!(agent_count_label(3), "3 agent review comments");
  }
}
