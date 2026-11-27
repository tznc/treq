use gpui::{Modifiers, MouseButton, TestAppContext};

use super::*;

#[gpui::test]
fn terminal_pane_hides_its_chip_without_terminals(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  assert!(!h.shows(gpui::Role::Toolbar, "Terminal pane controls"));
  assert!(!h.shows(gpui::Role::Group, "Terminals"));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  assert!(h.shows(gpui::Role::Group, "Terminals"));
  h.click("terminal-collapse");
  assert!(!h.shows(gpui::Role::Group, "Terminals"));
  h.click("terminal-expand");
  assert!(h.shows(gpui::Role::Group, "Terminals"));
}

#[gpui::test]
fn terminal_pane_opens_the_active_agent_session(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  // The backend kept what the session starts with.
  host.pending.lock().unwrap().insert(
    3,
    launch_data::PendingSession {
      pending_prompt: Some("fix it".into()),
      permission_mode: Some("plan".into()),
      agent: Some("codex".into()),
      workspace_path: None,
    },
  );
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  assert!(h.shows(gpui::Role::Group, "Claude 1 terminal"));
  assert_eq!(active_column(&mut h).as_deref(), Some("Claude 1 terminal"));
  assert!(h.shows(gpui::Role::Button, "Go to Add login"));
  let pty = agent_pty();
  let calls = host.calls();
  assert!(
    calls.contains(&format!("register chat 3 {pty} Claude 1")),
    "{calls:?}"
  );
  assert!(
    calls.contains(&"prepare codex Some(\"fix it\")".to_string()),
    "{calls:?}"
  );
  assert!(
    calls.contains(&format!("spawn {pty} in None running Some(\"codex\")")),
    "{calls:?}"
  );
  let summaries = h.sent().into_iter().find_map(|a| match a {
    ScreenAction::Field { field, value } if field == "terminal.summaries" => Some(value),
    _ => None,
  });
  assert_eq!(
    summaries,
    Some(serde_json::json!([{
      "id": "agent-3",
      "kind": "agent",
      "name": "Claude 1",
      "branchName": "feature/login",
      "repoPath": "/repo",
      "isMainRepo": false,
      "agent": "codex",
      "isStreaming": false,
    }]))
  );
}

/// A supporting repository on screen: the backend numbers its sessions
/// relative to the window's main repository, so the pane must too.
#[gpui::test]
fn terminal_pane_opens_an_agent_session_of_a_supporting_repo(cx: &mut TestAppContext) {
  let host = FakeHost {
    supporting: vec!["/support".into()],
    ..terminal_host()
  };
  let mut route = route_json(Some(7), "main");
  route["repoPath"] = "/support".into();
  route["code"] = true.into();
  route["terminal"] = serde_json::json!({});
  let route = serde_json::from_value(route).expect("terminal route");
  let (mut h, host) = workspace_harness(cx, host, route);
  let pane_id = terminal_data::pane_session_id("/support", "/repo", 3);
  set_terminal(
    &mut h,
    serde_json::json!({ "activeSessionId": pane_id, "mainRepoPath": "/repo" }),
  );
  assert!(h.shows(gpui::Role::Group, "Claude 1 terminal"));
  let pty = terminal_data::agent_pty_id("/support", 3);
  let calls = host.calls();
  assert!(
    calls
      .iter()
      .any(|c| c.starts_with(&format!("spawn {pty} "))),
    "{calls:?}"
  );
}

#[gpui::test]
fn terminal_draws_output_and_types_into_the_pty(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  host.output(&pty, "hello \x1b[32mworld\x1b[0m\r\n$ ");
  h.cx.run_until_parked();
  let host_dyn: terminal_data::Host = host.clone();
  let (live, fresh) = terminal_data::attach(&host_dyn, &pty, 80, 24);
  assert!(!fresh);
  assert_eq!(live.screen_text(), "hello world\n$");
  click_terminal(&mut h);
  h.cx.simulate_input("ls");
  h.cx.simulate_keystrokes("enter ctrl-c");
  let writes: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("write "))
    .collect();
  assert_eq!(
    writes,
    vec![
      format!("write {pty} \"l\""),
      format!("write {pty} \"s\""),
      format!("write {pty} \"\\r\""),
      format!("write {pty} \"\\u{{3}}\""),
    ]
  );
}

#[gpui::test]
fn pasting_text_into_a_terminal_types_the_text(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  click_terminal(&mut h);
  h.cx
    .write_to_clipboard(gpui::ClipboardItem::new_string("hello agent".into()));
  h.cx.simulate_keystrokes("cmd-v");
  assert_eq!(
    pty_writes(&host),
    vec![format!("write {pty} \"hello agent\"")]
  );
}

/// Claude Code and Codex attach an image whose path is pasted, as they do
/// for an image file dropped onto a native terminal.
#[gpui::test]
fn pasting_a_clipboard_image_pastes_the_path_it_was_saved_to(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  host.output(&pty, "\x1b[?2004h");
  h.cx.run_until_parked();
  click_terminal(&mut h);
  let png = b"\x89PNG\r\n\x1a\n\0\0\0\0".to_vec();
  h.cx
    .write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
      gpui::ImageFormat::Png,
      png,
    )));
  h.cx.simulate_keystrokes("cmd-v");
  h.cx.run_until_parked();
  assert!(
    host
      .calls()
      .contains(&"save pasted image (12 bytes)".to_string()),
    "{:?}",
    host.calls()
  );
  assert_eq!(
    pty_writes(&host),
    vec![format!(
      "write {pty} \"\\u{{1b}}[200~'/tmp/treq-pasted-images/paste-1.png'\\u{{1b}}[201~\""
    )]
  );
}

#[gpui::test]
fn pasting_an_image_into_a_remote_session_saves_nothing(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  h.cx
    .write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
      gpui::ImageFormat::Png,
      b"\x89PNG\r\n\x1a\n\0\0\0\0".to_vec(),
    )));
  h.cx.simulate_keystrokes("cmd-v");
  h.cx.run_until_parked();
  assert!(
    !host
      .calls()
      .iter()
      .any(|c| c.starts_with("save pasted image")),
    "{:?}",
    host.calls()
  );
  assert_eq!(pty_writes(&host), Vec::<String>::new());
}

/// The pane hears of a terminal's output once per burst, and of its end
/// once the process has written nothing for two seconds.
#[gpui::test]
fn terminal_streams_until_the_process_goes_quiet(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  let streaming = |h: &Harness| {
    h.sent().into_iter().rev().find_map(|a| match a {
      ScreenAction::Field { field, value } if field == "terminal.summaries" => {
        value[0]["isStreaming"].as_bool()
      }
      _ => None,
    })
  };
  let wait = |h: &mut Harness, ms: u64| {
    h.cx
      .executor()
      .advance_clock(std::time::Duration::from_millis(ms));
    h.cx.run_until_parked();
  };
  host.output(&pty, "working");
  h.cx.run_until_parked();
  assert_eq!(streaming(&h), Some(true));
  wait(&mut h, 1500);
  host.output(&pty, " more");
  h.cx.run_until_parked();
  wait(&mut h, 1000);
  assert_eq!(streaming(&h), Some(true), "it wrote a second ago");
  wait(&mut h, 1100);
  assert_eq!(streaming(&h), Some(false));
  host.output(&pty, " again");
  h.cx.run_until_parked();
  assert_eq!(streaming(&h), Some(true));
}

/// A terminal resized while its session starts hands the PTY the new size
/// once it runs: a program drawing for a wider PTY than the grid wraps
/// every line, so redraws stack up and overwrite each other.
#[gpui::test]
fn terminal_pty_follows_a_resize_while_it_starts(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  let pty = agent_pty();
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.terminal =
        Some(serde_json::from_value(serde_json::json!({ "activeSessionId": 3 })).expect("route"));
    }
  });
  assert!(
    !host.calls().iter().any(|c| c.starts_with("spawn ")),
    "the session is still starting"
  );
  h.cx
    .simulate_resize(gpui::size(gpui::px(900.), gpui::px(900.)));
  h.cx.run_until_parked();
  let host_dyn: terminal_data::Host = host.clone();
  let (live, _) = terminal_data::attach(&host_dyn, &pty, 80, 24);
  let grid = live.emulator.lock().unwrap().size();
  let last = host
    .resizes
    .lock()
    .unwrap()
    .iter()
    .rev()
    .find(|(id, ..)| *id == pty)
    .map(|(_, cols, rows)| (*cols as usize, *rows as usize));
  assert_eq!(last, Some((grid.cols, grid.rows)));
}

/// A grid that returns to the PTY's size before it settles leaves the PTY
/// alone.
#[gpui::test]
fn terminal_pty_keeps_its_size_when_the_grid_returns_to_it(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let wait = |h: &mut Harness, ms: u64| {
    h.cx
      .executor()
      .advance_clock(std::time::Duration::from_millis(ms));
    h.cx.run_until_parked();
  };
  wait(&mut h, 1000);
  let before = host.resizes.lock().unwrap().len();
  for width in [1100., 1400.] {
    h.cx
      .simulate_resize(gpui::size(gpui::px(width), gpui::px(900.)));
  }
  wait(&mut h, 200);
  assert_eq!(host.resizes.lock().unwrap().len(), before);
}

/// A resize reaches the PTY once the grid stops changing, not per drag
/// step: each one makes the program (an agent's TUI) redraw its screen.
#[gpui::test]
fn terminal_pty_resizes_once_the_grid_settles(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  let wait = |h: &mut Harness, ms: u64| {
    h.cx
      .executor()
      .advance_clock(std::time::Duration::from_millis(ms));
    h.cx.run_until_parked();
  };
  wait(&mut h, 1000);
  let resizes = |host: &FakeHost| -> Vec<(u16, u16)> {
    let resizes = host.resizes.lock().unwrap();
    resizes
      .iter()
      .filter(|(id, ..)| *id == pty)
      .map(|(_, cols, rows)| (*cols, *rows))
      .collect()
  };
  let before = resizes(&host).len();
  for width in [1300., 1200., 1100.] {
    h.cx
      .simulate_resize(gpui::size(gpui::px(width), gpui::px(900.)));
    wait(&mut h, 30);
  }
  assert_eq!(resizes(&host).len(), before, "no resize while dragging");
  wait(&mut h, 100);
  let host_dyn: terminal_data::Host = host.clone();
  let (live, _) = terminal_data::attach(&host_dyn, &pty, 80, 24);
  let grid = live.emulator.lock().unwrap().size();
  assert_eq!(
    resizes(&host)[before..],
    [(grid.cols as u16, grid.rows as u16)]
  );
}

#[gpui::test]
fn terminal_search_keeps_what_is_typed_out_of_the_pty(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  click_terminal(&mut h);
  click_named(&mut h, gpui::Role::Button, "Search terminal");
  assert!(h.shows(gpui::Role::Search, "Find in terminal"));
  h.cx.simulate_input("dark");
  h.cx.run_until_parked();
  let writes: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("write "))
    .collect();
  assert_eq!(writes, Vec::<String>::new());
}

#[gpui::test]
fn terminal_pane_drops_the_active_column_on_a_click_outside(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  assert_eq!(active_column(&mut h).as_deref(), Some("Claude 1 terminal"));
  h.click("terminal-maximize");
  // The pane's controls are part of it.
  assert_eq!(active_column(&mut h).as_deref(), Some("Claude 1 terminal"));
  h.click("terminal-restore");
  h.click("workspace-header");
  assert_eq!(active_column(&mut h), None);
}

#[gpui::test]
fn terminal_pane_picks_the_environment_new_terminals_run_in(cx: &mut TestAppContext) {
  let mut host = terminal_host();
  host.devcontainers = true;
  let (mut h, host) = workspace_harness(cx, host, terminal_route(serde_json::json!({})));
  assert!(!h.shows(gpui::Role::Button, "Local"));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-environment");
  assert!(h.shows(gpui::Role::Menu, "Environment"));
  h.click("environment-local");
  assert!(!h.shows(gpui::Role::Menu, "Environment"));
  assert!(
    host
      .calls()
      .contains(&"set repo /repo environment=local".to_string()),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn terminal_pane_opens_and_closes_shells_by_shortcut(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  click_terminal(&mut h);
  h.cx.simulate_keystrokes("cmd-\\");
  h.cx.run_until_parked();
  assert!(h.shows(gpui::Role::Group, "Shell terminal"));
  let spawned_shell = host.calls().iter().any(|c| {
    c.starts_with("spawn shell--repo--treq-workspaces-login-")
      && c.contains("Some(\"/repo/.treq/workspaces/login\") running Some(\"export\")")
  });
  assert!(spawned_shell, "{:?}", host.calls());
  // The new shell has focus: Cmd+W closes it, and the agent next to it
  // takes focus.
  h.cx.simulate_keystrokes("cmd-w");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Group, "Shell terminal"));
  assert!(h.shows(gpui::Role::Group, "Claude 1 terminal"));
  h.cx.simulate_keystrokes("cmd-w");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Group, "Claude 1 terminal"));
  assert!(host.calls().contains(&format!("close {}", agent_pty())));
  assert!(h.sent().contains(&field("terminal.closeSession", 3.into())));
  // The session's prompt and skill files go with it.
  let removed = host
    .calls()
    .iter()
    .any(|c| c.starts_with("remove files") && c.contains("treq-agent-prompt-"));
  assert!(removed, "{:?}", host.calls());
  // Closing the last terminal hides the pane, chip included.
  assert!(!h.shows(gpui::Role::Toolbar, "Terminal pane controls"));
}

#[gpui::test]
fn terminal_pane_keeps_its_terminals_across_settings(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  click_terminal(&mut h);
  h.cx.simulate_keystrokes("cmd-\\");
  assert!(h.shows(gpui::Role::Group, "Shell terminal"));
  let workspace = h.screen.read_with(&h.cx, |s, _| s.snapshot().clone());
  let spawns = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("spawn "))
      .count()
  };
  let spawned = spawns(&host);

  h.update(|s| *s = settings());
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Region, "Terminal pane"));
  // The hidden pane takes no shortcuts.
  h.cx.simulate_keystrokes("cmd-\\");
  h.update(|s| *s = workspace);
  h.cx.run_until_parked();

  assert!(h.shows(gpui::Role::Group, "Claude 1 terminal"));
  assert!(h.shows(gpui::Role::Group, "Shell terminal"));
  assert_eq!(spawns(&host), spawned, "{:?}", host.calls());
}

#[gpui::test]
fn terminal_pane_shows_a_closed_windows_terminals_again(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  click_terminal(&mut h);
  h.cx.simulate_keystrokes("cmd-\\");
  assert!(h.shows(gpui::Role::Group, "Shell terminal"));
  let spawns = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("spawn "))
      .count()
  };
  let spawned = spawns(&host);
  h.cx.update(|window, _| window.remove_window());
  h.cx.run_until_parked();
  drop(h);

  // Another window opens on the repository: its pane shows them, running.
  let mut other = Harness::with_host(cx, terminal_route(serde_json::json!({})), host.clone());
  other
    .cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  other.cx.run_until_parked();
  assert!(other.shows(gpui::Role::Group, "Claude 1 terminal"));
  assert!(other.shows(gpui::Role::Group, "Shell terminal"));
  assert_eq!(spawns(&host), spawned, "{:?}", host.calls());

  // A third window on it finds none left to show.
  let mut third = Harness::with_host(cx, terminal_route(serde_json::json!({})), host);
  third.cx.run_until_parked();
  assert!(!third.shows(gpui::Role::Group, "Shell terminal"));
}

#[gpui::test]
fn terminal_pane_asks_react_for_a_new_agent(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  click_terminal(&mut h);
  h.cx.simulate_keystrokes("cmd-]");
  h.cx.run_until_parked();
  assert!(h.sent().contains(&field(
    "terminal.newAgent",
    "/repo/.treq/workspaces/login".into()
  )));
}

#[gpui::test]
fn terminal_pane_runs_dashboard_commands(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 1, "kind": "send", "sessionId": 3, "text": "review this" }],
    }),
  );
  assert!(host
    .calls()
    .contains(&format!("write {} \"review this\\n\"", agent_pty())));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 2, "kind": "toggleMaximize" }],
    }),
  );
  assert!(h.shows(gpui::Role::Button, "Restore"));
  assert!(
    !h.shows(gpui::Role::Group, "README"),
    "the maximized pane covers the tab"
  );
  assert!(
    h.shows(gpui::Role::Banner, "Workspace header"),
    "the header stays above it"
  );
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 3, "kind": "toggleCollapse" }],
    }),
  );
  assert!(h.shows(gpui::Role::Button, "Expand"));
}

/// A session that becomes active (a review that just started, say) shows:
/// a collapsed pane expands so its column is on screen.
#[gpui::test]
fn terminal_pane_expands_for_a_newly_active_session(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 1, "kind": "toggleCollapse" }],
    }),
  );
  assert!(h.shows(gpui::Role::Button, "Expand"), "collapsed");
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 4 }));
  assert!(
    !h.shows(gpui::Role::Button, "Expand"),
    "the new session's column shows"
  );
  assert!(active_column(&mut h).is_some());
}

#[gpui::test]
fn terminal_pane_activates_the_selected_workspaces_terminal(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 1, "kind": "newShell", "dir": "/repo" }],
    }),
  );
  // Selecting the home repository makes its shell the active terminal.
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.workspace_id = None;
    }
  });
  h.cx.run_until_parked();
  h.cx.simulate_keystrokes("cmd-w");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Group, "Shell terminal"));
  assert!(h.shows(gpui::Role::Group, "Claude 1 terminal"));
  assert!(!host.calls().contains(&format!("close {}", agent_pty())));
}

/// Clicking a terminal in another workspace switches to that workspace: the
/// clicked terminal stays active, rather than the row jumping to the
/// workspace's first one.
#[gpui::test]
fn switching_to_the_active_terminals_workspace_keeps_it_active(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [
        { "nonce": 1, "kind": "newShell", "dir": "/repo" },
        { "nonce": 2, "kind": "newShell", "dir": "/repo" },
      ],
    }),
  );
  let shells: Vec<String> = host
    .calls()
    .iter()
    .filter_map(|c| c.strip_prefix("spawn ").and_then(|c| c.split(' ').next()))
    .filter(|id| id.starts_with("shell-"))
    .map(str::to_string)
    .collect();
  assert_eq!(shells.len(), 2, "{:?}", host.calls());
  // The newest shell is active, as a click on it would leave it.
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.workspace_id = None;
    }
  });
  h.cx.run_until_parked();
  h.cx.simulate_keystrokes("cmd-w");
  h.cx.run_until_parked();
  let closed = format!("close {}", shells[1]);
  assert!(host.calls().contains(&closed), "{:?}", host.calls());
}

/// The pane shows every workspace's terminals: the shown workspace's are
/// highlighted, the others dimmed, and switching workspace swaps them.
#[gpui::test]
fn terminal_pane_highlights_the_shown_workspaces_terminals(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 1, "kind": "newShell", "dir": "/repo" }],
    }),
  );
  // Workspace 7 on screen: its agent is current, the home shell dimmed.
  assert!(column_is(&mut h, "Claude 1 terminal", CURRENT));
  assert!(column_is(&mut h, "Shell terminal", DIMMED));
  assert!(!column_is(&mut h, "Claude 1 terminal", DIMMED));
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.workspace_id = None;
    }
  });
  h.cx.run_until_parked();
  assert!(
    column_is(&mut h, "Shell terminal", CURRENT),
    "the home shell is current"
  );
  assert!(
    column_is(&mut h, "Claude 1 terminal", DIMMED),
    "workspace 7's agent dims"
  );
  assert!(
    h.shows(gpui::Role::Group, "Shell terminal") && h.shows(gpui::Role::Group, "Claude 1 terminal")
  );
}

/// A dimmed column stays usable: hovering it undims it.
#[gpui::test]
fn terminal_pane_undims_a_hovered_column(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 1, "kind": "newShell", "dir": "/repo" }],
    }),
  );
  assert!(column_is(&mut h, "Shell terminal", DIMMED));
  let screen = h.rendered();
  let column =
    screen.get_by_role(gpui_testing_library::by::role(gpui::Role::Group).name("Shell terminal"));
  screen.user().hover(&column);
  h.cx.run_until_parked();
  assert!(
    !column_is(&mut h, "Shell terminal", DIMMED),
    "hovered, so not dimmed"
  );
}

/// Double-clicks the shown terminal's title, as the user renames it.
fn double_click_title(h: &mut Harness) {
  let title = by_id_prefix(h, "terminal-title-");
  h.rendered().user().dbl_click(&title);
  h.cx.run_until_parked();
}

/// The name the pane last told the sidebar for each terminal.
fn terminal_names(h: &Harness) -> Vec<String> {
  fields(h, "terminal.summaries")
    .last()
    .and_then(|s| s.as_array().cloned())
    .unwrap_or_default()
    .iter()
    .map(|s| s["name"].as_str().unwrap_or_default().to_string())
    .collect()
}

#[gpui::test]
fn an_agent_terminal_is_renamed_from_its_title(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  double_click_title(&mut h);
  assert!(h.shows(gpui::Role::Group, "Rename terminal"));
  // The name starts selected, so typing replaces it.
  h.cx.simulate_input("Fix login");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Group, "Rename terminal"));
  assert!(host
    .calls()
    .contains(&"rename session 3 in /repo to Fix login".to_string()));
  assert_eq!(terminal_names(&h), ["Fix login"]);
  // Emptied, an agent keeps its name.
  double_click_title(&mut h);
  h.cx.simulate_keystrokes("backspace enter");
  h.cx.run_until_parked();
  assert_eq!(terminal_names(&h), ["Fix login"]);
}

#[gpui::test]
fn a_shell_is_named_until_cleared_and_escape_keeps_its_name(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({ "commands": [{ "nonce": 1, "kind": "newShell", "dir": "/repo" }] }),
  );
  assert_eq!(terminal_names(&h), ["Shell"]);
  double_click_title(&mut h);
  h.cx.simulate_input("dev server");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert_eq!(terminal_names(&h), ["dev server"]);
  double_click_title(&mut h);
  h.cx.simulate_input("typo");
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Group, "Rename terminal"));
  assert_eq!(terminal_names(&h), ["dev server"]);
  // An empty name shows "Shell" again.
  double_click_title(&mut h);
  h.cx.simulate_keystrokes("backspace enter");
  h.cx.run_until_parked();
  assert_eq!(terminal_names(&h), ["Shell"]);
  assert!(!host.calls().iter().any(|c| c.starts_with("rename")));
}

#[gpui::test]
fn a_remote_session_is_named_on_its_host(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  let mut open = open_remote_command(1);
  open["commands"][0]["remote"]["title"] = "deploy".into();
  set_terminal(&mut h, open);
  assert_eq!(terminal_names(&h), ["deploy"]);
  double_click_title(&mut h);
  h.cx.simulate_input("  tail logs  ");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(
    host
      .calls()
      .contains(&"rename shell-k1 to tail logs".to_string()),
    "{:?}",
    host.calls()
  );
  assert_eq!(terminal_names(&h), ["tail logs"]);
}

fn open_remote_command(nonce: u64) -> serde_json::Value {
  serde_json::json!({
    "commands": [{
      "nonce": nonce,
      "kind": "openRemote",
      "remote": {
        "endpoint": crate::gpui_shell::remote_repo::tests::repository().endpoint,
        "repositoryId": "/srv/app",
        "workspaceId": "root",
        "remoteWorkingDirectory": "/srv/app",
        "label": "shell-k1",
        "launch": { "type": "shell" },
        "reattach": false,
      },
    }],
  })
}

#[gpui::test]
fn terminal_pane_attaches_a_remote_session(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  assert!(h.shows(gpui::Role::Group, "Remote terminal"));
  assert!(h.shows(gpui::Role::Group, "devbox.example session"));
  assert!(h.shows(gpui::Role::Button, "Stop"));
  assert!(h.shows(gpui::Role::Button, "Detach"));
  let attached = host.attached();
  assert_eq!(attached.len(), 1, "{:?}", host.calls());
  assert!(attached[0].starts_with("remote-pty-ep-1-root-shell-k1-"));
  assert!(!attaching(&mut h), "attached");
  h.cx.simulate_input("ls");
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .iter()
    .any(|c| c.starts_with(&format!("write {}", attached[0]))));
  // The session ended on the host: the header says so, no overlay.
  *host.remote_running.lock().unwrap() = Some(false);
  host.exit(&attached[0], Some(0));
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"list shell-k1".to_string()));
  assert!(h.shows(gpui::Role::Status, "(session ended, exit 0)"));
  assert!(!connection_lost(&mut h));
}

#[gpui::test]
fn a_closed_windows_remote_session_shows_again(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  let attached = host.attached();
  h.cx.update(|window, _| window.remove_window());
  h.cx.run_until_parked();
  drop(h);

  // Its channel still open, the next window draws it without attaching.
  let mut other = Harness::with_host(cx, terminal_route(serde_json::json!({})), host.clone());
  other
    .cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  other.cx.run_until_parked();
  assert!(other.shows(gpui::Role::Group, "devbox.example session"));
  assert_eq!(host.attached(), attached, "{:?}", host.calls());
  // Its channel dropped since: the column attaches to the host's session.
  other.cx.update(|window, _| window.remove_window());
  other.cx.run_until_parked();
  drop(other);
  host.ptys.lock().unwrap().clear();
  let mut third = Harness::with_host(cx, terminal_route(serde_json::json!({})), host.clone());
  third
    .cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  third.cx.run_until_parked();
  assert!(third.shows(gpui::Role::Group, "devbox.example session"));
  assert_eq!(
    host.attached(),
    vec![attached[0].clone(), attached[0].clone()]
  );
}

#[gpui::test]
fn a_dropped_remote_session_reattaches_on_every_drop(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  *host.remote_running.lock().unwrap() = Some(true);
  host.exit(&host.attached()[0], None);
  h.cx.run_until_parked();
  let attached = host.attached();
  assert_eq!(attached.len(), 2, "reattached on its own");
  assert_ne!(attached[0], attached[1], "with a fresh local id");
  assert!(host.calls().contains(&format!("close {}", attached[0])));
  // Once the session talks again, a later drop gets its own attempt.
  host.output(&attached[1], "$ ");
  h.cx.run_until_parked();
  host.exit(&attached[1], None);
  h.cx.run_until_parked();
  assert_eq!(host.attached().len(), 3);
  assert!(!connection_lost(&mut h));
}

#[gpui::test]
fn a_lost_host_is_retried_once_before_offering_reattach(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  // The host is unreachable: `PtyList` fails and so does the attach.
  *host.remote_running.lock().unwrap() = None;
  *host.attach_down.lock().unwrap() = true;
  host.exit(&host.attached()[0], None);
  h.cx.run_until_parked();
  assert_eq!(host.attached().len(), 2, "tried once on its own");
  assert!(connection_lost(&mut h));
  *host.attach_down.lock().unwrap() = false;
  h.click("remote-terminal-reattach");
  h.cx.run_until_parked();
  assert_eq!(host.attached().len(), 3);
  assert!(!connection_lost(&mut h));
}

#[gpui::test]
fn an_unreachable_remote_session_offers_reattach_and_close(cx: &mut TestAppContext) {
  let host = FakeHost {
    failing: vec!["remote_attach"],
    ..terminal_host()
  };
  let (mut h, host) = workspace_harness(cx, host, terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  assert!(connection_lost(&mut h));
  assert_eq!(host.attached().len(), 2, "retried once first");
  h.click("remote-terminal-close");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Group, "Remote terminal"));
  assert!(host
    .calls()
    .iter()
    .any(|c| c.starts_with("close remote-pty-")));
}

#[gpui::test]
fn stopping_a_remote_session_ends_it_on_the_host(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  h.click("remote-terminal-stop");
  h.cx.run_until_parked();
  assert!(host.calls().contains(&"stop shell-k1".to_string()));
  assert!(!h.shows(gpui::Role::Group, "Remote terminal"));
}

#[gpui::test]
fn detaching_a_remote_session_keeps_it_running(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  h.click("remote-terminal-detach");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Group, "Remote terminal"));
  assert!(!host.calls().contains(&"stop shell-k1".to_string()));
  assert!(host
    .calls()
    .iter()
    .any(|c| c.starts_with("close remote-pty-")));
}

#[gpui::test]
fn a_failed_remote_write_shows_the_error_with_reattach(cx: &mut TestAppContext) {
  let host = FakeHost {
    failing: vec!["write_pty"],
    ..terminal_host()
  };
  let (mut h, host) = workspace_harness(cx, host, terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  assert!(!connection_lost(&mut h));
  h.cx.simulate_input("ls");
  h.cx.run_until_parked();
  // The first failure reattaches on its own.
  assert_eq!(host.attached().len(), 2, "{:?}", host.calls());
  assert!(!connection_lost(&mut h));
  // Failing again before the session says anything shows the error.
  h.cx.simulate_input("ls");
  h.cx.run_until_parked();
  assert!(connection_lost(&mut h), "{:?}", host.calls());
  assert!(h.shows(gpui::Role::Button, "Reattach"));
  h.click("remote-terminal-reattach");
  h.cx.run_until_parked();
  assert_eq!(host.attached().len(), 3);
  assert!(!connection_lost(&mut h));
}

#[gpui::test]
fn a_remote_terminal_has_no_search(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  h.cx.simulate_keystrokes("cmd-f");
  assert!(!h.shows(gpui::Role::Search, "Find in terminal"));
  assert!(h.shows(gpui::Role::Group, "devbox.example session"));
}

fn drop_files(h: &mut Harness, at: gpui::Point<gpui::Pixels>) {
  let paths = gpui::ExternalPaths(["/tmp/a b.txt".into()].into_iter().collect());
  h.cx.simulate_event(gpui::FileDropEvent::Entered {
    position: at,
    paths,
  });
  h.cx
    .simulate_event(gpui::FileDropEvent::Submit { position: at });
  h.cx.run_until_parked();
}

#[gpui::test]
fn files_dropped_on_a_shell_are_typed_but_not_on_a_remote_session(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({ "commands": [{ "nonce": 1, "kind": "newShell", "dir": "/repo" }] }),
  );
  let at = h
    .rendered()
    .get_by_role(gpui_testing_library::by::role(gpui::Role::Group).name("Shell terminal"))
    .bounds()
    .center();
  drop_files(&mut h, at);
  let writes = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("write "))
      .count()
  };
  assert_eq!(writes(&host), 1, "{:?}", host.calls());

  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  let at = by_id_prefix(&mut h, "terminal-column-").bounds().center();
  drop_files(&mut h, at);
  assert_eq!(writes(&host), 0, "{:?}", host.calls());
}

/// A native terminal pastes a drop, so an agent that asked for bracketed
/// paste attaches a dropped image path as it would a pasted one.
#[gpui::test]
fn files_dropped_on_an_agent_are_pasted(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  host.output(&pty, "\x1b[?2004h");
  h.cx.run_until_parked();
  let at = by_id_prefix(&mut h, "terminal-column-agent-")
    .bounds()
    .center();
  drop_files(&mut h, at);
  assert_eq!(
    pty_writes(&host),
    vec![format!(
      "write {pty} \"\\u{{1b}}[200~'/tmp/a b.txt'\\u{{1b}}[201~\""
    )]
  );
}

fn drop_target(h: &mut Harness, column: &str) -> bool {
  by_id_prefix(h, &format!("terminal-drop-target-{column}")).is_visible()
}

/// Files dragged over the pane light up the terminal they would land in,
/// and only that one.
#[gpui::test]
fn files_dragged_over_a_terminal_highlight_it_as_the_drop_target(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({
      "activeSessionId": 3,
      "commands": [{ "nonce": 1, "kind": "newShell", "dir": "/repo" }],
    }),
  );
  assert!(!drop_target(&mut h, "agent-"));
  assert!(!drop_target(&mut h, "shell-"));

  let agent = by_id_prefix(&mut h, "terminal-column-agent-")
    .bounds()
    .center();
  let paths = gpui::ExternalPaths(["/tmp/shot.png".into()].into_iter().collect());
  h.cx.simulate_event(gpui::FileDropEvent::Entered {
    position: agent,
    paths,
  });
  h.cx.run_until_parked();
  assert!(drop_target(&mut h, "agent-"));
  assert!(!drop_target(&mut h, "shell-"));

  let shell = by_id_prefix(&mut h, "terminal-column-shell-")
    .bounds()
    .center();
  h.cx
    .simulate_event(gpui::FileDropEvent::Pending { position: shell });
  h.cx.run_until_parked();
  assert!(!drop_target(&mut h, "agent-"));
  assert!(drop_target(&mut h, "shell-"));

  h.cx.simulate_event(gpui::FileDropEvent::Exited);
  h.cx.run_until_parked();
  assert!(!drop_target(&mut h, "agent-"));
  assert!(!drop_target(&mut h, "shell-"));
}

/// Dragging a `treq send` attachment lights up the terminal it would land in.
#[gpui::test]
fn an_attachment_dragged_over_a_terminal_highlights_it_as_the_drop_target(cx: &mut TestAppContext) {
  let (mut h, _) = send_harness(cx);
  h.cx.run_until_parked();
  let from = h
    .cx
    .debug_bounds("terminal-send-preview-t1")
    .expect("card")
    .center();
  let to = by_id_prefix(&mut h, "terminal-column-agent-")
    .bounds()
    .center();
  h.cx
    .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
  h.cx
    .simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
  h.cx
    .simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
  h.cx.run_until_parked();
  assert!(h.cx.update(|_, cx| cx.has_active_drag()));
  assert!(drop_target(&mut h, "agent-"));
}

/// A remote session takes no drops, so it never shows as a drop target.
#[gpui::test]
fn a_remote_session_is_not_a_drop_target(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  let at = by_id_prefix(&mut h, "terminal-column-").bounds().center();
  let paths = gpui::ExternalPaths(["/tmp/shot.png".into()].into_iter().collect());
  h.cx.simulate_event(gpui::FileDropEvent::Entered {
    position: at,
    paths,
  });
  h.cx.run_until_parked();
  let targets = h
    .rendered()
    .elements()
    .into_iter()
    .filter(|n| {
      matches!(n.element_id(), gpui::ElementId::Name(id) if id.starts_with("terminal-drop-target-"))
    })
    .count();
  assert_eq!(targets, 0);
}

#[gpui::test]
fn double_clicking_a_remote_session_scrolls_it_fully_into_view(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  let open = |nonce: u64, label: &str| {
    let mut command = open_remote_command(nonce);
    command["commands"][0]["remote"]["label"] = label.into();
    command
  };
  set_terminal(&mut h, open(1, "shell-a"));
  let mut commands = open(1, "shell-a")["commands"].as_array().cloned().unwrap();
  for (nonce, label) in [(2, "shell-b"), (3, "shell-c")] {
    commands.extend(open(nonce, label)["commands"].as_array().cloned().unwrap());
  }
  set_terminal(&mut h, serde_json::json!({ "commands": commands }));
  let container = h
    .cx
    .debug_bounds("terminal-scroll-container")
    .expect("container");
  // The newest session opened in view; drag the row's scrollbar back to
  // the start so the last one overflows again.
  let thumb = h
    .cx
    .debug_bounds("terminal-row-scrollbar")
    .expect("scrollbar");
  let grab = thumb.center();
  let to = gpui::point(container.left(), grab.y);
  h.cx.simulate_event(gpui::MouseDownEvent {
    position: grab,
    modifiers: Modifiers::none(),
    button: MouseButton::Left,
    click_count: 1,
    first_mouse: false,
  });
  h.cx.simulate_event(gpui::MouseMoveEvent {
    position: to,
    modifiers: Modifiers::none(),
    pressed_button: Some(MouseButton::Left),
  });
  h.cx.simulate_event(gpui::MouseUpEvent {
    position: to,
    modifiers: Modifiers::none(),
    button: MouseButton::Left,
    click_count: 1,
  });
  h.cx.run_until_parked();
  let header = h.cx.debug_bounds("remote-terminal-header").expect("header");
  assert!(
    header.right() > container.right(),
    "the last session overflows the row: {header:?} {container:?}"
  );
  let at = gpui::point(container.right() - gpui::px(8.), header.center().y);
  for click_count in [1, 2] {
    h.cx.simulate_event(gpui::MouseDownEvent {
      position: at,
      modifiers: Modifiers::none(),
      button: MouseButton::Left,
      click_count,
      first_mouse: false,
    });
    h.cx.simulate_event(gpui::MouseUpEvent {
      position: at,
      modifiers: Modifiers::none(),
      button: MouseButton::Left,
      click_count,
    });
  }
  h.cx.run_until_parked();
  let header = h.cx.debug_bounds("remote-terminal-header").expect("header");
  assert!(
    header.right() <= container.right() + gpui::px(1.),
    "{header:?} {container:?}"
  );
}

#[gpui::test]
fn double_clicking_a_session_clipped_on_the_left_scrolls_it_into_view(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  let mut commands = Vec::new();
  for (nonce, label) in [(1, "shell-a"), (2, "shell-b"), (3, "shell-c")] {
    let mut command = open_remote_command(nonce);
    command["commands"][0]["remote"]["label"] = label.into();
    commands.extend(command["commands"].as_array().cloned().unwrap());
  }
  set_terminal(&mut h, serde_json::json!({ "commands": commands }));
  let container = h
    .cx
    .debug_bounds("terminal-scroll-container")
    .expect("container");
  // Drag the row's scrollbar to the end so the first session is clipped
  // by the left edge.
  let thumb = h
    .cx
    .debug_bounds("terminal-row-scrollbar")
    .expect("scrollbar");
  let grab = thumb.center();
  let to = gpui::point(container.right(), grab.y);
  h.cx.simulate_event(gpui::MouseDownEvent {
    position: grab,
    modifiers: Modifiers::none(),
    button: MouseButton::Left,
    click_count: 1,
    first_mouse: false,
  });
  h.cx.simulate_event(gpui::MouseMoveEvent {
    position: to,
    modifiers: Modifiers::none(),
    pressed_button: Some(MouseButton::Left),
  });
  h.cx.simulate_event(gpui::MouseUpEvent {
    position: to,
    modifiers: Modifiers::none(),
    button: MouseButton::Left,
    click_count: 1,
  });
  h.cx.run_until_parked();
  let before = h
    .cx
    .debug_bounds("terminal-row-scrollbar")
    .expect("scrollbar");
  assert!(
    before.left() > container.left() + gpui::px(1.),
    "{before:?}"
  );
  let header = h.cx.debug_bounds("remote-terminal-header").expect("header");
  let at = gpui::point(container.left() + gpui::px(8.), header.center().y);
  for click_count in [1, 2] {
    h.cx.simulate_event(gpui::MouseDownEvent {
      position: at,
      modifiers: Modifiers::none(),
      button: MouseButton::Left,
      click_count,
      first_mouse: false,
    });
    h.cx.simulate_event(gpui::MouseUpEvent {
      position: at,
      modifiers: Modifiers::none(),
      button: MouseButton::Left,
      click_count,
    });
  }
  h.cx.run_until_parked();
  let after = h
    .cx
    .debug_bounds("terminal-row-scrollbar")
    .expect("scrollbar");
  assert!(after.left() < before.left(), "{before:?} {after:?}");
}

#[gpui::test]
fn a_remote_repository_asks_react_for_new_shells(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(
    &mut h,
    serde_json::json!({
      "remoteShell": true,
      "commands": [{ "nonce": 1, "kind": "newShell", "dir": "/srv/app" }],
    }),
  );
  assert_eq!(
    fields(&h, "terminal.newRemoteShell"),
    vec![serde_json::json!("/srv/app")]
  );
  assert!(!host.calls().iter().any(|c| c.starts_with("spawn ")));
  assert!(!h.shows(gpui::Role::Group, "Shell terminal"));
}

#[gpui::test]
fn terminal_pane_tooltips_show_shortcuts_as_kbd(cx: &mut TestAppContext) {
  use crate::gpui_shell::popup::{PopupRequest, TooltipContent};
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let at = h
    .cx
    .debug_bounds("terminal-collapse")
    .expect("collapse")
    .center();
  h.cx.simulate_mouse_move(at, None, Modifiers::none());
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(300));
  h.cx.run_until_parked();
  let shown = h.popups.borrow().iter().any(|p| {
    matches!(p, PopupRequest::ShowTooltip { content: TooltipContent::Shortcut { text, keys }, .. }
      if text == "Collapse" && *keys == crate::gpui_shell::keys::hint("⌘ + J"))
  });
  assert!(shown, "{:?}", h.popups.borrow());
}

#[gpui::test]
fn header_buttons_open_their_popovers_over_the_window(cx: &mut TestAppContext) {
  for (button, name) in [
    ("workspace-details", "Workspace details"),
    ("workspace-checks", "Checks"),
  ] {
    let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
    h.click(button);
    h.cx.run_until_parked();
    // Drawn in the repo window itself, over an in-window backdrop.
    assert!(h.shows(gpui::Role::Dialog, name), "{button}");
    assert!(h.shows(gpui::Role::Banner, "Workspace header"));
    assert!(h.sent().is_empty(), "{button}");
  }
}

#[gpui::test]
fn opening_details_hides_its_tooltip(cx: &mut TestAppContext) {
  use crate::gpui_shell::popup::PopupRequest;
  let (mut h, _) = workspace_harness(cx, feature_host(), workspace_route());
  let at = h
    .cx
    .debug_bounds("workspace-details")
    .expect("details")
    .center();
  h.cx.simulate_mouse_move(at, None, Modifiers::none());
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(300));
  h.cx.run_until_parked();
  assert!(h
    .popups
    .borrow()
    .iter()
    .any(|p| matches!(p, PopupRequest::ShowTooltip { .. })));
  h.click("workspace-details");
  // Hovering again while the popover shows keeps the tooltip hidden.
  h.cx.simulate_mouse_move(
    at + gpui::point(gpui::px(1.), gpui::px(0.)),
    None,
    Modifiers::none(),
  );
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(300));
  h.cx.run_until_parked();
  let popups = h.popups.borrow();
  let last_tooltip = popups.iter().rev().find(|p| {
    matches!(
      p,
      PopupRequest::ShowTooltip { .. } | PopupRequest::HideTooltip
    )
  });
  assert!(
    matches!(last_tooltip, Some(PopupRequest::HideTooltip)),
    "{popups:?}"
  );
  drop(popups);
  assert!(h.shows(gpui::Role::Dialog, "Workspace details"));
}

fn send_assets() -> serde_json::Value {
  let pty = agent_pty();
  serde_json::json!([
    { "id": "t1", "repo": "/repo", "ptySessionId": pty, "mediaType": "text",
      "path": "/tmp/notes.txt", "title": "notes.txt" },
    { "id": "i1", "repo": "/repo", "ptySessionId": pty, "mediaType": "image",
      "path": "/tmp/treq-missing-shot.png", "title": "shot.png" },
    { "id": "b1", "repo": "/repo", "ptySessionId": pty, "mediaType": "browser",
      "path": "/tmp/page", "title": "page" },
    { "id": "o1", "repo": "/repo", "ptySessionId": "session-other-9", "mediaType": "text",
      "path": "/tmp/other.txt", "title": "other.txt" },
  ])
}

fn send_harness(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  *host.send.lock().unwrap() = serde_json::from_value(send_assets()).expect("assets");
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  (h, host)
}

fn hover(h: &mut Harness, selector: &'static str) {
  h.cx.run_until_parked();
  let at = h.cx.debug_bounds(selector).expect(selector).center();
  h.cx.simulate_mouse_move(at, None, Modifiers::none());
  h.cx.run_until_parked();
}

#[gpui::test]
fn terminal_shows_its_send_attachments_and_dismisses_them(cx: &mut TestAppContext) {
  let (mut h, host) = send_harness(cx);
  assert!(h.shows(gpui::Role::Group, "Attachments"));
  assert!(h.shows(gpui::Role::Button, "Open notes.txt"));
  assert!(h.shows(gpui::Role::Button, "Open shot.png"));
  // Browser previews are the browser panel's; other terminals keep theirs.
  assert!(!h.shows(gpui::Role::Button, "Open page"));
  assert!(!h.shows(gpui::Role::Button, "Open other.txt"));
  assert!(!h.shows(gpui::Role::Button, "Remove notes.txt"));
  hover(&mut h, "terminal-send-preview-t1");
  assert!(h.shows(gpui::Role::Button, "Remove notes.txt"));
  // The name is its own element, clipped at the end, with a tooltip.
  assert!(h.shows(gpui::Role::Label, "notes.txt"));
  // Cmd+C over a thumbnail copies its quoted path.
  h.cx.simulate_keystrokes("cmd-c");
  assert_eq!(
    h.cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
    Some("'/tmp/notes.txt'")
  );
  hover(&mut h, "terminal-send-preview-t1");
  h.click("terminal-send-dismiss-t1");
  assert!(h.sent().contains(&ScreenAction::Field {
    field: "terminal.dismissAsset".into(),
    value: "t1".into(),
  }));
  assert!(host.calls().contains(&"dismiss asset t1".to_string()));
  assert!(!h.shows(gpui::Role::Button, "Open notes.txt"));
  assert!(!h.shows(gpui::Role::Dialog, "Preview"));
}

#[gpui::test]
fn lightbox_sends_line_comments_on_a_text_attachment(cx: &mut TestAppContext) {
  let (mut h, host) = send_harness(cx);
  h.click("terminal-send-preview-t1");
  assert!(h.shows(gpui::Role::Dialog, "Preview"));
  assert!(host
    .calls()
    .contains(&"read asset /tmp/notes.txt".to_string()));
  assert!(h.shows(gpui::Role::Row, "Line 3"));
  // Text has no zoom.
  assert!(!h.shows(gpui::Role::Button, "Zoom in"));
  h.click("treq-send-text-line-2");
  assert!(h.shows(gpui::Role::Group, "New comment"));
  h.cx.simulate_input("rename this");
  h.click("treq-send-line-comment-save");
  assert!(!h.shows(gpui::Role::Group, "New comment"));
  assert!(h.shows(gpui::Role::Group, "Comment on line 2"));
  h.click("treq-send-review-input");
  h.cx.simulate_input("Thanks");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  let prompt = "Review of notes.txt\n\nFile: /tmp/notes.txt\n\nThanks\n\nInline comments:\n\nnotes.txt:2\n```\nsecond\n```\n> rename this\n";
  assert!(
    host.calls().contains(&format!("chat message 3 {prompt:?}")),
    "{:?}",
    host.calls()
  );
  let sent = format!("write {} {:?}", agent_pty(), format!("{prompt}\r"));
  assert!(pty_writes(&host).contains(&sent), "{:?}", pty_writes(&host));
  // Sent: nothing is left to lose on close.
  assert!(!h.shows(gpui::Role::Group, "Comment on line 2"));
  h.click("treq-send-close");
  assert!(!h.shows(gpui::Role::Dialog, "Preview"));
}

#[gpui::test]
fn lightbox_asks_before_dropping_unsent_comments(cx: &mut TestAppContext) {
  let (mut h, _) = send_harness(cx);
  h.click("terminal-send-preview-t1");
  h.click("treq-send-close");
  assert!(!h.shows(gpui::Role::Dialog, "Preview"));
  h.click("terminal-send-preview-t1");
  h.click("treq-send-review-input");
  h.cx.simulate_input("hmm");
  h.cx.simulate_keystrokes("escape");
  assert!(h.shows(gpui::Role::Dialog, "Discard unsent comments?"));
  h.click("treq-send-unsaved-keep");
  assert!(!h.shows(gpui::Role::Dialog, "Discard unsent comments?"));
  assert!(h.shows(gpui::Role::Dialog, "Preview"));
  h.click("treq-send-close");
  h.click("treq-send-unsaved-discard");
  assert!(!h.shows(gpui::Role::Dialog, "Preview"));
}

/// The e2e suite finds a `treq send` thumbnail and its lightbox by role.
#[gpui::test]
fn send_thumbnail_and_lightbox_are_accessible(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let (mut h, _) = send_harness(cx);
  let screen = a11y_screen(&mut h);
  for name in ["Open notes.txt", "Open shot.png"] {
    expect(screen.get_by_role(by::role(gpui::Role::Button).name(name))).to_be_visible();
  }
  h.click("terminal-send-preview-t1");
  let screen = a11y_screen(&mut h);
  expect(screen.get_by_role(by::role(gpui::Role::Dialog).name("Preview"))).to_be_visible();
}

#[gpui::test]
fn lightbox_moves_between_attachments(cx: &mut TestAppContext) {
  let (mut h, _) = send_harness(cx);
  h.click("terminal-send-preview-t1");
  assert!(h.shows(gpui::Role::Group, "Text preview"));
  h.click("treq-send-next");
  assert!(h.shows(gpui::Role::Image, "Drag to highlight a region"));
  assert!(h.shows(gpui::Role::Button, "Zoom in"));
  h.cx.simulate_keystrokes("left");
  assert!(h.shows(gpui::Role::Group, "Text preview"));
}

#[gpui::test]
fn lightbox_comments_on_markdown_in_preview_and_markdown_tabs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  *host.send.lock().unwrap() = serde_json::from_value(serde_json::json!([
    { "id": "m1", "repo": "/repo", "ptySessionId": agent_pty(), "mediaType": "text",
      "path": "/tmp/HANDOFF.md", "title": "HANDOFF.md" },
  ]))
  .expect("assets");
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-send-preview-m1");
  // Markdown opens as a preview, commented on by block.
  assert!(h.shows(gpui::Role::Document, "Rendered markdown"));
  assert!(!h.shows(gpui::Role::Row, "Line 2"));
  h.click("treq-send-block-0");
  assert!(h.shows(gpui::Role::Group, "New comment"));
  h.cx.simulate_input("reword");
  h.click("treq-send-line-comment-save");
  assert!(h.shows(gpui::Role::Group, "Comment on lines 1–3"));
  // The block's comment is a line comment on its source lines.
  h.click("treq-send-view-markdown");
  assert!(!h.shows(gpui::Role::Document, "Rendered markdown"));
  assert!(h.shows(gpui::Role::Group, "Comment on lines 1–3"));
  h.click("treq-send-text-line-2");
  h.cx.simulate_input("rename this");
  h.click("treq-send-line-comment-save");
  assert!(h.shows(gpui::Role::Group, "Comment on line 2"));
  // Both show in the preview, in one review.
  h.click("treq-send-view-preview");
  assert!(h.shows(gpui::Role::Group, "Comment on lines 1–3"));
  assert!(h.shows(gpui::Role::Group, "Comment on line 2"));
  h.click("treq-send-review-input");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  let calls = host.calls().join("\n");
  assert!(calls.contains("HANDOFF.md:1:3"), "{calls}");
  assert!(calls.contains("HANDOFF.md:2"), "{calls}");
}

#[gpui::test]
fn lightbox_shows_comments_in_a_margin_beside_what_they_annotate(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  *host.send.lock().unwrap() = serde_json::from_value(serde_json::json!([
    { "id": "m1", "repo": "/repo", "ptySessionId": agent_pty(), "mediaType": "text",
      "path": "/tmp/HANDOFF.md", "title": "HANDOFF.md" },
  ]))
  .expect("assets");
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-send-preview-m1");
  // No comments, no margin.
  assert!(!h.shows(gpui::Role::Group, "Comments"));
  h.click("treq-send-block-0");
  // The Preview's comment box is written in the margin.
  assert!(h.shows(gpui::Role::Group, "Comments"));
  h.cx.simulate_input("reword");
  h.click("treq-send-line-comment-save");
  h.cx.run_until_parked();
  assert!(h.shows(gpui::Role::Group, "Comments"));
  let beside = |h: &mut Harness, anchor: &'static str| {
    let doc = h.cx.debug_bounds(anchor).expect(anchor);
    let card = h.cx.debug_bounds("treq-send-line-comment-1").expect("card");
    assert!(card.left() >= doc.right(), "{card:?} not right of {doc:?}");
    assert!(
      card.top() >= doc.top() && card.top() < doc.bottom(),
      "{card:?} not beside {doc:?}"
    );
  };
  beside(&mut h, "treq-send-block-0");
  h.click("treq-send-view-markdown");
  h.cx.run_until_parked();
  beside(&mut h, "treq-send-text-line-1");
}

#[gpui::test]
fn lightbox_comments_can_suggest_a_change_without_applying_it(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  *host.send.lock().unwrap() = serde_json::from_value(serde_json::json!([
    { "id": "m1", "repo": "/repo", "ptySessionId": agent_pty(), "mediaType": "text",
      "path": "/tmp/HANDOFF.md", "title": "HANDOFF.md" },
  ]))
  .expect("assets");
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-send-preview-m1");
  h.click("treq-send-view-markdown");
  h.click("treq-send-text-line-2");
  // As in code review: the picked lines, as a suggestion block to edit.
  h.click("treq-send-suggest-change");
  h.cx.simulate_input("2nd");
  h.click("treq-send-line-comment-save");
  h.cx.run_until_parked();
  assert!(h.shows(gpui::Role::Group, "Comment on line 2"));
  assert!(shows_hidden(&mut h, gpui::Role::Group, "Suggestion"));
  // Nothing to apply it to: the agent gets it in the review.
  assert!(!shows_hidden(
    &mut h,
    gpui::Role::Button,
    "Apply suggestion"
  ));
  h.click("treq-send-view-preview");
  assert!(shows_hidden(&mut h, gpui::Role::Group, "Suggestion"));
  h.click("treq-send-review-input");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  let calls = host.calls().join("\n");
  assert!(calls.contains("```suggestion"), "{calls}");
}

#[gpui::test]
fn lightbox_preview_writes_comments_in_the_margin_without_moving_the_text(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  *host.send.lock().unwrap() = serde_json::from_value(serde_json::json!([
    { "id": "m1", "repo": "/repo", "ptySessionId": agent_pty(), "mediaType": "text",
      "path": "/tmp/HANDOFF.md", "title": "HANDOFF.md" },
  ]))
  .expect("assets");
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-send-preview-m1");
  h.click("treq-send-block-0");
  h.cx.run_until_parked();
  let block = h.cx.debug_bounds("treq-send-block-0").expect("block");
  let input = h
    .cx
    .debug_bounds("treq-send-line-comment-composer")
    .expect("composer");
  // The box is in the comments column, beside the block, not under it.
  assert!(
    input.left() >= block.right(),
    "{input:?} not right of {block:?}"
  );
  assert!(
    input.top() >= block.top() && input.top() < block.bottom(),
    "{input:?} not beside {block:?}"
  );
  // Saving it leaves the block where and as tall as it was.
  h.cx.simulate_input("reword");
  h.click("treq-send-line-comment-save");
  h.cx.run_until_parked();
  assert_eq!(
    h.cx
      .debug_bounds("treq-send-block-0")
      .expect("block")
      .size
      .height,
    block.size.height
  );
}

#[gpui::test]
fn lightbox_highlights_an_image_region_for_the_review(cx: &mut TestAppContext) {
  let (mut h, host) = send_harness(cx);
  h.click("terminal-send-preview-i1");
  let surface = h
    .cx
    .debug_bounds("treq-send-highlight-surface")
    .expect("surface");
  let width = f32::from(surface.size.width);
  // A click without a drag zooms in, and again out.
  h.click("treq-send-highlight-surface");
  assert!(h.shows(gpui::Role::Group, "Image at 200%"));
  let zoomed = h.cx.debug_bounds("treq-send-highlight-layer").unwrap();
  assert!((f32::from(zoomed.size.width) - width * 2.).abs() < 1.);
  h.cx.simulate_click(
    zoomed.origin + gpui::point(gpui::px(50.), gpui::px(50.)),
    Modifiers::none(),
  );
  assert!(h.shows(gpui::Role::Image, "Drag to highlight a region"));
  let surface = h.cx.debug_bounds("treq-send-highlight-surface").unwrap();
  let from = surface.origin + gpui::point(gpui::px(10.), gpui::px(10.));
  let to = surface.origin + gpui::point(gpui::px(200.), gpui::px(150.));
  h.cx
    .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
  h.cx
    .simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
  h.cx
    .simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
  assert!(h.shows(gpui::Role::Group, "Region 1"));
  assert!(h.shows(gpui::Role::Group, "Comment on region 1"));
  h.click("treq-send-review-input");
  h.cx.simulate_input("too big");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  // The image can't be read here, so the review goes without it.
  assert!(host
    .calls()
    .contains(&"notify toast Could not attach highlighted image".to_string()));
  let writes = pty_writes(&host);
  assert!(
    writes.iter().any(
      |w| w.contains("File: /tmp/treq-missing-shot.png") && w.contains("Highlights:\\n1. Region")
    ),
    "{writes:?}"
  );
  assert!(!h.shows(gpui::Role::Group, "Region 1"));
}

#[gpui::test]
fn terminal_reports_the_mouse_once_the_program_asks(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  click_terminal(&mut h);
  assert!(pty_writes(&host).is_empty(), "no reports before asked");
  host.output(&pty, "\x1b[?1000h\x1b[?1006h");
  h.cx.run_until_parked();
  click_terminal(&mut h);
  let writes = pty_writes(&host);
  assert_eq!(writes.len(), 2, "{writes:?}");
  assert!(
    writes[0].contains("[<0;") && writes[0].ends_with("M\""),
    "{writes:?}"
  );
  assert!(
    writes[1].contains("[<0;") && writes[1].ends_with("m\""),
    "{writes:?}"
  );
}

#[gpui::test]
fn lightbox_unsent_comments_dialog_keeps_on_escape_and_discards_on_enter(cx: &mut TestAppContext) {
  let (mut h, _) = send_harness(cx);
  h.click("terminal-send-preview-t1");
  h.click("treq-send-review-input");
  h.cx.simulate_input("hmm");
  h.click("treq-send-close");
  assert!(h.shows(gpui::Role::Dialog, "Discard unsent comments?"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.shows(gpui::Role::Dialog, "Discard unsent comments?"));
  assert!(h.shows(gpui::Role::Dialog, "Preview"));
  h.click("treq-send-close");
  assert!(h.shows(gpui::Role::Dialog, "Discard unsent comments?"));
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Dialog, "Preview"));
}

#[gpui::test]
fn agent_terminals_register_for_messages_by_harness_and_session(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  let calls = host.calls();
  assert!(
    calls.contains(&format!(
      "msg register {pty} claude-3 /repo Some(\"feature/login\")"
    )),
    "{calls:?}"
  );
}

/// A terminal whose shell never ran the agent (stuck at `quote>`) is not
/// a running agent: `treq agent msg --list` must not show it.
#[gpui::test]
fn agent_terminals_register_for_messages_only_once_the_agent_starts(cx: &mut TestAppContext) {
  let host = terminal_host();
  host.not_started.lock().unwrap().push(agent_pty());
  let (mut h, host) = workspace_harness(cx, host, terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let registered = |host: &FakeHost| host.calls().iter().any(|c| c.starts_with("msg register"));
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_secs(2));
  h.cx.run_until_parked();
  assert!(!registered(&host), "{:?}", host.calls());

  host.not_started.lock().unwrap().clear();
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_secs(1));
  h.cx.run_until_parked();
  assert!(registered(&host), "{:?}", host.calls());
}

#[gpui::test]
fn typing_into_an_agent_terminal_notes_the_keystroke(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  click_terminal(&mut h);
  h.cx.simulate_input("l");
  assert!(
    host.calls().contains(&format!("msg key {pty}")),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn waiting_messages_show_a_badge_and_a_banner(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let pty = agent_pty();
  assert!(!h.shows(gpui::Role::Status, "1 message from codex-1 is waiting."));
  host.msg_waiting.lock().unwrap().insert(
    pty.clone(),
    terminal_data::WaitingMessages {
      count: 1,
      from: "codex-1".into(),
    },
  );
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_secs(2));
  h.cx.run_until_parked();
  assert!(h.shows(gpui::Role::Status, "1 message from codex-1 is waiting."));
  assert!(h.shows(gpui::Role::Status, "1 message waiting"));
  assert!(
    !h.shows(gpui::Role::Button, "Show message"),
    "the banner has no Show message toggle"
  );
  let screen = h.rendered();
  let banner = screen.get_by_role(
    gpui_testing_library::by::role(gpui::Role::Status).name("1 message from codex-1 is waiting."),
  );
  assert_eq!(
    gpui_testing_library::within(&banner)
      .query_all_by_role(gpui_testing_library::by::role(gpui::Role::Button))
      .len(),
    1,
    "the banner never shows the text, only Deliver"
  );
  h.click("terminal-msg-deliver");
  assert!(
    host.calls().contains(&format!("msg deliver {pty}")),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn a_remote_agent_session_registers_for_messages_and_a_shell_does_not(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, open_remote_command(1));
  assert!(!host
    .calls()
    .iter()
    .any(|c| c.starts_with("msg register remote")));

  let mut agent = open_remote_command(2);
  agent["commands"][0]["remote"]["label"] = "agent-k2".into();
  agent["commands"][0]["remote"]["launch"] =
    serde_json::json!({ "type": "agent", "agent": "codex", "args": [] });
  set_terminal(&mut h, agent);
  let registered: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("msg register remote"))
    .collect();
  assert_eq!(registered.len(), 1, "{:?}", host.calls());
  assert!(registered[0].ends_with(" agent-k2"), "{registered:?}");
}

/// The terminal pane is window-wide: it stays at the bottom while the
/// GitHub view replaces the workspace view.
#[gpui::test]
fn terminal_pane_shows_on_the_github_view(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  assert!(h.shows(gpui::Role::Group, "Terminals"));
  h.update(|s| {
    *s = serde_json::from_value(serde_json::json!({
      "kind": "github",
      "dark": false,
      "repoPath": "/repo",
      "tab": "prs",
      "filter": "open",
      "creating": false,
      "runningChecks": [],
      "globalTerminal": { "repoPath": "/repo", "activeSessionId": 3 },
    }))
    .expect("github route");
  });
  assert!(h.shows(gpui::Role::Group, "Terminals"));
}

/// The name of the terminal column the user last picked.
fn active_column(h: &mut Harness) -> Option<String> {
  use gpui_testing_library::by;
  h.rendered()
    .query_all_by_role(by::role(gpui::Role::Group).selected(true))
    .first()
    .and_then(|column| column.name())
}

const CURRENT: &str = "This workspace's terminal";
const DIMMED: &str = "Another workspace's terminal, dimmed";

/// Whether the terminal column `name` reads as `state`.
fn column_is(h: &mut Harness, name: &str, state: &str) -> bool {
  use gpui_testing_library::by;
  h.rendered()
    .query_by_role(by::role(gpui::Role::Group).name(name).value_text(state))
    .is_some()
}

/// Whether a remote column says its connection was lost.
fn connection_lost(h: &mut Harness) -> bool {
  use gpui_testing_library::by;
  !h.rendered()
    .query_all_by_role(by::role(gpui::Role::Alert))
    .is_empty()
}

/// Whether a remote column says it is attaching.
fn attaching(h: &mut Harness) -> bool {
  h.shows(gpui::Role::Status, "Starting remote session…")
    || h.shows(gpui::Role::Status, "Reattaching…")
}

/// Clicking an agent header's workspace label navigates to that workspace.
#[gpui::test]
fn terminal_workspace_label_click_navigates(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let label = by_id_prefix(&mut h, "terminal-workspace-label-");
  h.rendered().user().click(&label);
  let navigated = h
    .sent()
    .into_iter()
    .any(|a| matches!(a, ScreenAction::Field { field, .. } if field == "terminal.navigate"));
  assert!(navigated, "{:?}", h.sent());
}

/// A session whose workspace is known only by its path (the launch kept
/// it, the workspace row isn't loaded) still navigates to that workspace,
/// not to the main repository.
#[gpui::test]
fn terminal_workspace_label_click_navigates_by_path(cx: &mut TestAppContext) {
  let mut host = terminal_host();
  host.sessions[0].workspace_id = None;
  host.pending.lock().unwrap().insert(
    3,
    launch_data::PendingSession {
      pending_prompt: None,
      permission_mode: None,
      agent: None,
      workspace_path: Some("/repo/.treq/workspaces/b".into()),
    },
  );
  let (mut h, _) = workspace_harness(cx, host, terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  let label = by_id_prefix(&mut h, "terminal-workspace-label-");
  h.rendered().user().click(&label);
  let value = h.sent().into_iter().find_map(|a| match a {
    ScreenAction::Field { field, value } if field == "terminal.navigate" => Some(value),
    _ => None,
  });
  assert_eq!(
    value,
    Some(serde_json::json!({ "key": "/repo/.treq/workspaces/b", "isMainRepo": false }))
  );
}

#[gpui::test]
fn terminal_agent_header_links_to_what_it_reviews(cx: &mut TestAppContext) {
  let mut host = terminal_host();
  host.sessions[0].link_url = Some("https://github.com/o/r/pull/4305".into());
  let (mut h, host) = workspace_harness(cx, host, terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  assert!(h.shows(gpui::Role::Button, "Go to Add login"));
  assert!(h.shows(gpui::Role::Link, "https://github.com/o/r/pull/4305"));
  let link = by_id_prefix(&mut h, "terminal-link-label-");
  h.rendered().user().click(&link);
  assert!(
    host
      .calls()
      .contains(&"open https://github.com/o/r/pull/4305".to_string()),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn terminal_agent_header_has_no_link_without_one(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  assert!(h.shows(gpui::Role::Button, "Go to Add login"));
  // The workspace header's issue link is not the terminal's.
  let in_terminal = h
    .rendered()
    .query_all_by_role(gpui_testing_library::by::role(gpui::Role::Link))
    .into_iter()
    .filter(|link| {
      link
        .path()
        .iter()
        .any(|id| matches!(id, gpui::ElementId::Name(n) if n.starts_with("terminal-column-")))
    })
    .count();
  assert_eq!(in_terminal, 0);
}

#[test]
fn terminal_link_labels_name_the_pull_request() {
  assert_eq!(
    super::super::terminal::link_label("https://github.com/o/r/pull/4305"),
    "#4305"
  );
  assert_eq!(
    super::super::terminal::link_label("https://linear.app/x/issue/ENG-1"),
    "linear.app"
  );
}

/// A file thumbnail is a square chip with its name centered under the icon.
#[gpui::test]
fn send_file_thumbnail_is_square_with_centered_name(cx: &mut TestAppContext) {
  let (mut h, _) = send_harness(cx);
  let chip = h.cx.debug_bounds("terminal-send-preview-t1").expect("chip");
  assert_eq!(chip.size.width, chip.size.height, "square chip");
  let title = h.cx.debug_bounds("terminal-send-name-t1").expect("title");
  assert!(
    title.size.width < chip.size.width - gpui::px(8.),
    "title hugs its text"
  );
  let offset = (title.center().x - chip.center().x).abs();
  assert!(offset < gpui::px(1.), "title centered, off by {offset:?}");
}

/// A long workspace title truncates in a narrow header instead of running
/// under the search and close buttons.
#[gpui::test]
fn terminal_workspace_label_truncates_before_actions(cx: &mut TestAppContext) {
  let mut host = terminal_host();
  for w in &mut host.workspaces {
    w.title = "Exclude products without images from rankings and more".into();
  }
  let (mut h, _) = workspace_harness(cx, host, terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.cx
    .simulate_resize(gpui::size(gpui::px(520.), gpui::px(800.)));
  h.cx.run_until_parked();
  let label = by_id_prefix(&mut h, "terminal-workspace-label-").bounds();
  let search = by_id_prefix(&mut h, "terminal-search-").bounds();
  assert!(
    label.right() <= search.left(),
    "label {label:?} overlaps search {search:?}"
  );
}

/// Whether an element of `role` named `name` is in the tree, shown or not.
fn shows_hidden(h: &mut Harness, role: gpui::Role, name: &str) -> bool {
  h.rendered()
    .query_by_role(gpui_testing_library::by::role(role).name(name).hidden(true))
    .is_some()
}

/// The grid size of the terminal drawing `pty`.
fn grid(host: &Arc<FakeHost>, pty: &str) -> terminal_emu::Size {
  let host: terminal_data::Host = host.clone();
  let (live, _) = terminal_data::attach(&host, pty, 80, 24);
  let size = live.emulator.lock().unwrap().size();
  size
}

/// A divider drag draws a frame per step: each grid fits its new column in
/// the frame the divider moves, not in a second frame after it.
#[gpui::test]
fn terminal_divider_drag_fits_the_grids_in_one_frame(cx: &mut TestAppContext) {
  let (mut h, host, frames) =
    super::idle::counted(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  click_terminal(&mut h);
  h.cx.simulate_keystrokes("cmd-\\");
  h.cx.run_until_parked();
  let ptys = host.ptys.lock().unwrap().clone();
  assert_eq!(ptys.len(), 2);
  let selector = super::changes_large::leak(format!("terminal-{}", ptys[0]));
  let left = h.cx.debug_bounds(selector).expect("left terminal");
  let mut at = gpui::point(left.right() + gpui::px(2.), left.center().y);
  h.cx
    .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
  let drawn = frames.get();
  // A hand moves a few pixels per event, and the divider keeps up.
  for _ in 0..20 {
    at.x -= gpui::px(3.);
    h.cx
      .simulate_mouse_move(at, Some(MouseButton::Left), Modifiers::none());
  }
  let narrowed = h.cx.debug_bounds(selector).expect("left terminal");
  assert_eq!(narrowed.size.width, left.size.width - gpui::px(60.));
  assert_eq!(frames.get() - drawn, 20);
}

/// Dragging the pane's top edge draws a frame per step: the grid fits the
/// new height in the frame the edge moves.
#[gpui::test]
fn terminal_height_drag_fits_the_grid_in_one_frame(cx: &mut TestAppContext) {
  let (mut h, host, frames) =
    super::idle::counted(cx, terminal_host(), terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  click_terminal(&mut h);
  let rows = grid(&host, &agent_pty()).rows;
  let pane = h.cx.debug_bounds("workspace-terminal-pane").expect("pane");
  let mut at = gpui::point(pane.center().x, pane.top() - gpui::px(2.));
  h.cx
    .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
  let drawn = frames.get();
  for _ in 0..20 {
    at.y -= gpui::px(3.);
    h.cx
      .simulate_mouse_move(at, Some(MouseButton::Left), Modifiers::none());
  }
  assert!(grid(&host, &agent_pty()).rows > rows);
  assert_eq!(frames.get() - drawn, 20);
}
