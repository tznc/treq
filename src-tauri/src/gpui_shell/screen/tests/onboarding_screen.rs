use gpui::TestAppContext;

use super::*;

#[test]
fn builds_the_checklist_from_the_check() {
  let list = checklist::from_statuses("Before you start", None, true);
  assert!(list.validating);
  assert_eq!(list.groups.len(), 3);
  // Nothing is reported missing before the first check returns.
  assert!(!list.groups[1].error);
  assert!(list
    .groups
    .iter()
    .flat_map(|g| &g.tools)
    .all(|t| t.installed.is_none()));

  let statuses = [("git", false), ("claude", false), ("gh", true)].map(|(binary, installed)| {
    crate::core::PrerequisiteStatus {
      binary: binary.into(),
      installed,
    }
  });
  let list = checklist::from_statuses("Before you start", Some(&statuses), false);
  let [required, agents, optional] = &list.groups[..] else {
    panic!("three groups");
  };
  assert_eq!(required.label, "Required");
  assert!(required.tools[0].required);
  assert_eq!(required.tools[0].installed, Some(false));
  assert!(agents.error, "no agent CLI is installed");
  assert_eq!(agents.tools.len(), 4);
  assert_eq!(optional.tools[0].installed, Some(true));
  assert_eq!(
    optional.tools[0].note.as_deref(),
    Some("Needed for pull requests and issues")
  );
  // The binaries are the ones the backend checks, in its order.
  let binaries: Vec<_> = list
    .groups
    .iter()
    .flat_map(|g| &g.tools)
    .map(|t| t.binary.as_str())
    .collect();
  assert_eq!(binaries, crate::core::PREREQUISITE_BINARIES);
}

#[test]
fn serializes_actions_as_react_reads_them() {
  let json = serde_json::to_value(ScreenAction::OpenUrl {
    url: "https://cli.github.com/".into(),
  })
  .expect("serialize");
  assert_eq!(
    json,
    serde_json::json!({"kind": "openUrl", "url": "https://cli.github.com/"})
  );
  assert_eq!(
    serde_json::to_value(ScreenAction::RecheckPrerequisites).expect("serialize"),
    serde_json::json!({"kind": "recheckPrerequisites"})
  );
}

#[gpui::test]
fn the_window_models_file_picker_reports_a_dismissal(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, workspace_route());
  let scope = file_picker::Scope {
    repo_path: "/repo".into(),
    workspace_id: None,
    workspace_path: "/repo".into(),
  };
  h.screen
    .update(&mut h.cx, |s, cx| s.set_file_picker(Some(scope), cx));
  assert!(h.drawn("file-picker"));
  h.click_backdrop();
  assert_eq!(
    h.sent().last(),
    Some(&ScreenAction::Press {
      id: "filePicker.close".into()
    })
  );
  h.screen
    .update(&mut h.cx, |s, cx| s.set_file_picker(None, cx));
  assert!(!h.drawn("file-picker"));
}

#[gpui::test]
fn the_ui_zoom_scales_pixel_lengths_as_css_zoom_does(cx: &mut TestAppContext) {
  use crate::gpui_shell::ui::{apply_zoom, UiZoom};
  let mut card = |zoom: u32| {
    let mut h = Harness::new(cx, onboarding());
    h.cx.update(|window, cx| {
      cx.set_global(UiZoom(zoom));
      apply_zoom(window, cx);
      window.refresh();
    });
    h.cx.run_until_parked();
    h.cx
      .debug_bounds("onboarding-open-repo")
      .expect("card")
      .size
  };
  let (normal, zoomed) = (card(100), card(150));
  assert!(
    (f32::from(zoomed.height) - f32::from(normal.height) * 1.5).abs() < 1.,
    "{normal:?} {zoomed:?}"
  );
}

#[gpui::test]
fn onboarding_runs_its_own_actions(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    installed: vec![("git", false), ("gh", true)],
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  // This computer's tools are checked as the screen opens, shown in its
  // popover.
  assert!(!h.drawn("install-git"));
  h.click("machine-tools-local");
  assert!(h.drawn("install-git"));
  assert!(!h.drawn("install-gh"));
  h.click("prerequisites-recheck");
  h.click("install-git");
  h.click("onboarding-open-repo");
  h.click("recent-local-0");
  assert_eq!(
    host.calls(),
    [
      "check",
      "check",
      "open https://git-scm.com/downloads",
      "open_local /home/me/app",
    ]
  );
  // The window model asks for the folder.
  assert_eq!(h.sent(), vec![ScreenAction::OpenRepo]);
}

#[gpui::test]
fn onboarding_groups_recent_repositories_by_machine(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  for selector in [
    "onboarding-machines",
    "machine-local",
    "machine-ssh:ep-1",
    "machine-cloud",
    "machine-devcontainer:c1",
    "recent-local-0",
    "recent-ssh:ep-1-0",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(!h.drawn("onboarding-no-recents"));
  // A remote repository opens through the window, by its saved record; a
  // machine's row connects to it.
  h.click("recent-ssh:ep-1-0");
  h.click("machine-ssh:ep-1");
  // A dev container opens its repository here.
  h.click("machine-devcontainer:c1");
  h.click("machine-local");
  // Before its workspace runs, the cloud row sets it up.
  h.click("machine-cloud");
  assert_eq!(
    h.sent(),
    vec![
      press("machines.openSaved:r1"),
      press("machines.connect:ep-1"),
      ScreenAction::OpenRepo,
      press("machines.cloud"),
    ]
  );
  assert_eq!(host.calls(), ["check", "open_local /home/me/web"]);
}

#[gpui::test]
fn onboarding_adds_the_dev_containers_docker_lists_to_its_rows(cx: &mut TestAppContext) {
  use machines::{Machine, Place, Recent};
  let mut base = listing();
  base
    .machines
    .retain(|m| !matches!(m.place, Place::DevContainer { .. }));
  base.containers = true;
  let host = Arc::new(FakeHost {
    listing: base,
    containers: (
      vec![Machine {
        id: "devcontainer:c2".into(),
        place: Place::DevContainer {
          container_id: "c2".into(),
          repo_path: "/home/me/api".into(),
          running: true,
        },
        name: "api".into(),
        detail: "Running".into(),
        terminals: None,
      }],
      vec![Recent {
        machine_id: "devcontainer:c2".into(),
        name: "api".into(),
        path: "/home/me/api".into(),
        saved_id: None,
      }],
    ),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host);
  assert!(h.drawn("machine-local"));
  assert!(h.drawn("machine-devcontainer:c2"));
  assert!(!h.drawn("machine-devcontainer:c1"));
}

#[gpui::test]
fn onboarding_offers_sign_in_instead_of_a_cloud_row_when_signed_out(cx: &mut TestAppContext) {
  let mut signed_out = listing();
  signed_out.machines.retain(|m| m.id != "cloud");
  signed_out.signed_out = true;
  let host = Arc::new(FakeHost {
    listing: signed_out,
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  assert!(!h.drawn("machine-cloud"));
  h.click("onboarding-sign-in");
  assert!(host
    .calls()
    .contains(&"open http://localhost:3001/sign-in?source=desktop".to_string()));
  let mut h = Harness::with_host(
    cx,
    onboarding(),
    Arc::new(FakeHost {
      listing: listing(),
      ..FakeHost::default()
    }),
  );
  assert!(
    !h.drawn("onboarding-sign-in"),
    "signed in: the cloud row instead"
  );
  assert!(h.drawn("machine-cloud"));
}

#[gpui::test]
fn onboarding_checks_a_host_only_when_its_popover_opens(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    host_tools: Some(Ok(vec![
      crate::core::PrerequisiteStatus {
        binary: "git".into(),
        installed: true,
      },
      crate::core::PrerequisiteStatus {
        binary: "tmux".into(),
        installed: false,
      },
    ])),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  assert_eq!(host.calls(), ["check"]);
  h.click("machine-tools-ssh:ep-1");
  assert!(h.drawn("machine-tools-popover"));
  // jj is required on a host and tmux is offered; Git is there.
  assert!(h.drawn("install-tmux"));
  assert!(h.drawn("install-jj"));
  assert!(!h.drawn("install-git"));
  assert!(host.calls().iter().any(|c| c.starts_with("tools Ssh")));
  // Removing the host closes the popover and lists the machines again.
  h.click("machine-remove");
  assert!(!h.drawn("machine-tools-popover"));
  // In-app confirmation first: Cancel keeps the host.
  h.answer("Cancel");
  assert!(!host.calls().iter().any(|c| c.starts_with("remove Ssh")));
  h.click("machine-tools-ssh:ep-1");
  h.click("machine-remove");
  h.answer("Remove host");
  assert!(host.calls().iter().any(|c| c.starts_with("remove Ssh")));
}

#[gpui::test]
fn onboarding_switches_a_hosts_terminals_to_tmux_after_confirming(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    host_tools: Some(Ok(vec![])),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  h.click("machine-tools-ssh:ep-1");
  assert!(h.drawn("machine-terminals"));
  h.click("machine-terminal-backend");
  // Switching closes the host's open terminals, so it asks first.
  h.answer("Cancel");
  assert!(!host.calls().iter().any(|c| c.starts_with("terminals")));
  h.click("machine-tools-ssh:ep-1");
  h.click("machine-terminal-backend");
  h.answer("Use tmux");
  assert!(
    host
      .calls()
      .iter()
      .any(|c| c.starts_with("terminals Ssh") && c.ends_with("Tmux")),
    "{:?}",
    host.calls()
  );
}

/// Opens the SSH host's popover and reads what it says about terminals.
fn host_terminals_text(cx: &mut TestAppContext, host: Arc<FakeHost>) -> (Harness, Vec<String>) {
  use gpui_testing_library::prelude::*;
  let mut h = Harness::with_host(cx, onboarding(), host);
  h.click("machine-tools-ssh:ep-1");
  assert!(h.drawn("machine-terminals-status"));
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  let status: Vec<String> = screen
    .get_all_by_role(by::role(gpui::Role::Status))
    .iter()
    .filter_map(|n| n.name())
    .collect();
  let texts = [
    "Treq daemon running",
    "Treq daemon not running",
    "Couldn't ask the host",
    "tmux",
  ]
  .iter()
  .filter(|t| status.iter().any(|s| s.contains(**t)))
  .map(|t| t.to_string())
  .collect();
  (h, texts)
}

#[gpui::test]
fn onboarding_shows_the_daemon_the_host_reports(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    host_tools: Some(Ok(vec![])),
    terminal_status: Some(Ok(running_daemon(2))),
    ..FakeHost::default()
  });
  let (mut h, texts) = host_terminals_text(cx, host.clone());
  assert!(
    texts.contains(&"Treq daemon running".to_string()),
    "{texts:?}"
  );
  assert!(h.drawn("machine-terminals-status"));
  assert!(host
    .calls()
    .iter()
    .any(|c| c.starts_with("read terminal status Ssh")));
}

#[gpui::test]
fn onboarding_says_when_the_daemon_is_not_running(cx: &mut TestAppContext) {
  let mut status = running_daemon(0);
  status.daemon = None;
  let host = Arc::new(FakeHost {
    listing: listing(),
    host_tools: Some(Ok(vec![])),
    terminal_status: Some(Ok(status)),
    ..FakeHost::default()
  });
  let (_, texts) = host_terminals_text(cx, host);
  assert!(
    texts.contains(&"Treq daemon not running".to_string()),
    "{texts:?}"
  );
}

#[gpui::test]
fn onboarding_says_when_the_host_cannot_report_its_terminals(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    host_tools: Some(Ok(vec![])),
    terminal_status: Some(Err("ssh: connection refused".into())),
    ..FakeHost::default()
  });
  let (_, texts) = host_terminals_text(cx, host);
  assert!(
    texts.contains(&"Couldn't ask the host".to_string()),
    "{texts:?}"
  );
}

#[gpui::test]
fn onboarding_resets_a_hosts_terminals_after_confirming(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    host_tools: Some(Ok(vec![])),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  h.click("machine-tools-ssh:ep-1");
  h.click("machine-reset-terminals");
  h.answer("Cancel");
  assert!(!host
    .calls()
    .iter()
    .any(|c| c.starts_with("reset terminals")));
  h.click("machine-tools-ssh:ep-1");
  h.click("machine-reset-terminals");
  h.answer("Reset terminals");
  assert!(
    host
      .calls()
      .iter()
      .any(|c| c.starts_with("reset terminals Ssh")),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn onboarding_shows_no_terminal_settings_for_this_computer(cx: &mut TestAppContext) {
  let mut h = Harness::with_host(
    cx,
    onboarding(),
    Arc::new(FakeHost {
      listing: listing(),
      ..FakeHost::default()
    }),
  );
  h.click("machine-tools-local");
  assert!(h.drawn("machine-tools-popover"));
  assert!(!h.drawn("machine-terminals"));
}

#[gpui::test]
fn onboarding_says_why_a_host_could_not_be_checked(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host);
  h.click("machine-tools-ssh:ep-1");
  assert!(h.drawn("machine-tools-error"));
}

#[gpui::test]
fn onboarding_without_recents_says_where_they_appear(cx: &mut TestAppContext) {
  let mut h = Harness::with_host(cx, onboarding(), Arc::new(FakeHost::default()));
  assert!(h.drawn("onboarding-no-recents"));
}

fn onboarding_with_form(form: serde_json::Value) -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "onboarding",
    "dark": false,
    "remoteRepo": form,
  }))
  .expect("onboarding")
}

#[gpui::test]
fn onboarding_draws_the_remote_repository_form(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(
    cx,
    onboarding_with_form(serde_json::json!({
      "endpointId": "ep-1",
      "selector": true,
      "saved": [{ "id": "r1", "display_name": "app", "canonical_remote_path": "/srv/app" }],
      "path": "/srv/new",
      "probe": { "is_repo": false, "needs_clone": true },
      "cloneUrl": "",
      "confirmInit": false,
      "error": "probe failed",
    })),
    host,
  );
  for selector in [
    "onboarding-remote-form",
    "remote-repository-selector",
    "remote-browser",
    "remote-repo-not-a-repo",
    "remote-repo-clone-url",
    "remote-repo-init",
    "remote-repo-error",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  // The group lists the saved repositories; the form only opens new ones.
  assert!(!h.drawn("remote-saved-repo"));
  assert!(!h.drawn("remote-repo-open"));
  assert!(!h.drawn("remote-repo-cutoff"));
  // Clone and init already show for the folder.
  assert!(!h.drawn("remote-repo-use-folder"));
  h.click("remote-repo-confirm-init");
  // Initialize waits for the confirmation; Clone for a URL.
  h.click("remote-repo-init");
  h.click("remote-repo-clone");
  assert_eq!(
    h.sent(),
    vec![ScreenAction::Field {
      field: "remoteRepo.confirmInit".into(),
      value: true.into()
    }]
  );
}

fn browsed_form(path: &str, is_repo: bool, show_hidden: bool) -> serde_json::Value {
  serde_json::json!({
    "endpointId": "ep-1",
    "selector": true,
    "path": path,
    "showHidden": show_hidden,
    "listing": {
      "path": path,
      "parent": "/home",
      "is_repo": is_repo,
      "entries": [
        { "name": ".config", "path": format!("{path}/.config"), "hidden": true, "is_repo": false },
        { "name": "app", "path": format!("{path}/app"), "hidden": false, "is_repo": true },
      ],
    },
  })
}

#[gpui::test]
fn the_remote_browser_lists_folders_and_hides_dot_folders(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(
    cx,
    onboarding_with_form(browsed_form("/home/dev", false, false)),
    host,
  );
  assert!(h.drawn("remote-browse-folder-app"));
  assert!(
    !h.drawn("remote-browse-folder-.config"),
    "hidden by default"
  );
  assert!(h.drawn("remote-repo-use-folder"));
  assert!(!h.drawn("remote-repo-open"));
  h.click("remote-browse-folder-app");
  h.click("remote-browse-crumb-1");
  h.click("remote-browse-home");
  h.click("remote-browse-hidden");
  h.click("remote-repo-use-folder");
  assert_eq!(
    h.sent(),
    vec![
      field("remoteRepo.browse", "/home/dev/app".into()),
      field("remoteRepo.browse", "/home".into()),
      field("remoteRepo.browse", "~".into()),
      field("remoteRepo.showHidden", true.into()),
      press("remoteRepo.useFolder"),
    ]
  );
}

#[gpui::test]
fn the_remote_browser_shows_hidden_folders_and_opens_a_repository(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(
    cx,
    onboarding_with_form(browsed_form("/home/dev/app", true, true)),
    host,
  );
  assert!(h.drawn("remote-browse-folder-.config"));
  assert!(!h.drawn("remote-repo-use-folder"));
  h.click("remote-repo-open");
  assert_eq!(h.sent(), vec![press("remoteRepo.open")]);
}

#[gpui::test]
fn onboarding_offers_reauthentication_when_cut_off(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    listing: listing(),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(
    cx,
    onboarding_with_form(serde_json::json!({
      "endpointId": "ep-1",
      "cutoff": "session_ended",
      "selector": true,
    })),
    host,
  );
  assert!(h.drawn("remote-repo-cutoff"));
  assert!(!h.drawn("remote-repository-selector"));
  h.click("remote-repo-reauthenticate");
  assert_eq!(h.sent(), vec![press("remoteRepo.reauthenticate")]);
}

#[gpui::test]
fn onboarding_adds_machines_from_their_sections(cx: &mut TestAppContext) {
  let mut empty = listing();
  empty
    .machines
    .retain(|m| matches!(m.place, machines::Place::Local));
  empty.ssh = true;
  empty.containers = true;
  let host = Arc::new(FakeHost {
    listing: empty,
    picked: Some("/home/me/web".into()),
    container_error: Some("The Dev Containers CLI is not installed.".into()),
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host.clone());
  // Both sections show while empty, each with its +.
  assert!(h.drawn("machines-add-ssh"));
  assert!(h.drawn("machines-add-container"));
  h.click("machines-add-ssh");
  assert_eq!(h.sent(), vec![press("machines.addSsh")]);
  h.click("machines-add-container");
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"container up /home/me/web".to_string()));
  assert!(h.drawn("machines-container-status"), "why it did not start");

  let mut off = listing();
  off
    .machines
    .retain(|m| matches!(m.place, machines::Place::Local));
  let host = Arc::new(FakeHost {
    listing: off,
    ..FakeHost::default()
  });
  let mut h = Harness::with_host(cx, onboarding(), host);
  assert!(!h.drawn("machines-add-ssh"), "the previews are off");
  assert!(!h.drawn("machines-add-container"));
}
