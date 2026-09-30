use super::{
  dispatch_agent_request, handle_cli_command, is_supported_cli_command, normalize_repo_path,
  parse_agent_mode, parse_agent_mode_or_default, parse_remote_command_request,
  workspace_dir_name_from_cwd,
};
use crate::agent_dispatch;
use crate::local_db;
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use tauri_plugin_cli::{Matches, SubcommandMatches};
use tempfile::TempDir;

fn make_subcommand(name: &str) -> SubcommandMatches {
  let mut sub = SubcommandMatches::default();
  sub.name = name.to_string();
  sub.matches = Matches::default();
  sub
}

#[test]
fn unknown_subcommand_is_not_handled_by_cli_dispatch() {
  let subcommand = make_subcommand("open");
  assert!(handle_cli_command(&subcommand).is_none());
}

#[test]
fn supports_new_top_level_commands() {
  assert!(is_supported_cli_command("add"));
  assert!(is_supported_cli_command("set"));
  assert!(is_supported_cli_command("st"));
  assert!(is_supported_cli_command("mv"));
  assert!(is_supported_cli_command("agent"));
  assert!(is_supported_cli_command("repo"));
  assert!(is_supported_cli_command("workspace"));
  assert!(is_supported_cli_command("changes"));
  assert!(is_supported_cli_command("file"));
  assert!(is_supported_cli_command("commits"));
  assert!(is_supported_cli_command("conflicts"));
  assert!(!is_supported_cli_command("open"));
}

#[test]
fn rejects_removed_commands() {
  assert!(!is_supported_cli_command("ls"));
}

#[test]
fn remote_review_commands_are_declared_in_tauri_config() {
  let config =
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json")).unwrap();
  let json: Value = serde_json::from_str(&config).unwrap();
  let commands = json["plugins"]["cli"]["subcommands"].as_object().unwrap();
  for name in [
    "repo",
    "workspace",
    "changes",
    "file",
    "commits",
    "conflicts",
  ] {
    assert!(commands.contains_key(name), "missing CLI command {name}");
  }
}

#[test]
fn commit_is_handled_by_cli_dispatch() {
  let subcommand = make_subcommand("commit");
  assert!(handle_cli_command(&subcommand).is_some());
}

#[test]
fn send_is_handled_by_cli_dispatch() {
  let subcommand = make_subcommand("send");
  assert!(handle_cli_command(&subcommand).is_some());
}

#[test]
fn diff_is_handled_by_cli_dispatch() {
  let subcommand = make_subcommand("diff");
  assert!(handle_cli_command(&subcommand).is_some());
}

#[test]
fn resolve_is_handled_by_cli_dispatch() {
  let subcommand = make_subcommand("resolve");
  assert!(handle_cli_command(&subcommand).is_some());
}

#[test]
fn resolve_subcommand_defines_commit_id_positional() {
  let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
  let config = fs::read_to_string(config_path).expect("failed to read tauri.conf.json");
  let json: Value = serde_json::from_str(&config).expect("failed to parse tauri.conf.json");
  let resolve = json["plugins"]["cli"]["subcommands"]["resolve"]
    .as_object()
    .expect("resolve subcommand must exist");
  let args = resolve
    .get("args")
    .and_then(Value::as_array)
    .expect("resolve args must be an array");

  let commit_id = args
    .iter()
    .find(|arg| arg.get("name").and_then(Value::as_str) == Some("commit_id"))
    .expect("resolve must define commit_id positional arg");
  assert_eq!(commit_id.get("index").and_then(Value::as_i64), Some(1));
  assert_eq!(
    commit_id.get("takesValue").and_then(Value::as_bool),
    Some(true)
  );
  assert_eq!(
    commit_id.get("required").and_then(Value::as_bool),
    Some(true)
  );
}

#[test]
fn send_subcommand_defines_optional_path_positional() {
  let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
  let config = fs::read_to_string(config_path).expect("failed to read tauri.conf.json");
  let json: Value = serde_json::from_str(&config).expect("failed to parse tauri.conf.json");
  let send = json["plugins"]["cli"]["subcommands"]["send"]
    .as_object()
    .expect("send subcommand must exist");
  let args = send
    .get("args")
    .and_then(Value::as_array)
    .expect("send args must be an array");

  let path = args
    .iter()
    .find(|arg| arg.get("name").and_then(Value::as_str) == Some("path"))
    .expect("send must define path positional arg");
  assert_eq!(path.get("index").and_then(Value::as_i64), Some(1));
  assert_eq!(path.get("takesValue").and_then(Value::as_bool), Some(true));
  assert_ne!(path.get("required").and_then(Value::as_bool), Some(true));
}

#[test]
fn diff_subcommand_defines_optional_workspace_name() {
  let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
  let config = fs::read_to_string(config_path).expect("failed to read tauri.conf.json");
  let json: Value = serde_json::from_str(&config).expect("failed to parse tauri.conf.json");
  let diff = json["plugins"]["cli"]["subcommands"]["diff"]
    .as_object()
    .expect("diff subcommand must exist");
  let args = diff
    .get("args")
    .and_then(Value::as_array)
    .expect("diff args must be an array");

  let workspace_name = args
    .iter()
    .find(|arg| arg.get("name").and_then(Value::as_str) == Some("workspace_name"))
    .expect("diff must define workspace_name positional arg");
  assert_eq!(workspace_name.get("index").and_then(Value::as_i64), Some(1));
  assert_eq!(
    workspace_name.get("takesValue").and_then(Value::as_bool),
    Some(true)
  );
  assert_ne!(
    workspace_name.get("required").and_then(Value::as_bool),
    Some(true)
  );
}

#[test]
fn send_subcommand_defines_a_browser_flag() {
  let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
  let config = fs::read_to_string(config_path).expect("failed to read tauri.conf.json");
  let json: Value = serde_json::from_str(&config).expect("failed to parse tauri.conf.json");
  let send = json["plugins"]["cli"]["subcommands"]["send"]
    .as_object()
    .expect("send subcommand must exist");
  let args = send
    .get("args")
    .and_then(Value::as_array)
    .expect("send args must be an array");

  let browser = args
    .iter()
    .find(|arg| arg.get("name").and_then(Value::as_str) == Some("browser"))
    .expect("send must define a browser flag arg");
  assert_ne!(
    browser.get("takesValue").and_then(Value::as_bool),
    Some(true),
    "browser must be a boolean flag, not a value-taking option"
  );
}

#[test]
fn asset_protocol_allows_files_below_hidden_workspace_directories() {
  let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
  let config = fs::read_to_string(config_path).expect("failed to read tauri.conf.json");
  let json: Value = serde_json::from_str(&config).expect("failed to parse tauri.conf.json");
  let scope = &json["app"]["security"]["assetProtocol"]["scope"];

  assert_eq!(
    scope["requireLiteralLeadingDot"].as_bool(),
    Some(false),
    "workspace paths pass through the hidden .treq directory"
  );
}

#[test]
fn parse_agent_mode_maps_edit_and_plan() {
  assert_eq!(
    parse_agent_mode("edit").expect("edit mode should parse"),
    "acceptEdits"
  );
  assert_eq!(
    parse_agent_mode("plan").expect("plan mode should parse"),
    "plan"
  );
}

#[test]
fn parse_agent_mode_defaults_to_edit_when_missing() {
  assert_eq!(
    parse_agent_mode_or_default(None).expect("missing mode should default"),
    "acceptEdits"
  );
}

#[test]
fn parse_agent_mode_or_default_uses_explicit_mode() {
  assert_eq!(
    parse_agent_mode_or_default(Some("plan")).expect("plan should parse"),
    "plan"
  );
}

#[test]
fn parse_agent_mode_rejects_invalid_mode() {
  let error = parse_agent_mode("invalid").expect_err("invalid mode must fail");
  assert!(error.contains("invalid mode"));
}

#[test]
fn normalize_repo_path_falls_back_for_missing_path() {
  let missing = Path::new("/definitely/not/present/treq-missing-path");
  let normalized = normalize_repo_path(missing);
  assert_eq!(normalized, missing.to_string_lossy());
}

#[test]
fn workspace_dir_name_from_cwd_reads_treq_workspaces_component() {
  let nested = Path::new("/repo/.treq/workspaces/feat-ui/src/lib");
  assert_eq!(
    workspace_dir_name_from_cwd(nested).as_deref(),
    Some("feat-ui")
  );
  assert_eq!(
    workspace_dir_name_from_cwd(Path::new("/repo/.treq/workspaces/feat-ui")).as_deref(),
    Some("feat-ui")
  );
  assert_eq!(workspace_dir_name_from_cwd(Path::new("/repo/src")), None);
}

#[test]
fn positional_cli_args_take_values_in_tauri_config() {
  let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
  let config = fs::read_to_string(config_path).expect("failed to read tauri.conf.json");
  let json: Value = serde_json::from_str(&config).expect("failed to parse tauri.conf.json");

  let subcommands = json["plugins"]["cli"]["subcommands"]
    .as_object()
    .expect("plugins.cli.subcommands must be an object");

  for (subcommand_name, subcommand) in subcommands {
    let args = match subcommand.get("args").and_then(Value::as_array) {
      Some(args) => args,
      None => continue,
    };

    for arg in args {
      let has_index = arg.get("index").is_some();
      if !has_index {
        continue;
      }

      let takes_value = arg
        .get("takesValue")
        .and_then(Value::as_bool)
        .unwrap_or(false);
      let arg_name = arg
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");

      assert!(
        takes_value,
        "positional arg '{}' in subcommand '{}' must set takesValue=true",
        arg_name, subcommand_name
      );
    }
  }
}

#[test]
fn mv_subcommand_uses_source_and_destination_positionals() {
  let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
  let config = fs::read_to_string(config_path).expect("failed to read tauri.conf.json");
  let json: Value = serde_json::from_str(&config).expect("failed to parse tauri.conf.json");
  let mv = json["plugins"]["cli"]["subcommands"]["mv"]
    .as_object()
    .expect("mv subcommand must exist");
  let args = mv
    .get("args")
    .and_then(Value::as_array)
    .expect("mv args must be an array");

  let source = args
    .iter()
    .find(|arg| arg.get("name").and_then(Value::as_str) == Some("source"))
    .expect("mv must define source positional arg");
  assert_eq!(source.get("index").and_then(Value::as_i64), Some(1));
  assert_eq!(
    source.get("takesValue").and_then(Value::as_bool),
    Some(true)
  );

  let destination = args
    .iter()
    .find(|arg| arg.get("name").and_then(Value::as_str) == Some("destination"))
    .expect("mv must define destination positional arg");
  assert_eq!(destination.get("index").and_then(Value::as_i64), Some(2));
  assert_eq!(
    destination.get("takesValue").and_then(Value::as_bool),
    Some(true)
  );
}

#[test]
fn commit_subcommand_defines_workspace_message_and_push_args() {
  let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
  let config = fs::read_to_string(config_path).expect("failed to read tauri.conf.json");
  let json: Value = serde_json::from_str(&config).expect("failed to parse tauri.conf.json");
  let commit = json["plugins"]["cli"]["subcommands"]["commit"]
    .as_object()
    .expect("commit subcommand must exist");
  let args = commit
    .get("args")
    .and_then(Value::as_array)
    .expect("commit args must be an array");

  let workspace_name = args
    .iter()
    .find(|arg| arg.get("name").and_then(Value::as_str) == Some("workspace_name"))
    .expect("commit must define workspace_name positional arg");
  assert_eq!(workspace_name.get("index").and_then(Value::as_i64), Some(1));
  assert_eq!(
    workspace_name.get("takesValue").and_then(Value::as_bool),
    Some(true)
  );
  assert_eq!(
    workspace_name.get("required").and_then(Value::as_bool),
    Some(true)
  );

  let message = args
    .iter()
    .find(|arg| arg.get("name").and_then(Value::as_str) == Some("message"))
    .expect("commit must define message arg");
  assert_eq!(message.get("short").and_then(Value::as_str), Some("m"));
  assert_eq!(
    message.get("takesValue").and_then(Value::as_bool),
    Some(true)
  );
  assert_eq!(message.get("required").and_then(Value::as_bool), Some(true));

  let push = args
    .iter()
    .find(|arg| arg.get("name").and_then(Value::as_str) == Some("push"))
    .expect("commit must define push arg");
  assert_eq!(push.get("index"), None, "push must not be positional");
  assert!(
    !push
      .get("takesValue")
      .and_then(Value::as_bool)
      .unwrap_or(false),
    "push must be a boolean flag"
  );
}

fn env_lock() -> &'static Mutex<()> {
  static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
  LOCK.get_or_init(|| Mutex::new(()))
}

fn seed_registry(instances: Vec<agent_dispatch::RegisteredInstance>, repo_path: &str) {
  local_db::init_local_db(repo_path).expect("init local db");
  for instance in instances {
    local_db::upsert_instance_registry(repo_path, instance).expect("upsert instance");
  }
}

#[test]
fn dispatch_agent_request_selects_matching_instance_and_handles_ack() {
  let _guard = env_lock().lock().unwrap();
  let temp = TempDir::new().expect("temp");

  let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
  let endpoint = listener.local_addr().unwrap().to_string();
  let handle = thread::spawn(move || {
    let (mut stream, _) = listener.accept().expect("accept");
    let mut payload = String::new();
    stream.read_to_string(&mut payload).expect("read");
    let request: agent_dispatch::AgentDispatchRequest =
      serde_json::from_str(payload.trim()).expect("json");
    assert_eq!(request.branch, "feat/x");
    let response =
      serde_json::to_string(&agent_dispatch::AgentDispatchResponse::handled()).expect("serialize");
    stream.write_all(response.as_bytes()).expect("write");
  });

  let repo = temp.path().join("repo");
  std::fs::create_dir_all(&repo).expect("mkdir");
  let normalized_repo = agent_dispatch::normalize_repo_path(repo.to_str().unwrap());
  seed_registry(
    vec![agent_dispatch::RegisteredInstance {
      instance_id: "instance-1".to_string(),
      pid: 1,
      started_at: 1,
      last_heartbeat_at: agent_dispatch::now_millis(),
      endpoint,
      windows: vec![agent_dispatch::InstanceWindowSnapshot {
        window_label: "main".to_string(),
        normalized_repo_path: normalized_repo,
        focused: true,
        last_focused_at: Some(agent_dispatch::now_millis()),
        supporting: false,
      }],
    }],
    repo.to_str().unwrap(),
  );

  let result = dispatch_agent_request(
    repo.to_str().unwrap(),
    "feat/x",
    "hello",
    "plan",
    "codex",
    "req-1",
  );
  assert!(result.is_ok());
  handle.join().expect("join");
}

#[test]
fn dispatch_agent_request_returns_error_when_no_matching_instance() {
  let _guard = env_lock().lock().unwrap();
  let temp = TempDir::new().expect("temp");
  let repo = temp.path().join("repo");
  std::fs::create_dir_all(&repo).expect("mkdir");
  seed_registry(vec![], repo.to_str().unwrap());
  let result = dispatch_agent_request(
    "/tmp/unknown-repo",
    "feat/x",
    "hello",
    "plan",
    "codex",
    "req-1",
  );
  assert!(result.is_err());
  assert!(result.unwrap_err().contains("No running Treq instance"));
}

#[test]
fn dispatch_agent_request_surfaces_ack_timeout_error() {
  let _guard = env_lock().lock().unwrap();
  let temp = TempDir::new().expect("temp");

  let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
  let endpoint = listener.local_addr().unwrap().to_string();
  let handle = thread::spawn(move || {
    let (_stream, _) = listener.accept().expect("accept");
    thread::sleep(Duration::from_millis(900));
  });

  let repo = temp.path().join("repo");
  std::fs::create_dir_all(&repo).expect("mkdir");
  let normalized_repo = agent_dispatch::normalize_repo_path(repo.to_str().unwrap());
  seed_registry(
    vec![agent_dispatch::RegisteredInstance {
      instance_id: "instance-timeout".to_string(),
      pid: 1,
      started_at: 1,
      last_heartbeat_at: agent_dispatch::now_millis(),
      endpoint,
      windows: vec![agent_dispatch::InstanceWindowSnapshot {
        window_label: "main".to_string(),
        normalized_repo_path: normalized_repo,
        focused: true,
        last_focused_at: Some(agent_dispatch::now_millis()),
        supporting: false,
      }],
    }],
    repo.to_str().unwrap(),
  );

  let result = dispatch_agent_request(
    repo.to_str().unwrap(),
    "feat/x",
    "hello",
    "plan",
    "codex",
    "req-1",
  );
  assert!(result.is_err());
  let error = result.unwrap_err();
  assert!(
    error.contains("failed reading dispatch response")
      || error.contains("invalid dispatch response payload")
  );
  handle.join().expect("join");
}

fn remote_matches(pairs: &[(&str, &str)]) -> Matches {
  let mut matches = Matches::default();
  for (name, value) in pairs {
    let mut arg = tauri_plugin_cli::ArgData::default();
    arg.value = Value::String((*value).to_string());
    matches.args.insert((*name).to_string(), arg);
  }
  matches
}

#[test]
fn parses_every_newly_exposed_remote_mutation_at_the_cli_boundary() {
  let cases: &[(&str, &[(&str, &str)])] = &[
    (
      "repo",
      &[
        ("action", "init"),
        ("repo", "/tmp/r"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "repo",
      &[
        ("action", "clone"),
        ("repo", "/tmp/dest"),
        ("value", "git@ex:x.git"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "workspace",
      &[
        ("action", "create"),
        ("repo", "/tmp/r"),
        ("value", "feat"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "workspace",
      &[
        ("action", "rename"),
        ("repo", "/tmp/r"),
        ("workspace", "1"),
        ("value", "feat-2"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "workspace",
      &[
        ("action", "update"),
        ("repo", "/tmp/r"),
        ("workspace", "1"),
        ("value", "desc"),
      ],
    ),
    (
      "workspace",
      &[("action", "delete"), ("repo", "/tmp/r"), ("workspace", "1")],
    ),
    (
      "workspace",
      &[
        ("action", "move"),
        ("repo", "/tmp/r"),
        ("workspace", "1"),
        ("target", "2"),
        ("value", "abc"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "workspace",
      &[
        ("action", "rebase"),
        ("repo", "/tmp/r"),
        ("workspace", "1"),
        ("target", "main"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "file",
      &[("action", "restore"), ("repo", "/tmp/r"), ("path", "a.txt")],
    ),
    (
      "file",
      &[
        ("action", "patch"),
        ("repo", "/tmp/r"),
        ("path", "a.txt"),
        ("value", "YQ=="),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "commits",
      &[
        ("action", "create"),
        ("repo", "/tmp/r"),
        ("value", "msg"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "commits",
      &[
        ("action", "describe"),
        ("repo", "/tmp/r"),
        ("workspace", "1"),
        ("target", "abc"),
        ("value", "msg"),
      ],
    ),
    (
      "commits",
      &[
        ("action", "split"),
        ("repo", "/tmp/r"),
        ("workspace", "1"),
        ("target", "abc"),
        ("value", "a.rs"),
        ("path", "b.rs:1-2"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "commits",
      &[
        ("action", "move"),
        ("repo", "/tmp/r"),
        ("workspace", "1"),
        ("target", "abc"),
        ("value", "2"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "commits",
      &[
        ("action", "abandon"),
        ("repo", "/tmp/r"),
        ("workspace", "1"),
        ("target", "abc"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "conflicts",
      &[
        ("action", "resolve"),
        ("repo", "/tmp/r"),
        ("target", "rev"),
        ("value", "ours"),
        ("idempotency-key", "k1"),
      ],
    ),
    ("git", &[("action", "fetch"), ("repo", "/tmp/r")]),
    (
      "git",
      &[
        ("action", "bookmark-track"),
        ("repo", "/tmp/r"),
        ("value", "main"),
        ("target", "origin"),
      ],
    ),
    (
      "git",
      &[
        ("action", "push"),
        ("repo", "/tmp/r"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "agent-remote",
      &[
        ("action", "start"),
        ("repo", "/tmp/r"),
        ("workspace", "1"),
        ("target", "claude"),
        ("value", "hi"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "agent-remote",
      &[
        ("action", "input"),
        ("repo", "/tmp/r"),
        ("workspace", "1"),
        ("value", "more"),
        ("idempotency-key", "k1"),
      ],
    ),
    (
      "agent-remote",
      &[("action", "status"), ("repo", "/tmp/r"), ("workspace", "1")],
    ),
    (
      "agent-remote",
      &[("action", "stop"), ("repo", "/tmp/r"), ("workspace", "1")],
    ),
    (
      "agent-remote",
      &[("action", "logs"), ("repo", "/tmp/r"), ("workspace", "1")],
    ),
  ];
  for (command, args) in cases {
    parse_remote_command_request(command, &remote_matches(args))
      .unwrap_or_else(|e| panic!("{command} {args:?}: {e}"));
  }
}

#[test]
fn parses_repo_usage_into_machine_usage_request() {
  let request = parse_remote_command_request(
    "repo",
    &remote_matches(&[("action", "usage"), ("repo", "/srv/repos")]),
  )
  .unwrap();
  assert_eq!(
    request,
    crate::core::remote::TreqCommandRequest::MachineUsage {
      root: "/srv/repos".into()
    }
  );
}

#[test]
fn rejects_unknown_remote_action_and_arbitrary_command_kinds() {
  let error = parse_remote_command_request(
    "repo",
    &remote_matches(&[("action", "exec-shell"), ("repo", "/tmp/r")]),
  )
  .unwrap_err();
  assert!(error.contains("unknown repo action"));
}

#[test]
fn create_commit_requires_idempotency_key_at_cli_boundary() {
  let error = parse_remote_command_request(
    "commits",
    &remote_matches(&[("action", "create"), ("repo", "/tmp/r"), ("value", "msg")]),
  )
  .unwrap_err();
  assert!(error.contains("idempotency-key is required"));
}

#[test]
fn describe_commit_allows_missing_idempotency_key_at_cli_boundary() {
  parse_remote_command_request(
    "commits",
    &remote_matches(&[
      ("action", "describe"),
      ("repo", "/tmp/r"),
      ("workspace", "1"),
      ("target", "abc"),
      ("value", "msg"),
    ]),
  )
  .unwrap();
}

fn create_workspace_metadata(pairs: &[(&str, &str)]) -> Result<Option<String>, String> {
  let mut all = vec![
    ("action", "create"),
    ("repo", "/tmp/r"),
    ("value", "feat"),
    ("idempotency-key", "k1"),
  ];
  all.extend_from_slice(pairs);
  match parse_remote_command_request("workspace", &remote_matches(&all))? {
    crate::core::remote::TreqCommandRequest::CreateWorkspace { metadata, .. } => Ok(metadata),
    other => panic!("unexpected request {other:?}"),
  }
}

#[test]
fn workspace_create_applies_the_title_flag() {
  let metadata = create_workspace_metadata(&[
    ("title", "My title"),
    ("metadata", r#"{"description":"d"}"#),
  ])
  .unwrap();
  let parsed = crate::core::workspaces::parse_workspace_metadata(metadata.as_deref());
  assert_eq!(parsed.title.as_deref(), Some("My title"));
  assert_eq!(parsed.description.as_deref(), Some("d"));
}

#[test]
fn workspace_create_rejects_invalid_metadata_json() {
  for bad in ["not json", "[1]"] {
    let error = create_workspace_metadata(&[("metadata", bad)]).unwrap_err();
    assert!(error.starts_with("invalid_arguments:"), "{error}");
  }
}

/// One sample of every typed remote command, used to check the wire format
/// end to end: `cli_args` must only emit flags the remote CLI declares, and
/// parsing those args back must give the same request.
fn every_typed_remote_request() -> Vec<crate::core::remote::TreqCommandRequest> {
  use crate::core::remote::{FileRevision, TreqCommandRequest as R};
  use crate::core::remote_pty::PtyLaunchSpec;
  use crate::core::workspaces::HunkSpec;
  let repo = || "/srv/r".to_string();
  let key = || "k1".to_string();
  let hunk = || HunkSpec {
    file_path: "c.rs".into(),
    start_line: 3,
    end_line: 5,
  };
  vec![
    R::InspectRepository { repo: repo() },
    R::RepositoryStatus { repo: repo() },
    R::ListBranches { repo: repo() },
    R::ListWorkspaces { repo: repo() },
    R::InspectWorkspace {
      repo: repo(),
      workspace: "1".into(),
    },
    R::ListChanges {
      repo: repo(),
      workspace: Some("1".into()),
    },
    R::DiffFile {
      repo: repo(),
      workspace: Some("1".into()),
      path: "a.rs".into(),
    },
    R::ReadFile {
      repo: repo(),
      workspace: Some("1".into()),
      path: "a.rs".into(),
      revision: FileRevision::Parent,
      start_line: Some(1),
      end_line: Some(20),
    },
    R::ListCommits {
      repo: repo(),
      workspace: None,
    },
    R::ListConflicts {
      repo: repo(),
      workspace: Some("1".into()),
    },
    R::WorkspaceDiff {
      repo: repo(),
      workspace: "1".into(),
    },
    R::CommitDiff {
      repo: repo(),
      workspace: Some("1".into()),
      revision: "abc".into(),
    },
    R::CommitFileDiff {
      repo: repo(),
      workspace: Some("1".into()),
      revision: "abc".into(),
      path: "a.rs".into(),
    },
    R::SearchFiles {
      repo: repo(),
      workspace: Some("1".into()),
      query: "main".into(),
      limit: Some(4),
    },
    R::WorkspaceChangeMarker {
      repo: repo(),
      workspace: Some("1".into()),
    },
    R::MachineUsage { root: repo() },
    R::ProbeRepo { repo: repo() },
    R::CloneRepo {
      repo_url: "git@example.com:x.git".into(),
      destination: repo(),
      idempotency_key: key(),
    },
    R::InitRepo {
      repo: repo(),
      idempotency_key: key(),
    },
    R::CreateWorkspace {
      repo: repo(),
      branch_name: "feat".into(),
      source_branch: Some("main".into()),
      metadata: Some(r#"{"title":"T"}"#.into()),
      idempotency_key: key(),
    },
    R::RenameWorkspace {
      repo: repo(),
      workspace: "1".into(),
      new_name: "feat-2".into(),
      idempotency_key: key(),
    },
    R::UpdateWorkspace {
      repo: repo(),
      workspace: "1".into(),
      target_branch: Some("main".into()),
      title: Some("Title".into()),
      description: Some("desc".into()),
    },
    R::DeleteWorkspace {
      repo: repo(),
      workspace: "1".into(),
    },
    R::MoveWorkspaceChanges {
      repo: repo(),
      workspace: "feat-a".into(),
      destination: "feat-b".into(),
      files: vec!["a.rs".into(), "b.rs".into()],
      hunks: vec![hunk()],
      commits: vec!["abc".into()],
      idempotency_key: key(),
    },
    R::RebaseWorkspace {
      repo: repo(),
      workspace: "1".into(),
      target_branch: "main".into(),
      idempotency_key: key(),
    },
    R::RestoreFile {
      repo: repo(),
      workspace: Some("1".into()),
      path: "a.rs".into(),
    },
    R::PatchFile {
      repo: repo(),
      workspace: Some("1".into()),
      path: "a.rs".into(),
      patch_base64: "YQ==".into(),
      idempotency_key: key(),
    },
    R::CreateCommit {
      repo: repo(),
      workspace: Some("1".into()),
      message: "wip".into(),
      base_change_id: Some("base00000000".into()),
      idempotency_key: key(),
    },
    R::DescribeCommit {
      repo: repo(),
      workspace: "1".into(),
      commit: "abc".into(),
      message: "m".into(),
    },
    R::SplitCommit {
      repo: repo(),
      workspace: "1".into(),
      commit: "@".into(),
      files: vec!["a.rs".into()],
      hunks: vec![hunk()],
      message: "first".into(),
      idempotency_key: key(),
    },
    R::MoveCommit {
      repo: repo(),
      workspace: "1".into(),
      commit: "abc".into(),
      target_workspace: "2".into(),
      idempotency_key: key(),
    },
    R::AbandonCommit {
      repo: repo(),
      workspace: "1".into(),
      commit: "abc".into(),
      idempotency_key: key(),
    },
    R::ResolveConflict {
      repo: repo(),
      revision: "abc".into(),
      sides: vec!["ours".into()],
      idempotency_key: key(),
    },
    R::GitFetch { repo: repo() },
    R::GitBookmarkTrack {
      repo: repo(),
      bookmark: "main".into(),
      remote_name: "origin".into(),
    },
    R::GitPush {
      repo: repo(),
      workspace: Some("1".into()),
      idempotency_key: key(),
    },
    R::AgentStart {
      repo: repo(),
      workspace: "1".into(),
      agent: "claude".into(),
      prompt: "hi".into(),
      idempotency_key: key(),
    },
    R::AgentInput {
      repo: repo(),
      workspace: "1".into(),
      input: "more".into(),
      idempotency_key: key(),
    },
    R::AgentStatus {
      repo: repo(),
      workspace: "1".into(),
    },
    R::AgentStop {
      repo: repo(),
      workspace: "1".into(),
    },
    R::AgentLogs {
      repo: repo(),
      workspace: "1".into(),
    },
    R::PtyStart {
      repo: repo(),
      workspace: "1".into(),
      label: "shell".into(),
      remote_dir: repo(),
      launch: PtyLaunchSpec::Shell,
      cols: 80,
      rows: 24,
      idempotency_key: key(),
    },
    R::PtyList {
      repo: repo(),
      workspace: Some("1".into()),
    },
    R::PtyStop {
      repo: repo(),
      workspace: "1".into(),
      label: "shell".into(),
    },
    R::PtyAttachCommand {
      repo: repo(),
      workspace: "1".into(),
      label: "shell".into(),
      remote_dir: repo(),
      launch: PtyLaunchSpec::Shell,
      cols: 80,
      rows: 24,
    },
  ]
}

/// Splits `cli_args` output into the subcommand name and its `--flag value`
/// pairs, with the positional action under `action`.
fn cli_args_to_pairs(args: &[String]) -> (String, Vec<(String, String)>) {
  let mut pairs = vec![("action".to_string(), args[1].clone())];
  let mut rest = args[2..].iter();
  while let Some(flag) = rest.next() {
    let name = flag
      .strip_prefix("--")
      .unwrap_or_else(|| panic!("expected a flag, got {flag}"));
    let value = rest.next().expect("every flag takes a value").clone();
    pairs.push((name.to_string(), value));
  }
  (args[0].clone(), pairs)
}

#[test]
fn every_typed_remote_request_covers_every_kind() {
  let kinds: Vec<_> = every_typed_remote_request()
    .iter()
    .map(|r| r.kind_name())
    .collect();
  for kind in crate::core::remote::TreqCommandRequest::KIND_NAMES {
    assert!(kinds.contains(kind), "missing sample for {kind}");
  }
}

#[test]
fn every_remote_cli_flag_is_declared_in_tauri_config() {
  // The remote VM parses argv with the CLI config in tauri.conf.json, which
  // rejects undeclared flags. A flag emitted by `cli_args` but missing there
  // would make the command fail only over SSH.
  let config =
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json")).unwrap();
  let json: Value = serde_json::from_str(&config).unwrap();
  let subcommands = &json["plugins"]["cli"]["subcommands"];
  for request in every_typed_remote_request() {
    let args = request.cli_args().unwrap();
    let (command, pairs) = cli_args_to_pairs(&args);
    let declared: Vec<&str> = subcommands[&command]["args"]
      .as_array()
      .unwrap_or_else(|| panic!("missing CLI subcommand {command}"))
      .iter()
      .filter_map(|arg| arg["name"].as_str())
      .collect();
    for (name, _) in pairs {
      assert!(
        declared.contains(&name.as_str()),
        "{} emits --{name}, which `{command}` does not declare",
        request.kind_name()
      );
    }
  }
}

#[test]
fn every_typed_remote_request_round_trips_through_cli_args() {
  // Fields dropped between `cli_args` and the VM-side parser silently change
  // what a remote mutation does (for example a move losing its files).
  for request in every_typed_remote_request() {
    // `repo inspect` has its own handler with human-readable output.
    if request.kind_name() == "InspectRepository" {
      continue;
    }
    let args = request.cli_args().unwrap();
    let (command, pairs) = cli_args_to_pairs(&args);
    let borrowed: Vec<(&str, &str)> = pairs
      .iter()
      .filter(|(name, _)| name != "format")
      .map(|(n, v)| (n.as_str(), v.as_str()))
      .collect();
    let parsed = parse_remote_command_request(&command, &remote_matches(&borrowed))
      .unwrap_or_else(|e| panic!("{}: {e}", request.kind_name()));
    assert_eq!(
      parsed,
      request,
      "{} did not round-trip",
      request.kind_name()
    );
  }
}

#[cfg(unix)]
#[test]
fn symlink_report_separates_created_links_from_missing_sources() {
  let dir = tempfile::tempdir().unwrap();
  std::os::unix::fs::symlink(dir.path(), dir.path().join("node_modules")).unwrap();
  let (linked, missing) = super::workspace_handlers::split_symlinks(
    dir.path(),
    &["node_modules".to_string(), "target".to_string()],
  );
  assert_eq!(linked, vec!["node_modules".to_string()]);
  assert_eq!(missing, vec!["target".to_string()]);
}

#[cfg(feature = "tauri-test")]
mod commit_outcomes {
  use super::super::workspace_handlers::commit_workspace_for_cli;
  use crate::e2e_test_helpers::TestRepo;

  #[test]
  fn refuses_when_there_is_nothing_to_commit() {
    let repo = TestRepo::new().unwrap();
    let ws = repo.create_workspace_simple("feat/empty").unwrap();
    let err = commit_workspace_for_cli(&repo.repo_path, &ws, "msg", false).unwrap_err();
    assert!(err.contains("nothing to commit"), "{err}");
  }

  #[test]
  fn refuses_a_whitespace_only_message() {
    let repo = TestRepo::new().unwrap();
    let ws = repo.create_workspace_simple("feat/blank").unwrap();
    TestRepo::write_workspace_file(&repo.workspace_full_path(&ws), "a.txt", "a\n").unwrap();
    let err = commit_workspace_for_cli(&repo.repo_path, &ws, "   ", false).unwrap_err();
    assert!(err.contains("message"), "{err}");
  }

  #[test]
  fn a_failed_push_says_the_commit_landed() {
    let repo = TestRepo::new().unwrap();
    let ws = repo.create_workspace_simple("feat/push").unwrap();
    TestRepo::write_workspace_file(&repo.workspace_full_path(&ws), "a.txt", "a\n").unwrap();
    let err = commit_workspace_for_cli(&repo.repo_path, &ws, "msg", true).unwrap_err();
    assert!(
      err.contains("committed to 'feat/push'") && err.contains("push failed"),
      "{err}"
    );
  }
}

mod repo_root_detection {
  use super::super::find_repo_root;
  use std::fs;

  #[test]
  fn a_git_worktree_with_a_dot_git_file_is_a_repo_root() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = dir.path().join("wt");
    fs::create_dir_all(worktree.join("src")).unwrap();
    fs::write(
      worktree.join(".git"),
      "gitdir: /elsewhere/.git/worktrees/wt\n",
    )
    .unwrap();
    assert_eq!(find_repo_root(&worktree.join("src")), Some(worktree));
  }

  #[test]
  fn a_jj_only_repo_is_a_repo_root() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".jj")).unwrap();
    assert_eq!(find_repo_root(dir.path()), Some(dir.path().to_path_buf()));
  }

  #[test]
  fn a_treq_workspace_resolves_to_its_home_repo() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    let workspace = dir.path().join(".treq/workspaces/feat-a");
    fs::create_dir_all(workspace.join(".jj")).unwrap();
    fs::create_dir_all(workspace.join("src")).unwrap();
    assert_eq!(
      find_repo_root(&workspace.join("src")),
      Some(dir.path().to_path_buf())
    );
  }

  #[test]
  fn no_repo_markers_means_no_root() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(find_repo_root(dir.path()), None);
  }
}

#[cfg(unix)]
mod implicit_stdin {
  use super::super::fd_has_input;
  use std::time::{Duration, Instant};

  fn pipe() -> (i32, i32) {
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    (fds[0], fds[1])
  }

  #[test]
  fn an_idle_open_pipe_is_not_input() {
    let (read, write) = pipe();
    let started = Instant::now();
    assert!(!fd_has_input(read, Duration::from_millis(50)));
    assert!(started.elapsed() < Duration::from_secs(5));
    unsafe {
      libc::close(read);
      libc::close(write);
    }
  }

  #[test]
  fn a_pipe_with_data_or_eof_is_input() {
    let (read, write) = pipe();
    assert_eq!(unsafe { libc::write(write, b"{}".as_ptr().cast(), 2) }, 2);
    assert!(fd_has_input(read, Duration::from_millis(50)));
    unsafe { libc::close(read) };

    let (read, write) = pipe();
    unsafe { libc::close(write) };
    assert!(fd_has_input(read, Duration::from_millis(50)));
    unsafe { libc::close(read) };
  }
}

mod structured_errors {
  use super::super::run_structured_command;
  use super::remote_matches;

  fn run(pairs: &[(&str, &str)], result: Result<(), String>) -> (bool, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let ok = run_structured_command(&remote_matches(pairs), &mut out, &mut err, |_| {
      result.clone()
    });
    (
      ok,
      String::from_utf8(out).unwrap(),
      String::from_utf8(err).unwrap(),
    )
  }

  #[test]
  fn argument_errors_print_a_json_error_body() {
    let (ok, out, err) = run(&[("format", "json")], Err("--workspace is required".into()));
    assert!(!ok);
    assert!(err.is_empty(), "{err}");
    let body: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(body["error"]["code"], "invalid_arguments");
    assert_eq!(body["error"]["message"], "--workspace is required");
  }

  #[test]
  fn argument_errors_print_to_stderr_in_human_format() {
    let (ok, out, err) = run(&[], Err("unknown workspace action 'x'".into()));
    assert!(!ok);
    assert!(out.is_empty(), "{out}");
    assert_eq!(err.trim(), "Error: unknown workspace action 'x'");
  }

  #[test]
  fn an_invalid_format_is_reported() {
    let (ok, _, err) = run(&[("format", "yaml")], Ok(()));
    assert!(!ok);
    assert!(err.contains("invalid format 'yaml'"), "{err}");
  }

  #[test]
  fn success_prints_nothing_extra() {
    let (ok, out, err) = run(&[("format", "json")], Ok(()));
    assert!(ok);
    assert!(out.is_empty() && err.is_empty());
  }
}

mod repo_arg_resolution {
  use super::*;

  fn parsed_repo(repo: &str) -> String {
    match parse_remote_command_request(
      "changes",
      &remote_matches(&[("action", "list"), ("repo", repo)]),
    )
    .unwrap()
    {
      crate::core::remote::TreqCommandRequest::ListChanges { repo, .. } => repo,
      other => panic!("unexpected request {other:?}"),
    }
  }

  #[test]
  fn a_workspace_path_resolves_to_its_home_repo() {
    assert_eq!(parsed_repo("/srv/r/.treq/workspaces/feat-a"), "/srv/r");
    assert_eq!(parsed_repo("/srv/r/.treq/workspaces/feat-a/src"), "/srv/r");
  }

  #[test]
  fn repo_paths_are_absolute_without_dots_or_trailing_slashes() {
    assert_eq!(parsed_repo("/srv/r"), "/srv/r");
    assert_eq!(parsed_repo("/srv/r/"), "/srv/r");
    assert_eq!(parsed_repo("/srv/./x/../r"), "/srv/r");
    let cwd = std::env::current_dir().unwrap();
    assert_eq!(parsed_repo("."), cwd.to_str().unwrap());
  }

  #[test]
  fn a_clone_destination_inside_a_workspace_is_kept() {
    let request = parse_remote_command_request(
      "repo",
      &remote_matches(&[
        ("action", "clone"),
        ("repo", "/srv/r/.treq/workspaces/feat-a/vendor/"),
        ("value", "git@ex:x.git"),
        ("idempotency-key", "k1"),
      ]),
    )
    .unwrap();
    assert!(matches!(
      request,
      crate::core::remote::TreqCommandRequest::CloneRepo { destination, .. }
        if destination == "/srv/r/.treq/workspaces/feat-a/vendor"
    ));
  }
}

mod help_text {
  use super::*;
  use crate::cli::args::subcommand_help;

  fn cli_commands() -> serde_json::Map<String, Value> {
    let config =
      fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json")).unwrap();
    let json: Value = serde_json::from_str(&config).unwrap();
    json["plugins"]["cli"]["subcommands"]
      .as_object()
      .unwrap()
      .clone()
  }

  #[test]
  fn every_command_and_arg_is_described() {
    for (name, command) in cli_commands() {
      assert!(
        command["description"].is_string(),
        "{name} has no description"
      );
      for arg in command["args"].as_array().into_iter().flatten() {
        assert!(
          arg["description"].is_string(),
          "{name} {} has no description",
          arg["name"]
        );
      }
    }
  }

  #[test]
  fn structured_command_help_lists_every_action_and_an_example() {
    let mut actions: Vec<(String, String)> = every_typed_remote_request()
      .iter()
      .map(|request| {
        let args = request.cli_args().unwrap();
        (args[0].clone(), args[1].clone())
      })
      .collect();
    for action in ["add", "list", "resolve", "delete"] {
      actions.push(("agent-review".into(), action.into()));
    }
    for (command, action) in actions {
      let help = subcommand_help(&command);
      assert!(
        help.contains(action.as_str()),
        "`treq {command} --help` omits {action}"
      );
      assert!(
        help.contains("Example"),
        "`treq {command} --help` has no example"
      );
    }
  }

  #[test]
  fn diff_help_says_it_shows_conflicts_not_a_diff() {
    assert!(subcommand_help("diff").contains("changes workspace-diff"));
  }
}

mod numeric_args {
  use super::*;

  fn file_read(extra: &[(&str, &str)]) -> Result<crate::core::remote::TreqCommandRequest, String> {
    let mut args = vec![("action", "read"), ("repo", "/tmp/r"), ("path", "a.rs")];
    args.extend_from_slice(extra);
    parse_remote_command_request("file", &remote_matches(&args))
  }

  #[test]
  fn file_read_rejects_line_zero_and_reversed_ranges() {
    file_read(&[("start-line", "2"), ("end-line", "2")]).unwrap();
    for extra in [
      &[("start-line", "0")][..],
      &[("end-line", "0")][..],
      &[("start-line", "5"), ("end-line", "2")][..],
    ] {
      let error = file_read(extra).unwrap_err();
      assert!(
        error.starts_with("invalid_arguments:"),
        "{extra:?}: {error}"
      );
    }
  }

  #[test]
  fn file_search_rejects_a_zero_limit() {
    let error = parse_remote_command_request(
      "file",
      &remote_matches(&[("action", "search"), ("repo", "/tmp/r"), ("limit", "0")]),
    )
    .unwrap_err();
    assert!(
      error.contains("--limit must be a positive integer"),
      "{error}"
    );
  }

  #[test]
  fn negative_numbers_parse_as_values_and_are_rejected() {
    let matches = crate::cli::args::parse([
      "treq",
      "file",
      "read",
      "--repo",
      "/tmp/r",
      "--path",
      "a.rs",
      "--start-line",
      "-1",
    ])
    .unwrap();
    let sub = matches.subcommand.unwrap();
    let error = parse_remote_command_request("file", &sub.matches).unwrap_err();
    assert!(
      error.contains("--start-line must be a positive integer"),
      "{error}"
    );
  }
}

mod notify {
  use super::{handle_cli_command, is_supported_cli_command, make_subcommand};
  use crate::cli::workspace_handlers::parse_notify_args;
  use crate::notify_dispatch::NotifyEvent;

  fn notify_matches(args: &[&str]) -> tauri_plugin_cli::Matches {
    let argv = ["treq", "notify"].into_iter().chain(args.iter().copied());
    crate::cli::args::parse(argv)
      .expect("notify arguments parse")
      .subcommand
      .expect("notify subcommand")
      .matches
  }

  #[test]
  fn notify_is_a_supported_cli_command() {
    assert!(is_supported_cli_command("notify"));
    assert!(handle_cli_command(&make_subcommand("notify")).is_some());
  }

  #[test]
  fn parses_message_into_a_message_event() {
    let matches = notify_matches(&["Tests pass.\nReady for review"]);
    assert_eq!(
      parse_notify_args(&matches),
      Ok((
        NotifyEvent::Message,
        Some("Tests pass. Ready for review".to_string())
      ))
    );
  }

  #[test]
  fn parses_agent_exited_flag_without_a_message() {
    let matches = notify_matches(&["--agent-exited"]);
    assert_eq!(
      parse_notify_args(&matches),
      Ok((NotifyEvent::AgentExited, None))
    );
  }

  #[test]
  fn rejects_missing_or_blank_message() {
    assert!(parse_notify_args(&notify_matches(&[])).is_err());
    assert!(parse_notify_args(&notify_matches(&["   "])).is_err());
  }

  #[test]
  fn rejects_message_combined_with_agent_exited() {
    let error = parse_notify_args(&notify_matches(&["hi", "--agent-exited"])).unwrap_err();
    assert!(error.contains("--agent-exited"), "{error}");
  }

  #[test]
  fn notify_without_arguments_exits_with_failure() {
    assert_eq!(crate::cli::args::run(["treq", "notify"]), 1);
  }

  /// Dispatches to a fake app that replies `response`. Returns the result and the JSON it got.
  fn dispatch_to_fake_app(
    response: crate::agent_dispatch::AgentDispatchResponse,
  ) -> (Result<Option<String>, String>, serde_json::Value) {
    use crate::agent_dispatch::{
      normalize_repo_path, now_millis, InstanceWindowSnapshot, RegisteredInstance,
    };
    use crate::notify_dispatch::{NotifyDispatchRequest, NOTIFY_KIND};
    use std::io::{Read, Write};

    let _guard = super::env_lock().lock().unwrap();
    let temp = super::TempDir::new().expect("temp");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("mkdir");
    let repo = repo.to_str().unwrap().to_string();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = listener.local_addr().unwrap().to_string();
    let app = std::thread::spawn(move || {
      let (mut stream, _) = listener.accept().expect("accept");
      let mut payload = String::new();
      stream.read_to_string(&mut payload).expect("read");
      let reply = serde_json::to_string(&response).expect("serialize");
      stream.write_all(reply.as_bytes()).expect("write");
      serde_json::from_str::<serde_json::Value>(&payload).expect("json")
    });
    super::seed_registry(
      vec![RegisteredInstance {
        instance_id: "instance-1".to_string(),
        pid: 1,
        started_at: 1,
        last_heartbeat_at: now_millis(),
        endpoint,
        windows: vec![InstanceWindowSnapshot {
          window_label: "main".to_string(),
          normalized_repo_path: normalize_repo_path(&repo),
          focused: false,
          last_focused_at: None,
        }],
      }],
      &repo,
    );

    let result = crate::cli::dispatch_notify_request(&NotifyDispatchRequest {
      kind: NOTIFY_KIND.to_string(),
      request_id: "notify-1".to_string(),
      repo,
      workspace: Some("feat/login".to_string()),
      event: NotifyEvent::AgentExited,
      message: None,
    });
    (result, app.join().expect("join"))
  }

  #[test]
  fn dispatch_notify_request_sends_notify_json_to_the_app() {
    let (result, payload) =
      dispatch_to_fake_app(crate::agent_dispatch::AgentDispatchResponse::handled());
    assert_eq!(result, Ok(None));
    assert_eq!(payload["kind"], "notify");
    assert_eq!(payload["event"], "agent_exited");
    assert_eq!(payload["workspace"], "feat/login");
  }

  #[test]
  fn dispatch_notify_request_returns_the_apps_skip_reason() {
    let (result, _) = dispatch_to_fake_app(crate::agent_dispatch::AgentDispatchResponse::skipped(
      "a Treq window is focused",
    ));
    assert_eq!(result, Ok(Some("a Treq window is focused".to_string())));
  }
}

fn parsed_subcommand(args: &[&str]) -> SubcommandMatches {
  let matches = super::args::parse(args.iter().copied()).expect("args should parse");
  *matches.subcommand.expect("subcommand")
}

#[test]
fn parses_repo_flag_on_workspace_commands() {
  for args in [
    &["treq", "add", "feat/x", "-r", "svc"][..],
    &["treq", "agent", "feat/x", "do it", "--repo", "svc"][..],
    &["treq", "st", "-r", "svc"][..],
    &["treq", "commit", "feat/x", "-r", "svc", "-m", "msg"][..],
  ] {
    let sub = parsed_subcommand(args);
    assert_eq!(
      sub.matches.args.get("repo").and_then(|a| a.value.as_str()),
      Some("svc"),
      "{args:?}"
    );
  }
}

#[test]
fn mv_keeps_short_r_for_ranges_and_takes_long_repo() {
  let sub = parsed_subcommand(&["treq", "mv", "a", "b", "--repo", "svc", "-r", "f.rs:1-2"]);
  assert_eq!(
    sub.matches.args.get("repo").and_then(|a| a.value.as_str()),
    Some("svc")
  );
  assert!(sub
    .matches
    .args
    .get("ranges")
    .is_some_and(|a| a.occurrences == 1));
}

#[test]
fn agent_rejects_home_repo_branch() {
  let sub = parsed_subcommand(&["treq", "agent", ".", "do it", "-r", "svc"]);
  assert!(!super::workspace_handlers::handle_workspace_agent(
    &sub.matches
  ));
}
