//! The sidebar's Open Workspaces dialog.

use gpui::{Role, TestAppContext};
use gpui_testing_library::prelude::*;

use super::{
  github::{acme, pr},
  linear_view::{issue, user},
  workspace_dialogs::workspace,
  *,
};
use crate::{
  core::BranchWorkspaceRequest, github::GhPullRequest,
  gpui_shell::screen::open_workspaces_data::OpenWorkspacesHost, linear::LinearIssue,
};

/// What the fake backend's pull request search returns.
#[derive(Default)]
pub(super) struct Fake {
  /// The user's own open pull requests.
  mine: Vec<GhPullRequest>,
  /// Everyone's.
  all: Vec<GhPullRequest>,
  /// Those assigned to the user, which `assignee:@me` searches.
  assigned: Vec<GhPullRequest>,
}

impl OpenWorkspacesHost for FakeHost {
  fn fetch(&self, repo_path: &str) {
    self.record(format!("fetch {repo_path}"));
  }

  fn open_prs(&self, repo: &str, mine: bool, query: &str) -> Result<Vec<GhPullRequest>, String> {
    let whose = if mine { "mine" } else { "all" };
    self.record(format!("read open prs {repo} {whose} {query:?}"));
    if self.failing.contains(&"open_prs") {
      return Err("gh: API rate limit exceeded".into());
    }
    let (prs, query) = match query.strip_prefix("assignee:@me") {
      Some(rest) => (&self.open_workspaces.assigned, rest.trim()),
      None if mine => (&self.open_workspaces.mine, query),
      None => (&self.open_workspaces.all, query),
    };
    Ok(
      prs
        .iter()
        .filter(|pr| pr.title.to_lowercase().contains(&query.to_lowercase()))
        .cloned()
        .collect(),
    )
  }

  fn open_workspaces(
    &self,
    repo_path: &str,
    requests: &[BranchWorkspaceRequest],
  ) -> Vec<Result<(i64, bool), String>> {
    let branches: Vec<String> = requests
      .iter()
      .map(|r| format!("{}@{}", r.head_branch, r.base_branch))
      .collect();
    self.record(format!(
      "open workspaces {repo_path} {}",
      branches.join(",")
    ));
    requests
      .iter()
      .enumerate()
      .map(|(i, r)| {
        if self.failing.contains(&"open_workspaces") && i > 0 {
          return Err("create failed".into());
        }
        let existing = self
          .github
          .workspaces
          .iter()
          .find(|w| w.branch_name == r.head_branch);
        Ok(existing.map_or((200 + i as i64, true), |w| (w.id, false)))
      })
      .collect()
  }

  fn open_issue(&self, repo_path: &str, issue: &LinearIssue) -> Result<(i64, bool), String> {
    self.record(format!("open issue {repo_path} {}", issue.identifier));
    Ok((300, true))
  }
}

fn dialog(host: FakeHost, cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  dialog_with(host, false, cx)
}

fn dialog_with(host: FakeHost, linear: bool, cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  dialog_on(host, linear, None, cx)
}

fn dialog_on(
  host: FakeHost,
  linear: bool,
  stack_on: Option<&str>,
  cx: &mut TestAppContext,
) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(host);
  let snapshot: ScreenSnapshot = serde_json::from_value(serde_json::json!({
    "kind": "onboarding",
    "dark": false,
    "modal": {
      "kind": "openWorkspaces",
      "repoPath": "/repo",
      "repos": ["/repo"],
      "linear": linear,
      "stackOn": stack_on,
    },
  }))
  .expect("snapshot");
  let h = Harness::with_host(cx, snapshot, host.clone());
  (h, host)
}

fn screen(h: &mut Harness) -> gpui_testing_library::Screen {
  // Past the debounce, then every read the dialog started.
  h.cx
    .background_executor
    .advance_clock(std::time::Duration::from_millis(400));
  h.cx.run_until_parked();
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  screen
}

fn status(screen: &gpui_testing_library::Screen) -> String {
  screen
    .get_by_role(by::role(Role::Status))
    .name()
    .unwrap_or_default()
}

/// Opens the popover that lists each line.
fn show_lines(h: &mut Harness) -> gpui_testing_library::Screen {
  let screen = screen(h);
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Status)));
  self::screen(h)
}

fn with_branches(host: FakeHost) -> FakeHost {
  FakeHost {
    remote_branches: vec![
      "main".into(),
      "feat/a".into(),
      "feat/b".into(),
      "feat/x".into(),
    ],
    ..host
  }
}

#[gpui::test]
fn pasted_lines_show_what_will_be_opened(cx: &mut TestAppContext) {
  let mut host = with_branches(FakeHost::default());
  host.github.workspaces = vec![workspace(7, "feat/b", Some("main"))];
  let (mut h, host) = dialog(host, cx);
  let screen = screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Heading).name("New workspace"))).to_be_visible();
  assert_eq!(
    status(&screen),
    "Type a line or pick an item to create workspaces."
  );

  screen.user().type_text(
    &screen.get_by_label_text("Workspaces"),
    "feat/a\norigin/feat/b\nnope\nfeat/a\nmain",
  );
  let screen = self::screen(&mut h);
  assert_eq!(status(&screen), "Will create 2 workspaces, 1 already open");
  expect(screen.get_by_role(by::role(Role::Alert).name("1 line can't be opened"))).to_be_visible();
  // Each line shows in a popover from the summary.
  assert!(screen
    .query_by_role(by::role(Role::ListItem).name("feat/b: already open"))
    .is_none());
  let screen = show_lines(&mut h);
  expect(screen.get_by_role(by::role(Role::ListItem).name("main: 'main' is the default branch")))
    .to_be_visible();
  expect(screen.get_by_role(by::role(Role::ListItem).name("feat/b: already open"))).to_be_visible();
  // A branch no one has made is made for its workspace.
  expect(screen.get_by_role(by::role(Role::ListItem).name("nope: new branch"))).to_be_visible();

  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Create 3 workspaces")));
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"open workspaces /repo feat/a@main,feat/b@main".to_string()));
  assert!(host.calls().contains(&"create nope from - {}".to_string()));
  assert!(h.sent().contains(&field(
    "openWorkspaces.opened",
    serde_json::json!({"repoPath": "/repo", "id": 200}),
  )));
  let screen = self::screen(&mut h);
  assert!(screen
    .query_by_role(by::role(Role::Heading).name("New workspace"))
    .is_none());
}

#[gpui::test]
fn pull_request_links_open_their_head_branch(cx: &mut TestAppContext) {
  let mut host = with_branches(FakeHost::default());
  host.github.remote = acme();
  host.github.pr = Some(pr(42, "feat/x", "release"));
  let (mut h, host) = dialog(host, cx);
  let screen = screen(&mut h);

  screen.user().type_text(
    &screen.get_by_label_text("Workspaces"),
    "https://github.com/acme/treq/pull/42\nhttps://github.com/other/app/pull/1",
  );
  let screen = show_lines(&mut h);
  assert_eq!(status(&screen), "Will create feat/x from PR #42");
  expect(screen.get_by_role(by::role(Role::ListItem).name("feat/x: PR #42 · new"))).to_be_visible();
  expect(
    screen.get_by_role(
      by::role(Role::ListItem)
        .name("https://github.com/other/app/pull/1: other/app is not one of your repositories"),
    ),
  )
  .to_be_visible();
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Create workspace")));
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"open workspaces /repo feat/x@release".to_string()));
}

#[gpui::test]
fn the_pull_request_list_shows_the_users_own_first(cx: &mut TestAppContext) {
  let mut host = FakeHost::default();
  host.github.remote = acme();
  host.open_workspaces.mine = vec![pr(5, "feat/mine", "main")];
  host.open_workspaces.all = vec![pr(5, "feat/mine", "main"), pr(6, "feat/theirs", "main")];
  let (mut h, host) = dialog(host, cx);
  let screen = screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Heading).name("Your pull requests"))).to_be_visible();
  assert!(screen
    .query_by_role(by::role(Role::CheckBox).name("#6 Change 6"))
    .is_none());

  // A search looks through everyone's.
  screen
    .user()
    .type_text(&screen.get_by_label_text("Search pull requests"), "6");
  let screen = self::screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Heading).name("Matching pull requests")))
    .to_be_visible();
  expect(screen.get_by_role(by::role(Role::CheckBox).name("#6 Change 6"))).to_be_visible();
  assert!(host
    .calls()
    .contains(&"read open prs acme/treq all \"6\"".to_string()));
}

/// The input's spinner while a search waits or runs.
fn searching(screen: &gpui_testing_library::Screen) -> bool {
  screen
    .query_by_role(by::role(Role::ProgressIndicator).name("Searching pull requests"))
    .is_some()
}

#[gpui::test]
fn a_search_shows_a_spinner_until_its_results_arrive(cx: &mut TestAppContext) {
  let mut host = FakeHost::default();
  host.github.remote = acme();
  host.open_workspaces.all = vec![pr(5, "feat/a", "main"), pr(6, "feat/b", "main")];
  let (mut h, _) = dialog(host, cx);
  let screen = screen(&mut h);
  assert!(!searching(&screen));

  // Typing shows the spinner at once, over the old list, before the debounce.
  screen
    .user()
    .type_text(&screen.get_by_label_text("Search pull requests"), "6");
  screen.redraw();
  assert!(searching(&screen));
  expect(screen.get_by_role(by::role(Role::CheckBox).name("#5 Change 5"))).to_be_visible();

  let screen = self::screen(&mut h);
  assert!(!searching(&screen));
  expect(screen.get_by_role(by::role(Role::CheckBox).name("#6 Change 6"))).to_be_visible();
  assert!(screen
    .query_by_role(by::role(Role::CheckBox).name("#5 Change 5"))
    .is_none());
}

#[gpui::test]
fn a_search_with_no_matches_says_so(cx: &mut TestAppContext) {
  let mut host = FakeHost::default();
  host.github.remote = acme();
  host.open_workspaces.all = vec![pr(5, "feat/a", "main")];
  let (mut h, _) = dialog(host, cx);
  let screen = screen(&mut h);
  screen
    .user()
    .type_text(&screen.get_by_label_text("Search pull requests"), "zzz");
  let screen = self::screen(&mut h);
  assert!(!searching(&screen));
  // The empty states carry no accessible text; their ids tell them apart.
  assert!(h
    .cx
    .debug_bounds("open-workspaces-no-matching-pull-requests")
    .is_some());
  assert!(h
    .cx
    .debug_bounds("open-workspaces-no-open-pull-requests")
    .is_none());
}

#[gpui::test]
fn a_failed_search_shows_its_error(cx: &mut TestAppContext) {
  let mut host = FakeHost::default();
  host.github.remote = acme();
  host.failing = vec!["open_prs"];
  let (mut h, _) = dialog(host, cx);
  let screen = screen(&mut h);
  screen
    .user()
    .type_text(&screen.get_by_label_text("Search pull requests"), "6");
  let screen = self::screen(&mut h);
  assert!(!searching(&screen));
  assert!(h.cx.debug_bounds("open-workspaces-prs-error").is_some());
  expect(screen.get_by_role(by::role(Role::Button).name("Try again"))).to_be_visible();
}

#[gpui::test]
fn assigned_to_me_searches_the_users_assigned_pull_requests(cx: &mut TestAppContext) {
  let mut host = FakeHost::default();
  host.github.remote = acme();
  host.open_workspaces.mine = vec![pr(5, "feat/mine", "main")];
  host.open_workspaces.all = vec![pr(5, "feat/mine", "main"), pr(6, "feat/theirs", "main")];
  host.open_workspaces.assigned = vec![pr(6, "feat/theirs", "main"), pr(7, "feat/other", "main")];
  let (mut h, host) = dialog(host, cx);
  let screen = screen(&mut h);
  let toggle = || by::role(Role::Button).name("Assigned to me");
  expect(screen.get_by_role(toggle())).not().to_be_pressed();

  screen.user().click(&screen.get_by_role(toggle()));
  let screen = self::screen(&mut h);
  expect(screen.get_by_role(toggle())).to_be_pressed();
  expect(screen.get_by_role(by::role(Role::Heading).name("Assigned to you"))).to_be_visible();
  expect(screen.get_by_role(by::role(Role::CheckBox).name("#7 Change 7"))).to_be_visible();
  assert!(screen
    .query_by_role(by::role(Role::CheckBox).name("#5 Change 5"))
    .is_none());
  assert!(host
    .calls()
    .contains(&"read open prs acme/treq all \"assignee:@me\"".to_string()));

  // It narrows a search too.
  screen
    .user()
    .type_text(&screen.get_by_label_text("Search pull requests"), "6");
  let screen = self::screen(&mut h);
  expect(screen.get_by_role(by::role(Role::CheckBox).name("#6 Change 6"))).to_be_visible();
  assert!(screen
    .query_by_role(by::role(Role::CheckBox).name("#7 Change 7"))
    .is_none());
  assert!(host
    .calls()
    .contains(&"read open prs acme/treq all \"assignee:@me 6\"".to_string()));

  // Off again, the search covers everyone's.
  screen.user().click(&screen.get_by_role(toggle()));
  let screen = self::screen(&mut h);
  expect(screen.get_by_role(toggle())).not().to_be_pressed();
  assert_eq!(
    host.calls().last().map(String::as_str),
    Some("read open prs acme/treq all \"6\"")
  );
}

#[gpui::test]
fn picked_pull_requests_are_opened(cx: &mut TestAppContext) {
  let mut host = FakeHost::default();
  host.github.remote = acme();
  host.open_workspaces.all = vec![pr(5, "feat/five", "main"), pr(6, "feat/six", "main")];
  let (mut h, host) = dialog(host, cx);
  let screen = screen(&mut h);
  // The user has none, so everyone's show.
  expect(screen.get_by_role(by::role(Role::Heading).name("Open pull requests"))).to_be_visible();

  let six = screen.get_by_role(by::role(Role::CheckBox).name("#6 Change 6"));
  screen.user().click(&six);
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::CheckBox).name("#5 Change 5")));
  let screen = self::screen(&mut h);
  expect(screen.get_by_role(by::role(Role::CheckBox).name("#6 Change 6"))).to_be_checked();
  assert_eq!(status(&screen), "Will create 2 workspaces");

  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Create 2 workspaces")));
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"open workspaces /repo feat/six@main,feat/five@main".to_string()));
}

#[gpui::test]
fn a_failed_open_keeps_the_dialog_with_the_error(cx: &mut TestAppContext) {
  let host = FakeHost {
    failing: vec!["open_workspaces"],
    ..with_branches(FakeHost::default())
  };
  let (mut h, _) = dialog(host, cx);
  let screen = screen(&mut h);
  screen
    .user()
    .type_text(&screen.get_by_label_text("Workspaces"), "feat/a\nfeat/b");
  let screen = self::screen(&mut h);
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Create 2 workspaces")));
  let screen = self::screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Alert).name("feat/b: create failed"))).to_be_visible();
  expect(screen.get_by_role(by::role(Role::Heading).name("New workspace"))).to_be_visible();
}

#[gpui::test]
fn repositories_without_github_hide_the_pull_request_list(cx: &mut TestAppContext) {
  let (mut h, host) = dialog(FakeHost::default(), cx);
  let screen = screen(&mut h);
  assert!(screen.query_by_label_text("Search pull requests").is_none());
  assert!(!host.calls().iter().any(|c| c.starts_with("read open prs")));
}

fn linear_host() -> FakeHost {
  let me = user("u1", "Me");
  let mut host = FakeHost::default();
  host.linear.viewer = Some(me.clone());
  host.linear.issues = vec![
    issue("1", ("In Progress", "started"), Some(me.clone())),
    issue("2", ("Done", "completed"), Some(me)),
    issue("3", ("Todo", "unstarted"), Some(user("u2", "You"))),
  ];
  host
}

#[gpui::test]
fn linear_issues_are_picked_from_the_users_task_list(cx: &mut TestAppContext) {
  let mut host = linear_host();
  host.github.remote = acme();
  let (mut h, host) = dialog_with(host, true, cx);
  let screen = screen(&mut h);
  // GitHub's tab first; Linear's beside it.
  expect(screen.get_by_role(by::role(Role::Tab).name("GitHub"))).to_be_selected();
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Tab).name("Linear")));
  let screen = self::screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Heading).name("Your issues"))).to_be_visible();
  // Done issues and other people's are left out.
  assert!(screen
    .query_by_role(by::role(Role::CheckBox).name("ENG-2 Issue 2"))
    .is_none());
  assert!(screen
    .query_by_role(by::role(Role::CheckBox).name("ENG-3 Issue 3"))
    .is_none());

  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::CheckBox).name("ENG-1 Issue 1")));
  let screen = show_lines(&mut h);
  assert_eq!(status(&screen), "Will create eng-1 from ENG-1");
  expect(screen.get_by_role(by::role(Role::ListItem).name("eng-1: new"))).to_be_visible();

  // A search looks through everyone's.
  screen
    .user()
    .type_text(&screen.get_by_label_text("Search issues"), "ENG-3");
  let screen = self::screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Heading).name("Matching issues"))).to_be_visible();
  expect(screen.get_by_role(by::role(Role::CheckBox).name("ENG-3 Issue 3"))).to_be_visible();

  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Create workspace")));
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"open issue /repo ENG-1".to_string()));
  assert!(!host
    .calls()
    .iter()
    .any(|c| c.starts_with("open workspaces")));
  assert!(h.sent().contains(&field(
    "openWorkspaces.opened",
    serde_json::json!({"repoPath": "/repo", "id": 300}),
  )));
}

#[gpui::test]
fn linear_shows_without_github(cx: &mut TestAppContext) {
  let (mut h, _) = dialog_with(linear_host(), true, cx);
  let screen = screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Tab).name("Linear"))).to_be_selected();
  assert!(screen
    .query_by_role(by::role(Role::Tab).name("GitHub"))
    .is_none());
  expect(screen.get_by_role(by::role(Role::CheckBox).name("ENG-1 Issue 1"))).to_be_visible();
}

/// The Linear host with ENG-4 a sub-issue of ENG-1.
fn sub_issue_host() -> FakeHost {
  let mut host = linear_host();
  let mut child = issue("4", ("Todo", "unstarted"), host.linear.viewer.clone());
  child.parent_id = Some("1".into());
  child.parent_identifier = Some("ENG-1".into());
  host.linear.issues.push(child);
  host
}

fn pick_issue(h: &mut Harness, name: &str) {
  let screen = screen(h);
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::CheckBox).name(name)));
}

#[gpui::test]
fn a_sub_issue_picked_with_its_parent_stacks_on_it(cx: &mut TestAppContext) {
  let (mut h, host) = dialog_with(sub_issue_host(), true, cx);
  // The sub-issue first: the parent still opens before it.
  pick_issue(&mut h, "ENG-4 Issue 4");
  pick_issue(&mut h, "ENG-1 Issue 1");

  let screen = show_lines(&mut h);
  expect(screen.get_by_role(by::role(Role::ListItem).name("eng-4: on eng-1 · new")))
    .to_be_visible();
  expect(screen.get_by_role(by::role(Role::ListItem).name("eng-1: new"))).to_be_visible();

  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Create 2 workspaces")));
  h.cx.run_until_parked();
  let opened: Vec<_> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("open issue"))
    .collect();
  assert_eq!(
    opened,
    vec!["open issue /repo ENG-1", "open issue /repo ENG-4"]
  );
}

#[gpui::test]
fn a_sub_issue_stacks_on_its_parents_open_workspace(cx: &mut TestAppContext) {
  let mut host = sub_issue_host();
  let mut parent = workspace(7, "octocat/eng-1-parent", Some("main"));
  parent.metadata = Some(r#"{"linear_issue_key":"ENG-1"}"#.into());
  host.github.workspaces = vec![parent];
  let (mut h, _) = dialog_with(host, true, cx);
  pick_issue(&mut h, "ENG-4 Issue 4");

  let screen = show_lines(&mut h);
  expect(screen.get_by_role(by::role(Role::ListItem).name("eng-4: on octocat/eng-1-parent · new")))
    .to_be_visible();
}

#[gpui::test]
fn titles_and_new_branches_stack_on_the_picked_branch(cx: &mut TestAppContext) {
  let mut host = with_branches(FakeHost::default());
  host.github.workspaces = vec![workspace(7, "feat/base", Some("main"))];
  host.workspaces = vec![workspace(7, "feat/base", Some("main"))];
  let (mut h, host) = dialog_on(host, false, Some("feat/base"), cx);
  let screen = screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Heading).name("New workspace"))).to_be_visible();

  screen.user().type_text(
    &screen.get_by_label_text("Workspaces"),
    "Fix the login flow\nfeat/a",
  );
  let screen = show_lines(&mut h);
  assert_eq!(status(&screen), "Will create 2 workspaces");
  // A title names its branch by the repository's pattern.
  expect(screen.get_by_role(by::role(Role::ListItem).name("treq/fix-the-login-flow: new branch")))
    .to_be_visible();

  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Create 2 workspaces")));
  h.cx.run_until_parked();
  let calls = host.calls();
  assert!(
    calls.contains(&"open workspaces /repo feat/a@feat/base".to_string()),
    "{calls:?}"
  );
  assert!(
    calls.contains(
      &r#"create treq/fix-the-login-flow from - {"title":"Fix the login flow"}"#.to_string()
    ),
    "{calls:?}"
  );
  assert!(
    calls.contains(&"retarget treq/fix-the-login-flow -> feat/base".to_string()),
    "{calls:?}"
  );
}

#[gpui::test]
fn indented_lines_stack_in_order_on_the_line_above(cx: &mut TestAppContext) {
  let (mut h, host) = dialog(with_branches(FakeHost::default()), cx);
  let screen = screen(&mut h);
  screen.user().type_text(
    &screen.get_by_label_text("Workspaces"),
    "Fix the login flow\n  feat/a\n  Add tests\nfeat/b",
  );
  let screen = show_lines(&mut h);
  assert_eq!(status(&screen), "Will create 4 workspaces, 2 stacked");
  expect(
    screen.get_by_role(by::role(Role::ListItem).name("feat/a: on treq/fix-the-login-flow · new")),
  )
  .to_be_visible();
  expect(
    screen.get_by_role(by::role(Role::ListItem).name("treq/add-tests: on feat/a · new branch")),
  )
  .to_be_visible();

  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Create 4 workspaces")));
  h.cx.run_until_parked();
  let calls = host.calls();
  let at = |call: &str| {
    calls
      .iter()
      .position(|c| c == call)
      .unwrap_or_else(|| panic!("{call} not in {calls:?}"))
  };
  // Each parent is made before the line stacked on it.
  let parent = at(r#"create treq/fix-the-login-flow from - {"title":"Fix the login flow"}"#);
  let child = at("open workspaces /repo feat/a@treq/fix-the-login-flow");
  let grandchild = at(r#"create treq/add-tests from - {"title":"Add tests"}"#);
  assert!(parent < child && child < grandchild, "{calls:?}");
  at("retarget treq/add-tests -> feat/a");
  at("open workspaces /repo feat/b@main");
}

#[gpui::test]
fn tab_indents_the_selected_lines(cx: &mut TestAppContext) {
  let (mut h, _) = dialog(with_branches(FakeHost::default()), cx);
  let screen = screen(&mut h);
  let lines = screen.get_by_label_text("Workspaces");
  screen.user().type_text(&lines, "feat/a\nfeat/b\nfeat/x");
  screen.user().keyboard("{Shift>}{ArrowUp}{/Shift}");
  screen.user().tab();
  let screen = show_lines(&mut h);
  // `to_have_value` collapses whitespace, so read the text itself.
  assert_eq!(
    screen.get_by_label_text("Workspaces").value().as_deref(),
    Some("feat/a\n  feat/b\n  feat/x")
  );
  expect(screen.get_by_role(by::role(Role::ListItem).name("feat/x: on feat/b · new")))
    .to_be_visible();

  screen.user().keyboard("{Shift>}{Tab}{/Shift}");
  // `to_have_value` collapses whitespace, so read the text itself.
  assert_eq!(
    screen.get_by_label_text("Workspaces").value().as_deref(),
    Some("feat/a\nfeat/b\nfeat/x")
  );
}

#[gpui::test]
fn reopening_the_dialog_does_not_draw_the_assigned_list_as_the_users_own(cx: &mut TestAppContext) {
  use super::kept_tabs::first_frame;
  let mut host = FakeHost::default();
  host.github.remote = acme();
  host.open_workspaces.mine = vec![pr(5, "feat/mine", "main")];
  host.open_workspaces.assigned = vec![pr(7, "feat/other", "main")];
  let (mut h, _) = dialog(host, cx);
  let screen = screen(&mut h);
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Assigned to me")));
  let _ = self::screen(&mut h);

  let modal = h.screen.read_with(&h.cx, |s, _| s.snapshot().modal.clone());
  h.update(|s| s.modal = None);
  h.cx.run_until_parked();
  h.update(|s| s.modal = modal);
  // The toggle is off again, so the first frame is the user's own list.
  first_frame(&mut h);
  let window = h.cx.update(|window, _| window.window_handle());
  let first = gpui_testing_library::Screen::new(&h.cx, window);
  first.redraw();
  expect(first.get_by_role(by::role(Role::Button).name("Assigned to me")))
    .not()
    .to_be_pressed();
  expect(first.get_by_role(by::role(Role::CheckBox).name("#5 Change 5"))).to_be_visible();
  assert!(first
    .query_by_role(by::role(Role::CheckBox).name("#7 Change 7"))
    .is_none());
}

#[gpui::test]
fn reopening_the_dialog_draws_the_pull_requests_it_read_last(cx: &mut TestAppContext) {
  use super::kept_tabs::{drawn_now, first_frame, reads};
  let mut host = FakeHost::default();
  host.github.remote = acme();
  host.open_workspaces.mine = vec![pr(5, "feat/mine", "main")];
  let (mut h, host) = dialog(host, cx);
  let _ = screen(&mut h);
  assert_eq!(reads(&host, "read open prs"), 1);

  let modal = h.screen.read_with(&h.cx, |s, _| s.snapshot().modal.clone());
  h.update(|s| s.modal = None);
  h.cx.run_until_parked();
  h.update(|s| s.modal = modal);
  // Before any read lands, the first frame has the rows.
  first_frame(&mut h);
  let window = h.cx.update(|window, _| window.window_handle());
  let first = gpui_testing_library::Screen::new(&h.cx, window);
  first.redraw();
  expect(first.get_by_role(by::role(Role::CheckBox).name("#5 Change 5"))).to_be_visible();
  assert!(!drawn_now(&mut h, "open-workspaces-prs-loading-shown"));
  // And the list is read again behind them.
  let _ = screen(&mut h);
  assert_eq!(reads(&host, "read open prs"), 2);
}

mod pr_list {
  use super::pr;
  use crate::gpui_shell::screen::open_workspaces::PrList;

  #[test]
  fn an_older_answer_does_not_end_a_newer_search() {
    let mut list = PrList::default();
    let older = list.start();
    let newer = list.start();
    assert!(!list.settle(older, Ok((vec![pr(5, "feat/a", "main")], false))));
    assert!(list.loading);
    assert!(list.items.is_empty());

    assert!(list.settle(newer, Ok((vec![pr(6, "feat/b", "main")], false))));
    assert!(!list.loading);
    assert_eq!(list.items[0].number, 6);
  }

  #[test]
  fn an_empty_answer_ends_the_search_and_clears_the_list() {
    let mut list = PrList::default();
    let first = list.start();
    list.settle(first, Ok((vec![pr(5, "feat/a", "main")], true)));
    let search = list.start();
    assert!(list.loading);
    assert!(list.settle(search, Ok((Vec::new(), false))));
    assert!(!list.loading);
    assert!(list.items.is_empty());
    assert!(!list.mine);
  }

  #[test]
  fn an_error_ends_the_search_and_the_next_start_clears_it() {
    let mut list = PrList::default();
    let search = list.start();
    assert!(list.settle(search, Err("offline".into())));
    assert!(!list.loading);
    assert_eq!(list.error.as_deref(), Some("offline"));
    list.start();
    assert!(list.error.is_none());
  }
}

mod pr_query {
  use crate::gpui_shell::screen::open_workspaces::{pr_query, PrQuery};

  #[test]
  fn no_search_reads_the_users_own_first() {
    assert_eq!(pr_query("  ", false), PrQuery::MineFirst);
  }

  #[test]
  fn a_search_reads_everyones() {
    assert_eq!(pr_query(" fix ", false), PrQuery::Search("fix".into()));
  }

  #[test]
  fn assigned_to_me_adds_the_assignee_qualifier() {
    assert_eq!(pr_query("", true), PrQuery::Search("assignee:@me".into()));
    assert_eq!(
      pr_query(" fix ", true),
      PrQuery::Search("assignee:@me fix".into())
    );
  }
}

/// Round 7 `integrations-scale` measurements. Run with `cargo test
/// --features gpui-shell --lib dlg_scale -- --ignored --nocapture
/// --test-threads=1`.
mod dlg_scale {
  use super::super::changes_large::{frame, report, timed};
  use super::*;

  #[gpui::test]
  #[ignore = "benchmark; run with --ignored --nocapture"]
  fn dlg_scale_linear_issues(cx: &mut TestAppContext) {
    let n: usize = std::env::var("PERF_N")
      .ok()
      .and_then(|v| v.parse().ok())
      .unwrap_or(1000);
    let mut host = FakeHost::default();
    host.linear.viewer = Some(user("u1", "Me"));
    host.linear.issues = (0..n)
      .map(|i| {
        issue(
          &i.to_string(),
          ("Todo", "unstarted"),
          Some(user("u2", "You")),
        )
      })
      .collect();
    host.github.remote = acme();
    let (mut h, _) = dialog_with(host, true, cx);
    let screen = screen(&mut h);
    screen
      .user()
      .click(&screen.get_by_role(by::role(Role::Tab).name("Linear")));
    let screen = self::screen(&mut h);
    let mut f: Vec<_> = (0..10).map(|_| frame(&mut h)).collect();
    report(&format!("dialog linear tab, {n} issues"), &mut f);
    let mut keys: Vec<_> = (0..8)
      .map(|i| {
        timed(&mut h, |h| {
          let s = self::screen(h);
          s.user().type_text(
            &s.get_by_label_text("Workspaces"),
            if i % 2 == 0 { "a" } else { "\n" },
          );
        })
      })
      .collect();
    report("dialog type in lines box", &mut keys);
    drop(screen);
  }
}

#[gpui::test]
fn the_dialog_closes_on_escape(cx: &mut TestAppContext) {
  let (mut h, _) = dialog(FakeHost::default(), cx);
  let screen = screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Heading).name("New workspace"))).to_be_visible();
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![press("openWorkspaces.close")]);
}

#[gpui::test]
fn secondary_enter_in_the_lines_creates_the_workspaces(cx: &mut TestAppContext) {
  let (mut h, host) = dialog(with_branches(FakeHost::default()), cx);
  let screen = screen(&mut h);
  screen
    .user()
    .type_text(&screen.get_by_label_text("Workspaces"), "feat/a");
  let _ = self::screen(&mut h);
  h.cx.simulate_keystrokes("secondary-enter");
  h.cx.run_until_parked();
  assert!(
    host
      .calls()
      .contains(&"open workspaces /repo feat/a@main".to_string()),
    "{:?}",
    host.calls()
  );
}
