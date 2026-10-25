//! The GitHub view (`GitHubPanel`) drawn by GPUI.

use std::collections::HashMap;

use gpui::TestAppContext;

use super::{workspace_dialogs::workspace, *};
use crate::{
  core::{JobInfo, RunJobSummary, RunSummary, SetupScriptStatus, WorkflowInfo},
  github::{
    GhAuthor, GhIssue, GhIssueComment, GhLabel, GhListPage, GhPullRequest, GhReactionGroup,
    GitRemoteInfo, PrCheckEntry, PrCiStatus, PrInfo,
  },
  gpui_shell::screen::{
    github::GithubView,
    github_data::{GithubHost, IssueChange, LogLine, PrChange, TreqChecks},
  },
};

/// What the fake backend's GitHub reads return.
#[derive(Default)]
pub(super) struct Fake {
  /// `None`: the repository has no GitHub remote.
  pub(super) remote: Option<GitRemoteInfo>,
  /// Pages by number, from 1.
  issue_pages: Vec<GhListPage<GhIssue>>,
  pr_pages: Vec<GhListPage<GhPullRequest>>,
  issue: Option<GhIssue>,
  pub(super) pr: Option<GhPullRequest>,
  checks: Option<PrCiStatus>,
  pub(super) workspaces: Vec<crate::local_db::Workspace>,
  statuses: HashMap<String, Option<PrInfo>>,
  default_branch: Option<String>,
  treq: TreqChecks,
  history: Vec<RunSummary>,
  logs_enabled: bool,
  /// Calls that fail, by name.
  pub(super) fail: Vec<&'static str>,
}

fn author(login: &str) -> GhAuthor {
  GhAuthor {
    login: login.into(),
    avatar_url: None,
  }
}

fn issue(number: u64, title: &str) -> GhIssue {
  GhIssue {
    number,
    title: title.into(),
    state: "OPEN".into(),
    url: format!("https://github.com/acme/treq/issues/{number}"),
    body: Some("Steps to reproduce".into()),
    author: author("ada"),
    labels: vec![GhLabel {
      name: "bug".into(),
      color: "d73a4a".into(),
    }],
    created_at: "2026-10-01T10:00:00Z".into(),
    updated_at: "2026-10-01T10:00:00Z".into(),
    comments: Some(vec![GhIssueComment {
      id: "c1".into(),
      body: "Seen it too".into(),
      author: author("bob"),
      created_at: "2026-10-02T10:00:00Z".into(),
      reactions: Vec::new(),
    }]),
  }
}

pub(super) fn pr(number: u64, head: &str, base: &str) -> GhPullRequest {
  GhPullRequest {
    number,
    title: format!("Change {number}"),
    state: "OPEN".into(),
    url: format!("https://github.com/acme/treq/pull/{number}"),
    body: Some("Does a thing".into()),
    author: author("ada"),
    labels: Vec::new(),
    head_ref_name: head.into(),
    base_ref_name: base.into(),
    merge_state_status: None,
    created_at: "2026-10-18T10:00:00Z".into(),
    updated_at: "2026-10-18T10:00:00Z".into(),
    comments: None,
    is_draft: false,
  }
}

fn page<T>(items: Vec<T>, more: bool, cursor: Option<&str>) -> GhListPage<T> {
  GhListPage {
    items,
    has_more: more,
    end_cursor: cursor.map(str::to_string),
  }
}

pub(super) fn acme() -> Option<GitRemoteInfo> {
  Some(GitRemoteInfo {
    owner: "acme".into(),
    repo: "treq".into(),
    full_name: "acme/treq".into(),
  })
}

impl FakeHost {
  fn gh_fails(&self, call: &str) -> Result<(), String> {
    if self.github.fail.contains(&call) {
      Err(format!("gh exited with error: {call} failed"))
    } else {
      Ok(())
    }
  }
}

impl GithubHost for FakeHost {
  fn remote(&self, _: &str) -> Result<Option<GitRemoteInfo>, String> {
    Ok(self.github.remote.clone())
  }

  fn list_issues(
    &self,
    repo: &str,
    state: &str,
    after: Option<&str>,
  ) -> Result<GhListPage<GhIssue>, String> {
    self.record(format!("read issues {repo} {state} {after:?}"));
    self.gh_fails("list_issues")?;
    let index = match after {
      None => 0,
      Some(cursor) => self
        .github
        .issue_pages
        .iter()
        .position(|p| p.end_cursor.as_deref() == Some(cursor))
        .map_or(usize::MAX, |i| i + 1),
    };
    Ok(
      self
        .github
        .issue_pages
        .get(index)
        .cloned()
        .unwrap_or_else(|| page(Vec::new(), false, None)),
    )
  }

  fn list_prs(&self, repo: &str, state: &str, n: u32) -> Result<GhListPage<GhPullRequest>, String> {
    self.record(format!("read prs {repo} {state} {n}"));
    self.gh_fails("list_prs")?;
    Ok(
      self
        .github
        .pr_pages
        .get(n as usize - 1)
        .cloned()
        .unwrap_or_else(|| page(Vec::new(), false, None)),
    )
  }

  fn issue(&self, _: &str, number: u64) -> Result<GhIssue, String> {
    self.record(format!("read issue {number}"));
    self.gh_fails("issue")?;
    self.github.issue.clone().ok_or_else(|| "not found".into())
  }

  fn pr(&self, _: &str, number: u64) -> Result<GhPullRequest, String> {
    self.record(format!("read pr {number}"));
    self.github.pr.clone().ok_or_else(|| "not found".into())
  }

  fn pr_checks(&self, _: &str, number: u64) -> Result<Option<PrCiStatus>, String> {
    self.record(format!("read checks {number}"));
    Ok(self.github.checks.clone())
  }

  fn change_issue(&self, repo: &str, number: u64, change: &IssueChange) -> Result<(), String> {
    self.record(format!("issue {repo}#{number} {change:?}"));
    self.gh_fails("change_issue")
  }

  fn change_pr(&self, repo: &str, number: u64, change: &PrChange) -> Result<(), String> {
    self.record(format!("pr {repo}#{number} {change:?}"));
    self.gh_fails("change_pr")
  }

  fn react(&self, comment_id: &str, content: &str, on: bool) -> Result<(), String> {
    self.record(format!("react {comment_id} {content} {on}"));
    self.gh_fails("react")
  }

  fn create_issue(&self, repo: &str, title: &str, body: &str) -> Result<u64, String> {
    self.record(format!("create issue {repo} {title:?} {body:?}"));
    self.gh_fails("create_issue")?;
    Ok(77)
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
    self.record(format!(
      "create pr {repo} {title:?} {body:?} {head}->{base} draft={draft}"
    ));
    self.gh_fails("create_pr")?;
    Ok(88)
  }

  fn workspaces(&self, _: &str) -> Result<Vec<crate::local_db::Workspace>, String> {
    self.record("read workspaces".into());
    Ok(self.github.workspaces.clone())
  }

  fn pr_statuses(&self, _: &str) -> HashMap<String, Option<PrInfo>> {
    self.github.statuses.clone()
  }

  fn default_branch(&self, _: &str) -> Result<String, String> {
    self
      .github
      .default_branch
      .clone()
      .ok_or_else(|| "no default branch".into())
  }

  fn push_workspace(&self, _: &str, workspace_id: i64) -> Result<(), String> {
    self.record(format!("push {workspace_id}"));
    self.gh_fails("push")
  }

  fn open_pr_workspace(&self, _: &str, pr: &GhPullRequest) -> Result<(i64, bool), String> {
    self.record(format!("open workspace {}", pr.head_ref_name));
    self.gh_fails("open_pr_workspace")?;
    Ok((9, true))
  }

  fn refresh_pr_status(&self, _: &str, branch: &str) {
    self.record(format!("refresh pr status {branch}"));
  }

  fn logs_enabled(&self) -> bool {
    self.github.logs_enabled
  }

  fn treq_checks(&self, _: &str, workspace_id: i64) -> TreqChecks {
    self.record(format!("read treq checks {workspace_id}"));
    self.github.treq.clone()
  }

  fn run_history(&self, _: &str, _: i64, filename: &str) -> Result<Vec<RunSummary>, String> {
    self.record(format!("read history {filename}"));
    Ok(self.github.history.clone())
  }

  fn run_logs(&self, _: &str, run_id: i64, job_id: &str) -> Result<Vec<LogLine>, String> {
    self.record(format!("read logs {run_id} {job_id}"));
    Ok(vec![
      LogLine {
        stream: "stdout".into(),
        body: "ok".into(),
      },
      LogLine {
        stream: "stderr".into(),
        body: "warn".into(),
      },
    ])
  }

  fn trust_repo(&self, repo_path: &str) -> Result<(), String> {
    self.record(format!("trust {repo_path}"));
    Ok(())
  }

  fn acts_locally(&self, _: &str) -> bool {
    !self.react_actions
  }

  fn run_checks(
    &self,
    target: &crate::core::running_checks::ChecksTarget,
    run: &crate::core::running_checks::CheckRun,
  ) -> Result<Vec<crate::core::checks::JobResult>, String> {
    self.record(format!(
      "run {} in {}",
      run.key(target.workspace_id),
      target.workspace_path
    ));
    if self.failing.contains(&"run_checks") {
      return Err("no runner".into());
    }
    Ok(vec![crate::core::checks::JobResult {
      job_id: "test".into(),
      steps: vec![],
      success: !self.failing.contains(&"checks_fail"),
    }])
  }
}

fn route(tab: &str, selected: Option<u64>, creating: bool) -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "github",
    "dark": false,
    "repoPath": "/repo",
    "tab": tab,
    "filter": "open",
    "selected": selected,
    "creating": creating,
    "runningChecks": [],
  }))
  .expect("route")
}

fn harness(
  cx: &mut TestAppContext,
  github: Fake,
  snapshot: ScreenSnapshot,
) -> (Harness, Arc<FakeHost>) {
  harness_with(
    cx,
    FakeHost {
      github,
      ..FakeHost::default()
    },
    snapshot,
  )
}

fn harness_with(
  cx: &mut TestAppContext,
  host: FakeHost,
  snapshot: ScreenSnapshot,
) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(host);
  let h = Harness::with_host(cx, snapshot, host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  h.cx.run_until_parked();
  (h, host)
}

fn view<T>(h: &mut Harness, f: impl FnOnce(&GithubView) -> T) -> T {
  h.screen.read_with(&h.cx, |screen, cx| {
    f(screen.github.as_ref().expect("github view").read(cx))
  })
}

fn view_mut<T>(h: &mut Harness, f: impl FnOnce(&mut GithubView) -> T) -> T {
  h.screen.update(&mut h.cx, |screen, cx| {
    let view = screen.github.clone().expect("github view");
    view.update(cx, |view, _| f(view))
  })
}

/// Types `value` into `field`, as its control reports it.
fn type_into(h: &mut Harness, field: &str, value: &str) {
  h.screen.update(&mut h.cx, |screen, cx| {
    let view = screen.github.clone().expect("github view");
    view.update(cx, |view, cx| {
      view.handle(
        ScreenAction::Field {
          field: field.into(),
          value: value.into(),
        },
        cx,
      )
    });
  });
  h.cx.run_until_parked();
}

fn issues() -> Fake {
  Fake {
    remote: acme(),
    issue_pages: vec![
      page(
        vec![issue(1, "Crash on start"), issue(2, "Slow sync")],
        true,
        Some("c1"),
      ),
      page(
        vec![issue(2, "Slow sync"), issue(3, "Typo")],
        false,
        Some("c2"),
      ),
    ],
    issue: Some(issue(1, "Crash on start")),
    ..Fake::default()
  }
}

#[gpui::test]
fn github_lists_issues_and_hands_moves_to_react(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, issues(), route("issues", None, false));
  assert!(h.drawn("github-issue-1"));
  assert!(h.drawn("github-issue-2"));
  assert!(!h.drawn("github-detail"));
  // The two reads run side by side, in no set order.
  let mut calls = host.calls();
  calls.sort();
  assert_eq!(
    calls,
    ["read issues acme/treq open None", "read workspaces"]
  );

  h.click("github-issue-2");
  h.click("github-filter-closed");
  h.click("github-tab-prs");
  h.click("github-new");
  assert_eq!(
    h.sent(),
    [
      press("github.select:2"),
      press("github.filter:closed"),
      press("github.tab:prs"),
      press("github.new"),
    ]
  );
}

#[gpui::test]
fn github_loads_more_pages_once_each(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, issues(), route("issues", None, false));
  h.click("github-load-more");
  h.cx.run_until_parked();
  assert!(h.drawn("github-issue-3"));
  assert!(!h.drawn("github-load-more"));
  // The repeated #2 shows once.
  assert_eq!(view_mut(&mut h, |v| v.issue_items().len()), 3);
  assert!(host
    .calls()
    .contains(&"read issues acme/treq open Some(\"c1\")".to_string()));
  // A refresh reads both pages again.
  h.click("github-refresh");
  h.cx.run_until_parked();
  let reads = host
    .calls()
    .iter()
    .filter(|c| c.starts_with("read issues"))
    .count();
  assert_eq!(reads, 4);
}

#[gpui::test]
fn github_reads_a_new_filter_from_the_route(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, issues(), route("issues", None, false));
  h.update(|s| {
    if let View::Github(route) = &mut s.view {
      route.filter = crate::gpui_shell::screen::github_data::StateFilter::Closed;
    }
  });
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"read issues acme/treq closed None".to_string()));
}

#[gpui::test]
fn github_shows_list_failures_and_retries(cx: &mut TestAppContext) {
  let (mut h, host) = harness(
    cx,
    Fake {
      fail: vec!["list_issues"],
      ..issues()
    },
    route("issues", None, false),
  );
  assert!(h.drawn("github-list-error"));
  h.click("github-list-retry");
  h.cx.run_until_parked();
  let reads = host
    .calls()
    .iter()
    .filter(|c| c.starts_with("read issues"))
    .count();
  assert_eq!(reads, 2);
}

#[gpui::test]
fn github_tabs_fit_their_triggers(cx: &mut TestAppContext) {
  // `TabsList` is `inline-flex`: as wide as its tabs, not the list.
  let (mut h, _) = harness(cx, issues(), route("issues", None, false));
  h.cx.run_until_parked();
  let tabs = h.cx.debug_bounds("github-tabs").expect("tabs");
  let list = h.cx.debug_bounds("github-list").expect("list");
  assert!(tabs.size.width < list.size.width / 2.);
}

#[gpui::test]
fn github_without_a_remote_says_so(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, Fake::default(), route("issues", Some(4), false));
  assert!(!h.drawn("github-new"));
  assert!(!h.drawn("github-detail"));
  assert_eq!(host.calls(), ["read workspaces"]);
}

fn prs() -> Fake {
  let mut fixed = pr(2, "fix/login", "main");
  fixed.author = author("bob");
  fixed.is_draft = true;
  Fake {
    remote: acme(),
    pr_pages: vec![page(vec![pr(1, "feat/a", "main"), fixed], false, None)],
    pr: Some(pr(1, "feat/a", "main")),
    ..Fake::default()
  }
}

#[gpui::test]
fn github_filters_the_loaded_prs(cx: &mut TestAppContext) {
  let (mut h, _) = harness(cx, prs(), route("prs", None, false));
  assert!(h.drawn("github-pr-1") && h.drawn("github-pr-2"));
  type_into(&mut h, "github.search", "login");
  assert!(!h.drawn("github-pr-1") && h.drawn("github-pr-2"));
  type_into(&mut h, "github.search", "");
  h.click("github-filters");
  assert!(h.drawn("github-filters-popover"));
  type_into(&mut h, "github.filter.author", "ada");
  assert!(h.drawn("github-pr-1") && !h.drawn("github-pr-2"));
  assert_eq!(view(&mut h, |v| v.filters.active_count()), 1);
  type_into(&mut h, "github.filter.author", "nobody");
  assert!(!h.drawn("github-pr-1"));
  h.click("github-filters-clear");
  assert!(h.drawn("github-pr-1") && h.drawn("github-pr-2"));
}

#[gpui::test]
fn github_issue_detail_comments_closes_and_reopens(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, issues(), route("issues", Some(1), false));
  assert!(h.drawn("github-detail"));
  assert!(h.drawn("github-edit"));
  type_into(&mut h, "github.comment", "On it");
  h.click("github-comment");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"issue acme/treq#1 Comment(\"On it\")".to_string()));
  assert_eq!(view(&mut h, |v| v.comment.clone()), "");

  h.click("github-close-options");
  assert!(h.drawn("github-close-menu"));
  h.click("github-close-not-planned");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"issue acme/treq#1 Close(\"not planned\")".to_string()));
  h.click("github-close-issue");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"issue acme/treq#1 Close(\"completed\")".to_string()));

  h.click("github-agent");
  h.click("github-detail-close");
  assert_eq!(
    h.sent(),
    [
      ScreenAction::Field {
        field: "github.startPrompt".into(),
        value: serde_json::json!({
          "number": 1,
          "url": "https://github.com/acme/treq/issues/1",
          "title": "Crash on start",
        }),
      },
      press("github.close"),
    ]
  );
}

#[gpui::test]
fn github_issue_failures_toast(cx: &mut TestAppContext) {
  let (mut h, host) = harness(
    cx,
    Fake {
      fail: vec!["change_issue"],
      ..issues()
    },
    route("issues", Some(1), false),
  );
  h.click("github-close-issue");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"notify toast Failed to close issue".to_string()));
}

fn heart(count: u32, mine: bool) -> GhReactionGroup {
  GhReactionGroup {
    content: "HEART".into(),
    count,
    viewer_has_reacted: mine,
  }
}

fn reacted_issue() -> Fake {
  let mut fake = issues();
  let issue = fake.issue.as_mut().unwrap();
  issue.comments.as_mut().unwrap()[0].reactions = vec![heart(2, false)];
  fake
}

fn reactions_of(h: &mut Harness, comment: usize) -> Vec<GhReactionGroup> {
  view(h, |v| {
    v.detail.issue.as_ref().unwrap().comments.as_ref().unwrap()[comment]
      .reactions
      .clone()
  })
}

#[gpui::test]
fn github_comment_reactions_toggle_and_pick(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, reacted_issue(), route("issues", Some(1), false));
  assert!(h.drawn("reaction-c1-HEART"));

  // A chip joins the reaction, then leaves it.
  h.click("reaction-c1-HEART");
  h.cx.run_until_parked();
  assert_eq!(reactions_of(&mut h, 0), [heart(3, true)]);
  h.click("reaction-c1-HEART");
  h.cx.run_until_parked();
  assert_eq!(reactions_of(&mut h, 0), [heart(2, false)]);

  // The picker adds a new one and closes.
  h.click("reaction-add-c1");
  assert!(h.drawn("reaction-picker"));
  h.click("reaction-pick-c1-THUMBS_UP");
  h.cx.run_until_parked();
  assert!(!h.drawn("reaction-picker"));
  assert!(h.drawn("reaction-c1-THUMBS_UP"));
  assert_eq!(
    host
      .writes()
      .into_iter()
      .filter(|w| w.starts_with("react"))
      .collect::<Vec<_>>(),
    [
      "react c1 HEART true",
      "react c1 HEART false",
      "react c1 THUMBS_UP true",
    ]
  );
}

#[gpui::test]
fn github_comment_reaction_failure_is_undone(cx: &mut TestAppContext) {
  let (mut h, host) = harness(
    cx,
    Fake {
      fail: vec!["react"],
      ..reacted_issue()
    },
    route("issues", Some(1), false),
  );
  h.click("reaction-c1-HEART");
  h.cx.run_until_parked();
  assert_eq!(reactions_of(&mut h, 0), [heart(2, false)]);
  assert!(host
    .writes()
    .contains(&"notify toast Failed to react".to_string()));
}

#[gpui::test]
fn github_edits_and_deletes_an_issue(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, issues(), route("issues", Some(1), false));
  h.click("github-edit");
  assert!(h.drawn("github-edit-save"));
  type_into(&mut h, "github.edit.title", "Crash on launch");
  h.click("github-edit-save");
  h.cx.run_until_parked();
  assert!(host.writes().contains(
    &"issue acme/treq#1 Edit { title: \"Crash on launch\", body: \"Steps to reproduce\" }"
      .to_string()
  ));
  assert!(!h.drawn("github-edit-save"));

  h.click("github-delete");
  assert!(h.drawn("github-delete-dialog"));
  h.click("github-delete-cancel");
  assert!(!h.drawn("github-delete-dialog"));
  h.click("github-delete");
  h.click("github-delete-confirm");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"issue acme/treq#1 Delete".to_string()));
  assert!(!h.drawn("github-delete-dialog"));
  assert_eq!(h.sent(), [press("github.close")]);
}

#[gpui::test]
fn github_creates_an_issue(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, issues(), route("issues", None, true));
  assert!(h.drawn("github-new-issue"));
  type_into(&mut h, "github.newIssue.title", "New bug");
  h.click("github-new-issue-submit");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"create issue acme/treq \"New bug\" \"\"".to_string()));
  h.click("github-new-issue-cancel");
  assert_eq!(
    h.sent(),
    [press("github.created:77"), press("github.close")]
  );
}

fn checks() -> PrCiStatus {
  let check = |name: &str, bucket: &str| PrCheckEntry {
    name: name.into(),
    bucket: bucket.into(),
    link: format!("https://ci/{name}"),
    duration_secs: Some(65),
  };
  PrCiStatus {
    state: "failure".into(),
    total: 2,
    passed: 1,
    failed: 1,
    pending: 0,
    checks: vec![check("lint", "pass"), check("test", "fail")],
  }
}

#[gpui::test]
fn github_pr_detail_changes_the_pr(cx: &mut TestAppContext) {
  let (mut h, host) = harness(
    cx,
    Fake {
      checks: Some(checks()),
      ..prs()
    },
    route("prs", Some(1), false),
  );
  assert!(h.drawn("github-ci"));
  // Failed checks first.
  assert!(h.drawn("github-check-0"));
  h.click("github-check-0");
  assert!(host.calls().contains(&"open https://ci/test".to_string()));
  h.click("github-ci");
  assert!(h.drawn("github-ci-popover"));

  h.click("github-draft");
  h.cx.run_until_parked();
  h.click("github-close-pr");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(writes.contains(&"pr acme/treq#1 Draft(true)".to_string()));
  assert!(writes.contains(&"pr acme/treq#1 Close".to_string()));
  assert!(writes.contains(&"refresh pr status feat/a".to_string()));

  h.click("github-review");
  assert_eq!(h.sent(), [press("github.review:1")]);
}

#[gpui::test]
fn github_pr_detail_opens_its_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, prs(), route("prs", Some(1), false));
  h.click("github-workspace");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(writes.contains(&"open workspace feat/a".to_string()));
  assert!(writes.contains(&"notify toast Workspace created".to_string()));
  assert_eq!(h.sent(), [press("github.openWorkspace:9")]);
}

fn treq() -> TreqChecks {
  TreqChecks {
    trusted: true,
    workflows: vec![WorkflowInfo {
      filename: "ci.yml".into(),
      name: "CI".into(),
      jobs: vec![JobInfo {
        id: "test".into(),
        name: "Test".into(),
        steps: Vec::new(),
      }],
    }],
    latest: HashMap::from([(
      "ci.yml".to_string(),
      RunSummary {
        id: 5,
        filename: "ci.yml".into(),
        status: "passed".into(),
        started_at: "2026-10-19T10:00:00Z".into(),
        completed_at: None,
        jobs: vec![RunJobSummary {
          job_id: "test".into(),
          status: "passed".into(),
          steps: Vec::new(),
          has_logs: true,
        }],
      },
    )]),
    setup: Some(SetupScriptStatus {
      configured: true,
      status: "passed".into(),
      run_id: None,
    }),
  }
}

#[gpui::test]
fn github_pr_detail_runs_its_workspace_checks(cx: &mut TestAppContext) {
  let fake = Fake {
    workspaces: vec![workspace(3, "feat/a", Some("main"))],
    treq: treq(),
    ..prs()
  };
  let (mut h, host) = harness(cx, fake, route("prs", Some(1), false));
  let reads = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("read treq checks"))
      .count()
  };
  let before = reads(&host);
  h.click("github-run-0");
  h.click("github-run-0-0");
  h.click("github-rerun-setup");
  h.cx.run_until_parked();
  let runs: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("run "))
    .collect();
  assert_eq!(
    runs,
    [
      "run 3:ci.yml in /repo/.treq/workspaces/feat/a",
      "run 3:ci.yml:test in /repo/.treq/workspaces/feat/a",
      "run 3:<setup> in /repo/.treq/workspaces/feat/a",
    ]
  );
  assert!(h.sent().is_empty(), "React runs none of them");
  assert_eq!(reads(&host), before + 3, "each end reads the checks again");
  assert!(host
    .calls()
    .contains(&"notify workspaceChanged".to_string()));
}

#[gpui::test]
fn github_pr_detail_shows_a_run_started_elsewhere_as_running(cx: &mut TestAppContext) {
  let fake = Fake {
    workspaces: vec![workspace(3, "feat/a", Some("main"))],
    treq: treq(),
    ..prs()
  };
  let (mut h, host) = harness(cx, fake, route("prs", Some(1), false));
  h.cx.update(|_, cx| {
    let mut running = crate::gpui_shell::checks_run::Running::default();
    running.0.start("/repo", "3:ci.yml");
    cx.set_global(running);
  });
  h.cx.run_until_parked();
  let keys = h.screen.read_with(&h.cx, |screen, cx| {
    screen
      .github
      .as_ref()
      .expect("github view")
      .read(cx)
      .running_keys(cx)
  });
  assert_eq!(keys, ["3:ci.yml"]);
  // Its button is disabled while it runs.
  h.click("github-run-0");
  h.cx.run_until_parked();
  assert!(!host.calls().iter().any(|c| c.starts_with("run ")));
}

#[gpui::test]
fn github_pr_detail_hands_remote_checks_to_react(cx: &mut TestAppContext) {
  let fake = Fake {
    workspaces: vec![workspace(3, "feat/a", Some("main"))],
    treq: treq(),
    history: treq().latest.into_values().collect(),
    logs_enabled: true,
    ..prs()
  };
  let host = FakeHost {
    github: fake,
    react_actions: true,
    ..FakeHost::default()
  };
  let (mut h, host) = harness_with(cx, host, route("prs", Some(1), false));
  assert!(h.drawn("github-treq-checks"));
  assert!(h.drawn("setup-script-status"));
  h.click("github-run-0");
  h.click("github-run-0-0");
  h.click("github-rerun-setup");
  h.click("github-open-logs");
  let target = serde_json::json!({
    "repoPath": "/repo",
    "workspaceId": 3,
    "workspacePath": "/repo/.treq/workspaces/feat/a",
  });
  let run = |job: Option<&str>| {
    let mut value = target.clone();
    value["filename"] = "ci.yml".into();
    value["jobId"] = job.into();
    ScreenAction::Field {
      field: "github.checks.run".into(),
      value,
    }
  };
  assert_eq!(
    h.sent(),
    [
      run(None),
      run(Some("test")),
      ScreenAction::Field {
        field: "github.checks.setup".into(),
        value: target,
      },
      press("github.openLogs:3"),
    ]
  );

  // A run React started shows as running, and its end reads the checks again.
  h.update(|s| {
    if let View::Github(route) = &mut s.view {
      route.running_checks = vec!["3:ci.yml".into()];
    }
  });
  h.cx.run_until_parked();
  let reads = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("read treq checks"))
      .count()
  };
  assert_eq!(reads(&host), 2);

  h.click("github-history-0");
  h.cx.run_until_parked();
  h.click("run-history-item-5");
  h.cx.run_until_parked();
  assert!(h.drawn("run-job-logs-5-test"));
  assert!(host.calls().contains(&"read logs 5 test".to_string()));
}

#[gpui::test]
fn github_pr_detail_asks_to_trust_the_repository(cx: &mut TestAppContext) {
  let fake = Fake {
    workspaces: vec![workspace(3, "feat/a", Some("main"))],
    treq: TreqChecks {
      trusted: false,
      ..treq()
    },
    ..prs()
  };
  let (mut h, host) = harness(cx, fake, route("prs", Some(1), false));
  h.click("github-trust");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"trust /repo".to_string()));
}

fn new_pr_fake() -> Fake {
  let mut titled = workspace(4, "fix/login-bug", None);
  titled.description = Some("Fixes login".into());
  let open = PrInfo {
    number: 3,
    title: String::new(),
    state: "OPEN".into(),
    url: String::new(),
    head_ref_name: String::new(),
    base_ref_name: String::new(),
    merge_state_status: None,
    is_draft: false,
    body: String::new(),
  };
  Fake {
    workspaces: vec![
      workspace(1, "trunk", None),
      workspace(2, "has-pr", None),
      titled,
    ],
    statuses: HashMap::from([("has-pr".to_string(), Some(open))]),
    default_branch: Some("trunk".into()),
    ..prs()
  }
}

#[gpui::test]
fn github_new_pr_from_a_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, new_pr_fake(), route("prs", None, true));
  assert!(h.drawn("github-new-pr-dialog"));
  // The default branch's workspace and one with an open PR are not offered.
  assert!(!h.drawn("github-new-pr-ws-1"));
  assert!(!h.drawn("github-new-pr-ws-2"));
  h.click("github-new-pr-ws-4");
  assert!(h.drawn("github-new-pr-create"));
  h.click("github-new-pr-type");
  h.click("github-new-pr-type-draft");
  h.click("github-new-pr-create");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(writes.contains(&"push 4".to_string()));
  assert!(writes.contains(
    &"create pr acme/treq \"fix: login bug\" \"Fixes login\" fix/login-bug->trunk draft=true"
      .to_string()
  ));
  assert!(writes.contains(&"refresh pr status fix/login-bug".to_string()));
  assert!(writes.contains(&"notify toast Draft PR created".to_string()));
  assert_eq!(h.sent(), [press("github.created:88")]);
}

#[gpui::test]
fn github_new_pr_from_a_branch(cx: &mut TestAppContext) {
  let (mut h, host) = harness(
    cx,
    Fake {
      fail: vec!["create_pr"],
      ..new_pr_fake()
    },
    route("prs", None, true),
  );
  h.click("github-new-pr-tab-branch");
  type_into(&mut h, "github.newPr.branch.title", "Add docs");
  type_into(&mut h, "github.newPr.branch.head", "docs");
  h.click("github-new-pr-create");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"create pr acme/treq \"Add docs\" \"\" docs->trunk draft=false".to_string()));
  // The failure shows in the form.
  assert!(view(&mut h, |v| v.new_pr.as_ref().and_then(|n| n.error.clone())).is_some());
  h.click("github-new-pr-cancel");
  assert_eq!(h.sent(), [press("github.close")]);
}

#[gpui::test]
fn github_reads_an_issue_ahead_while_the_pointer_rests_on_its_row(cx: &mut TestAppContext) {
  use super::kept_tabs::{drawn_now, first_frame, reads};
  use crate::gpui_shell::screen::prefetch::HOVER_INTENT;
  let (mut h, host) = harness(cx, issues(), route("issues", None, false));
  let at = h.cx.debug_bounds("github-issue-2").expect("row").center();
  h.cx.simulate_mouse_move(at, None, gpui::Modifiers::none());
  h.cx.executor().advance_clock(HOVER_INTENT);
  h.cx.run_until_parked();
  assert_eq!(reads(&host, "read issue 2"), 1);

  // Opened, its detail is in the first frame, and is read again behind it.
  h.update(|s| {
    if let View::Github(route) = &mut s.view {
      route.selected = Some(2);
    }
  });
  first_frame(&mut h);
  assert!(drawn_now(&mut h, "github-detail"));
  assert!(!drawn_now(&mut h, "github-detail-loading-shown"));
  h.cx.run_until_parked();
  assert_eq!(reads(&host, "read issue 2"), 2);
}

fn thread_fake(count: usize) -> Fake {
  let mut p = pr(1, "feat/b0", "main");
  p.comments = Some(
    (0..count)
      .map(|i| GhIssueComment {
        id: format!("c{i}"),
        body: format!("comment {i}"),
        author: author("bob"),
        created_at: "2026-10-02T10:00:00Z".into(),
        reactions: Vec::new(),
      })
      .collect(),
  );
  Fake {
    remote: acme(),
    pr_pages: vec![page(vec![pr(1, "feat/b0", "main")], false, None)],
    pr: Some(p),
    ..Fake::default()
  }
}

#[gpui::test]
fn github_long_comment_thread_folds_the_middle_and_expands_in_place(cx: &mut TestAppContext) {
  let (mut h, _) = harness(cx, thread_fake(8), route("prs", Some(1), false));
  h.cx.run_until_parked();
  let shown = |h: &mut Harness| -> Vec<usize> {
    (0..8)
      .filter(|i| h.drawn(super::changes_large::leak(format!("reaction-add-c{i}"))))
      .collect()
  };
  // The first comment and the newest three; four are behind the fold.
  assert_eq!(shown(&mut h), [0, 5, 6, 7]);
  assert!(h.drawn("github-comments-show-earlier"));
  h.click("github-comments-show-earlier");
  h.cx.run_until_parked();
  assert_eq!(shown(&mut h), (0..8).collect::<Vec<_>>());
  assert!(!h.drawn("github-comments-show-earlier"));
}

#[gpui::test]
fn github_short_comment_thread_is_not_folded(cx: &mut TestAppContext) {
  // Folding one comment would only trade it for a row.
  let (mut h, _) = harness(cx, thread_fake(5), route("prs", Some(1), false));
  h.cx.run_until_parked();
  assert!(h.drawn("reaction-add-c2"));
  assert!(!h.drawn("github-comments-show-earlier"));
}

/// Round 7 `integrations-scale` measurements: the lists and details with
/// hundreds of rows. Run with `cargo test --features gpui-shell --lib
/// gh_scale -- --ignored --nocapture --test-threads=1`.
mod gh_scale {
  use super::super::changes_large::{frame, report, timed};
  use super::*;

  fn n() -> usize {
    std::env::var("PERF_N")
      .ok()
      .and_then(|v| v.parse().ok())
      .unwrap_or(1000)
  }

  fn long_body(i: usize) -> String {
    format!(
      "## Comment {i}\n\nSome **bold** text and `code` with a [link](https://x.test/{i}).\n\n- one\n- two\n- three\n\n```rust\nfn f{i}() {{ println!(\"{i}\"); }}\n```\n\n{}",
      "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12)
    )
  }

  fn comments(count: usize) -> Vec<GhIssueComment> {
    (0..count)
      .map(|i| GhIssueComment {
        id: format!("c{i}"),
        body: long_body(i),
        author: author("bob"),
        created_at: "2026-10-02T10:00:00Z".into(),
        reactions: Vec::new(),
      })
      .collect()
  }

  fn prs_fake(count: usize) -> Fake {
    let prs: Vec<_> = (0..count)
      .map(|i| {
        let mut p = pr(i as u64 + 1, &format!("feat/b{i}"), "main");
        p.author = author(&format!("user{}", i % 17));
        p.title = format!("Change number {i} touching the thing {}", i % 31);
        p
      })
      .collect();
    Fake {
      remote: acme(),
      pr_pages: vec![page(prs, false, None)],
      ..Fake::default()
    }
  }

  fn issues_fake(count: usize) -> Fake {
    let items: Vec<_> = (0..count)
      .map(|i| {
        issue(
          i as u64 + 1,
          &format!("Issue {i} about the thing {}", i % 31),
        )
      })
      .collect();
    Fake {
      remote: acme(),
      issue_pages: vec![page(items, false, None)],
      ..Fake::default()
    }
  }

  fn notify(h: &mut Harness) {
    h.screen.update(&mut h.cx, |screen, cx| {
      let view = screen.github.clone().expect("github view");
      view.update(cx, |_, cx| cx.notify());
    });
  }

  #[gpui::test]
  #[ignore = "benchmark; run with --ignored --nocapture"]
  fn gh_scale_pr_list(cx: &mut TestAppContext) {
    let (mut h, _) = harness(cx, prs_fake(n()), route("prs", None, false));
    let rows = view_mut(&mut h, |v| v.pr_items().len());
    println!("PR rows loaded: {rows}");
    let mut f: Vec<_> = (0..10).map(|_| frame(&mut h)).collect();
    report("pr list steady frame", &mut f);
    let mut keys: Vec<_> = (0..10)
      .map(|i| {
        timed(&mut h, |h| {
          type_into(h, "github.search", &format!("thing {}", i % 7))
        })
      })
      .collect();
    report("pr list search keystroke", &mut keys);
    type_into(&mut h, "github.search", "");
    let mut hov: Vec<_> = (0..10).map(|_| timed(&mut h, notify)).collect();
    report("pr list notify (hover)", &mut hov);
  }

  #[gpui::test]
  #[ignore = "benchmark; run with --ignored --nocapture"]
  fn gh_scale_issue_list(cx: &mut TestAppContext) {
    let (mut h, _) = harness(cx, issues_fake(n()), route("issues", None, false));
    println!(
      "issue rows: {}",
      view_mut(&mut h, |v| v.issue_items().len())
    );
    let mut f: Vec<_> = (0..10).map(|_| frame(&mut h)).collect();
    report("issue list steady frame", &mut f);
  }

  fn detail_fake(count: usize) -> Fake {
    let mut fake = prs_fake(3);
    let mut p = pr(1, "feat/b0", "main");
    p.body = Some(long_body(0));
    p.comments = Some(comments(count));
    fake.pr = Some(p);
    fake
  }

  #[gpui::test]
  #[ignore = "benchmark; run with --ignored --nocapture"]
  fn gh_scale_pr_detail_comments(cx: &mut TestAppContext) {
    for count in [10usize, 60, 300] {
      let (mut h, _) = harness(cx, detail_fake(count), route("prs", Some(1), false));
      h.cx.run_until_parked();
      let mut f: Vec<_> = (0..10).map(|_| frame(&mut h)).collect();
      report(&format!("pr detail {count} comments"), &mut f);
      let mut hov: Vec<_> = (0..10).map(|_| timed(&mut h, notify)).collect();
      report(&format!("pr detail {count} notify"), &mut hov);
    }
  }
}
