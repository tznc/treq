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
}

impl OpenWorkspacesHost for FakeHost {
  fn fetch(&self, repo_path: &str) {
    self.record(format!("fetch {repo_path}"));
  }

  fn open_prs(&self, repo: &str, mine: bool, query: &str) -> Result<Vec<GhPullRequest>, String> {
    let whose = if mine { "mine" } else { "all" };
    self.record(format!("read open prs {repo} {whose} {query:?}"));
    let prs = if mine {
      &self.open_workspaces.mine
    } else {
      &self.open_workspaces.all
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
    let branches: Vec<&str> = requests.iter().map(|r| r.head_branch.as_str()).collect();
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
  let host = Arc::new(host);
  let snapshot: ScreenSnapshot = serde_json::from_value(serde_json::json!({
    "kind": "onboarding",
    "dark": false,
    "modal": {
      "kind": "openWorkspaces",
      "repoPath": "/repo",
      "repos": ["/repo"],
      "linear": linear,
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
  expect(screen.get_by_role(by::role(Role::Heading).name("Open Workspaces"))).to_be_visible();
  assert_eq!(status(&screen), "Nothing to open yet.");

  screen.user().type_text(
    &screen.get_by_label_text("Branches or links"),
    "feat/a\norigin/feat/b\nnope\nfeat/a\nmain",
  );
  let screen = self::screen(&mut h);
  assert_eq!(
    status(&screen),
    "1 workspace will be created, 1 already open"
  );
  expect(screen.get_by_role(by::role(Role::Alert).name("2 lines can't be opened"))).to_be_visible();
  // Each line shows in a popover from the summary.
  assert!(screen
    .query_by_role(by::role(Role::ListItem).name("feat/b: already open"))
    .is_none());
  let screen = show_lines(&mut h);
  expect(screen.get_by_role(by::role(Role::ListItem).name("main: 'main' is the default branch")))
    .to_be_visible();
  expect(screen.get_by_role(by::role(Role::ListItem).name("feat/b: already open"))).to_be_visible();

  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Open 2 Workspaces")));
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"open workspaces /repo feat/a,feat/b".to_string()));
  assert!(h.sent().contains(&field(
    "openWorkspaces.opened",
    serde_json::json!({"repoPath": "/repo", "id": 200}),
  )));
  let screen = self::screen(&mut h);
  assert!(screen
    .query_by_role(by::role(Role::Heading).name("Open Workspaces"))
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
    &screen.get_by_label_text("Branches or links"),
    "https://github.com/acme/treq/pull/42\nhttps://github.com/other/app/pull/1",
  );
  let screen = show_lines(&mut h);
  assert_eq!(status(&screen), "1 workspace will be created");
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
    .click(&screen.get_by_role(by::role(Role::Button).name("Open 1 Workspace")));
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"open workspaces /repo feat/x".to_string()));
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
  assert_eq!(status(&screen), "2 workspaces will be created");

  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Open 2 Workspaces")));
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"open workspaces /repo feat/six,feat/five".to_string()));
}

#[gpui::test]
fn a_failed_open_keeps_the_dialog_with_the_error(cx: &mut TestAppContext) {
  let host = FakeHost {
    failing: vec!["open_workspaces"],
    ..with_branches(FakeHost::default())
  };
  let (mut h, _) = dialog(host, cx);
  let screen = screen(&mut h);
  screen.user().type_text(
    &screen.get_by_label_text("Branches or links"),
    "feat/a\nfeat/b",
  );
  let screen = self::screen(&mut h);
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Open 2 Workspaces")));
  let screen = self::screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Alert).name("feat/b: create failed"))).to_be_visible();
  expect(screen.get_by_role(by::role(Role::Heading).name("Open Workspaces"))).to_be_visible();
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
  assert_eq!(status(&screen), "1 workspace will be created");
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
    .click(&screen.get_by_role(by::role(Role::Button).name("Open 1 Workspace")));
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
