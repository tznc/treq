use gpui::{Modifiers, TestAppContext};

use super::*;

// The dialogs drawn over a screen: agent prompt, prompt history and stashes.

fn history_host() -> FakeHost {
  FakeHost {
    prompts: vec![
      prompt_entry(
        3,
        Some(7),
        Some("login fixes"),
        "Fix the login button",
        Some("claude"),
      ),
      prompt_entry(2, None, None, "Refactor the\nrouter", None),
      prompt_entry(1, Some(9), None, "Gone", Some("codex")),
    ],
    ..FakeHost::default()
  }
}

#[test]
fn prompt_history_names_workspaces_as_react_does() {
  use modal::prompt_history::{format_for_copy, workspace_label};
  let entries = [
    prompt_entry(1, Some(7), Some("login"), "a", None),
    prompt_entry(2, None, None, "b", None),
    prompt_entry(3, Some(9), None, "c", None),
  ];
  assert_eq!(workspace_label(&entries[0]), "login");
  assert_eq!(workspace_label(&entries[1]), "Home repo");
  assert_eq!(workspace_label(&entries[2]), "Deleted workspace");
  let copied = format_for_copy(&entries[..2]);
  assert!(copied.starts_with("[login] 10/19/20"), "{copied}");
  assert!(copied.contains("\na\n\n---\n\n[Home repo] "), "{copied}");
  assert!(copied.ends_with("\nb"), "{copied}");
}

#[gpui::test]
fn prompt_history_shows_the_newest_prompt_first(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, history_host(), history_modal());
  for selector in [
    "prompt-history-modal",
    "prompt-history-list",
    "prompt-history-item-0",
    "prompt-history-item-2",
    "prompt-history-detail-text",
    "prompt-history-copy-all",
    "prompt-history-run",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(host.calls().contains(&"read prompt history".to_string()));
  let selected = h.screen.read_with(&h.cx, |s, cx| {
    s.prompt_history.as_ref().map(|v| v.read(cx).selected)
  });
  assert_eq!(selected, Some(Some(3)));
  h.click("prompt-history-item-1");
  let selected = h.screen.read_with(&h.cx, |s, cx| {
    s.prompt_history.as_ref().map(|v| v.read(cx).selected)
  });
  assert_eq!(selected, Some(Some(2)));
}

#[gpui::test]
fn prompt_history_opens_on_the_requested_prompt(cx: &mut TestAppContext) {
  let (h, _) = modal_harness(
    cx,
    history_host(),
    serde_json::json!({ "kind": "promptHistory", "repoPath": "/repo", "initialSelectedId": 1 }),
  );
  let selected = h.screen.read_with(&h.cx, |s, cx| {
    s.prompt_history.as_ref().map(|v| v.read(cx).selected)
  });
  assert_eq!(selected, Some(Some(1)));
}

#[gpui::test]
fn prompt_history_copies_and_runs_prompts(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, history_host(), history_modal());
  h.click("prompt-history-copy");
  assert_eq!(h.clipboard().as_deref(), Some("Fix the login button"));
  h.click("prompt-history-copy-all");
  let all = h.clipboard().expect("clipboard");
  assert!(all.starts_with("[login fixes] 10/19/2026, "), "{all}");
  assert!(all.contains("\n\n---\n\n[Home repo] "), "{all}");
  assert_eq!(
    host.writes(),
    [
      "notify toast Prompt copied",
      "notify toast All prompts copied"
    ]
  );
  h.click("prompt-history-item-2");
  h.click("prompt-history-run");
  assert_eq!(
    fields(&h, "promptHistory.run"),
    vec![serde_json::json!({ "prompt": "Gone", "workspaceId": 9 })]
  );
}

#[gpui::test]
fn prompt_history_closes_through_react(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, history_host(), history_modal());
  h.click_backdrop();
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    vec![press("promptHistory.close"), press("promptHistory.close")]
  );
}

#[gpui::test]
fn prompt_history_says_when_there_is_nothing(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), history_modal());
  assert!(h.drawn("prompt-history-empty"));
  assert!(!h.drawn("prompt-history-copy-all"));
}

#[gpui::test]
fn stash_modal_lists_stashes_with_the_first_diff(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, stash_host(), stash_modal());
  for selector in [
    "stash-modal",
    "stash-list-item-0",
    "stash-list-item-1",
    "stash-detail",
    "stash-diff-pane",
    "stash-diff-file-0",
    "stash-apply-button",
    "stash-more-menu",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(host.calls().contains(&"read stashes".to_string()));
  assert!(host.calls().contains(&"read stash diff 1".to_string()));
  h.click("stash-list-item-1");
  assert!(host.calls().contains(&"read stash diff 2".to_string()));
}

#[gpui::test]
fn stash_modal_applies_a_stash_to_a_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, stash_host(), stash_modal());
  h.click("stash-apply-button");
  for selector in [
    "stash-apply-popover",
    "stash-apply-workspace-select",
    "stash-apply-option-.",
    "stash-apply-option-treq/login",
    "stash-apply-option-treq/docs",
    "stash-apply-new-workspace",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  h.click("stash-apply-option-treq/login");
  h.cx.run_until_parked();
  assert!(!h.drawn("stash-apply-popover"));
  assert_eq!(
    host.writes(),
    [
      "apply stash 1 treq/login",
      "notify workspaceChanged",
      "notify toast Stash applied"
    ]
  );
}

#[gpui::test]
fn stash_modal_applies_to_the_home_repo(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, stash_host(), stash_modal());
  h.click("stash-apply-button");
  h.click("stash-apply-option-.");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"apply stash 1 .".to_string()));
}

#[gpui::test]
fn stash_modal_reports_a_failed_apply(cx: &mut TestAppContext) {
  let host = FakeHost {
    apply_error: Some("conflict".into()),
    ..stash_host()
  };
  let (mut h, host) = modal_harness(cx, host, stash_modal());
  h.click("stash-apply-button");
  h.click("stash-apply-option-treq/docs");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "apply stash 1 treq/docs",
      "notify toast Failed to apply stash"
    ]
  );
}

#[gpui::test]
fn stash_modal_applies_to_a_new_workspace_in_react(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, stash_host(), stash_modal());
  h.click("stash-apply-button");
  h.click("stash-apply-new-workspace");
  h.cx.run_until_parked();
  let sent = fields(&h, "stash.applyToNew");
  assert_eq!(sent.len(), 1);
  assert_eq!(sent[0]["id"], 1);
  assert_eq!(sent[0]["short_commit_id"], "abc1");
  assert!(!h.drawn("stash-apply-popover"));
}

#[gpui::test]
fn stash_modal_copies_a_patch_and_deletes_a_stash(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, stash_host(), stash_modal());
  h.click("stash-more-menu");
  h.click("stash-copy-patch");
  h.cx.run_until_parked();
  assert_eq!(h.clipboard().as_deref(), Some("patch of stash 1"));
  h.click("stash-more-menu");
  h.click("stash-delete");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"delete stash 1".to_string()));
  assert!(host
    .writes()
    .contains(&"notify toast Stash deleted".to_string()));
  // The list reloads and the next stash is selected.
  assert!(h.drawn("stash-list-item-0"));
  assert!(!h.drawn("stash-list-item-1"));
  let selected = h
    .screen
    .read_with(&h.cx, |s, cx| s.stash.as_ref().map(|v| v.read(cx).selected));
  assert_eq!(selected, Some(Some(2)));
}

#[gpui::test]
fn stash_modal_explains_an_empty_list(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), stash_modal());
  assert!(h.drawn("stash-empty"));
  assert!(!h.drawn("stash-list"));
}

fn agent_view<T>(h: &Harness, f: impl FnOnce(&modal::agent_prompt::AgentPromptView) -> T) -> T {
  h.screen.read_with(&h.cx, |s, cx| {
    f(s.agent_prompt.as_ref().expect("agent prompt").read(cx))
  })
}

#[gpui::test]
fn agent_prompt_starts_with_what_react_sent(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  for selector in [
    "agent-prompt-dialog",
    "agent-prompt-workspace",
    "agent-task-input",
    "task-input",
    "task-add",
    "task-agent",
    "task-submit",
    "task-submit-menu",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert_eq!(agent_view(&h, |v| v.selected), Some(7));
  assert_eq!(agent_view(&h, |v| v.task_text.clone()), "fix it");
  assert_eq!(
    agent_view(&h, |v| v.agents()),
    (Agent::Claude, Agent::Claude)
  );
}

#[gpui::test]
fn agent_prompt_starts_the_session_in_the_backend(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.click("task-submit");
  // Until the dialog closes, the prompt is not sent twice.
  h.click("task-submit");
  assert_eq!(
    tasks(&mut h, &host),
    vec![launch_data::TaskRequest {
      repo_path: "/repo".into(),
      workspace_id: Some(7),
      text: "fix it".into(),
      issue: None,
      agent: "claude".into(),
      mode: "acceptEdits".into(),
      save_as_repo_default: false,
    }]
  );
  // Started: React closes the dialog; nothing else goes to React.
  assert_eq!(h.sent(), vec![press("agentPrompt.close")]);
  assert!(agent_view(&h, |v| v.submitting()));
}

#[gpui::test]
fn agent_prompt_toasts_a_failed_start_and_allows_another(cx: &mut TestAppContext) {
  let host = FakeHost {
    task_error: Some("jj failed".into()),
    ..FakeHost::default()
  };
  let (mut h, host) = modal_harness(cx, host, agent_modal());
  h.click("task-submit");
  assert_eq!(tasks(&mut h, &host).len(), 1);
  assert!(host
    .calls()
    .contains(&"notify toast Failed to create task".to_string()));
  assert!(h.sent().is_empty());
  assert!(!agent_view(&h, |v| v.submitting()));
}

#[gpui::test]
fn agent_prompt_plans_instead_of_editing(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.click("task-submit-menu");
  h.click("task-plan");
  assert_eq!(tasks(&mut h, &host)[0].mode, "plan");
}

#[gpui::test]
fn agent_prompt_runs_the_default_agent_unless_changed(cx: &mut TestAppContext) {
  let host = FakeHost::with_settings(&[("/repo:default_agent", "codex")]);
  let (mut h, host) = modal_harness(cx, host, agent_modal());
  assert_eq!(agent_view(&h, |v| v.agents()), (Agent::Codex, Agent::Codex));
  // Codex has no plan mode.
  assert!(!h.drawn("task-submit-menu"));
  h.click("task-agent");
  assert!(h.drawn("task-agent-menu"));
  h.click("task-agent-cursor");
  assert_eq!(
    agent_view(&h, |v| v.agents()),
    (Agent::Codex, Agent::Cursor)
  );
  assert!(h.drawn("task-save-default-box"));
  assert!(h.drawn("task-submit-menu"));
  h.click("task-save-default-box");
  h.click("task-submit");
  let start = &tasks(&mut h, &host)[0];
  assert_eq!(start.agent, "cursor");
  assert!(start.save_as_repo_default);
}

#[gpui::test]
fn agent_prompt_picks_a_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.click("agent-prompt-workspace");
  assert!(h.drawn("agent-prompt-workspace-popover"));
  assert!(h.drawn("agent-prompt-workspace-main"));
  assert!(h.drawn("agent-prompt-workspace-treq/login"));
  h.click("agent-prompt-workspace-main");
  assert_eq!(agent_view(&h, |v| v.selected), None);
  assert!(!h.drawn("agent-prompt-workspace-popover"));
  h.click("task-submit");
  assert_eq!(tasks(&mut h, &host)[0].workspace_id, None);
}

#[gpui::test]
fn agent_prompt_runs_an_attached_issue_in_its_own_workspace(cx: &mut TestAppContext) {
  let mut modal = agent_modal();
  modal["initialPrompt"] = serde_json::Value::Null;
  modal["initialWorkspaceId"] = serde_json::Value::Null;
  modal["issue"] = serde_json::json!({
    "source": "linear",
    "key": "ENG-101",
    "title": "Login is slow",
    "includeSubItems": true,
    "id": "issue-1",
    "url": "https://linear.app/t/ENG-101",
    "subItemIds": ["child-1"],
  });
  let (mut h, host) = modal_harness(cx, FakeHost::default(), modal);
  assert!(h.drawn("agent-prompt-issue-note"));
  assert!(h.drawn("issue-chip"));
  assert!(!h.drawn("agent-prompt-workspace"));
  // An issue alone is enough to start.
  h.click("task-submit");
  let start = &tasks(&mut h, &host)[0];
  assert_eq!(start.text, "");
  assert_eq!(
    start.issue,
    Some(crate::core::agent_launch::IssueAttachment {
      source: crate::core::agent_launch::IssueSource::Linear,
      id: "issue-1".into(),
      key: "ENG-101".into(),
      url: "https://linear.app/t/ENG-101".into(),
      title: "Login is slow".into(),
      include_sub_items: true,
      sub_item_ids: vec!["child-1".into()],
    })
  );
}

#[gpui::test]
fn agent_prompt_detaches_an_issue(cx: &mut TestAppContext) {
  let mut modal = agent_modal();
  modal["issue"] = serde_json::json!({
    "source": "github",
    "key": "#42",
    "title": "Bug",
    "includeSubItems": false,
  });
  let (mut h, host) = modal_harness(cx, FakeHost::default(), modal);
  h.click("issue-chip-remove");
  assert!(!h.drawn("issue-chip"));
  assert!(h.drawn("agent-prompt-workspace"));
  h.click("task-submit");
  assert_eq!(tasks(&mut h, &host)[0].issue, None);
}

#[gpui::test]
fn agent_prompt_suggests_files_for_mentions(cx: &mut TestAppContext) {
  let host = FakeHost {
    files: vec!["src/login.rs".into(), "src/logout.rs".into()],
    ..FakeHost::default()
  };
  let mut modal = agent_modal();
  modal["initialPrompt"] = serde_json::Value::Null;
  let (mut h, _) = modal_harness(cx, host, modal);
  h.click("agent-task-input");
  h.cx.simulate_input("look at @log");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(500));
  assert!(h.drawn("task-mention-0"));
  h.click("task-mention-1");
  assert_eq!(
    agent_view(&h, |v| v.task_text.clone()),
    "look at @src/logout.rs "
  );
}

#[gpui::test]
fn agent_prompt_waits_while_react_starts_the_session(cx: &mut TestAppContext) {
  let mut modal = agent_modal();
  modal["starting"] = true.into();
  let (mut h, host) = modal_harness(cx, FakeHost::default(), modal);
  h.click("task-submit");
  assert!(tasks(&mut h, &host).is_empty());
  assert!(agent_view(&h, |v| v.submitting()));
}

#[gpui::test]
fn agent_prompt_keeps_its_text_while_react_refreshes(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.click("agent-task-input");
  h.cx.simulate_input(" now");
  let mut refreshed = agent_modal();
  refreshed["workspaces"] = serde_json::json!([]);
  h.set_modal(Some(refreshed));
  // The workspace went away; the text stays.
  assert_eq!(agent_view(&h, |v| v.selected), None);
  assert!(agent_view(&h, |v| v.task_text.contains("now")));
}

#[gpui::test]
fn agent_prompt_closes_through_react(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.click_backdrop();
  assert_eq!(h.sent(), vec![press("agentPrompt.close")]);
}

#[gpui::test]
fn dialogs_cover_the_screen_until_closed(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), history_modal());
  assert!(h.drawn("prompt-history-modal"));
  assert!(h.drawn("onboarding-open-repo"));
  h.set_modal(None);
  assert!(!h.drawn("prompt-history-modal"));
  // Another kind replaces the first.
  h.set_modal(Some(stash_modal()));
  assert!(h.drawn("stash-modal"));
}

// -- The remote dialogs ------------------------------------------------------

#[gpui::test]
fn setup_dialog_opens_on_your_own_vm_from_the_ssh_hosts_plus(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    serde_json::json!({ "setup": { "allowUserManaged": true, "screen": "ownVm" } }),
  );
  assert!(h.drawn("remote-username"));
  assert!(!h.drawn("remote-setup-own-vm"));
  // An action pair and no X; Cancel closes, as there is no choice to go
  // back to.
  assert!(!h.drawn("remote-setup-close"));
  assert!(!h.drawn("remote-setup-back"));
  h.click("remote-setup-cancel");
  assert_eq!(h.sent(), vec![press("remote.setup.close")]);
}

#[gpui::test]
fn setup_dialog_asks_to_sign_in_before_the_cloud_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(
    cx,
    FakeHost::default(),
    serde_json::json!({ "setup": { "allowUserManaged": true, "screen": "managed" } }),
  );
  // The machines page's cloud row opens straight on the cloud workspace.
  assert!(h.drawn("cloud-create"));
  assert!(!h.drawn("remote-setup-own-vm"));
  h.set_account(account_state(serde_json::json!({})));
  assert!(h.drawn("remote-setup-signed-out"));
  assert!(!h.drawn("cloud-create"));
  h.click("remote-setup-sign-in");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"open http://localhost:3001/sign-in?source=desktop".to_string()));
  h.set_account(account_state(serde_json::json!({ "user": ada() })));
  assert!(h.drawn("cloud-create"));
}

fn ssh_key(label: &str) -> crate::core::remote_local_keys::LocalSshIdentity {
  crate::core::remote_local_keys::LocalSshIdentity {
    reference: format!("/home/me/.ssh/{label}.pub"),
    label: label.into(),
    fingerprint_sha256: "SHA256:key".into(),
    algorithm: "ssh-ed25519".into(),
  }
}

fn endpoint_json() -> serde_json::Value {
  serde_json::json!({
    "id": "user-managed-1",
    "instance_id": null,
    "source": { "type": "user_managed" },
    "hostname": "devbox.example",
    "port": 22,
    "username": "me",
    "host_keys": [],
    "authentication": { "type": "public_key", "key_reference": "/home/me/.ssh/id_ed25519.pub" },
  })
}

fn terminal_remote_route() -> serde_json::Value {
  serde_json::json!({
    "terminal": {
      "endpoint": endpoint_json(),
      "repositoryId": "/srv/project",
      "workspaceId": "root",
    }
  })
}

fn remote_session(
  label: &str,
  running: bool,
) -> crate::core::pty_remote_supervisor::PtySessionInfo {
  crate::core::pty_remote_supervisor::PtySessionInfo {
    session_name: format!("treq-pty-{label}"),
    workspace: "root".into(),
    label: label.into(),
    running,
    title: None,
    idle_secs: None,
  }
}

#[test]
fn parses_the_remote_dialogs_from_the_snapshot() {
  let snapshot = remote_snapshot(serde_json::json!({
    "setup": { "allowUserManaged": true, "registering": true },
    "ambiguous": "connection reset",
  }));
  let setup = snapshot.remote.setup.as_ref().expect("setup");
  assert!(setup.allow_user_managed && setup.registering);
  assert!(snapshot.remote.is_open());
  assert!(!remote_snapshot(serde_json::json!({})).remote.is_open());
  // An empty reason is no reason, as `Boolean(reason)` reads it.
  assert!(!remote_snapshot(serde_json::json!({ "ambiguous": "" }))
    .remote
    .is_open());
  let terminal = remote_snapshot(terminal_remote_route());
  assert_eq!(
    terminal
      .remote
      .terminal
      .expect("terminal")
      .endpoint
      .hostname,
    "devbox.example"
  );
}

#[gpui::test]
fn setup_dialog_offers_both_paths_and_goes_back(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    setup_route(true, serde_json::json!({})),
  );
  assert!(h.drawn("remote-setup-dialog"));
  assert!(h.drawn("remote-setup-managed"));
  assert!(h.drawn("remote-setup-own-vm"));
  assert!(!h.drawn("remote-setup-back"));
  h.click("remote-setup-managed");
  assert!(h.drawn("cloud-create"));
  assert!(!h.drawn("remote-setup-managed"));
  h.click("remote-setup-back");
  assert!(h.drawn("remote-setup-managed"));
  assert!(!h.drawn("cloud-create"));
}

#[gpui::test]
fn setup_dialog_opens_on_the_cloud_workspace_without_the_preview(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    setup_route(false, serde_json::json!({})),
  );
  assert!(h.drawn("cloud-create"));
  assert!(!h.drawn("remote-setup-managed"));
  // Nothing to go back to.
  assert!(!h.drawn("remote-setup-back"));
}

#[gpui::test]
fn setup_dialog_runs_the_cloud_workspace_flows_in_rust(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(
    cx,
    FakeHost::default(),
    setup_route(false, serde_json::json!({})),
  );
  // Opening it reads the cloud workspace afresh.
  assert!(host.calls().contains(&"read cloud workspace".to_string()));
  h.click("cloud-create");

  let failed = serde_json::json!({
    "instance": { "status": "failed", "providerResourceId": "machine-1", "diskQuotaGb": 10 },
    "endpoint": { "username": "treq", "hostname": "vm.example", "port": 22 },
  });
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "cloud": failed }),
  ));
  h.click("cloud-repair");
  // Deleting asks first, in a confirmation of its own.
  h.click("cloud-delete");
  assert!(h.drawn("cloud-delete-dialog"));
  h.click("cloud-delete-dialog-cancel");
  assert!(!h.drawn("cloud-delete-dialog"));
  h.click("cloud-delete");
  h.click("cloud-delete-dialog-confirm");
  assert!(!h.drawn("cloud-delete-dialog"));

  let ready = serde_json::json!({
    "instance": { "status": "ready", "providerResourceId": "machine-1", "diskQuotaGb": 10 },
    "endpoint": { "username": "treq", "hostname": "vm.example", "port": 22 },
  });
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "cloud": ready }),
  ));
  h.click("cloud-open-repositories");
  assert!(h.sent().is_empty());

  let pro = serde_json::json!({ "provisioningError": "[pro_required] Upgrade to create one." });
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "cloud": pro }),
  ));
  h.click("cloud-upgrade");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert_eq!(
    writes[..4],
    [
      "cloud Provision",
      "cloud Repair",
      "cloud Delete",
      "cloud OpenRepositories"
    ]
  );
  assert_eq!(writes.len(), 5);
  assert!(writes[4].starts_with("open ") && writes[4].ends_with("/dashboard"));
}

#[gpui::test]
fn setup_dialog_closes_from_its_backdrop_only(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    setup_route(true, serde_json::json!({})),
  );
  assert!(h.drawn("remote-setup-dialog"));
  // A click in the panel stays in the dialog.
  h.click("remote-setup-dialog");
  assert!(h.sent().is_empty());
  h.cx
    .simulate_click(gpui::point(gpui::px(5.), gpui::px(5.)), Modifiers::none());
  assert_eq!(h.sent(), vec![press("remote.setup.close")]);
}

fn own_vm_host() -> FakeHost {
  FakeHost {
    ssh_keys: vec![ssh_key("id_rsa"), ssh_key("id_ed25519")],
    ..FakeHost::default()
  }
}

#[gpui::test]
fn user_managed_form_confirms_the_host_key_before_registering(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(cx, own_vm_host(), setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  assert!(h.drawn("remote-hostname"));
  // Continue waits for a hostname and a username; the key is the default.
  h.click("remote-continue");
  assert!(host.writes().is_empty());
  type_into(&mut h, "remote-hostname", " devbox.example ");
  h.click("remote-continue");
  assert!(host.writes().is_empty());
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["probe devbox.example:22"]);
  assert!(h.drawn("remote-trust-confirmation"));
  // Nothing is registered until trust is confirmed, and Continue is gone.
  assert!(h.sent().is_empty());
  assert!(!h.drawn("remote-continue"));

  h.click("remote-trust-connect");
  assert_eq!(
    fields(&h, "remote.registerUserManaged"),
    vec![serde_json::json!({
      "display_name": "devbox.example",
      "hostname": "devbox.example",
      "port": 22,
      "username": "me",
      "host_key_fingerprint": "SHA256:abc123",
      "auth_identity_reference": "/home/me/.ssh/id_ed25519.pub",
      "alias": null,
    })]
  );
}

#[gpui::test]
fn user_managed_form_can_decline_the_host_key(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(cx, own_vm_host(), setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  type_into(&mut h, "remote-display-name", "Dev box");
  type_into(&mut h, "remote-hostname", "10.0.0.4");
  type_into(&mut h, "remote-username", "me");
  // Typed after the default 22.
  type_into(&mut h, "remote-port", "22");
  h.click("remote-continue");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["probe 10.0.0.4:2222"]);
  h.click("remote-trust-cancel");
  assert!(!h.drawn("remote-trust-confirmation"));
  assert!(h.drawn("remote-continue"));
  assert!(h.sent().is_empty());
  h.click("remote-continue");
  h.click("remote-trust-connect");
  let values = fields(&h, "remote.registerUserManaged");
  assert_eq!(values[0]["display_name"], "Dev box");
  assert_eq!(values[0]["port"], 2222);
}

#[gpui::test]
fn user_managed_form_shows_why_the_host_key_failed(cx: &mut TestAppContext) {
  let host = FakeHost {
    host_key: Some(Err("connection refused".into())),
    ..own_vm_host()
  };
  let (mut h, _) = remote_harness(cx, host, setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  type_into(&mut h, "remote-hostname", "10.0.0.4");
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  assert!(h.drawn("remote-probe-error"));
  assert!(!h.drawn("remote-trust-confirmation"));
  assert!(h.drawn("remote-continue"));
}

#[gpui::test]
fn user_managed_form_fills_from_an_ssh_alias(cx: &mut TestAppContext) {
  let host = FakeHost {
    resolved_alias: Some(Ok(crate::core::remote_ssh_config::ResolvedSshAlias {
      alias: "prod".into(),
      hostname: "prod.example".into(),
      port: 2200,
      username: Some("deploy".into()),
      identity_file: None,
    })),
    ..own_vm_host()
  };
  let (mut h, host) = remote_harness(cx, host, setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  assert!(!h.drawn("remote-alias"));
  h.click("remote-alias-toggle");
  assert!(h.drawn("remote-alias"));
  h.click("remote-alias-autofill");
  // Needs an alias first.
  assert!(host.writes().is_empty());
  type_into(&mut h, "remote-alias", "prod");
  h.click("remote-alias-autofill");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["resolve prod"]);
  h.click("remote-continue");
  h.cx.run_until_parked();
  // The alias only filled the fields: the host key is still confirmed.
  assert_eq!(host.writes(), ["resolve prod", "probe prod.example:2200"]);
  h.click("remote-trust-connect");
  let values = fields(&h, "remote.registerUserManaged");
  assert_eq!(values[0]["username"], "deploy");
  assert_eq!(values[0]["alias"], "prod");
}

#[gpui::test]
fn user_managed_form_is_locked_while_react_registers(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(cx, own_vm_host(), setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  type_into(&mut h, "remote-hostname", "10.0.0.4");
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  h.click("remote-trust-connect");
  // Back is disabled until React is done.
  h.click("remote-setup-back");
  assert!(h.drawn("remote-hostname"));
  for registering in [true, false] {
    h.update(|s| s.remote.setup.as_mut().expect("setup").registering = registering);
  }
  h.click("remote-setup-back");
  assert!(h.drawn("remote-setup-managed"));
}

#[gpui::test]
fn terminal_dialog_lists_sessions_to_reattach(cx: &mut TestAppContext) {
  let host = FakeHost {
    remote_sessions: Some(Ok(vec![
      crate::core::pty_remote_supervisor::PtySessionInfo {
        title: Some("Fix the login".into()),
        ..remote_session("claude-1", true)
      },
      remote_session("shell-2", false),
    ])),
    ..FakeHost::default()
  };
  let (mut h, host) = remote_harness(cx, host, terminal_remote_route());
  assert!(h.drawn("remote-terminal-dialog"));
  assert_eq!(host.writes(), ["sessions devbox.example:/srv/project:root"]);
  assert!(h.drawn("remote-reattach-1"));
  // A stopped session cannot be reattached.
  h.click("remote-reattach-1");
  assert!(h.sent().is_empty());
  h.click("remote-reattach-0");
  h.click("remote-start-new");
  assert_eq!(
    fields(&h, "remote.terminal.open"),
    vec![
      serde_json::json!({ "label": "claude-1", "title": "Fix the login", "reattach": true }),
      serde_json::json!({ "label": null, "title": null, "reattach": false }),
    ]
  );
}

#[gpui::test]
fn terminal_dialog_starts_a_session_when_listing_fails(cx: &mut TestAppContext) {
  let host = FakeHost {
    remote_sessions: Some(Err("ssh: connect refused".into())),
    ..FakeHost::default()
  };
  let (mut h, _) = remote_harness(cx, host, terminal_remote_route());
  assert!(h.drawn("remote-terminal-error"));
  assert!(!h.drawn("remote-reattach-0"));
  h.click("remote-start-new");
  assert_eq!(
    fields(&h, "remote.terminal.open"),
    vec![serde_json::json!({ "label": null, "title": null, "reattach": false })]
  );
}

#[gpui::test]
fn terminal_dialog_lists_again_for_another_workspace(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(cx, FakeHost::default(), terminal_remote_route());
  assert!(h.drawn("remote-start-new"));
  h.update(|s| s.remote.terminal.as_mut().expect("terminal").workspace_id = "feature".into());
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "sessions devbox.example:/srv/project:root",
      "sessions devbox.example:/srv/project:feature"
    ]
  );
  h.click("remote-terminal-dialog");
  assert!(h.sent().is_empty());
  h.cx
    .simulate_click(gpui::point(gpui::px(5.), gpui::px(5.)), Modifiers::none());
  assert_eq!(h.sent(), vec![press("remote.terminal.close")]);
}

#[gpui::test]
fn ambiguous_dialog_dismisses_or_refreshes(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    serde_json::json!({ "ambiguous": "Connection reset while pushing" }),
  );
  assert!(h.drawn("remote-ambiguous-dialog"));
  assert!(h.drawn("remote-ambiguous-reason"));
  h.click("remote-ambiguous-dismiss");
  h.click("remote-ambiguous-refresh");
  assert_eq!(
    h.sent(),
    vec![
      press("remote.ambiguous.dismiss"),
      press("remote.ambiguous.refresh")
    ]
  );
  h.update(|s| s.remote.ambiguous = None);
  assert!(!h.drawn("remote-ambiguous-dialog"));
}

#[gpui::test]
fn dialogs_stack_in_react_order(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    serde_json::json!({
      "setup": { "allowUserManaged": true, "cloud": {} },
      "ambiguous": "Connection reset",
    }),
  );
  // The later dialog is above the earlier: its buttons take the click.
  h.click("remote-ambiguous-dismiss");
  assert_eq!(h.sent(), vec![press("remote.ambiguous.dismiss")]);
}

#[gpui::test]
fn user_managed_form_suggests_ssh_config_aliases(cx: &mut TestAppContext) {
  let (mut h, host) = remote_harness(cx, own_vm_host(), setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  assert!(!h.drawn("remote-hostname-suggestion-0"));
  type_into(&mut h, "remote-hostname", "DEV");
  h.cx.run_until_parked();
  // Aliases that contain what is typed, whatever the case; "staging" does not.
  assert!(h.drawn("remote-hostname-suggestion-0"));
  assert!(!h.drawn("remote-hostname-suggestion-1"));
  h.click("remote-hostname-suggestion-0");
  // Picking one only fills the field: nothing is probed or trusted.
  assert!(!h.drawn("remote-hostname-suggestion-0"));
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["probe devbox:22"]);
}

/// `open-non-git-folder.test.tsx`: the window model offers to initialize
/// a folder without `.git` first (see the window tests); one that still
/// has none toasts, creates no `.treq` and remembers nothing.
#[test]
fn a_folder_that_is_not_a_repository_is_not_opened() {
  let dir = tempfile::TempDir::new().unwrap();
  let path = dir.path().to_str().unwrap();
  let host = FakeHost::default();
  let mut shown = Vec::new();

  onboarding::open_repo(&host, path, |p| shown.push(p.to_string()));

  assert_eq!(host.calls(), ["notify toast Not a Git repository"]);
  assert!(shown.is_empty(), "not the window's repository");
  assert!(!dir.path().join(".treq").exists());
  assert!(!dir.path().join(".git").exists());
}

#[test]
fn opening_a_repository_remembers_it_and_reports_it_open() {
  let dir = tempfile::TempDir::new().unwrap();
  std::fs::create_dir(dir.path().join(".git")).unwrap();
  let path = dir.path().to_str().unwrap();
  let host = FakeHost::default();
  let mut shown = Vec::new();

  onboarding::open_repo(&host, path, |p| shown.push(p.to_string()));

  assert_eq!(
    host.calls(),
    [
      format!("set last_opened_repo_path={path}"),
      "notify repoOpened".into(),
      format!("notify toast Opened {}", path.rsplit('/').next().unwrap()),
    ]
  );
  assert_eq!(shown, [path]);
}

// -- ui::Dialog behaviour: Escape closes, Enter runs the verb, and a dialog
// with no action pair closes from its X.

#[gpui::test]
fn prompt_history_closes_from_its_x(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, history_host(), history_modal());
  h.click("prompt-history-modal-close");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![press("promptHistory.close")]);
}

#[gpui::test]
fn stash_modal_closes_on_escape_and_from_its_x(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, stash_host(), stash_modal());
  assert!(h.drawn("stash-modal"));
  h.cx.simulate_keystrokes("escape");
  h.click("stash-modal-close");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![press("stash.close"), press("stash.close")]);
}

#[gpui::test]
fn agent_prompt_closes_on_escape_and_from_its_x(cx: &mut TestAppContext) {
  let (mut h, _) = modal_harness(cx, FakeHost::default(), agent_modal());
  h.cx.simulate_keystrokes("escape");
  h.click("agent-prompt-dialog-close");
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    vec![press("agentPrompt.close"), press("agentPrompt.close")]
  );
}

#[gpui::test]
fn setup_dialog_closes_on_escape(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    setup_route(true, serde_json::json!({})),
  );
  assert!(h.drawn("remote-setup-dialog"));
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![press("remote.setup.close")]);
}

#[gpui::test]
fn terminal_dialog_closes_on_escape(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(cx, FakeHost::default(), terminal_remote_route());
  assert!(h.drawn("remote-start-new"));
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![press("remote.terminal.close")]);
}

#[gpui::test]
fn ambiguous_dialog_dismisses_on_escape_and_refreshes_on_enter(cx: &mut TestAppContext) {
  let (mut h, _) = remote_harness(
    cx,
    FakeHost::default(),
    serde_json::json!({ "ambiguous": "Connection reset while pushing" }),
  );
  assert!(h.drawn("remote-ambiguous-dialog"));
  h.cx.simulate_keystrokes("escape");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert_eq!(
    h.sent(),
    vec![
      press("remote.ambiguous.dismiss"),
      press("remote.ambiguous.refresh")
    ]
  );
}
