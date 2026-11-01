//! Property tests for the CLI: argv parsing, argument helpers, status output
//! and the dispatch requests the CLI sends to the running app.
//! Cases are bounded for CI; set PROPTEST_CASES for a longer run.

use super::agent_review_handlers::strip_suggestion_fence;
use super::args::parse;
use super::status_output::{format_pr_status_lines, format_workspace_stack_lines};
use super::{absolute_repo_arg, classify_cli_error, split_csv, workspace_dir_name_from_cwd};
use crate::agent_dispatch::{
  AgentDispatchRequest, AgentDispatchResponse, InstanceWindowSnapshot, RegisteredInstance,
};
use crate::notify_dispatch::{normalize_message, NotifyDispatchRequest, NotifyEvent};
use crate::send_dispatch::{SendArtifactRecord, SendDispatchRequest};
use proptest::prelude::*;
use std::path::{Component, Path, PathBuf};

/// Same knobs as `tests/common::proptest_config`: a fixed seed, and
/// PROPTEST_CASES / PROPTEST_SEED to override.
fn config(cases: u32) -> ProptestConfig {
  let env_u64 = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok());
  ProptestConfig {
    cases: env_u64("PROPTEST_CASES").map_or(cases, |n| n as u32),
    rng_seed: proptest::test_runner::RngSeed::Fixed(
      env_u64("PROPTEST_SEED").unwrap_or(0x636c_6921),
    ),
    failure_persistence: None,
    ..ProptestConfig::default()
  }
}

/// argv tokens: real subcommands and flags mixed with junk, unicode and `--`.
fn token() -> impl Strategy<Value = String> {
  prop_oneof![
    prop::sample::select(vec![
      "add",
      "commit",
      "st",
      "workspace",
      "send",
      "notify",
      "agent-review",
      "list",
      "status",
      "diff",
      "--",
      "-",
      "--help",
      "-h",
      "--version",
      "-m",
      "-d",
      "-k",
      "--push",
      "--repo",
      "--format",
      "json",
      "human",
      "-1",
      "--=",
      "",
      " ",
      "feat/x",
    ])
    .prop_map(str::to_string),
    "\\PC{0,12}",
    "-{1,2}[a-z\\-]{0,10}",
  ]
}

fn opt_text() -> impl Strategy<Value = Option<String>> {
  prop::option::of("\\PC{0,20}")
}

fn workspace(
  id: i64,
  branch: String,
  target: Option<String>,
  title: String,
) -> crate::local_db::Workspace {
  crate::local_db::Workspace {
    id,
    repo_path: "/repo".to_string(),
    workspace_name: branch.clone(),
    workspace_path: branch.clone(),
    branch_name: branch,
    created_at: String::new(),
    refreshed_at: None,
    metadata: None,
    target_branch: target,
    title,
    description: None,
    moved_files: None,
    not_on_remote: false,
    sparse_patterns: None,
    archived: false,
  }
}

proptest! {
  #![proptest_config(config(256))]

  #[test]
  fn parse_never_panics_on_arbitrary_argv(rest in prop::collection::vec(token(), 0..8)) {
    let argv = std::iter::once("treq".to_string()).chain(rest);
    let _ = parse(argv);
  }

  #[cfg(unix)]
  #[test]
  fn parse_never_panics_on_non_utf8_argv(raw in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..8), 0..5)) {
    use std::os::unix::ffi::OsStringExt;
    let mut argv = vec![std::ffi::OsString::from("treq")];
    argv.extend(raw.into_iter().map(std::ffi::OsString::from_vec));
    let _ = parse(argv);
  }

  #[test]
  fn parse_counts_repeated_flags_without_overflow(n in 1usize..400) {
    let mut argv = vec!["treq", "commit", "feat/x", "-m", "msg"];
    argv.extend(std::iter::repeat_n("--push", n));
    let sub = parse(argv).unwrap().subcommand.unwrap();
    prop_assert_eq!(&sub.matches.args["push"].value, &serde_json::Value::Bool(true));
    prop_assert!(sub.matches.args["push"].occurrences >= 1);
  }

  /// Values survive the trip through argv: what the user typed is what the
  /// handler sees, including unicode and repeated `-k`.
  #[test]
  fn add_values_round_trip_through_argv(
    branch in "[^-\\s\\p{C}][^\\p{C}]{0,15}",
    desc in "[^-\\p{C}][^\\p{C}]{0,15}",
    links in prop::collection::vec("[^-\\p{C}][^\\p{C}]{0,8}", 0..4),
  ) {
    let mut argv: Vec<String> = vec!["treq".into(), "add".into(), branch.clone(), "-d".into(), desc.clone()];
    for link in &links {
      argv.push("-k".into());
      argv.push(link.clone());
    }
    let sub = parse(argv).unwrap().subcommand.unwrap();
    prop_assert_eq!(sub.matches.args["branch_name"].value.as_str(), Some(branch.as_str()));
    prop_assert_eq!(sub.matches.args["description"].value.as_str(), Some(desc.as_str()));
    let got: Vec<String> = sub.matches.args["symlink"].value.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    prop_assert_eq!(got, links);
  }

  /// After `--`, a value starting with `-` is still a positional.
  #[test]
  fn double_dash_passes_dash_values_as_positionals(branch in "-[^\\p{C}]{0,10}") {
    let sub = parse(["treq".to_string(), "add".into(), "--".into(), branch.clone()]).unwrap().subcommand.unwrap();
    prop_assert_eq!(sub.matches.args["branch_name"].value.as_str(), Some(branch.as_str()));
  }

  #[test]
  fn classify_cli_error_always_returns_a_code(message in "\\PC{0,60}") {
    let code = classify_cli_error(&message);
    prop_assert!(!code.is_empty() && !code.contains(' '));
  }

  #[test]
  fn split_csv_yields_trimmed_non_empty_items(raw in opt_text()) {
    for item in split_csv(raw) {
      prop_assert!(!item.is_empty());
      prop_assert!(!item.contains(','));
      prop_assert_eq!(item.trim(), item.as_str());
    }
  }

  #[test]
  fn absolute_repo_arg_is_absolute_normalized_and_idempotent(raw in "[a-z./~]{1,20}") {
    let out = absolute_repo_arg(&raw).unwrap();
    let path = Path::new(&out);
    prop_assert!(path.is_absolute(), "{out}");
    prop_assert!(path.components().all(|c| !matches!(c, Component::CurDir | Component::ParentDir)));
    prop_assert_eq!(absolute_repo_arg(&out).unwrap(), out.clone());
  }

  #[test]
  fn absolute_repo_arg_never_accepts_a_blank_value(raw in "[ \t\r\n]{0,8}") {
    let error = absolute_repo_arg(&raw).unwrap_err();
    prop_assert_eq!(classify_cli_error(&error), "invalid_arguments");
  }

  #[test]
  fn workspace_dir_name_found_under_treq_workspaces(
    prefix in prop::collection::vec("[a-z]{1,6}", 0..3),
    name in "[^/\\p{C}.][^/\\p{C}]{0,10}",
    tail in prop::collection::vec("[a-z]{1,6}", 0..3),
  ) {
    let mut path = PathBuf::from("/");
    path.extend(&prefix);
    path.push(".treq");
    path.push("workspaces");
    path.push(&name);
    path.extend(&tail);
    prop_assert_eq!(workspace_dir_name_from_cwd(&path), Some(name));
  }

  #[test]
  fn workspace_dir_name_never_panics(raw in "[a-z./]{0,30}") {
    let _ = workspace_dir_name_from_cwd(Path::new(&raw));
  }

  #[test]
  fn suggestion_fence_round_trips(body in "[^\\p{C}`]{0,20}(\n[^\\p{C}`]{0,20}){0,3}") {
    let body = body.trim_end().to_string();
    prop_assume!(!body.starts_with('\n'));
    let fenced = format!("```suggestion\n{body}\n```");
    prop_assert_eq!(strip_suggestion_fence(&fenced), body);
  }

  #[test]
  fn unfenced_text_is_returned_unchanged(text in "\\PC{0,40}") {
    prop_assume!(!text.trim().starts_with("```suggestion"));
    prop_assert_eq!(strip_suggestion_fence(&text), text);
  }

  #[test]
  fn normalize_message_is_one_bounded_line_and_idempotent(raw in "(\\PC|\\s){0,300}") {
    if let Some(line) = normalize_message(&raw) {
      prop_assert!(!line.is_empty());
      prop_assert!(line.chars().count() <= crate::notify_dispatch::MAX_MESSAGE_CHARS);
      prop_assert!(!line.contains('\n'));
      prop_assert_eq!(normalize_message(&line), Some(line));
    } else {
      prop_assert!(raw.trim().is_empty());
    }
  }

  /// Every workspace of an acyclic stack is printed exactly once, at any
  /// title length or script.
  #[test]
  fn stack_prints_each_acyclic_workspace_once(
    nodes in prop::collection::vec((prop::option::of(any::<prop::sample::Index>()), "\\PC{0,30}"), 1..7),
  ) {
    let statuses: Vec<_> = nodes.iter().enumerate().map(|(i, (parent, title))| {
      // Parents only point at earlier entries, so the graph has no cycle.
      let target = match parent {
        Some(ix) if i > 0 => Some(format!("b{}", ix.index(i))),
        _ => Some("main".to_string()),
      };
      crate::core::WorkspaceSidebarStatus {
        current: workspace(i as i64, format!("b{i}"), target, title.clone()),
        has_conflicts: false,
        last_activity_at: None,
      }
    }).collect();
    let lines = format_workspace_stack_lines(&statuses);
    prop_assert_eq!(&lines[0], "Stack:");
    for i in 0..statuses.len() {
      let needle = format!("● b{i} — ");
      prop_assert_eq!(lines.iter().filter(|l| l.contains(&needle)).count(), 1, "{:?}", lines);
    }
  }

  #[test]
  fn pr_status_lines_never_panic(id in "\\PC{0,20}", state in "\\PC{0,10}", n in any::<[u32; 4]>()) {
    let checks = crate::github::PrCiStatus {
      state,
      total: n[0], passed: n[1], failed: n[2], pending: n[3],
      checks: Vec::new(),
    };
    let lines = format_pr_status_lines(&super::status_output::WorkspacePrStatus { github_id: id, checks: Some(checks) }, "  ");
    prop_assert_eq!(lines.len(), 2);
  }

  #[test]
  fn notify_request_json_round_trips(
    request_id in "\\PC{0,20}", repo in "\\PC{0,20}", workspace in opt_text(),
    exited in any::<bool>(), message in opt_text(),
  ) {
    let req = NotifyDispatchRequest {
      kind: "notify".into(), request_id, repo, workspace,
      event: if exited { NotifyEvent::AgentExited } else { NotifyEvent::Message }, message,
    };
    let back: NotifyDispatchRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
    prop_assert_eq!(back, req);
  }

  #[test]
  fn send_request_and_record_json_round_trip(
    request_id in "\\PC{0,20}", repo in "\\PC{0,20}", pty in opt_text(),
    media in "\\PC{0,8}", path in "\\PC{0,20}", title in opt_text(),
  ) {
    let mut req = SendDispatchRequest::new(request_id, repo, media, path);
    req.pty_session_id = pty;
    req.title = title;
    let back: SendDispatchRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
    prop_assert_eq!(&back, &req);
    let record = SendArtifactRecord::from_request(&req);
    let line = serde_json::to_string(&record).unwrap();
    prop_assert!(!line.contains('\n'), "manifest records must be one line");
    let back: SendArtifactRecord = serde_json::from_str(&line).unwrap();
    prop_assert_eq!(back, record);
  }

  #[test]
  fn agent_dispatch_types_json_round_trip(
    fields in prop::array::uniform6("\\PC{0,16}"), reason in opt_text(),
    pid in any::<u32>(), times in any::<[u64; 3]>(), focused in any::<bool>(), supporting in any::<bool>(),
  ) {
    let [request_id, repo, branch, prompt, mode, agent] = fields;
    let req = AgentDispatchRequest { request_id, repo, branch, prompt, mode, agent };
    let back: AgentDispatchRequest = serde_json::from_slice(&serde_json::to_vec(&req).unwrap()).unwrap();
    prop_assert_eq!(&back, &req);

    let resp = AgentDispatchResponse { status: req.mode.clone(), reason };
    let back: AgentDispatchResponse = serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
    prop_assert_eq!(back, resp);

    let instance = RegisteredInstance {
      instance_id: req.agent.clone(), pid, started_at: times[0], last_heartbeat_at: times[1],
      endpoint: req.repo.clone(),
      windows: vec![InstanceWindowSnapshot {
        window_label: req.branch.clone(), normalized_repo_path: req.prompt,
        focused, last_focused_at: Some(times[2]), supporting,
      }],
    };
    let back: RegisteredInstance = serde_json::from_str(&serde_json::to_string(&instance).unwrap()).unwrap();
    prop_assert_eq!(back, instance);
  }

  /// The request JSON from another process must never panic the parser.
  #[test]
  fn dispatch_requests_reject_garbage_without_panicking(raw in "\\PC{0,80}") {
    let _ = serde_json::from_str::<NotifyDispatchRequest>(&raw);
    let _ = serde_json::from_str::<SendDispatchRequest>(&raw);
    let _ = serde_json::from_str::<AgentDispatchRequest>(&raw);
  }
}
