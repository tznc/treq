use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn settings_saves_the_repository_itself(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[(
    "/repo:auto_review_trigger",
    "on-push",
  )]));
  let mut h = Harness::with_host(cx, settings(), host.clone());
  let screen = drawn_settings(&mut h);
  let repo = screen.repository.expect("repository tab");
  assert!(!repo.loading);
  assert_eq!(repo.current_branch.as_deref(), Some("main"));
  assert_eq!(repo.fields.branch_name_pattern, "treq/{name}");
  // The trigger's old name reads as the new one.
  assert_eq!(repo.fields.auto_review_trigger, "on-pull");
  assert_eq!(repo.model_options[0].label, "Use application default");

  // A switch saves as it flips: no Save button, and no toast.
  h.click("settings-auto-push");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "set repo /repo branch_name_pattern=treq/{name} included_copy_files= default_model= default_agent= auto_push=true review_prompt= review_agent= auto_review_trigger=on-pull",
      "notify repoSettingsChanged",
    ]
  );
  assert!(h.drawn("settings-saved-repo.autoPush"), "the Saved tick");
  h.click("settings-tab-application");
  h.click("settings-back");
  // React still owns the tab and closing the page.
  assert_eq!(
    h.sent(),
    vec![
      field("settings.tab", "application".into()),
      press("settings.close"),
    ]
  );
}

#[gpui::test]
fn settings_names_the_repository_it_configures(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  let repo = drawn_settings(&mut h).repository.expect("repository tab");
  assert_eq!(repo.name, "repo");
  assert_eq!(repo.path, "/repo");
  assert!(h.drawn("settings-repo-name"));
}

#[gpui::test]
fn settings_groups_the_repository_overrides(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  assert!(h.drawn("settings-repo-overrides"));
}

#[gpui::test]
fn settings_has_a_back_arrow_instead_of_save_and_close(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  assert!(h.drawn("settings-back"));
  assert!(!h.drawn("settings-save"));
  assert!(!h.drawn("settings-close"));
}

#[gpui::test]
fn settings_saves_a_typed_field_after_a_pause(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = Harness::with_host(cx, settings(), host.clone());
  h.click("settings-branch-pattern");
  h.cx.simulate_input("x");
  h.cx.run_until_parked();
  assert!(host.writes().is_empty(), "{:?}", host.writes());
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(600));
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(
    writes[0].starts_with("set repo /repo branch_name_pattern=") && writes[0].contains('x'),
    "{writes:?}"
  );
  assert!(h.drawn("settings-saved-repo.branchNamePattern"));
  // The tick fades after a moment.
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(2000));
  h.cx.run_until_parked();
  assert!(!h.drawn("settings-saved-repo.branchNamePattern"));
}

#[gpui::test]
fn settings_keeps_a_typed_value_and_shows_why_it_did_not_save(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    fail_repo_settings: true,
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, settings(), host);
  h.click("settings-branch-pattern");
  h.cx.simulate_input("x");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(600));
  h.cx.run_until_parked();
  assert!(h.drawn("settings-error-repo.branchNamePattern"));
  assert!(!h.drawn("settings-saved-repo.branchNamePattern"));
  let fields = drawn_settings(&mut h).repository.expect("drawn").fields;
  assert!(fields.branch_name_pattern.contains('x'), "{fields:?}");
}

#[gpui::test]
fn settings_takes_typed_values(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  // Controls report edits from inside the screen's update.
  h.click("settings-branch-pattern");
  h.cx.simulate_input("x");
  let fields = drawn_settings(&mut h).repository.expect("drawn").fields;
  assert!(fields.branch_name_pattern.contains('x'), "{fields:?}");
}

#[gpui::test]
fn settings_disables_what_the_yaml_file_sets(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    config: crate::repo_config::RepoConfig {
      branch_name_pattern: Some("team/{name}".into()),
      review_agent: Some("codex".into()),
      ..Default::default()
    },
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, settings(), host.clone());
  let repo = drawn_settings(&mut h).repository.expect("repository tab");
  assert_eq!(repo.managed, ["branchNamePattern", "reviewAgent"]);
  assert_eq!(
    repo.yaml_card,
    Some(settings::YamlCard {
      error: None,
      loading: false
    })
  );
  h.click("settings-yaml-reload");
  h.cx.run_until_parked();
  let reads = host
    .calls()
    .iter()
    .filter(|c| *c == "read config /repo")
    .count();
  assert_eq!(reads, 2);
}

#[gpui::test]
fn settings_saves_the_application_itself(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[
    ("terminal_font_size", "13"),
    ("ui_zoom", "120"),
  ]));
  let mut h = Harness::with_host(cx, settings_route("application"), host.clone());
  let app = drawn_settings(&mut h).application.expect("application tab");
  // A size older builds allowed shows as the nearest preset.
  assert_eq!(app.font_size, "12");
  assert_eq!(app.zoom, 120.);
  assert!(app.notify_agent_finished);
  assert_eq!(app.conflict_marker_style, "git");
  assert_eq!(app.checklist.title, "Command-line tools");

  let view = h
    .screen
    .read_with(&h.cx, |s, _| s.settings.clone().expect("drawn"));
  let send = |h: &mut Harness, action: ScreenAction| {
    view.update(&mut h.cx, |view, cx| view.handle(action, cx));
  };
  send(&mut h, field("app.theme", "dark".into()));
  h.cx.run_until_parked();
  send(&mut h, field("app.fontSize", "16".into()));
  h.cx.run_until_parked();
  send(&mut h, field("app.defaultAgent", "codex".into()));
  h.cx.run_until_parked();
  // Each choice saves as it is made, without a toast.
  assert_eq!(
    host.writes(),
    [
      "set theme=dark",
      "notify settingsChanged",
      "set default_model= default_agent= conflict_marker_style=git notify_agent_finished=true terminal_font_size=16 ui_zoom=120",
      "notify settingsChanged",
      "set default_model= default_agent=codex conflict_marker_style=git notify_agent_finished=true terminal_font_size=16 ui_zoom=120",
      "notify settingsChanged",
    ]
  );
}

#[gpui::test]
fn settings_runs_only_the_saved_setup_script(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    setup_script: "brew install jj".into(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, settings_route("application"), host.clone());
  let setup = drawn_settings(&mut h)
    .application
    .expect("drawn")
    .setup_script;
  assert!(setup.can_run);
  assert_eq!(setup.status, "Never run");
  // The Run row is below the fold of the test window.
  let view = h
    .screen
    .read_with(&h.cx, |s, _| s.settings.clone().expect("drawn"));
  view.update(&mut h.cx, |view, cx| {
    view.handle(press("app.setupScript.run"), cx)
  });
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["run setup"]);

  view.update(&mut h.cx, |view, cx| {
    view.handle(field("app.setupScript", "brew install gh".into()), cx)
  });
  let setup = drawn_settings(&mut h)
    .application
    .expect("drawn")
    .setup_script;
  assert!(!setup.can_run);
  assert_eq!(setup.status, "Saving the edited script…");
}

#[gpui::test]
fn settings_switches_previews_itself(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[(
    "feature_preview.skillsInstallation",
    "false",
  )]));
  let mut h = Harness::with_host(cx, settings_route("preview"), host.clone());
  let screen = drawn_settings(&mut h);
  assert!(!screen.tabs.contains(&settings::Tab::Skills));
  h.click("settings-preview-skillsInstallation");
  h.click("settings-preview-docs-logs");
  h.cx.run_until_parked();
  assert!(drawn_settings(&mut h).tabs.contains(&settings::Tab::Skills));
  assert_eq!(
    host.writes(),
    [
      "set feature_preview.skillsInstallation=true",
      "notify settingsChanged",
      "open http://localhost:3001/docs/concepts/logs",
    ]
  );
}

/// A Settings harness tall enough that the Code Review section is on screen.
fn code_review_settings(cx: &mut TestAppContext, host: Arc<FakeHost>) -> Harness {
  let h = Harness::with_host(cx, settings(), host);
  h.cx
    .simulate_resize(gpui::size(gpui::px(1100.), gpui::px(2400.)));
  h
}

fn prompt_edit(id: &str) -> crate::local_db::AgentReviewComment {
  let mut comment = agent_comment(Some("Skip style nits"));
  comment.id = id.into();
  comment.target_type = "review_prompt".into();
  comment.target_id = "repo".into();
  comment.file_path = "body.md".into();
  comment.comment_text = "You dismiss style comments".into();
  comment.quoted_text = Some("Flag style".into());
  comment
}

#[gpui::test]
fn settings_tunes_the_review_prompt_with_an_agent(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = code_review_settings(cx, host.clone());
  h.click("settings-tune-review-prompt");
  h.cx.run_until_parked();
  assert!(
    host
      .calls()
      .iter()
      .any(|c| c.starts_with("snapshot review prompt /repo")),
    "{:?}",
    host.calls()
  );
  let sent = h.sent();
  let ScreenAction::Field { field, value } = sent.last().expect("sent") else {
    panic!("not a field: {sent:?}");
  };
  assert_eq!(field, "reviewPrompt.tune");
  let prompt = value["prompt"].as_str().expect("prompt");
  assert!(prompt.contains("/repo/.treq/review-prompt"), "{prompt}");
  assert!(prompt.contains("--target-type review_prompt"), "{prompt}");
}

#[gpui::test]
fn settings_reviews_proposed_prompt_edits_with_the_review_cards(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    prompt_edits: std::sync::Mutex::new(vec![prompt_edit("e1"), prompt_edit("e2")]),
    ..FakeHost::default()
  });
  let mut h = code_review_settings(cx, host.clone());
  // The shared agent card: its header row, then Dismiss and Apply only,
  // Apply as the primary on the right, at the condensed size.
  assert!(h.drawn("prompt-edit-card"));
  assert!(h.drawn("prompt-edit-header"));
  assert!(!h.drawn("prompt-edit-resolve-e1"));
  let dismiss = h
    .cx
    .debug_bounds("prompt-edit-dismiss-e1")
    .expect("dismiss drawn");
  let apply = h
    .cx
    .debug_bounds("prompt-edit-apply-e1")
    .expect("apply drawn");
  let card = h.cx.debug_bounds("prompt-edit-card").expect("card drawn");
  assert!(
    dismiss.right() <= apply.left(),
    "{dismiss:?} then {apply:?}"
  );
  assert!(
    card.right() - apply.right() < gpui::px(12.),
    "Apply sits at the right edge"
  );
  assert!(
    apply.size.height <= gpui::px(24.),
    "condensed buttons: {apply:?}"
  );
  assert_eq!(
    dismiss.size.height, apply.size.height,
    "Dismiss matches Apply"
  );

  h.click("prompt-edit-apply-e1");
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"apply prompt edit e1".to_string()));
  let fields = drawn_settings(&mut h).repository.expect("drawn").fields;
  assert_eq!(fields.review_prompt, "applied e1");
  assert!(
    !h.drawn("prompt-edit-apply-e1"),
    "an applied edit leaves the list"
  );

  h.click("prompt-edit-dismiss-e2");
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"delete prompt edit e2".to_string()));
  assert!(!h.drawn("prompt-edit-card"));
}

#[gpui::test]
fn settings_dismisses_every_proposed_prompt_edit_at_once(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    prompt_edits: std::sync::Mutex::new(vec![prompt_edit("e1"), prompt_edit("e2")]),
    ..FakeHost::default()
  });
  let mut h = code_review_settings(cx, host.clone());
  // In the "Proposed edits" row, pulled to the right edge of the cards.
  let all = h
    .cx
    .debug_bounds("prompt-edits-dismiss-all")
    .expect("dismiss all drawn");
  let header = h
    .cx
    .debug_bounds("prompt-edits-header")
    .expect("header drawn");
  let card = h.cx.debug_bounds("prompt-edit-card").expect("card drawn");
  assert!(header.contains(&all.center()), "{all:?} in {header:?}");
  assert!(
    (card.right() - all.right()).abs() < gpui::px(1.),
    "{all:?} vs {card:?}"
  );

  h.click("prompt-edits-dismiss-all");
  h.cx.run_until_parked();
  let calls = host.calls();
  assert!(
    calls.contains(&"delete prompt edit e1".to_string()),
    "{calls:?}"
  );
  assert!(
    calls.contains(&"delete prompt edit e2".to_string()),
    "{calls:?}"
  );
  assert!(!h.drawn("prompt-edit-card"));
  assert!(!h.drawn("prompt-edits-dismiss-all"));
}

#[gpui::test]
fn settings_scrolls_its_content(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1100.), gpui::px(400.)));
  h.cx.run_until_parked();
  let before = h.cx.debug_bounds("settings-auto-push").expect("drawn");
  let scroll = h.cx.debug_bounds("settings-scroll").expect("drawn");
  h.cx.simulate_event(gpui::ScrollWheelEvent {
    position: scroll.center(),
    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-200.))),
    ..Default::default()
  });
  h.cx.run_until_parked();
  let after = h.cx.debug_bounds("settings-auto-push").expect("drawn");
  assert!(after.origin.y < before.origin.y, "{before:?} -> {after:?}");
}

#[gpui::test]
fn settings_fields_are_as_tall_as_react_ones(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  h.cx.run_until_parked();
  // `ui/input.tsx` and the `<select>`s are `h-10`.
  for selector in ["settings-branch-pattern", "settings-repo-default-model"] {
    let bounds = h.cx.debug_bounds(selector).expect("drawn");
    assert_eq!(bounds.size.height, gpui::px(40.), "{selector}");
  }
}
