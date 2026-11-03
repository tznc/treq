use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn code_tab_draws_files_history_and_readme(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  for selector in [
    "workspace-code",
    "code-action-row",
    "code-agent",
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
  assert!(!h.drawn("agent-task-input"), "no inline prompt card");
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

fn home_code_route() -> ScreenSnapshot {
  let mut route = route_json(None, "main");
  route["code"] = true.into();
  serde_json::from_value(route).expect("home route")
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
    "workspace-title",
    "workspace-tab-row",
    "workspace-description",
  ] {
    assert!(h.cx.debug_bounds(selector).is_some(), "{selector}");
  }
  assert!(h.cx.debug_bounds("workspace-overview-skeleton").is_none());
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
  assert!(!h.drawn("workspace-description-edit"));
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
fn the_about_card_keeps_the_description_but_not_the_title(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.cx.run_until_parked();
  let card = h.cx.debug_bounds("workspace-about").expect("card");
  let title = h.cx.debug_bounds("workspace-title").expect("title");
  let text = h.cx.debug_bounds("workspace-description").expect("text");
  assert!(card.contains(&text.center()));
  assert!(!card.intersects(&title), "{card:?} {title:?}");
}

#[gpui::test]
fn no_about_card_without_a_description_or_pull_request(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), code_route());
  h.cx.run_until_parked();
  assert!(h.drawn("workspace-code"));
  assert!(!h.drawn("workspace-about"));
}

#[gpui::test]
fn the_title_edits_in_the_header_on_any_tab(cx: &mut TestAppContext) {
  let mut route = route_json(Some(7), "main");
  route["tab"] = "commits".into();
  let route = serde_json::from_value(route).expect("route");
  let (mut h, host) = workspace_harness(cx, feature_host(), route);
  h.click("workspace-title-edit");
  h.cx.simulate_input("!");
  h.click("workspace-title-save");
  h.cx.run_until_parked();
  assert!(
    host.writes().iter().any(|c| c.starts_with("set title 7 ")),
    "{:?}",
    host.writes()
  );
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
fn the_header_shows_the_title_in_place_of_the_branch(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.cx.run_until_parked();
  let title = h.cx.debug_bounds("workspace-title").expect("title");
  let header = h.cx.debug_bounds("workspace-header").expect("header");
  assert!(header.contains(&title.center()), "{header:?} {title:?}");
  let branch = h.cx.debug_bounds("workspace-branch").expect("branch");
  assert!(
    !header.intersects(&branch),
    "the branch moved to the stack card"
  );
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

#[gpui::test]
fn code_tab_warns_when_rebase_held_for_a_dirty_parent(cx: &mut TestAppContext) {
  let host = FakeHost {
    rebase_hold: Some(crate::core::rebase_guard::RebaseHold::ParentDirty {
      branch: "feature/base".into(),
    }),
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.click("code-status");
  assert!(h.drawn("code-rebase-held-parent-dirty"));
  assert!(!h.drawn("code-rebase-held-diverged"));
}

#[gpui::test]
fn code_tab_warns_when_rebase_held_for_diverged_target(cx: &mut TestAppContext) {
  let host = FakeHost {
    rebase_hold: Some(crate::core::rebase_guard::RebaseHold::TargetDiverged {
      bookmark: "main".into(),
    }),
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.click("code-status");
  assert!(h.drawn("code-rebase-held-diverged"));
  assert!(!h.drawn("code-rebase-held-parent-dirty"));
}

#[gpui::test]
fn code_tab_notes_a_rebase_onto_a_stale_target(cx: &mut TestAppContext) {
  let host = FakeHost {
    rebase_hold: Some(crate::core::rebase_guard::RebaseHold::StaleTarget {
      error: "offline".into(),
    }),
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  h.click("code-status");
  assert!(h.drawn("code-rebase-stale-target"));
}

#[gpui::test]
fn code_tab_shows_no_rebase_warning_without_a_hold(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  for selector in [
    "code-rebase-held-parent-dirty",
    "code-rebase-held-diverged",
    "code-rebase-stale-target",
  ] {
    assert!(!h.drawn(selector), "{selector}");
  }
  assert!(host.calls().contains(&"read rebase hold 7".to_string()));
  assert!(!h.drawn("code-status"), "no alerts, no status button");
}

#[gpui::test]
fn code_tab_agent_button_opens_the_agent_prompt(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.click("code-agent");
  h.cx.run_until_parked();
  assert!(h.sent().contains(&press("code.agentPrompt")));
}

#[gpui::test]
fn home_code_tab_has_the_action_row_and_no_prompt_card(cx: &mut TestAppContext) {
  let mut host = code_host();
  host.loaded.workspace = None;
  let (mut h, _) = workspace_harness(cx, host, home_code_route());
  assert!(h.drawn("code-action-row"));
  assert!(h.drawn("code-agent"));
  assert!(h.drawn("code-go-to-file"));
  assert!(!h.drawn("agent-task-input"));
}

#[gpui::test]
fn code_tab_gathers_its_alerts_behind_the_status_button(cx: &mut TestAppContext) {
  let mut host = FakeHost {
    rebase_hold: Some(crate::core::rebase_guard::RebaseHold::StaleTarget {
      error: "offline".into(),
    }),
    ..code_host()
  };
  host
    .loaded
    .status
    .as_mut()
    .expect("code host reads a status")
    .conflicted_files = vec!["README.md".into()];
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn("code-status"));
  assert!(!h.drawn("code-conflicts"), "alerts wait in the popover");
  assert!(!h.drawn("code-rebase-stale-target"));
  h.click("code-status");
  assert!(h.drawn("code-status-popover"));
  assert!(h.drawn("code-conflicts"));
  assert!(h.drawn("code-rebase-stale-target"));
  h.click("code-view-conflicts");
  h.cx.run_until_parked();
  assert_eq!(
    fields(&h, "workspace.tab"),
    vec![serde_json::json!("changes")]
  );
}

#[gpui::test]
fn code_tab_offers_archive_when_the_branch_was_deleted_on_remote(cx: &mut TestAppContext) {
  let host = FakeHost {
    branch_deleted: true,
    ..code_host()
  };
  let (mut h, host) = workspace_harness(cx, host, code_route());
  assert!(
    !h.drawn("code-branch-deleted"),
    "alerts wait in the popover"
  );
  h.click("code-status");
  assert!(h.drawn("code-branch-deleted"));
  h.click("code-branch-deleted-archive");
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"archive workspace 7".to_string()));
}

#[gpui::test]
fn code_tab_shows_no_deleted_branch_alert_while_the_branch_exists(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, code_host(), code_route());
  assert!(!h.drawn("code-status"), "no alerts, no status button");
  assert!(host.calls().contains(&"read branch deleted 7".to_string()));
}

fn screen(h: &mut Harness) -> gpui_testing_library::Screen {
  h.cx.run_until_parked();
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  screen
}

/// Trunk commits sit under one "On main (read-only)" divider, not an
/// "Immutable" pill on every row.
#[gpui::test]
fn code_tab_groups_trunk_commits_under_a_read_only_divider(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let mut host = code_host();
  let mut target = commit("t", 1, 0);
  target["on_target_only"] = true.into();
  target["is_immutable"] = true.into();
  host.history = serde_json::from_value(serde_json::json!({
    "commits": [commit("a", 10, 4), target],
    "target_branch": "main",
    "workspace_branch": "feature/login"
  }))
  .ok();
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn("commit-item-t"));
  let screen = screen(&mut h);
  expect(screen.get_by_text("On main (read-only)")).to_be_visible();
  assert!(screen.query_by_text("Recent on main").is_none());
  assert!(screen.query_by_text("Immutable").is_none());
}
