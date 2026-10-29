//! The Linear view (`LinearPanel`) drawn from a faked Linear backend.

use std::sync::Arc;

use gpui::{Role, TestAppContext};
use gpui_testing_library::prelude::*;

use super::*;
use crate::{
  gpui_shell::popup::{MenuEntry, PopupRequest},
  linear::{LinearComment, LinearDocument, LinearIssue, LinearProject, LinearTeam, LinearUser},
};

/// What the faked Linear API returns.
#[derive(Default)]
pub(super) struct Fixture {
  pub(super) teams: Vec<LinearTeam>,
  pub(super) issues: Vec<LinearIssue>,
  pub(super) viewer: Option<LinearUser>,
  projects: Vec<LinearProject>,
  documents: Vec<LinearDocument>,
  /// Comments, by the id of what they are on.
  comments: Vec<(String, LinearComment)>,
  /// Whether reading issues and projects fails, as with no API key.
  failing: bool,
}

impl linear_data::LinearHost for FakeHost {
  fn linear_teams(&self, _: &str) -> Result<Vec<LinearTeam>, String> {
    self.record("read linear teams".into());
    Ok(self.linear.teams.clone())
  }

  fn linear_issues(&self, _: &str, team: Option<&str>) -> Result<Vec<LinearIssue>, String> {
    self.record(format!("read linear issues {team:?}"));
    if self.linear.failing {
      return Err(crate::linear::NOT_CONNECTED.into());
    }
    Ok(self.linear.issues.clone())
  }

  fn linear_viewer(&self, _: &str) -> Result<LinearUser, String> {
    self.linear.viewer.clone().ok_or_else(|| "no viewer".into())
  }

  fn linear_projects(&self, _: &str) -> Result<Vec<LinearProject>, String> {
    self.record("read linear projects".into());
    if self.linear.failing {
      return Err(crate::linear::NOT_CONNECTED.into());
    }
    Ok(self.linear.projects.clone())
  }

  fn linear_documents(&self, _: &str, project_id: &str) -> Result<Vec<LinearDocument>, String> {
    self.record(format!("read linear documents {project_id}"));
    Ok(self.linear.documents.clone())
  }

  fn linear_comments(
    &self,
    _: &str,
    of: &linear_data::Commented,
  ) -> Result<Vec<LinearComment>, String> {
    self.record(format!("read linear comments {}", of.id()));
    Ok(
      self
        .linear
        .comments
        .iter()
        .filter(|(on, _)| on == of.id())
        .map(|(_, c)| c.clone())
        .collect(),
    )
  }

  fn linear_prepare_review(
    &self,
    _: &str,
    of: &linear_data::Commented,
    snapshot: &crate::core::linear_review::LinearReviewSnapshot,
  ) -> Result<String, String> {
    self.record(format!(
      "prepare review {} {} {:?} {} comments",
      of.target_type(),
      of.id(),
      snapshot.title,
      snapshot.comments.len()
    ));
    Ok(format!("/repo/.treq/linear-review/{}", of.id()))
  }

  fn linear_review_comments(
    &self,
    _: &str,
    of: &linear_data::Commented,
  ) -> Result<Vec<crate::local_db::AgentReviewComment>, String> {
    Ok(
      self
        .agent
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.target_type == of.target_type() && c.target_id == of.id())
        .cloned()
        .collect(),
    )
  }

  fn linear_apply_suggestion(&self, _: &str, comment_id: &str) -> Result<(), String> {
    self.record(format!("apply linear suggestion {comment_id}"));
    Ok(())
  }
  fn linear_create_issue(
    &self,
    _: &str,
    team_id: &str,
    title: &str,
    description: &str,
  ) -> Result<LinearIssue, String> {
    self.record(format!(
      "create linear issue {team_id} {title:?} {description:?}"
    ));
    let mut created = issue("9", ("Todo", "unstarted"), None);
    created.title = title.into();
    Ok(created)
  }

  fn linear_update_issue(
    &self,
    _: &str,
    issue_id: &str,
    title: &str,
    description: &str,
  ) -> Result<LinearIssue, String> {
    self.record(format!(
      "update linear issue {issue_id} {title:?} {description:?}"
    ));
    let mut updated = issue(issue_id, ("Todo", "unstarted"), None);
    updated.title = title.into();
    Ok(updated)
  }

  fn linear_delete_issue(&self, _: &str, issue_id: &str) -> Result<(), String> {
    self.record(format!("delete linear issue {issue_id}"));
    Ok(())
  }
}

pub(super) fn user(id: &str, name: &str) -> LinearUser {
  LinearUser {
    id: id.into(),
    name: name.into(),
  }
}

pub(super) fn issue(id: &str, state: (&str, &str), assignee: Option<LinearUser>) -> LinearIssue {
  serde_json::from_value(serde_json::json!({
    "id": id,
    "identifier": format!("ENG-{id}"),
    "title": format!("Issue {id}"),
    "description": "Ship the API.",
    "state": {"name": state.0, "type": state.1},
    "labels": ["bug"],
    "branch_name": format!("eng-{id}"),
    "parent_id": null,
    "sub_issue_ids": [],
    "url": format!("https://linear.app/x/ENG-{id}"),
    "assignee": assignee,
    "priority": 2,
    "priority_label": "High",
    "project": null,
  }))
  .expect("issue")
}

fn project(id: &str, state: &str) -> LinearProject {
  serde_json::from_value(serde_json::json!({
    "id": id,
    "name": format!("Project {id}"),
    "description": "The plan.",
    "state": state,
    "target_date": "2026-12-01",
    "progress": 0.25,
    "url": format!("https://linear.app/p/{id}"),
    "lead": {"id": "u1", "name": "Ada"},
  }))
  .expect("project")
}

fn comment(id: &str, quote: Option<&str>) -> LinearComment {
  LinearComment {
    id: id.into(),
    body: "Why the API?".into(),
    user: Some(user("u2", "Bo")),
    created_at: "2026-10-19T10:00:00Z".into(),
    quoted_text: quote.map(str::to_string),
  }
}

fn review_comment(
  id: &str,
  target: (&str, &str),
  file: &str,
) -> crate::local_db::AgentReviewComment {
  serde_json::from_value(serde_json::json!({
    "id": id, "repo_path": "/repo", "target_type": target.0, "target_id": target.1,
    "file_path": file, "hunk_id": null, "start_line": 1, "end_line": 1, "side": null,
    "comment_text": "Say which API", "suggested_replacement": "Ship the REST API.",
    "status": "open", "source": "local-agent", "created_at": "", "resolved_at": null,
    "quoted_text": "Ship the API."
  }))
  .expect("review comment")
}

fn linear_route() -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "linear",
    "dark": false,
    "repoPath": "/repo",
  }))
  .expect("parse")
}

fn fixture() -> Fixture {
  Fixture {
    teams: vec![LinearTeam {
      id: "t1".into(),
      name: "Backend".into(),
      key: "BE".into(),
    }],
    issues: vec![
      issue("1", ("Todo", "unstarted"), Some(user("u1", "Ada"))),
      issue("2", ("Backlog", "backlog"), Some(user("u2", "Bo"))),
    ],
    viewer: Some(user("u1", "Ada")),
    projects: vec![project("p1", "started"), project("p2", "planned")],
    documents: vec![LinearDocument {
      id: "d1".into(),
      title: "Spec".into(),
      content: Some("The spec.".into()),
      url: "https://linear.app/d/d1".into(),
      updated_at: "2026-10-19T00:00:00Z".into(),
    }],
    comments: vec![("1".into(), comment("c1", Some("the API")))],
    failing: false,
  }
}

/// The harness's window, queried the way a user perceives it.
fn screen(h: &mut Harness) -> gpui_testing_library::Screen {
  h.cx.run_until_parked();
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  screen
}

fn linear_harness(cx: &mut TestAppContext, host: FakeHost) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(host);
  let mut h = Harness::with_host(cx, linear_route(), host.clone());
  assert!(h.drawn("linear-panel"));
  (h, host)
}

fn linear_host() -> FakeHost {
  FakeHost {
    linear: fixture(),
    ..FakeHost::default()
  }
}

fn view<T>(h: &mut Harness, f: impl FnOnce(&linear::LinearView) -> T) -> T {
  h.screen.read_with(&h.cx, |screen, cx| {
    f(screen.linear.as_ref().expect("linear view").read(cx))
  })
}

/// The entries of the menu shown last.
fn menu(h: &mut Harness) -> Vec<MenuEntry> {
  h.cx.run_until_parked();
  match h.popups.borrow().last() {
    Some(PopupRequest::ShowDropdown { entries, .. } | PopupRequest::ShowMenu { entries, .. }) => {
      entries.clone()
    }
    _ => panic!("no menu is shown"),
  }
}

/// Picks `text` in the submenu `submenu` of the menu shown last.
fn pick_in(h: &mut Harness, submenu: &str, text: &str) {
  let id = menu(h)
    .iter()
    .find_map(|e| match e {
      MenuEntry::Submenu { text: t, items, .. } if t == submenu => {
        items.iter().find_map(|i| match i {
          MenuEntry::Item { id, text: t, .. } if t == text => Some(*id),
          _ => None,
        })
      }
      _ => None,
    })
    .unwrap_or_else(|| panic!("{submenu} › {text} is not in the menu"));
  h.cx
    .update(|_, cx| crate::gpui_shell::popup::pick_native(id, cx));
  h.cx.run_until_parked();
}

fn field(name: &str, value: serde_json::Value) -> ScreenAction {
  ScreenAction::Field {
    field: name.into(),
    value,
  }
}

#[gpui::test]
fn linear_lists_issues_and_kicks_one_off(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, linear_host());
  assert!(host
    .calls()
    .contains(&"read linear issues None".to_string()));
  assert!(host.calls().contains(&"read linear teams".to_string()));
  assert!(h.drawn("linear-issue-1"));
  assert!(h.drawn("linear-issue-2"));
  h.click("linear-kickoff-1");
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    [field(
      "linear.kickoff",
      serde_json::json!({
        "id": "1",
        "identifier": "ENG-1",
        "url": "https://linear.app/x/ENG-1",
        "title": "Issue 1",
        "includeSubissues": false,
        "sub_issue_ids": [],
      })
    )]
  );
  // The agent button doesn't open the issue's page.
  assert!(view(&mut h, |v| v.open_issue.is_none()));

  // The row opens the page, which opens the issue in Linear and goes back.
  h.click("linear-issue-1");
  assert!(h.drawn("linear-issue-page-1"));
  assert!(!h.drawn("linear-issue-2"));
  h.click("linear-issue-open");
  assert!(host
    .calls()
    .contains(&"open https://linear.app/x/ENG-1".to_string()));
  h.click("linear-issue-back");
  assert!(!h.drawn("linear-issue-page-1"));
  assert!(h.drawn("linear-issue-2"));
}

#[gpui::test]
fn linear_standard_views_and_filters_narrow_the_list(cx: &mut TestAppContext) {
  let (mut h, _) = linear_harness(cx, linear_host());
  h.click("linear-view-my-issues");
  assert!(h.drawn("linear-issue-1"));
  assert!(!h.drawn("linear-issue-2"));
  h.click("linear-view-backlog");
  assert!(!h.drawn("linear-issue-1"));
  assert!(h.drawn("linear-issue-2"));
  h.click("linear-view-all");

  h.click("linear-issues-filter-trigger");
  let entries = menu(&mut h);
  assert_eq!(
    entries[0],
    MenuEntry::Label {
      text: "Filter by".into()
    }
  );
  let submenus: Vec<_> = entries
    .iter()
    .filter_map(|e| match e {
      MenuEntry::Submenu { text, .. } => Some(text.as_str()),
      _ => None,
    })
    .collect();
  assert_eq!(submenus, ["Assignee", "Priority", "Label", "Project"]);
  pick_in(&mut h, "Assignee", "Bo");
  assert!(h.drawn("linear-filter-chip-assignee"));
  assert!(!h.drawn("linear-issue-1"));
  assert!(h.drawn("linear-issue-2"));

  // The menu shows what is picked.
  h.click("linear-issues-filter-trigger");
  let detail = menu(&mut h).iter().find_map(|e| match e {
    MenuEntry::Submenu { text, detail, .. } if text == "Assignee" => detail.clone(),
    _ => None,
  });
  assert_eq!(detail.as_deref(), Some("Bo"));

  h.click("linear-filter-clear-assignee");
  assert!(!h.drawn("linear-filter-chip-assignee"));
  assert!(h.drawn("linear-issue-1"));

  // A filter that matches nothing.
  h.click("linear-view-my-issues");
  h.click("linear-issues-filter-trigger");
  pick_in(&mut h, "Label", "bug");
  assert!(h.drawn("linear-issue-1"));
}

#[gpui::test]
fn linear_team_selector_reads_that_teams_issues(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, linear_host());
  h.click("linear-team-selector");
  let entries = menu(&mut h);
  assert_eq!(
    entries[0],
    MenuEntry::Label {
      text: "Teams".into()
    }
  );
  assert!(matches!(
    &entries[2],
    MenuEntry::Item { text, radio: Some(true), .. } if text == "All Teams"
  ));
  h.pick("Backend");
  assert!(host
    .calls()
    .contains(&"read linear issues Some(\"BE\")".to_string()));
  assert_eq!(view(&mut h, |v| v.team.clone()), Some("BE".into()));
}

#[gpui::test]
fn linear_kanban_groups_issues_by_state(cx: &mut TestAppContext) {
  let (mut h, _) = linear_harness(cx, linear_host());
  h.click("linear-mode-kanban");
  assert!(h.drawn("linear-kanban-column-Backlog"));
  assert!(h.drawn("linear-kanban-column-Todo"));
  assert!(!h.drawn("linear-issue-1"));
  h.click("linear-kanban-kickoff-2");
  h.cx.run_until_parked();
  assert!(matches!(
    &h.sent()[..],
    [ScreenAction::Field { field, value }] if field == "linear.kickoff" && value["id"] == "2"
  ));
  assert!(view(&mut h, |v| v.open_issue.is_none()));
  // A card opens the issue's page.
  h.click("linear-kanban-card-2");
  assert!(h.drawn("linear-issue-page-2"));
  assert!(!h.drawn("linear-kanban-column-Todo"));
}

#[gpui::test]
fn linear_issue_shows_its_comments_and_starts_a_review(cx: &mut TestAppContext) {
  let mut host = linear_host();
  host.settings = std::sync::Mutex::new(
    [("/repo:review_agent".to_string(), Some("codex".to_string()))]
      .into_iter()
      .collect(),
  );
  let (mut h, host) = linear_harness(cx, host);
  h.click("linear-issue-1");
  assert!(h.drawn("linear-issue-page-1"));
  assert!(host.calls().contains(&"read linear comments 1".to_string()));
  assert!(h.drawn("linear-comment-c1"));
  h.click("linear-start-review-linear_issue-1");
  h.cx.run_until_parked();
  assert!(
    host
      .calls()
      .contains(&"prepare review linear_issue 1 \"ENG-1 Issue 1\" 1 comments".to_string()),
    "{:?}",
    host.calls()
  );
  // The review starts in the home repository; React only shows it.
  assert!(h.sent().is_empty(), "{:?}", h.sent());
  let launches = host.launches.lock().unwrap().clone();
  let [(request, true)] = &launches[..] else {
    panic!("one selected launch: {launches:?}");
  };
  assert_eq!(request.workspace_id, None);
  assert_eq!(request.agent.as_deref(), Some("codex"));
  assert_eq!(request.session_name.as_deref(), Some("AI Review"));
  assert_eq!(request.mode.as_deref(), Some("acceptEdits"));
  let prompt = request.prompt.as_deref().expect("prompt");
  assert!(prompt.contains("the Linear issue \"ENG-1 Issue 1\" and its 1 comment."));
  assert!(prompt.contains("/repo/.treq/linear-review/1"));
  assert!(!view(&mut h, |v| v
    .starting
    .contains(&linear_data::Commented::Issue("1".into()))));
  // Back to the list closes the comments.
  h.click("linear-issue-back");
  assert!(!h.drawn("linear-comment-c1"));
}

#[gpui::test]
fn linear_agent_review_cards_apply_and_resolve(cx: &mut TestAppContext) {
  let host = linear_host();
  *host.agent.lock().unwrap() = vec![
    review_comment("r1", ("linear_issue", "1"), "body.md"),
    review_comment("r2", ("linear_issue", "1"), "comments/c1.md"),
  ];
  let (mut h, host) = linear_harness(cx, host);
  h.click("linear-issue-1");
  assert!(h.drawn("linear-agent-review-body"));
  assert!(h.drawn("linear-agent-apply-r1"));
  assert!(h.drawn("linear-agent-apply-r2"));
  // The same card as Settings' prompt edits, plus Resolve.
  assert!(h.drawn("linear-agent-header"));
  assert!(h.drawn("linear-agent-dismiss-r1"));
  let resolve = h
    .cx
    .debug_bounds("linear-agent-resolve-r1")
    .expect("resolve drawn");
  let apply = h
    .cx
    .debug_bounds("linear-agent-apply-r1")
    .expect("apply drawn");
  assert!(resolve.right() <= apply.left(), "Apply is the last action");
  h.click("linear-agent-apply-r1");
  h.cx.run_until_parked();
  let calls = host.calls();
  let applied = calls
    .iter()
    .position(|c| c == "apply linear suggestion r1")
    .expect("applied");
  // The issue is read again so the new text shows.
  assert!(calls[applied..].contains(&"read linear issues None".to_string()));
  h.click("linear-agent-resolve-r2");
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"resolve agent comment r2".to_string()));
  assert!(!h.drawn("linear-agent-resolve-r2"));
}

#[gpui::test]
fn linear_projects_show_details_and_documents(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, linear_host());
  h.click("linear-section-projects");
  assert!(h.drawn("linear-projects-section"));
  assert!(host.calls().contains(&"read linear projects".to_string()));
  // The first project shows until another is picked.
  assert!(h.drawn("linear-project-detail"));
  assert!(host
    .calls()
    .contains(&"read linear documents p1".to_string()));
  assert!(host
    .calls()
    .contains(&"read linear comments p1".to_string()));
  h.click("linear-project-item-p2");
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"read linear documents p2".to_string()));

  h.click("linear-project-view-active");
  assert!(h.drawn("linear-project-item-p1"));
  assert!(!h.drawn("linear-project-item-p2"));

  h.click("linear-document-item-d1");
  assert!(h.drawn("linear-document-dialog"));
  assert!(host
    .calls()
    .contains(&"read linear comments d1".to_string()));
  h.click_backdrop();
  assert!(!h.drawn("linear-document-dialog"));

  h.click("linear-project-open");
  assert!(host
    .calls()
    .contains(&"open https://linear.app/p/p1".to_string()));
}

#[gpui::test]
fn linear_without_a_key_says_how_to_connect(cx: &mut TestAppContext) {
  let mut host = linear_host();
  host.linear.failing = true;
  let (mut h, _) = linear_harness(cx, host);
  assert!(!h.drawn("linear-no-issues"));
  let page = screen(&mut h);
  expect(page.get_by_text(crate::linear::NOT_CONNECTED)).to_be_visible();
  page
    .user()
    .click(&page.get_by_role(by::role(Role::Button).name("Open Settings")));
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    [ScreenAction::Press {
      id: "linear.openSettings".into()
    }]
  );
  // The whole view says it: no tabs to switch to, no team to pick.
  assert!(h.drawn("linear-connection-state"));
  assert!(!h.drawn("linear-section-projects"));
  assert!(!h.drawn("linear-team-selector"));
}

#[gpui::test]
fn linear_shows_when_no_issues_match(cx: &mut TestAppContext) {
  let mut host = linear_host();
  host.linear.issues.clear();
  let (mut h, _) = linear_harness(cx, host);
  assert!(h.drawn("linear-no-issues"));
}

#[gpui::test]
fn linear_creates_an_issue_in_the_shown_team(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, linear_host());
  h.click("linear-new-issue");
  assert!(h.drawn("linear-new-issue-form"));
  // No title, no create.
  h.click("linear-new-issue-submit");
  h.cx.run_until_parked();
  assert!(!host.calls().iter().any(|c| c.starts_with("create")));
  type_into(&mut h, "linear-new-issue-title", "New bug");
  h.click("linear-new-issue-submit");
  h.cx.run_until_parked();
  let calls = host.calls();
  let created = calls
    .iter()
    .position(|c| c == "create linear issue t1 \"New bug\" \"\"")
    .unwrap_or_else(|| panic!("{calls:?}"));
  // The list is read again and the new issue is the one inspected.
  assert!(calls[created..].contains(&"read linear issues None".to_string()));
  assert!(!h.drawn("linear-new-issue-form"));
  assert_eq!(view(&mut h, |v| v.open_issue.clone()), Some("9".into()));
}

#[gpui::test]
fn linear_new_issue_cancel_closes_the_form(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, linear_host());
  h.click("linear-new-issue");
  type_into(&mut h, "linear-new-issue-title", "Never mind");
  h.click("linear-new-issue-cancel");
  assert!(!h.drawn("linear-new-issue-form"));
  assert!(!host.calls().iter().any(|c| c.starts_with("create")));
}

#[gpui::test]
fn linear_edits_the_inspected_issue(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, linear_host());
  h.click("linear-issue-1");
  h.click("linear-issue-edit");
  assert!(h.drawn("linear-issue-edit-form"));
  type_into(&mut h, "linear-issue-edit-title", " v2");
  h.click("linear-issue-edit-save");
  h.cx.run_until_parked();
  let calls = host.calls();
  let updated = calls
    .iter()
    .position(|c| c == "update linear issue 1 \"Issue 1 v2\" \"Ship the API.\"")
    .unwrap_or_else(|| panic!("{calls:?}"));
  assert!(calls[updated..].contains(&"read linear issues None".to_string()));
  assert!(!h.drawn("linear-issue-edit-form"));
  assert!(h.drawn("linear-issue-page-1"));

  // Cancel leaves Linear alone.
  h.click("linear-issue-edit");
  h.click("linear-issue-edit-cancel");
  assert!(!h.drawn("linear-issue-edit-form"));
  assert_eq!(
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("update"))
      .count(),
    1
  );
}

#[gpui::test]
fn linear_deletes_the_inspected_issue_after_confirming(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, linear_host());
  h.click("linear-issue-1");
  h.click("linear-issue-delete");
  assert!(h.drawn("linear-delete-dialog"));
  h.click("linear-delete-cancel");
  assert!(!h.drawn("linear-delete-dialog"));
  assert!(!host.calls().iter().any(|c| c.starts_with("delete")));

  h.click("linear-issue-delete");
  h.click("linear-delete-confirm");
  h.cx.run_until_parked();
  let calls = host.calls();
  let deleted = calls
    .iter()
    .position(|c| c == "delete linear issue 1")
    .unwrap_or_else(|| panic!("{calls:?}"));
  assert!(calls[deleted..].contains(&"read linear issues None".to_string()));
  assert!(!h.drawn("linear-delete-dialog"));
  assert_eq!(view(&mut h, |v| v.open_issue.clone()), None);
  assert!(h.drawn("linear-issue-1"));
}

#[test]
fn linear_capitalizes_project_states() {
  assert_eq!(linear::capitalize("started"), "Started");
  assert_eq!(linear::capitalize("in progress"), "In Progress");
  assert_eq!(linear::capitalize(""), "");
}

#[gpui::test]
fn linear_document_comments_show_at_the_paragraph_they_quote(cx: &mut TestAppContext) {
  let mut host = linear_host();
  host.linear.documents[0].content = Some("First part.\n\nSecond part.\n\nThird part.".into());
  host
    .linear
    .comments
    .push(("d1".into(), comment("c2", Some("Second part"))));
  host
    .linear
    .comments
    .push(("d1".into(), comment("c3", None)));
  let (mut h, _) = linear_harness(cx, host);
  h.click("linear-section-projects");
  // A project without comments shows only its text.
  assert!(h.drawn("linear-project-detail"));
  assert!(!h.drawn("linear-comments-column"));

  h.click("linear-document-item-d1");
  assert!(h.drawn("linear-comment-c2"));
  assert!(h.drawn("linear-comment-c3"));
  let bounds = |h: &mut Harness, selector| h.cx.debug_bounds(selector).expect(selector);
  let quoted = bounds(&mut h, "linear-comment-c2");
  let column = bounds(&mut h, "linear-comments-column");
  let unquoted = bounds(&mut h, "linear-comment-c3");
  // The quoting comment sits in the text, above the comments under it.
  assert!(quoted.bottom() <= column.top(), "{quoted:?} {column:?}");
  assert!(column.contains(&unquoted.center()));
}

#[gpui::test]
fn linear_long_comment_thread_folds_the_middle_and_expands_in_place(cx: &mut TestAppContext) {
  let mut host = linear_host();
  host.linear.comments = (0..8)
    .map(|i| ("1".to_string(), comment(&format!("c{i}"), None)))
    .collect();
  let (mut h, _) = linear_harness(cx, host);
  h.click("linear-issue-1");
  h.cx.run_until_parked();
  let shown = |h: &mut Harness| -> Vec<usize> {
    (0..8)
      .filter(|i| h.drawn(super::changes_large::leak(format!("linear-comment-c{i}"))))
      .collect()
  };
  // The first comment and the newest three; four are behind the fold.
  assert_eq!(shown(&mut h), [0, 5, 6, 7]);
  assert!(h.drawn("linear-comments-show-earlier"));
  h.click("linear-comments-show-earlier");
  h.cx.run_until_parked();
  assert_eq!(shown(&mut h), (0..8).collect::<Vec<_>>());
  assert!(!h.drawn("linear-comments-show-earlier"));
}

/// Round 7 `integrations-scale` measurements. Run with `cargo test
/// --features gpui-shell --lib lin_scale -- --ignored --nocapture
/// --test-threads=1`.
mod lin_scale {
  use super::super::changes_large::{frame, report, timed};
  use super::*;

  fn n() -> usize {
    std::env::var("PERF_N")
      .ok()
      .and_then(|v| v.parse().ok())
      .unwrap_or(1000)
  }

  fn big_issue(i: usize) -> LinearIssue {
    let mut it = issue(
      &i.to_string(),
      match i % 4 {
        0 => ("Todo", "unstarted"),
        1 => ("In Progress", "started"),
        2 => ("Backlog", "backlog"),
        _ => ("Done", "completed"),
      },
      Some(user(&format!("u{}", i % 9), &format!("User {}", i % 9))),
    );
    it.description = Some(format!(
      "## Problem {i}\n\n{}\n\n- a\n- b\n",
      "The sync engine drops updates when the queue is full and nobody notices. ".repeat(30)
    ));
    it.labels = vec!["bug".into(), format!("area-{}", i % 12)];
    it
  }

  fn host(count: usize, comments: usize) -> FakeHost {
    let mut host = linear_host();
    host.linear.issues = (0..count).map(big_issue).collect();
    host.linear.comments = (0..comments)
      .map(|i| {
        let mut c = comment(&format!("c{i}"), None);
        c.body = format!(
          "Comment {i}\n\n{}\n\n```rust\nfn x() {{}}\n```\n",
          "Why the API? Because the old path is slow and brittle. ".repeat(10)
        );
        ("1".to_string(), c)
      })
      .collect();
    host
  }

  fn notify(h: &mut Harness) {
    h.screen.update(&mut h.cx, |screen, cx| {
      let view = screen.linear.clone().expect("linear view");
      view.update(cx, |_, cx| cx.notify());
    });
  }

  #[gpui::test]
  #[ignore = "benchmark; run with --ignored --nocapture"]
  fn lin_scale_list(cx: &mut TestAppContext) {
    let (mut h, _) = linear_harness(cx, host(n(), 0));
    let mut f: Vec<_> = (0..10).map(|_| frame(&mut h)).collect();
    report("linear list steady frame", &mut f);
    let mut hov: Vec<_> = (0..10).map(|_| timed(&mut h, notify)).collect();
    report("linear list notify", &mut hov);
    h.click("linear-mode-kanban");
    let mut k: Vec<_> = (0..10).map(|_| frame(&mut h)).collect();
    report("linear kanban steady frame", &mut k);
    h.click("linear-mode-list");
    let mut v: Vec<_> = (0..6)
      .map(|i| {
        timed(&mut h, |h| {
          h.click(
            [
              "linear-view-active",
              "linear-view-backlog",
              "linear-view-all",
            ][i % 3],
          )
        })
      })
      .collect();
    report("linear switch view tab", &mut v);
  }

  #[gpui::test]
  #[ignore = "benchmark; run with --ignored --nocapture"]
  fn lin_scale_comments(cx: &mut TestAppContext) {
    for count in [10usize, 60, 300] {
      let (mut h, _) = linear_harness(cx, host(50, count));
      h.click("linear-issue-1");
      h.cx.run_until_parked();
      let mut f: Vec<_> = (0..10).map(|_| frame(&mut h)).collect();
      report(&format!("linear issue open, {count} comments"), &mut f);
    }
  }
}

#[gpui::test]
fn linear_delete_dialog_cancels_on_escape_and_deletes_on_enter(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, linear_host());
  h.click("linear-issue-1");
  h.click("linear-issue-delete");
  assert!(h.drawn("linear-delete-dialog"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("linear-delete-dialog"));
  h.click("linear-issue-delete");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"delete linear issue 1".to_string()));
}

#[gpui::test]
fn linear_document_dialog_closes_on_escape_and_from_its_x(cx: &mut TestAppContext) {
  let (mut h, _) = linear_harness(cx, linear_host());
  h.click("linear-section-projects");
  h.click("linear-document-item-d1");
  assert!(h.drawn("linear-document-dialog"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("linear-document-dialog"));
  h.click("linear-document-item-d1");
  h.click("linear-document-dialog-close");
  assert!(!h.drawn("linear-document-dialog"));
}
