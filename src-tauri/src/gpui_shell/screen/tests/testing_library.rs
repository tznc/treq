//! Screen tests written with gpui-testing-library: they find elements by
//! role, name and text, as the React integration tests did with Testing
//! Library, and act through GPUI's real pointer and keyboard dispatch.
//! Elements opt in with `.test_support()` plus a role and accessible name.

use gpui::{Role, TestAppContext};
use gpui_testing_library::prelude::*;

use super::*;

/// The harness's window, queried the way a user perceives it.
fn screen(h: &mut Harness) -> gpui_testing_library::Screen {
  h.cx.run_until_parked();
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  screen
}

// agent-notification-setting.test.tsx

#[gpui::test]
fn the_agent_finished_notification_is_on_by_default(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings_route("application"));
  let screen = screen(&mut h);
  let toggle = screen.get_by_role(by::role(Role::Switch).name("Notify when an agent finishes"));
  expect(&toggle).to_be_checked();
}

#[gpui::test]
fn turning_the_agent_finished_notification_off_saves_it(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = Harness::with_host(cx, settings_route("application"), host.clone());
  let screen = screen(&mut h);
  let user = screen.user();

  let toggle = screen.get_by_role(by::role(Role::Switch).name("Notify when an agent finishes"));
  user.click(&toggle);
  expect(&toggle).not().to_be_checked();
  user.click(&screen.get_by_role(by::role(Role::Button).name("Save Settings")));
  h.cx.run_until_parked();

  let writes = host.writes();
  assert!(
    writes
      .iter()
      .any(|w| w.starts_with("set ") && w.contains("notify_agent_finished=false")),
    "{writes:?}"
  );
  assert!(
    writes.contains(&"notify toast Settings saved".to_string()),
    "{writes:?}"
  );
}

// plan-split-button.test.tsx

#[gpui::test]
fn claude_offers_plan_mode_from_the_submit_menu(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, FakeHost::default(), agent_modal());
  let screen = screen(&mut h);
  let user = screen.user();
  expect(screen.get_by_role(by::role(Role::Dialog).name("Start a new agent session")))
    .to_be_visible();
  expect(screen.get_by_role(by::role(Role::Button).name("Edit"))).to_be_visible();
  user.click(&screen.get_by_role(by::role(Role::Button).name("More submit options")));
  user.click(&screen.get_by_role(by::role(Role::MenuItem).name("Plan")));
  assert_eq!(tasks(&mut h, &host)[0].mode, "plan");
}

#[gpui::test]
fn the_prompt_dialog_targets_the_workspace_it_opened_on(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  let screen = screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Button).name("Workspace: treq/login"))).to_be_visible();

  let mut on_main = agent_modal();
  on_main["initialWorkspaceId"] = serde_json::Value::Null;
  h.set_modal(None);
  h.set_modal(Some(on_main));
  let screen = self::screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Button).name("Workspace: main"))).to_be_visible();
}

#[gpui::test]
fn agents_without_plan_mode_hide_the_submit_menu(cx: &mut TestAppContext) {
  let host = FakeHost::with_settings(&[("/repo:default_agent", "codex")]);
  let (mut h, _) = modal_harness(cx, host, agent_modal());
  let screen = screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Button).name("Run"))).to_be_visible();
  assert!(screen
    .query_by_role(by::role(Role::Button).name("More submit options"))
    .is_none());
}

// default-agent.test.tsx (UI parts)

#[gpui::test]
fn the_agent_picker_lists_every_agent(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  let screen = screen(&mut h);
  let picker = screen.get_by_role(by::role(Role::ComboBox).name("Agent"));
  expect(&picker).to_have_value("Claude");
  screen.user().click(&picker);
  screen.redraw();
  expect(&picker).to_be_expanded();
  let agents: Vec<String> = screen
    .get_all_by_role(by::role(Role::MenuItem))
    .iter()
    .filter_map(|n| n.name())
    .collect();
  assert_eq!(agents, ["Claude", "Codex", "Cursor", "Copilot"]);
}

#[gpui::test]
fn escape_closes_the_workspace_picker_then_the_dialog(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  let screen = screen(&mut h);
  let user = screen.user();
  user.click(&screen.get_by_role(by::role(Role::Button).name("Workspace: treq/login")));
  let search = screen.get_by_role(by::role(Role::TextInput).name("Search workspaces..."));
  expect(&search).to_have_focus();

  user.keyboard("{Escape}");
  assert!(screen
    .query_by_role(by::role(Role::TextInput).name("Search workspaces..."))
    .is_none());
  assert!(h.sent().is_empty(), "{:?}", h.sent());

  user.keyboard("{Escape}");
  assert_eq!(h.sent(), vec![press("agentPrompt.close")]);
}

#[gpui::test]
fn the_agent_picker_hides_agents_that_are_not_installed(cx: &mut TestAppContext) {
  let host = FakeHost {
    installed: vec![
      ("claude", true),
      ("codex", false),
      ("cursor-agent", true),
      ("copilot", false),
    ],
    ..FakeHost::default()
  };
  let (mut h, _) = modal_harness(cx, host, agent_modal());
  // The dialog is built on its first draw, and only then looks up the tools.
  screen(&mut h);
  let screen = screen(&mut h);
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::ComboBox).name("Agent")));
  let agents: Vec<String> = screen
    .get_all_by_role(by::role(Role::MenuItem))
    .iter()
    .filter_map(|n| n.name())
    .collect();
  assert_eq!(agents, ["Claude", "Cursor"]);
}

#[gpui::test]
fn choosing_another_agent_offers_to_make_it_the_repo_default(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, FakeHost::default(), agent_modal());
  let screen = screen(&mut h);
  let user = screen.user();
  let save_default = by::role(Role::CheckBox).name("Set as default for this repo");
  assert!(screen.query_by_role(save_default.clone()).is_none());

  user.click(&screen.get_by_role(by::role(Role::ComboBox).name("Agent")));
  user.click(&screen.get_by_role(by::role(Role::MenuItem).name("Codex")));
  let checkbox = screen.get_by_role(save_default.clone());
  expect(&checkbox).not().to_be_checked();
  user.click(&checkbox);
  expect(&checkbox).to_be_checked();

  // Back on the configured agent, there is nothing to save.
  user.click(&screen.get_by_role(by::role(Role::ComboBox).name("Agent")));
  user.click(&screen.get_by_role(by::role(Role::MenuItem).name("Claude")));
  screen.redraw();
  assert!(screen.query_by_role(save_default.clone()).is_none());

  user.click(&screen.get_by_role(by::role(Role::ComboBox).name("Agent")));
  user.click(&screen.get_by_role(by::role(Role::MenuItem).name("Codex")));
  user.click(&screen.get_by_role(save_default));
  user.click(&screen.get_by_role(by::role(Role::Button).name("Run")));
  let start = &tasks(&mut h, &host)[0];
  assert_eq!(start.agent, "codex");
  assert!(start.save_as_repo_default);
}

// workspace/create-dialog-branch-name.test.tsx

#[gpui::test]
fn the_create_dialog_names_the_branch_from_the_title(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    workspaces: vec![workspace_dialogs::workspace(7, "treq/login", Some("main"))],
    ..FakeHost::default()
  });
  let snapshot: ScreenSnapshot = serde_json::from_value(serde_json::json!({
    "kind": "onboarding",
    "dark": false,
    "modal": { "kind": "workspaceDialog", "repoPath": "/repo", "key": 1, "defaults": {} },
  }))
  .expect("snapshot");
  let mut h = Harness::with_host(cx, snapshot, host.clone());
  let screen = screen(&mut h);
  let user = screen.user();
  expect(screen.get_by_role(by::role(Role::Heading).name("Stack a new Workspace"))).to_be_visible();

  user.type_text(&screen.get_by_label_text("Title"), "Dark Mode");
  // The branch check is debounced.
  h.cx
    .background_executor
    .advance_clock(std::time::Duration::from_millis(600));
  h.cx.run_until_parked();
  screen.redraw();
  expect(screen.get_by_label_text("Branch Name")).to_have_value("treq/dark-mode");

  user.click(&screen.get_by_role(by::role(Role::Button).name("Create Workspace")));
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .iter()
    .any(|c| c.starts_with("create treq/dark-mode")));
  screen.redraw();
  assert!(screen
    .query_by_role(by::role(Role::Heading).name("Stack a new Workspace"))
    .is_none());
}

// settings.test.tsx: the tabs

#[gpui::test]
fn settings_tabs_show_which_one_is_open(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings_route("application"));
  let screen = screen(&mut h);
  expect(screen.get_by_role(by::role(Role::Tab).name("Application"))).to_be_selected();
  let repository = screen.get_by_role(by::role(Role::Tab).name("Repository"));
  expect(&repository).not().to_be_selected();
  screen.user().click(&repository);
  h.cx.run_until_parked();
  assert!(h
    .sent()
    .contains(&field("settings.tab", "repository".into())));
}

// feature-preview.test.tsx: the Skills tab follows its preview switch.

#[gpui::test]
fn turning_skills_installation_on_shows_the_skills_tab(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[(
    "feature_preview.skillsInstallation",
    "false",
  )]));
  let mut h = Harness::with_host(cx, settings_route("preview"), host.clone());
  let screen = screen(&mut h);
  assert!(screen
    .query_by_role(by::role(Role::Tab).name("Skills"))
    .is_none());
  let toggle = screen.get_by_role(by::role(Role::Switch).name("Skills installation"));
  expect(&toggle).not().to_be_checked();
  screen.user().click(&toggle);
  h.cx.run_until_parked();
  screen.redraw();
  expect(&toggle).to_be_checked();
  expect(screen.get_by_role(by::role(Role::Tab).name("Skills"))).to_be_visible();
  assert!(host
    .writes()
    .contains(&"set feature_preview.skillsInstallation=true".to_string()));
}

// The commit box: Commit waits for a message.

#[gpui::test]
fn commit_does_nothing_until_a_message_is_typed(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  let screen = screen(&mut h);
  let user = screen.user();
  let commit = || screen.get_by_role(by::role(Role::Button).name("Commit"));
  user.click(&commit());
  h.click("commit-message");
  user.keyboard("   ");
  user.click(&commit());
  h.cx.run_until_parked();
  assert!(
    !h.drawn("commit-message-error"),
    "a disabled Commit asks nothing"
  );
  assert!(!host.writes().iter().any(|w| w.starts_with("commit")));
  h.click("commit-message");
  user.keyboard("Fix");
  user.click(&commit());
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&"commit Some(7) \"Fix\"".to_string()),
    "{:?}",
    host.writes()
  );
}

// The remote chooser can be closed without choosing.

#[gpui::test]
fn the_remote_chooser_closes_from_its_x(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    setup_route(true, serde_json::json!({})),
  );
  let screen = screen(&mut h);
  screen
    .user()
    .click(&screen.get_by_role(by::role(Role::Button).name("Close")));
  assert_eq!(h.sent(), vec![press("remote.setup.close")]);
}

// Every control is reachable by name: these had none, so a screen reader
// read them as "button" and e2e tests clicked them by position.

fn button(name: &str) -> gpui_testing_library::ByRole {
  by::role(Role::Button).name(name)
}

#[gpui::test]
fn the_workspace_header_names_its_icon_buttons(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  let screen = screen(&mut h);
  for name in [
    "Workspace target",
    "View PR (#42, open)",
    "Open PR on web",
    "Sync with remote",
    "More workspace actions",
  ] {
    expect(screen.get_by_role(button(name))).to_be_visible();
  }
}

#[gpui::test]
fn the_workspace_header_names_the_create_pr_buttons(cx: &mut TestAppContext) {
  let host = FakeHost {
    loaded: feature_loaded(),
    pr: workspace_data::PrCache {
      pr: Some(None),
      ci: None,
    },
    ..FakeHost::default()
  };
  let (mut h, _) = workspace_harness(cx, host, workspace_route());
  let screen = screen(&mut h);
  expect(screen.get_by_role(button("Create PR"))).to_be_visible();
  expect(screen.get_by_role(button("More Create PR options"))).to_be_visible();
}

#[gpui::test]
fn the_create_dialog_names_its_target_button(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, workspace_dialogs::dialog(serde_json::json!({})));
  let screen = screen(&mut h);
  expect(screen.get_by_role(button("Workspace target"))).to_be_visible();
}

#[gpui::test]
fn the_prerequisites_recheck_is_named(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host);
  let screen = screen(&mut h);
  // It is in this computer's tools popover.
  let user = screen.user();
  // Nothing is installed in the fake, so its button says so.
  user.click(&screen.get_by_role(button("Tools missing on This computer")));
  expect(screen.get_by_role(button("Check again"))).to_be_visible();
}

#[gpui::test]
fn settings_selects_are_named_after_their_fields(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings_route("application"));
  let screen = screen(&mut h);
  for name in [
    "Theme",
    "Font Size",
    "Claude Code Model",
    "Default Agent",
    "Conflict Marker Style",
  ] {
    expect(screen.get_by_role(by::role(Role::ComboBox).name(name))).to_be_visible();
  }
}

#[gpui::test]
fn the_view_pr_button_reads_the_pull_requests_status(cx: &mut TestAppContext) {
  for (state, draft, label) in [
    ("OPEN", true, "Draft"),
    ("OPEN", false, "Open"),
    ("CLOSED", false, "Closed"),
    ("MERGED", false, "Merged"),
  ] {
    let mut host = feature_host();
    if let Some(Some(pr)) = host.pr.pr.as_mut() {
      pr.state = state.into();
      pr.is_draft = draft;
    }
    let (mut h, _) = workspace_harness(cx, host, workspace_route());
    let screen = screen(&mut h);
    let name = format!("View PR (#42, {})", label.to_lowercase());
    screen.get_by_role(by::role(Role::Button).name(name.as_str()));
  }
}

// The terminal pane (QA: a new shell hidden past the row's edge)

#[gpui::test]
fn a_new_shell_takes_focus_fully_in_view(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-body");
  // Each column keeps 40% of the row: two shells beside the agent overflow it.
  h.cx.simulate_keystrokes("cmd-\\");
  h.cx.run_until_parked();
  h.cx.simulate_keystrokes("cmd-\\");
  h.cx.run_until_parked();
  let shown = screen(&mut h);
  let shells = shown.query_all_by_role(by::role(Role::Group).name("Shell terminal"));
  assert_eq!(shells.len(), 2);
  let last = shells[1].snapshot();
  assert_eq!(
    last.visible_bounds(),
    last.bounds(),
    "the new shell is scrolled fully into view"
  );
  // In the 1400px window, past the agent and the first shell.
  assert!(
    last.bounds().right() <= gpui::px(1400.),
    "{:?}",
    last.bounds()
  );
  let first = shown.get_by_role(by::role(Role::Group).name("Claude 1 terminal"));
  assert!(first.bounds().left() < shells[0].bounds().left());
  assert!(h.drawn("terminal-row-scrollbar"), "the overflow shows");

  // What is typed goes to the new shell.
  h.cx.simulate_input("x");
  h.cx.run_until_parked();
  let spawned: Vec<String> = host
    .calls()
    .into_iter()
    .filter_map(|c| {
      c.strip_prefix("spawn ")
        .and_then(|rest| rest.split(' ').next().map(str::to_string))
    })
    .filter(|id| id.starts_with("shell-"))
    .collect();
  assert_eq!(spawned.len(), 2, "{:?}", host.calls());
  assert!(
    host
      .calls()
      .contains(&format!("write {} \"x\"", spawned[1])),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn account_deletion_refusal_is_announced_as_an_alert_in_a_dialog(cx: &mut TestAppContext) {
  let reason = "You are the last owner of Acme. Make another member an owner, or delete the organization, before deleting your account.";
  let host = Arc::new(FakeHost {
    account_deletion: Some(crate::core::supabase::ApiError {
      status: 409,
      code: "last_owner".into(),
      message: reason.into(),
      correlation_id: None,
    }),
    ..FakeHost::default()
  });
  let mut h = signed_in_as_ada(cx, host);
  let s = screen(&mut h);
  s.user()
    .click(&s.get_by_role(by::role(Role::Button).name("Delete Account")));
  let s = screen(&mut h);
  expect(s.get_by_role(by::role(Role::Dialog).name("Delete your Treq account?"))).to_be_visible();
  settings_act(
    &mut h,
    field("account.delete.email", "ada@example.com".into()),
  );
  h.click("account-delete-confirm");
  let s = screen(&mut h);
  expect(s.get_by_role(by::role(Role::Alert).name(reason))).to_be_visible();
}

#[gpui::test]
fn the_farewell_after_deleting_an_account_is_an_alert(cx: &mut TestAppContext) {
  let mut h = signed_in_as_ada(cx, Arc::new(FakeHost::default()));
  h.click("account-delete");
  settings_act(
    &mut h,
    field("account.delete.email", "ada@example.com".into()),
  );
  h.click("account-delete-confirm");
  h.set_account(account_state(serde_json::json!({})));
  let s = screen(&mut h);
  expect(s.get_by_role(by::role(Role::Alert).name("Sorry to see you go"))).to_be_visible();
}

fn setup_status(status: &str) -> crate::core::SetupScriptStatus {
  crate::core::SetupScriptStatus {
    configured: true,
    status: status.into(),
    run_id: Some(4),
  }
}

#[gpui::test]
fn code_tab_alerts_on_a_failed_setup_script_and_opens_its_logs(cx: &mut TestAppContext) {
  let host = FakeHost {
    setup_status: Some(setup_status("failed")),
    logs_preview: true,
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn("code-setup"));
  h.click("code-setup-logs");
  h.cx.run_until_parked();
  assert!(
    h.sent().contains(&ScreenAction::Field {
      field: "workspace.openLogs".into(),
      value: "setup-script".into(),
    }),
    "{:?}",
    h.sent()
  );
}

#[gpui::test]
fn code_tab_offers_setup_logs_only_once_the_script_has_run(cx: &mut TestAppContext) {
  let host = FakeHost {
    setup_status: Some(crate::core::SetupScriptStatus {
      run_id: None,
      ..setup_status("pending")
    }),
    logs_preview: true,
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn("code-setup"));
  assert!(!h.drawn("code-setup-logs"));
}

#[gpui::test]
fn code_tab_offers_no_setup_logs_without_the_logs_preview(cx: &mut TestAppContext) {
  let host = FakeHost {
    setup_status: Some(setup_status("failed")),
    ..code_host()
  };
  let (mut h, _) = workspace_harness(cx, host, code_route());
  assert!(h.drawn("code-setup"));
  assert!(!h.drawn("code-setup-logs"));
}

#[gpui::test]
fn code_tab_reruns_the_setup_script(cx: &mut TestAppContext) {
  let host = FakeHost {
    setup_status: Some(setup_status("passed")),
    ..code_host()
  };
  let (mut h, host) = workspace_harness(cx, host, code_route());
  h.click("code-setup-rerun");
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"run setup 7".to_string()));
}

#[gpui::test]
fn code_tab_draws_no_setup_alert_without_a_setup_script(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  assert!(!h.drawn("code-setup"));
}
