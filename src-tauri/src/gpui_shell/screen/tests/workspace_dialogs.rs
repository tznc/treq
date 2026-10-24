//! Dashboard's workspace dialogs drawn over a GPUI screen.

use std::sync::Arc;

use gpui::TestAppContext;

use super::*;
use crate::gpui_shell::screen::workspace_dialog_data::{
  append_path_suggestion, branch_name_error, new_workspace_chain, submit, ChainEntry, Position,
  Source, Submit, Submitted, Tab,
};

pub(super) fn workspace_json(id: i64, branch: &str, target: Option<&str>) -> serde_json::Value {
  serde_json::json!({
    "id": id,
    "repo_path": "/repo",
    "workspace_name": branch,
    "workspace_path": format!("/repo/.treq/workspaces/{branch}"),
    "branch_name": branch,
    "created_at": "2026-10-19T00:00:00Z",
    "refreshed_at": null,
    "metadata": null,
    "target_branch": target,
    "title": "",
    "description": null,
    "moved_files": null,
    "not_on_remote": false,
    "sparse_patterns": null
  })
}

pub(super) fn workspace(id: i64, branch: &str, target: Option<&str>) -> crate::local_db::Workspace {
  serde_json::from_value(workspace_json(id, branch, target)).expect("workspace row")
}

fn branch(name: &str) -> crate::jj::JjBranch {
  crate::jj::JjBranch {
    name: name.into(),
    is_current: false,
  }
}

fn host() -> FakeHost {
  FakeHost {
    workspaces: vec![
      workspace(7, "treq/login", Some("main")),
      workspace(8, "treq/login-2", Some("treq/login")),
    ],
    ..FakeHost::default()
  }
}

fn source_login() -> Source {
  Source::Workspace {
    id: 7,
    branch_name: "treq/login".into(),
    target_branch: Some("main".into()),
  }
}

fn form(branch: &str) -> Submit {
  Submit {
    repo_path: "/repo".into(),
    branch_name: branch.into(),
    workspaces: host().workspaces,
    ..Submit::default()
  }
}

/// The calls that change something, without reads or checks.
fn changes(host: &FakeHost) -> Vec<String> {
  host
    .calls()
    .into_iter()
    .filter(|c| !c.starts_with("read") && !c.starts_with("check") && c != "fetch")
    .collect()
}

#[test]
fn branch_names_follow_the_backend_rules() {
  let workspaces = host().workspaces;
  assert_eq!(branch_name_error("", &workspaces), None);
  assert_eq!(branch_name_error("treq/new", &workspaces), None);
  assert_eq!(
    branch_name_error("bad name", &workspaces).as_deref(),
    Some("Invalid branch name: contains ' '")
  );
  assert_eq!(
    branch_name_error("feat..x", &workspaces).as_deref(),
    Some("Invalid branch name: contains '..' or '@{'")
  );
  assert_eq!(
    branch_name_error(" treq/login ", &workspaces).as_deref(),
    Some("A workspace for treq/login already exists")
  );
}

#[test]
fn the_stack_card_shows_the_chain_the_workspace_joins() {
  let workspaces = host().workspaces;
  let branch = |name: &str, workspace: bool| ChainEntry::Branch {
    branch: name.into(),
    workspace,
  };
  assert_eq!(
    new_workspace_chain(&workspaces, "treq/login-2", Position::After, "main"),
    vec![
      ChainEntry::New,
      branch("treq/login-2", true),
      branch("treq/login", true)
    ]
  );
  assert_eq!(
    new_workspace_chain(&workspaces, "treq/login-2", Position::Before, "main"),
    vec![
      branch("treq/login-2", true),
      ChainEntry::New,
      branch("treq/login", true)
    ]
  );
  // A branch with no workspace sits on the default branch.
  assert_eq!(
    new_workspace_chain(&workspaces, "release", Position::After, "main"),
    vec![ChainEntry::New, branch("release", false)]
  );
  assert_eq!(
    new_workspace_chain(&workspaces, "main", Position::After, "main"),
    vec![ChainEntry::New]
  );
}

#[test]
fn symlink_suggestions_append_once() {
  assert_eq!(append_path_suggestion("", "node_modules"), "node_modules");
  assert_eq!(
    append_path_suggestion("target  \n", "node_modules"),
    "target\nnode_modules"
  );
  assert_eq!(
    append_path_suggestion("node_modules\ntarget", "target"),
    "node_modules\ntarget"
  );
}

#[test]
fn creates_a_workspace_from_the_remote_branch() {
  let host = host();
  let mut form = form("feature");
  form.title = " Dark mode ".into();
  form.sparse_paths = "src\n\n docs ".into();
  form.branch_check = Some((
    "feature".into(),
    crate::jj::BranchStatus {
      local_exists: false,
      remote_exists: true,
      remote_name: Some("origin".into()),
      remote_ref: Some("origin/feature".into()),
    },
  ));
  assert_eq!(submit(&host, &form), Submitted::Done(100));
  assert_eq!(
    changes(&host),
    [
      r#"create feature from origin/feature {"title":"Dark mode","sparse_patterns":["src","docs"]}"#,
      "notify toast Workspace created",
    ]
  );
}

#[test]
fn a_check_for_another_name_never_seeds_the_branch() {
  let host = host();
  let mut form = form("feature-2");
  form.branch_check = Some((
    "feature".into(),
    crate::jj::BranchStatus {
      local_exists: false,
      remote_exists: true,
      remote_name: Some("origin".into()),
      remote_ref: Some("origin/feature".into()),
    },
  ));
  submit(&host, &form);
  assert_eq!(changes(&host)[0], "create feature-2 from - {}");
}

#[test]
fn creates_a_workspace_for_the_target_branch_first() {
  let host = host();
  let mut form = form("feature");
  form.target_branch = Some("release".into());
  assert_eq!(submit(&host, &form), Submitted::Done(101));
  assert_eq!(
    changes(&host),
    [
      r#"create release from - {"description":"Workspace for release"}"#,
      "create feature from - {}",
      "retarget feature -> release",
      "notify toast Workspace created",
    ]
  );
}

#[test]
fn the_home_branch_needs_no_workspace_of_its_own() {
  let host = FakeHost {
    home_branch: Some("develop".into()),
    ..host()
  };
  let mut form = form("feature");
  form.target_branch = Some("develop".into());
  submit(&host, &form);
  assert_eq!(
    changes(&host),
    [
      "create feature from - {}",
      "retarget feature -> develop",
      "notify toast Workspace created",
    ]
  );
}

#[test]
fn stacks_after_or_before_a_workspace() {
  let host = host();
  let mut form = form("treq/next");
  form.source = source_login();
  form.description = "Next step".into();
  assert_eq!(submit(&host, &form), Submitted::Done(100));
  assert_eq!(
    changes(&host),
    [
      r#"create treq/next from treq/login {"description":"Next step"}"#,
      "retarget treq/next -> treq/login",
      "notify toast Stacked workspace created",
    ]
  );

  let host = self::host();
  form.position = Position::Before;
  form.description.clear();
  submit(&host, &form);
  assert_eq!(
    changes(&host),
    [
      "create treq/next from - -",
      "retarget treq/next -> main",
      "retarget treq/login -> treq/next",
      "notify toast Stacked workspace created",
    ]
  );
}

#[test]
fn splits_commits_off_into_a_new_workspace() {
  let host = host();
  let mut form = form("treq/split");
  form.source = source_login();
  form.selected_commits = vec!["abc".into(), "def".into()];
  form.title = "Split".into();
  assert_eq!(submit(&host, &form), Submitted::Done(100));
  assert_eq!(
    changes(&host),
    [
      r#"create treq/split from treq/login {"title":"Split"}"#,
      r#"move treq/login -> treq/split files=[] commits=["abc", "def"]"#,
      "notify toast Workspace created",
    ]
  );
}

#[test]
fn splits_files_before_the_source() {
  let host = host();
  let mut form = form("treq/split");
  form.source = source_login();
  form.position = Position::Before;
  form.tab = Tab::Changes;
  form.selected_hunks = 2;
  form.selected_files = vec!["a.rs".into()];
  submit(&host, &form);
  assert_eq!(
    changes(&host),
    [
      "create treq/split from main {}",
      r#"move treq/login -> treq/split files=["a.rs"] commits=[]"#,
      "retarget treq/login -> treq/split",
      "notify toast Workspace split",
    ]
  );
}

#[test]
fn moves_files_to_an_existing_workspace() {
  let host = host();
  let mut form = form("");
  form.source = source_login();
  form.move_to_existing = true;
  form.target_workspace_id = Some(8);
  form.tab = Tab::Changes;
  form.selected_hunks = 1;
  form.selected_files = vec!["a.rs".into()];
  assert_eq!(submit(&host, &form), Submitted::Done(8));
  assert_eq!(
    changes(&host),
    [
      r#"move treq/login -> treq/login-2 files=["a.rs"] commits=[]"#,
      "notify toast Files moved",
    ]
  );

  form.tab = Tab::Commits;
  assert_eq!(
    submit(&host, &form),
    Submitted::Failed("Please select commits or files to move".into())
  );
}

#[test]
fn takes_home_files_into_a_new_workspace() {
  let host = host();
  let mut form = form("feature");
  form.source = Source::Home;
  form.tab = Tab::Changes;
  form.selected_hunks = 3;
  form.selected_files = vec!["a.rs".into(), "b.rs".into()];
  form.symlinked_dirs = "node_modules".into();
  submit(&host, &form);
  assert_eq!(
    changes(&host),
    [
      r#"create feature from - {"moved_files":["a.rs","b.rs"],"symlinked_dirs":["node_modules"]}"#,
      "notify toast Workspace created",
    ]
  );
}

#[test]
fn applies_a_stash_onto_the_new_workspace() {
  let host = host();
  let mut form = form("treq/stash");
  form.source = Source::Home;
  form.target_branch = Some("main".into());
  form.apply_stash_id = Some(3);
  submit(&host, &form);
  assert_eq!(
    changes(&host),
    [
      "create treq/stash from main {}",
      "apply stash 3 onto treq/stash",
      "notify toast Workspace created",
    ]
  );
}

#[test]
fn a_failure_after_the_create_still_opens_the_workspace() {
  let host = FakeHost {
    failing: vec!["apply_stash"],
    ..host()
  };
  let mut form = form("treq/stash");
  form.apply_stash_id = Some(3);
  assert_eq!(submit(&host, &form), Submitted::Done(100));
  assert_eq!(
    changes(&host).last().map(String::as_str),
    Some("notify toast Workspace created with errors")
  );

  let host = FakeHost {
    failing: vec!["new_workspace"],
    ..self::host()
  };
  assert_eq!(
    submit(&host, &self::form("feature")),
    Submitted::Failed("create failed".into())
  );
  assert_eq!(
    changes(&host).last().map(String::as_str),
    Some("notify toast Failed to create workspace")
  );
}

fn with_modal(modal: serde_json::Value) -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "onboarding",
    "dark": false,
    "modal": modal,
  }))
  .expect("snapshot")
}

pub(super) fn dialog(defaults: serde_json::Value) -> ScreenSnapshot {
  with_modal(serde_json::json!({
    "kind": "workspaceDialog",
    "repoPath": "/repo",
    "key": 1,
    "defaults": defaults,
  }))
}

fn dialog_view(h: &mut Harness) -> Entity<workspace_dialog::WorkspaceDialog> {
  h.cx.run_until_parked();
  h.screen
    .read_with(&h.cx, |screen, _| match &screen.workspace_modal {
      Some(open) => match &open.modal {
        workspace_modal::Modal::WorkspaceDialog(view) => view.clone(),
        _ => panic!("another dialog is open"),
      },
      None => panic!("no dialog is open"),
    })
}

fn pause(h: &mut Harness) {
  h.cx
    .background_executor
    .advance_clock(std::time::Duration::from_millis(600));
  h.cx.run_until_parked();
}

#[gpui::test]
fn the_dialog_opens_over_the_screen_and_closes_through_react(cx: &mut TestAppContext) {
  let mut h = Harness::with_host(cx, dialog(serde_json::json!({})), Arc::new(host()));
  assert!(h.drawn("workspace-dialog"));
  assert!(h.drawn("new-workspace-stack-card"));
  // A plain create has no move section.
  assert!(!h.drawn("workspace-dialog-move-section"));
  h.click("workspace-dialog-cancel");
  assert!(!h.drawn("workspace-dialog"));
  assert_eq!(h.sent(), vec![press("workspaceDialog.close")]);
}

#[gpui::test]
fn opening_advanced_only_makes_the_dialog_taller(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    suggestions: (0..12)
      .map(|i| format!("a-long-gitignored-directory-{i}"))
      .collect(),
    ..host()
  });
  let mut h = Harness::with_host(cx, dialog(serde_json::json!({})), host);
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  h.cx.run_until_parked();
  let closed = h.cx.debug_bounds("workspace-dialog").expect("dialog");
  h.click("workspace-dialog-advanced");
  let open = h.cx.debug_bounds("workspace-dialog").expect("dialog");
  assert_eq!(open.size.width, closed.size.width);
  assert!(open.size.height > closed.size.height);
}

#[gpui::test]
fn the_title_names_the_branch_and_creates(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    branches: vec![branch("main"), branch("release")],
    ..host()
  });
  let mut h = Harness::with_host(cx, dialog(serde_json::json!({})), host.clone());
  h.click("workspace-dialog-title");
  h.cx.simulate_input("Dark Mode");
  pause(&mut h);
  let view = dialog_view(&mut h);
  h.cx.read(|cx| {
    let view = view.read(cx);
    assert_eq!(view.branch_name, "treq/dark-mode");
    assert_eq!(
      view.branch_state(),
      Some(workspace_dialog::BranchState::New)
    );
    assert_eq!(view.submit_label(), "Create Workspace");
  });
  assert!(host
    .calls()
    .contains(&"check branch treq/dark-mode".to_string()));
  h.click("workspace-dialog-submit");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![field("workspaceDialog.created", 100.into())]);
  assert!(!h.drawn("workspace-dialog"));
  assert_eq!(
    changes(&host),
    [
      r#"create treq/dark-mode from - {"title":"Dark Mode"}"#,
      "notify toast Workspace created",
    ]
  );
}

#[gpui::test]
fn a_taken_branch_cannot_be_created(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    local_branches: vec!["treq/old".into()],
    ..host()
  });
  let mut h = Harness::with_host(
    cx,
    dialog(serde_json::json!({ "branchName": "treq/login" })),
    host,
  );
  assert!(h.drawn("branch-name-error"));
  let view = dialog_view(&mut h);
  assert!(!h.cx.read(|cx| view.read(cx).can_submit()));

  h.update(|s| {
    s.modal = Some(
      serde_json::from_value(serde_json::json!({
        "kind": "workspaceDialog",
        "repoPath": "/repo",
        "key": 2,
        "defaults": { "branchName": "treq/old" },
      }))
      .expect("route"),
    )
  });
  pause(&mut h);
  assert!(h.drawn("branch-exists-local"));
}

#[gpui::test]
fn the_branch_picker_sets_the_target(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    branches: vec![branch("main"), branch("release")],
    ..host()
  });
  let mut h = Harness::with_host(cx, dialog(serde_json::json!({})), host.clone());
  h.click("workspace-dialog-target");
  assert!(h.drawn("branch-picker"));
  h.cx.simulate_input("rel");
  h.cx.simulate_keystrokes("enter");
  assert!(!h.drawn("branch-picker"));
  let view = dialog_view(&mut h);
  assert_eq!(
    h.cx.read(|cx| view.read(cx).target_branch.clone()),
    Some("release".into())
  );
  assert!(host.calls().contains(&"fetch".to_string()));
}

#[gpui::test]
fn moves_a_picked_commit_to_a_new_workspace(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    ahead: vec![
      crate::core::WorkspaceCommit {
        hash: "abcdef123456".into(),
        timestamp: "t".into(),
        message: "Fix login".into(),
      },
      crate::core::WorkspaceCommit {
        hash: "fedcba654321".into(),
        timestamp: "t".into(),
        message: String::new(),
      },
    ],
    ..host()
  });
  let mut h = Harness::with_host(
    cx,
    dialog(serde_json::json!({
      "source": { "kind": "workspace", "id": 7, "branchName": "treq/login", "targetBranch": "main" },
      "targetBranch": "treq/login",
      "preSelectedCommits": ["abcdef123456"],
      "description": "Fix login",
      "activeTab": "commits",
      "branchName": "treq/fix"
    })),
    host.clone(),
  );
  // Opened to move something, so the move section shows it.
  assert!(h.drawn("dialog-commit-0"));
  assert!(h.drawn("workspace-dialog-stacking-on"));
  let view = dialog_view(&mut h);
  assert_eq!(h.cx.read(|cx| view.read(cx).submit_label()), "Split 1 item");
  h.click("dialog-commit-1");
  assert_eq!(
    h.cx.read(|cx| view.read(cx).submit_label()),
    "Split 2 items"
  );
  h.click("dialog-commits-clear");
  assert_eq!(
    h.cx.read(|cx| view.read(cx).submit_label()),
    "Create Workspace"
  );
  h.click("dialog-commit-0");

  h.click("position-before");
  h.click("workspace-dialog-submit");
  h.cx.run_until_parked();
  assert_eq!(
    changes(&host),
    [
      r#"create treq/fix from main {"description":"Fix login"}"#,
      r#"move treq/login -> treq/fix files=[] commits=["abcdef123456"]"#,
      "retarget treq/login -> treq/fix",
      "notify toast Workspace created",
    ]
  );
}

#[gpui::test]
fn moves_picked_hunks_to_an_existing_workspace(cx: &mut TestAppContext) {
  let working: crate::jj::JjRevisionDiff = serde_json::from_value(serde_json::json!({
    "too_large_to_render": false,
    "uncommitted_files": [
      { "path": "a.rs", "status": "modified", "previous_path": null, "changed_line_count": 2, "diff_deferred": false }
    ],
    "committed_files": [],
    "hunks_by_file": [
      { "path": "a.rs", "hunks": [
        { "id": "h1", "header": "@@ -1 +1 @@", "lines": ["-a", "+b"], "patch": "" },
        { "id": "h2", "header": "@@ -9 +9 @@", "lines": ["-c", "+d"], "patch": "" }
      ] }
    ]
  }))
  .expect("diff");
  let host = Arc::new(FakeHost {
    working: Some(working),
    ..host()
  });
  let mut h = Harness::with_host(
    cx,
    dialog(serde_json::json!({
      "source": { "kind": "workspace", "id": 7, "branchName": "treq/login", "targetBranch": "main" },
      "preSelectedFiles": ["a.rs"],
      "activeTab": "changes"
    })),
    host.clone(),
  );
  h.click("move-to-existing");
  assert!(h.drawn("target-workspace"));
  assert!(!h.drawn("new-workspace-stack-card"));
  h.click("dialog-file-path-0");
  assert!(h.drawn("dialog-hunk-0-1"));
  h.click("dialog-hunk-0-1");
  let view = dialog_view(&mut h);
  h.cx.read(|cx| {
    let view = view.read(cx);
    assert_eq!(view.file_state("a.rs"), workspace_modal::Check::Some);
    assert_eq!(view.target_workspace_id, Some(8));
    assert_eq!(view.submit_label(), "Move 1 to Existing Workspace");
  });
  h.click("workspace-dialog-submit");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![field("workspaceDialog.created", 8.into())]);
  assert_eq!(
    changes(&host),
    [
      r#"move treq/login -> treq/login-2 files=["a.rs"] commits=[]"#,
      "notify toast Files moved",
    ]
  );
}

pub(super) fn rename() -> ScreenSnapshot {
  with_modal(serde_json::json!({
    "kind": "renameWorkspace",
    "repoPath": "/repo",
    "workspaceId": 7,
    "branchName": "treq/login",
  }))
}

#[gpui::test]
fn renames_once_the_dry_run_passes(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    local_branches: vec!["taken".into()],
    ..host()
  });
  let mut h = Harness::with_host(cx, rename(), host.clone());
  assert!(h.drawn("rename-workspace-dialog"));
  // The input starts focused with the branch selected at its end.
  h.cx.simulate_keystrokes("secondary-a backspace");
  h.cx.simulate_input("taken");
  pause(&mut h);
  assert!(h.drawn("rename-invalid"));
  h.click("rename-submit");
  assert!(!host.calls().iter().any(|c| c == "rename 7 -> taken"));

  h.click("rename-branch-name");
  h.cx.simulate_keystrokes("secondary-a backspace");
  h.cx.simulate_input("treq/signin");
  pause(&mut h);
  assert!(!h.drawn("rename-invalid"));
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(!h.drawn("rename-workspace-dialog"));
  assert_eq!(h.sent(), vec![press("renameWorkspace.close")]);
  assert_eq!(
    changes(&host),
    [
      "rename 7 -> taken (dry run)",
      "rename 7 -> treq/signin (dry run)",
      "rename 7 -> treq/signin",
      "notify toast Renamed to treq/signin",
      "notify workspaceChanged",
    ]
  );
}

fn deletion(current: Option<i64>) -> ScreenSnapshot {
  let workspaces = serde_json::json!([
    { "id": 1, "branchName": "main", "workspacePath": "/repo" },
    { "id": 7, "branchName": "treq/login", "workspacePath": "/repo/.treq/workspaces/login" },
    { "id": 8, "branchName": "treq/signup", "workspacePath": "/repo/.treq/workspaces/signup" },
  ]);
  let current = current.map(|id| {
    workspaces
      .as_array()
      .and_then(|all| all.iter().find(|w| w["id"] == id))
      .cloned()
      .expect("workspace")
  });
  with_modal(serde_json::json!({
    "kind": "workspaceDeletion",
    "repoPath": "/repo",
    "workspaces": workspaces,
    "currentWorkspace": current,
  }))
}

#[gpui::test]
fn deletion_lists_what_can_be_deleted(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    workspaces: vec![workspace(8, "treq/signup", None)],
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, deletion(Some(7)), host.clone());
  assert!(h.drawn("workspace-deletion"));
  // The current workspace's default row, then every workspace but home.
  assert!(h.drawn("workspace-deletion-item-2"));
  assert!(!h.drawn("workspace-deletion-item-3"));
  assert!(!h.drawn("workspace-deletion-home"));
  // Enter deletes the highlighted row: the current workspace's own.
  h.cx.simulate_keystrokes("down enter");
  h.cx.run_until_parked();
  // GPUI deletes it itself, closing the dialog first as `handleDelete` does.
  assert_eq!(h.sent(), vec![press("workspaceDeletion.close")]);
  assert!(!h.drawn("workspace-deletion"));
  // Then asks, and Enter confirms.
  assert!(h.drawn("confirm"));
  assert!(!host.calls().iter().any(|c| c.starts_with("delete")));
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  let calls: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("delete") || c.starts_with("notify"))
    .collect();
  assert_eq!(
    calls,
    [
      "notify workspacesRemoving",
      "delete workspace 8",
      "notify workspaceChanged",
      "notify workspacesRemoved",
      "notify toast Workspace deleted",
    ]
  );
}

#[gpui::test]
fn deletion_of_a_remote_workspace_goes_to_react(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    react_actions: true,
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, deletion(Some(7)), host);
  h.cx.simulate_keystrokes("down enter");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![field("workspaceDeletion.delete", 8.into())]);
}

#[gpui::test]
fn deletion_searches_and_closes(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, deletion(None));
  h.cx.simulate_input("signup");
  assert!(h.drawn("workspace-deletion-item-0"));
  assert!(!h.drawn("workspace-deletion-item-1"));
  h.cx.simulate_keystrokes("escape");
  assert_eq!(h.sent(), vec![press("workspaceDeletion.close")]);
}

#[gpui::test]
fn deletion_says_home_cannot_be_deleted(cx: &mut TestAppContext) {
  let mut h = Harness::new(
    cx,
    with_modal(serde_json::json!({
      "kind": "workspaceDeletion",
      "repoPath": "/repo",
      "workspaces": [{ "id": 1, "branchName": "main", "workspacePath": "/repo" }],
      "currentWorkspace": null,
    })),
  );
  assert!(h.drawn("workspace-deletion-home"));
}

fn change_move() -> ScreenSnapshot {
  with_modal(serde_json::json!({
    "kind": "confirmChangeMove",
    "repoPath": "/repo",
    "files": ["a.rs", "b.rs"],
    "sourceBranch": "treq/login",
    "destinationBranch": "treq/signup",
    "destinationLabel": "signup",
  }))
}

#[test]
fn the_move_confirmation_counts_the_files() {
  let Some(workspace_modal::ModalRoute::ConfirmChangeMove(route)) = change_move().modal else {
    unreachable!()
  };
  assert_eq!(route.title(), "Move 2 files to signup?");
  assert_eq!(
    route.description(),
    "This moves the selected uncommitted changes into the target workspace and removes them \
     from the current one."
  );
}

#[gpui::test]
fn confirms_and_moves_dropped_files(cx: &mut TestAppContext) {
  let host = Arc::new(host());
  let mut h = Harness::with_host(cx, change_move(), host.clone());
  assert!(h.drawn("change-move-dialog"));
  h.click("change-move-confirm");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![press("changeMove.done")]);
  assert_eq!(
    changes(&host),
    [
      r#"move treq/login -> treq/signup files=["a.rs", "b.rs"] commits=[]"#,
      "notify toast Files moved",
    ]
  );
}

#[gpui::test]
fn a_failed_move_keeps_the_confirmation_open(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    failing: vec!["move_changes"],
    ..host()
  });
  let mut h = Harness::with_host(cx, change_move(), host.clone());
  h.click("change-move-confirm");
  h.cx.run_until_parked();
  assert!(h.drawn("change-move-dialog"));
  assert!(h.sent().is_empty());
  assert_eq!(
    changes(&host).last().map(String::as_str),
    Some("notify toast Failed to move files")
  );
  h.click("change-move-cancel");
  assert_eq!(h.sent(), vec![press("changeMove.cancel")]);
}
