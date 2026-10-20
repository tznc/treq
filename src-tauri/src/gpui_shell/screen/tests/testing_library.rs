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
    writes.contains(&"notify toast Settings Saved".to_string()),
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
