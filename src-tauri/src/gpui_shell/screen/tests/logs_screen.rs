use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn logs_tab_lists_the_check_logs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(None));
  for selector in [
    "logs-tab",
    "repo-logs-output",
    "repo-log-line",
    "log-level-filter",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(
    host
      .calls()
      .contains(&"read check logs None None Some([])".to_string()),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn logs_tab_opens_on_the_setup_scripts_logs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(Some("setup-script")));
  assert!(h.drawn("repo-logs-output"));
  assert!(
    h.drawn("check-workflow-filter"),
    "the filter shows what is picked"
  );
  assert!(
    host
      .calls()
      .contains(&r#"read check logs None None Some(["__setup__"])"#.to_string()),
    "{:?}",
    host.calls()
  );
}

#[gpui::test]
fn logs_tab_sends_a_picked_line_to_an_agent(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(None));
  assert!(!h.drawn("log-inspect-panel"));
  h.click("repo-log-line");
  assert!(h.drawn("log-inspect-panel"));
  assert!(h.drawn("selection-count"));
  h.click("send-to-agent");
  h.cx.run_until_parked();
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1);
  assert_eq!(launches[0].0.session_name.as_deref(), Some("Logs"));
  assert_eq!(launches[0].0.mode.as_deref(), Some("plan"));
  let prompt = launches[0].0.prompt.clone().unwrap_or_default();
  assert!(
    prompt.starts_with("Here are 1 log line from my check logs."),
    "{prompt}"
  );
  assert!(prompt.contains("  build/Test  "), "{prompt}");
}

#[gpui::test]
fn logs_tab_opens_on_the_source_react_asks_for(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(Some("system")));
  assert!(h.drawn("system-logs-output"));
  h.click("open-log-folder");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"open log folder".to_string()));
}

#[gpui::test]
fn logs_tab_switches_source_from_its_menu(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(None));
  assert!(h.drawn("repo-logs-output"));
  h.click("log-source");
  h.pick("System");
  assert!(h.drawn("system-logs-output"));
  assert!(host.calls().contains(&"read system logs None".to_string()));
}

#[gpui::test]
fn logs_tab_shows_review_history(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(Some("reviews")));
  assert!(h.drawn("review-history-output"));
  assert!(host
    .calls()
    .contains(&"read review history None".to_string()));
  h.click("log-source");
  h.pick("System");
  h.click("log-source");
  h.pick("Reviews");
  assert!(h.drawn("review-history-output"));
}

#[gpui::test]
fn logs_tab_shows_an_agent_chat(cx: &mut TestAppContext) {
  use crate::core::agent_chat::{AgentChat, AgentType, ChatMessage, ChatRole};
  let mut host = logs_host();
  host.chats = vec![AgentChat {
    session_id: 4,
    pty_session_id: "p".into(),
    name: "Fix login".into(),
    agent: AgentType::Claude,
    workspace_id: Some(7),
    created_at: String::new(),
    screen_before_last_user_message: String::new(),
    messages: vec![ChatMessage {
      id: 1,
      role: ChatRole::User,
      message: "Why does login fail?".into(),
      time: "2026-10-19T10:00:00Z".into(),
    }],
  }];
  let (mut h, host) = workspace_harness(cx, host, logs_route(Some("agent-chats")));
  assert!(h.drawn("agent-chat-output"));
  assert!(h.drawn("agent-chat-line"));
  assert!(host.calls().contains(&"read agent chat 4".to_string()));
}

#[gpui::test]
fn logs_tab_runs_a_query_in_the_explorer(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(None));
  h.click("logs-explorer-view");
  assert!(h.drawn("logs-sql-explorer"));
  assert!(h.drawn("sql-editor"));
  h.click("run-query");
  h.cx.run_until_parked();
  assert!(
    host.calls().contains(
      &"run sql SELECT timestamp, severityText, json_extract(attributes, '$.job_id') AS job_id,"
        .to_string()
    ),
    "{:?}",
    host.calls()
  );
  assert!(h.drawn("sql-error"));
  assert!(!h.drawn("send-resultset-to-agent"));
}

#[gpui::test]
fn logs_tab_sends_a_result_set_to_an_agent(cx: &mut TestAppContext) {
  let mut host = logs_host();
  host.sql = Some(Ok(crate::core::checks_logs::SqlResult {
    columns: vec!["job_id".into()],
    rows: vec![vec![Some("build".into())]],
    row_count: 1,
  }));
  let (mut h, host) = workspace_harness(cx, host, logs_route(None));
  h.click("logs-explorer-view");
  h.click("run-query");
  h.cx.run_until_parked();
  assert!(h.drawn("sql-results"));
  h.click("send-resultset-to-agent");
  h.cx.run_until_parked();
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1);
  let prompt = launches[0].0.prompt.clone().unwrap_or_default();
  assert!(prompt.contains("Result (1 row):"), "{prompt}");
}
