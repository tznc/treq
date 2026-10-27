use gpui::{Modifiers, TestAppContext};

use super::*;

#[gpui::test]
fn code_tab_draws_files_history_and_readme(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  for selector in [
    "workspace-code",
    "agent-task-input",
    "workspace-description",
    "code-go-to-file",
    "code-entry-src",
    "code-entry-README.md",
    "code-readme",
    "commit-item-a",
    "commit-item-b",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  let calls = host.calls();
  assert!(
    calls.contains(&"read overview Some(7)".to_string()),
    "{calls:?}"
  );
  assert!(
    calls.iter().any(|c| c.starts_with("read commits Some(7)")),
    "{calls:?}"
  );
}

#[gpui::test]
fn code_tab_says_a_remote_file_tree_is_unavailable(cx: &mut TestAppContext) {
  let host = FakeHost {
    failing: vec!["remote_overview"],
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn("code-files-unavailable"));
  assert!(!h.drawn("code-files-empty"));
  assert!(h.drawn("code-go-to-file"), "Go to file still opens files");
}

#[gpui::test]
fn code_tab_says_when_commits_fail_to_load(cx: &mut TestAppContext) {
  let host = FakeHost {
    history: None,
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.cx.run_until_parked();
  assert!(
    h.drawn("code-history-error"),
    "not stuck on Loading commits..."
  );
  assert!(!h.drawn("code-history-error-popover"));
  h.click("code-history-show-error");
  assert!(
    h.drawn("code-history-error-popover"),
    "Show error opens the error"
  );
  assert!(!h.drawn("commit-item-a"));
}

#[gpui::test]
fn code_tab_hands_navigation_to_react(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.click("code-entry-src");
  h.click("commit-item-b");
  h.click("code-go-to-file");
  h.cx.run_until_parked();
  assert_eq!(
    fields(&h, "code.open"),
    vec![serde_json::json!({"name": "src", "isDirectory": true})]
  );
  assert_eq!(fields(&h, "code.commit"), vec![serde_json::json!("b")]);
  assert!(h.sent().contains(&press("code.filePicker")));
}

#[gpui::test]
fn code_tab_starts_a_task_in_the_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("task-input");
  h.cx.simulate_input("fix the login button");
  h.click("task-submit");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&r#"create session Some(7) "fix the login button""#.to_string()),
    "{:?}",
    host.writes()
  );
  let created = host.started_sessions();
  assert_eq!(created.len(), 1, "{:?}", h.sent());
  assert_eq!(created[0]["sessionId"], 31);
  assert_eq!(created[0]["workspaceId"], 7);
  assert_eq!(created[0]["pendingPrompt"], "fix the login button");
  assert_eq!(created[0]["permissionMode"], "acceptEdits");
  assert_eq!(created[0]["agent"], "claude");
}

#[gpui::test]
fn code_tab_runs_home_tasks_in_a_new_workspace(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.loaded.workspace = None;
  let mut route = route_json(None, "main");
  route["code"] = true.into();
  let route = serde_json::from_value(route).expect("home route");
  let (mut h, host) = workspace_harness(cx, host, route);
  assert!(h.drawn("task-new-workspace"));
  h.click("task-input");
  h.cx.simulate_input("fix login");
  h.click("task-submit");
  h.cx.run_until_parked();
  let writes = host.writes();
  let create = writes
    .iter()
    .find(|c| c.starts_with("create workspace "))
    .unwrap_or_else(|| panic!("{writes:?}"));
  // `treq/fix-login` is taken, so the branch gets a suffix.
  assert!(
    create.starts_with("create workspace treq/fix-login-"),
    "{create}"
  );
  assert!(
    writes.contains(&r#"create session Some(9) "fix login""#.to_string()),
    "{writes:?}"
  );
  assert_eq!(host.started_sessions()[0]["workspaceId"], 9);
}

fn home_code_route() -> ScreenSnapshot {
  let mut route = route_json(None, "main");
  route["code"] = true.into();
  serde_json::from_value(route).expect("home route")
}

#[gpui::test]
fn code_tab_turning_the_new_workspace_toggle_off_runs_home_tasks_at_home(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.loaded.workspace = None;
  let (mut h, host) = workspace_harness(cx, host, home_code_route());
  h.click("task-new-workspace");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&"set repo /repo home_task_new_workspace=false".to_string()),
    "{:?}",
    host.writes()
  );
  h.click("task-input");
  h.cx.simulate_input("fix login");
  h.click("task-submit");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(
    !writes.iter().any(|c| c.starts_with("create workspace ")),
    "{writes:?}"
  );
  assert!(
    writes.contains(&r#"create session None "fix login""#.to_string()),
    "{writes:?}"
  );
}

#[gpui::test]
fn code_tab_remembers_running_home_tasks_at_home(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.loaded.workspace = None;
  host
    .settings
    .lock()
    .unwrap()
    .insert("/repo:home_task_new_workspace".into(), Some("false".into()));
  let (mut h, host) = workspace_harness(cx, host, home_code_route());
  assert!(h.drawn("task-new-workspace"));
  h.click("task-input");
  h.cx.simulate_input("fix login");
  h.click("task-submit");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(
    !writes.iter().any(|c| c.starts_with("create workspace ")),
    "{writes:?}"
  );
  assert!(
    writes.contains(&r#"create session None "fix login""#.to_string()),
    "{writes:?}"
  );
}

#[gpui::test]
fn code_tab_lists_submodules_with_their_pin_and_syncs_them_on_request(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.loaded.workspace = None;
  host.overview.entries.push(crate::core::WorkspaceEntry {
    name: "vendor/lib".into(),
    path: "/repo/vendor/lib".into(),
    is_directory: true,
    modified_at: None,
    submodule_pin: Some("abcdef0123456789".into()),
    submodule_synced: Some(false),
    status: None,
  });
  let (mut h, host) = workspace_harness(cx, host, home_code_route());
  assert!(h.drawn("submodule-row-vendor/lib"));
  h.click("submodule-sync-box-vendor/lib");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&"sync submodule vendor/lib true".to_string()),
    "{:?}",
    host.writes()
  );
}

/// Draws the window once without running the reads a route change started,
/// so the frame shows what a user sees before they land.
fn draw_now(h: &mut Harness) {
  h.cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn code_tab_draws_what_it_read_when_returning_to_it(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  assert!(h.drawn("workspace-description"));
  h.update(|s| s.view = commits_route().view);
  assert!(h.drawn("workspace-commits"));
  h.update(|s| s.view = code_route().view);
  draw_now(&mut h);
  assert!(h.cx.debug_bounds("workspace-description").is_some());
  assert!(h.cx.debug_bounds("workspace-overview-skeleton").is_none());
}

#[gpui::test]
fn switching_workspaces_keeps_the_last_one_drawn_until_the_next_loads(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  assert!(h.drawn("workspace-description"));
  h.update(|s| {
    let mut route = route_json(Some(8), "main");
    route["code"] = true.into();
    route["frameHeight"] = 120.into();
    s.view = serde_json::from_value::<ScreenSnapshot>(route)
      .expect("route")
      .view;
  });
  draw_now(&mut h);
  for selector in [
    "workspace-branch",
    "workspace-tab-row",
    "workspace-description",
  ] {
    assert!(h.cx.debug_bounds(selector).is_some(), "{selector}");
  }
  assert!(h.cx.debug_bounds("workspace-overview-skeleton").is_none());
}

fn task_text(h: &mut Harness) -> Option<String> {
  h.cx.run_until_parked();
  h.screen.read_with(&h.cx, |s, cx| {
    let code = s.workspace.as_ref()?.read(cx).code.clone()?;
    Some(code.read(cx).task_text.clone())
  })
}

#[gpui::test]
fn file_picker_searches_the_workspace_and_inserts_the_picked_file(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("task-add");
  h.click("task-add-file");
  assert!(h.drawn("file-picker"));
  h.cx.simulate_input("log");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(500));
  assert!(host.calls().contains(&r#"read files "log""#.to_string()));
  assert!(h.drawn("file-picker-item-0") && h.drawn("file-picker-item-1"));
  h.click("file-picker-item-1");
  assert!(!h.drawn("file-picker"));
  assert_eq!(task_text(&mut h).as_deref(), Some("@src/logout.rs "));
}

#[gpui::test]
fn file_picker_lists_nothing_for_an_unknown_name_and_closes_on_escape(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.click("task-add");
  h.click("task-add-file");
  h.cx.simulate_input("nothing-here");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(500));
  assert!(h.drawn("file-picker"));
  assert!(!h.drawn("file-picker-item-0"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("file-picker"));
  assert_eq!(task_text(&mut h).as_deref(), Some(""));
}

#[gpui::test]
fn code_tab_offers_no_new_workspace_toggle_inside_a_workspace(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  assert!(h.drawn("agent-task-input"));
  assert!(!h.drawn("task-new-workspace"));
}

#[gpui::test]
fn clicking_empty_space_blurs_the_focused_input(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  let focused = |h: &mut Harness| {
    h.cx.run_until_parked();
    h.cx.update(|window, cx| window.focused(cx).is_some())
  };
  // The task input starts focused.
  assert!(focused(&mut h));
  // A mouse-down on a spot nothing focuses takes the focus off it, as in a
  // browser, so a key such as "?" then reaches the app's shortcuts.
  h.cx.simulate_click(
    gpui::point(gpui::px(1395.), gpui::px(895.)),
    Modifiers::none(),
  );
  assert!(!focused(&mut h));
  // Clicking an input focuses it again.
  h.click("task-input");
  assert!(focused(&mut h));
}

#[gpui::test]
fn code_tab_searches_mentions_without_waiting_when_the_host_is_local(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("task-input");
  h.cx.simulate_input("look at @log");
  // No clock advance: a zero `file_search_delay` skips the debounce timer.
  h.cx.run_until_parked();
  assert!(h.drawn("task-mention-0"));
  assert!(host.calls().contains(&r#"read files "log""#.to_string()));
}

#[gpui::test]
fn code_tab_suggests_files_for_mentions(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("task-input");
  h.cx.simulate_input("look at @log");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(500));
  assert!(h.drawn("task-mentions"));
  assert!(h.drawn("task-mention-0") && h.drawn("task-mention-1"));
  assert!(host.calls().contains(&r#"read files "log""#.to_string()));
  h.click("task-mention-1");
  h.cx.run_until_parked();
  let text = h.screen.read_with(&h.cx, |s, cx| {
    let code = s.workspace.as_ref()?.read(cx).code.clone()?;
    Some(code.read(cx).task_text.clone())
  });
  assert_eq!(text.as_deref(), Some("look at @src/logout.rs "));
}

#[gpui::test]
fn code_tab_saves_the_description(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("workspace-description-edit");
  assert!(h.drawn("workspace-description-input"));
  h.cx.simulate_input("!");
  h.click("workspace-description-save");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .iter()
      .any(|c| c.starts_with("set description 7 ") && c.contains('!')),
    "{:?}",
    host.writes()
  );
  assert!(!h.drawn("workspace-description-input"));
}

/// PR #42's record for `code_host`'s workspace: its description still the
/// PR's, or edited since.
fn synced(description: bool) -> Option<crate::local_db::PrSyncRecord> {
  let text = if description {
    "Fix the login flow"
  } else {
    "The PR body"
  };
  let title = code_host().loaded.workspace.map(|ws| ws.title);
  Some(crate::local_db::PrSyncRecord {
    pr_number: 42,
    title,
    description: Some(text.into()),
  })
}

/// Hands the Code tab `code_host`'s workspace with another description, as
/// the window's listing does after an edit.
fn show_description(h: &mut Harness, description: &str) {
  h.screen.update(&mut h.cx, |screen, cx| {
    let code = screen
      .workspace
      .as_ref()
      .unwrap()
      .read(cx)
      .code
      .clone()
      .unwrap();
    code.update(cx, |view, cx| {
      let mut parent = view.parent.clone();
      if let Some(ws) = parent.workspace.as_mut() {
        ws.description = Some(description.into());
      }
      view.set_parent(parent, false, cx);
    });
  });
}

#[gpui::test]
fn code_tab_says_the_description_follows_the_pr(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_sync: synced(true),
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.cx.run_until_parked();
  assert!(h.drawn("workspace-description-synced"));
  assert!(!h.drawn("workspace-description-resync"));
}

#[gpui::test]
fn code_tab_resyncs_a_locally_edited_description(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_sync: synced(false),
    ..code_host()
  };
  let (mut h, host) = workspace_harness(cx, host, code_route());
  h.cx.run_until_parked();
  assert!(!h.drawn("workspace-description-synced"));
  assert!(h.drawn("workspace-pr-draft"));
  // In the card's footer, under the description.
  let resync = h.cx.debug_bounds("workspace-description-resync").unwrap();
  let text = h.cx.debug_bounds("workspace-description").unwrap();
  let card = h.cx.debug_bounds("workspace-about").unwrap();
  assert!(resync.top() >= text.bottom(), "{resync:?} {text:?}");
  assert!(card.contains(&resync.center()), "{card:?} {resync:?}");
  h.click("workspace-description-resync");
  h.cx.run_until_parked();
  assert!(
    host.writes().contains(&"resync from PR 7".to_string()),
    "{:?}",
    host.writes()
  );
}

#[gpui::test]
fn code_tab_rereads_pr_sync_when_the_description_changes(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_sync: synced(true),
    ..code_host()
  };
  let (mut h, host) = workspace_harness(cx, host, code_route());
  h.cx.run_until_parked();
  let reads = || {
    host
      .calls()
      .iter()
      .filter(|c| c.as_str() == "read pr sync")
      .count()
  };
  let before = reads();
  show_description(&mut h, "Copied from the PR");
  h.cx.run_until_parked();
  assert!(reads() > before, "{:?}", host.calls());
}

#[gpui::test]
fn code_tab_shows_github_beside_a_local_draft(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_sync: synced(false),
    pr_text: Some(("Login fix on GitHub".into(), "The PR body".into())),
    ..code_host()
  };
  let (mut h, host) = workspace_harness(cx, host, code_route());
  h.cx.run_until_parked();
  // The draft shows first, editable.
  assert!(h.drawn("workspace-about-tab-draft") && h.drawn("workspace-about-tab-github"));
  assert!(h.drawn("workspace-description-edit"));
  assert!(!h.drawn("workspace-about-github"));
  h.click("workspace-about-tab-github");
  assert!(h.drawn("workspace-about-github"));
  assert!(!h.drawn("workspace-description-edit") && !h.drawn("workspace-title-edit"));
  assert!(host.calls().contains(&"read pr text".to_string()));
  // Sync stays in the footer under either tab.
  assert!(h.drawn("workspace-description-resync"));
  h.click("workspace-about-tab-draft");
  assert!(h.drawn("workspace-description-edit"));
}

#[gpui::test]
fn code_tab_has_no_tabs_without_a_draft(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_sync: synced(true),
    pr_text: Some(("t".into(), "b".into())),
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.cx.run_until_parked();
  assert!(!h.drawn("workspace-about-tab-github"));
  assert!(h.drawn("workspace-description-edit"));
}

#[gpui::test]
fn code_tab_holds_title_and_description_in_one_card(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.cx.run_until_parked();
  let card = h.cx.debug_bounds("workspace-about").expect("card");
  let title = h.cx.debug_bounds("workspace-title").expect("title");
  let text = h.cx.debug_bounds("workspace-description").expect("text");
  assert!(card.contains(&title.center()) && card.contains(&text.center()));
}

#[gpui::test]
fn code_tab_saves_a_pr_description_to_the_pr(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_sync: synced(true),
    ..code_host()
  };
  let (mut h, host) = workspace_harness(cx, host, code_route());
  h.click("workspace-description-edit");
  h.cx.simulate_input("!");
  h.click("workspace-description-save");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(
    writes.iter().any(|c| c.starts_with("set description 7 "))
      && writes.contains(&"push to PR 7".to_string()),
    "{writes:?}"
  );
}

#[gpui::test]
fn code_tab_saves_a_pr_title_as_a_local_draft(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_sync: synced(true),
    ..code_host()
  };
  let (mut h, host) = workspace_harness(cx, host, code_route());
  h.click("workspace-title-edit");
  h.cx.simulate_input("!");
  h.click("workspace-title-save-menu");
  h.click("workspace-title-save-draft");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(
    writes.iter().any(|c| c.starts_with("set title 7 ")),
    "{writes:?}"
  );
  assert!(!writes.contains(&"push to PR 7".to_string()), "{writes:?}");
  assert!(!h.drawn("workspace-title-input"));
}

#[gpui::test]
fn code_tab_offers_no_draft_without_a_pr(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("workspace-description-edit");
  assert!(!h.drawn("workspace-description-save-menu"));
  h.click("workspace-description-save");
  h.cx.run_until_parked();
  assert!(!host.writes().contains(&"push to PR 7".to_string()));
}

#[gpui::test]
fn code_tab_shows_no_sync_note_without_a_pr(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.cx.run_until_parked();
  assert!(!h.drawn("workspace-description-synced"));
  assert!(!h.drawn("workspace-description-resync"));
  assert!(!h.drawn("workspace-pr-draft"));
}

#[gpui::test]
fn code_tab_shows_the_title_above_the_description_not_in_the_header(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.cx.run_until_parked();
  let title = h.cx.debug_bounds("workspace-title").expect("title");
  let description = h
    .cx
    .debug_bounds("workspace-description")
    .expect("description");
  assert!(
    title.bottom() <= description.top(),
    "{title:?} {description:?}"
  );
  let header = h.cx.debug_bounds("workspace-header").expect("header");
  assert!(!header.intersects(&title), "{header:?} {title:?}");
}

#[gpui::test]
fn code_tab_saves_the_title(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("workspace-title-edit");
  assert!(h.drawn("workspace-title-input"));
  h.cx.simulate_input("!");
  h.click("workspace-title-save");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .iter()
      .any(|c| c.starts_with("set title 7 ") && c.contains('!')),
    "{:?}",
    host.writes()
  );
  assert!(!h.drawn("workspace-title-input"));
}

#[gpui::test]
fn code_tab_discards_title_edits_on_cancel_and_escape(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  h.click("workspace-title-edit");
  h.cx.simulate_input("!");
  h.click("workspace-title-cancel");
  assert!(!h.drawn("workspace-title-input"));
  h.click("workspace-title-edit");
  h.cx.simulate_input("!");
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert!(!h.drawn("workspace-title-input"));
  assert!(
    !host.writes().iter().any(|c| c.starts_with("set title")),
    "{:?}",
    host.writes()
  );
}
