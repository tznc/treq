//! `treq agent msg`: what the command line asks, sent to the treq instance
//! that runs this terminal.

use super::args::Matches;
use crate::core::agent_msg::coordinator::SendRequest;
use crate::core::agent_msg::model::{self, Kind};
use crate::core::agent_msg::protocol::Op;
use serde_json::Value;
use std::io::Write;
use std::time::Duration;

/// `--ask` exits with this when no reply came in time.
pub const TIMED_OUT_EXIT: i32 = 3;

/// How often `--ask` and `--inbox --wait` look again.
const POLL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
  Send {
    to: Option<String>,
    all: bool,
    repo: Option<String>,
    body: String,
    ask: bool,
    timeout: Duration,
  },
  Reply {
    id: String,
    body: String,
  },
  Inbox {
    peek: bool,
    wait: bool,
    kinds: Vec<Kind>,
  },
  Ack {
    ids: Vec<String>,
  },
  Status {
    to: Option<String>,
    body: String,
  },
  List {
    repo: Option<String>,
  },
}

/// Reads the flags into one [`Command`], refusing combinations that mean
/// nothing.
pub fn parse(matches: &Matches) -> Result<Command, String> {
  let text = |name: &str| {
    matches
      .args
      .get(name)
      .and_then(|arg| arg.value.as_str())
      .map(str::trim)
      .filter(|value| !value.is_empty())
      .map(str::to_string)
  };
  let flag = |name: &str| {
    matches
      .args
      .get(name)
      .and_then(|arg| arg.value.as_bool())
      .unwrap_or(false)
  };
  let values = |name: &str| -> Vec<String> {
    match matches.args.get(name).map(|arg| &arg.value) {
      Some(Value::Array(values)) => values
        .iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect(),
      Some(Value::String(value)) => vec![value.clone()],
      _ => Vec::new(),
    }
  };
  let positional = values("args");
  let reply = text("reply");
  let status = text("status");
  let acks = values("ack");
  let (inbox, list) = (flag("inbox"), flag("list"));
  let modes = [reply.is_some(), inbox, !acks.is_empty(), status.is_some(), list];
  if modes.iter().filter(|on| **on).count() > 1 {
    return Err("use one of --reply, --inbox, --ack, --status and --list at a time".into());
  }
  let refuse = |flags: &[&str], mode: &str| -> Result<(), String> {
    for name in flags {
      if flag(name) || text(name).is_some() {
        return Err(format!("--{name} does not go with {mode}"));
      }
    }
    Ok(())
  };
  let no_positional = |mode: &str| -> Result<(), String> {
    if positional.is_empty() {
      Ok(())
    } else {
      Err(format!("{mode} takes no agent or text"))
    }
  };
  let body_of = |value: &str| -> Result<String, String> {
    let body = value.trim();
    if body.is_empty() {
      Err("the message text is empty".into())
    } else {
      Ok(body.to_string())
    }
  };
  let send_flags = ["all", "ask", "timeout"];
  let inbox_flags = ["peek", "wait", "types"];

  if let Some(id) = reply {
    refuse(&send_flags, "--reply")?;
    refuse(&inbox_flags, "--reply")?;
    refuse(&["note"], "--reply")?;
    let [body] = positional.as_slice() else {
      return Err("usage: treq agent msg --reply <id> <text>".into());
    };
    return Ok(Command::Reply {
      id,
      body: body_of(body)?,
    });
  }
  if inbox {
    refuse(&send_flags, "--inbox")?;
    refuse(&["note"], "--inbox")?;
    no_positional("--inbox")?;
    let kinds = match text("types") {
      None => Vec::new(),
      Some(types) => types
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
          Kind::parse(name).ok_or_else(|| {
            format!("unknown type '{name}'. Use message, ask, reply or status")
          })
        })
        .collect::<Result<_, _>>()?,
    };
    return Ok(Command::Inbox {
      peek: flag("peek"),
      wait: flag("wait"),
      kinds,
    });
  }
  refuse(&inbox_flags, "this command (they go with --inbox)")?;
  if !acks.is_empty() {
    refuse(&send_flags, "--ack")?;
    refuse(&["note"], "--ack")?;
    no_positional("--ack")?;
    return Ok(Command::Ack { ids: acks });
  }
  if let Some(state) = status {
    refuse(&send_flags, "--status")?;
    if !["working", "blocked", "done"].contains(&state.as_str()) {
      return Err(format!("unknown status '{state}'. Use working, blocked or done"));
    }
    let to = match positional.as_slice() {
      [] => None,
      [to] => Some(to.clone()),
      _ => return Err("usage: treq agent msg --status <state> [--note <text>] [<agent>]".into()),
    };
    let body = match text("note") {
      Some(note) => format!("{state}: {note}"),
      None => state,
    };
    return Ok(Command::Status { to, body });
  }
  refuse(&["note"], "this command (it goes with --status)")?;
  if list {
    refuse(&send_flags, "--list")?;
    no_positional("--list")?;
    return Ok(Command::List { repo: text("repo") });
  }

  let all = flag("all");
  let ask = flag("ask");
  if all && ask {
    return Err("--ask waits for one reply, so it takes one agent, not --all".into());
  }
  let (to, body) = match (all, positional.as_slice()) {
    (true, [body]) => (None, body_of(body)?),
    (false, [to, body]) => (Some(to.clone()), body_of(body)?),
    (true, _) => return Err("usage: treq agent msg --all [--repo <repo>] <text>".into()),
    (false, _) => return Err("usage: treq agent msg <agent> <text>".into()),
  };
  let timeout = match text("timeout") {
    Some(value) => model::parse_timeout(&value)?,
    None => model::TIMEOUT,
  };
  Ok(Command::Send {
    to,
    all,
    repo: text("repo"),
    body,
    ask,
    timeout,
  })
}

/// `treq agent msg`: the exit code.
pub(super) fn handle(matches: &Matches) -> i32 {
  let command = match parse(matches) {
    Ok(command) => command,
    Err(error) => {
      super::log_cli_error(&format!("Error: {error}"));
      eprintln!("See: treq agent msg --help");
      return 1;
    }
  };
  let json = matches
    .args
    .get("json")
    .and_then(|arg| arg.value.as_bool())
    .unwrap_or(false);
  let result = super::detect_repo_path().and_then(|repo| {
    let instance = super::repo_instance(&repo)?;
    let client = IpcClient {
      endpoint: instance.endpoint,
      repo,
    };
    let pty = crate::send_dispatch::pty_session_id_from_env();
    run(
      &command,
      pty.as_deref(),
      json,
      &client,
      &mut std::io::stdout(),
      &std::thread::sleep,
    )
  });
  match result {
    Ok(code) => code,
    Err(error) => {
      super::log_cli_error(&format!("Error: {error}"));
      1
    }
  }
}

/// The instance this terminal belongs to, over its IPC endpoint.
struct IpcClient {
  endpoint: String,
  repo: String,
}

impl Client for IpcClient {
  fn call(&self, op: Op) -> Result<Value, String> {
    let request = crate::core::agent_msg::protocol::Request {
      kind: crate::core::agent_msg::protocol::KIND.into(),
      request_id: format!("msg-{}", chrono::Utc::now().timestamp_millis()),
      repo: self.repo.clone(),
      op,
    };
    crate::core::agent_msg::ipc::call(&self.endpoint, &request, Duration::from_secs(5))
  }
}

/// Sends one request to this terminal's treq instance; `Err` carries the
/// instance's reason.
pub trait Client {
  fn call(&self, op: Op) -> Result<Value, String>;
}

/// Runs `command` for the terminal `pty`, writing what it prints to `out`.
/// Returns the exit code.
pub fn run(
  command: &Command,
  pty: Option<&str>,
  json: bool,
  client: &dyn Client,
  out: &mut dyn Write,
  sleep: &dyn Fn(Duration),
) -> Result<i32, String> {
  let write = |out: &mut dyn Write, text: String| {
    out
      .write_all(text.as_bytes())
      .map_err(|e| format!("failed to write output: {e}"))
  };
  let pretty = |value: &Value| serde_json::to_string_pretty(value).unwrap_or_default() + "\n";
  let me = || {
    pty.map(str::to_string).ok_or_else(|| {
      "this works only in a terminal treq launched for an agent (TREQ_PTY_SESSION_ID is not set)"
        .to_string()
    })
  };
  match command {
    Command::List { repo } => {
      let data = client.call(Op::List {
        repo: repo.clone(),
        local_only: false,
      })?;
      if json {
        write(out, pretty(&data))?;
      } else {
        write(out, agents_table(&data))?;
      }
      Ok(0)
    }
    Command::Send {
      to,
      all,
      repo,
      body,
      ask,
      timeout,
    } => {
      let from_pty = me()?;
      let data = client.call(Op::Send(SendRequest {
        from_pty: from_pty.clone(),
        to: to.clone(),
        all: *all,
        repo: repo.clone(),
        kind: if *ask { Kind::Ask } else { Kind::Message },
        body: body.clone(),
        reply_to: None,
        timeout_ms: timeout.as_millis() as u64,
      }))?;
      let id = data["id"].as_str().unwrap_or_default().to_string();
      if !*ask {
        write(out, if json { pretty(&data) } else { sent_lines(&data) })?;
        return Ok(0);
      }
      if !json {
        write(out, sent_lines(&data))?;
        write(out, format!("… waiting for a reply to {id}\n"))?;
      }
      let polls = (timeout.as_millis() / POLL.as_millis()).max(1);
      for _ in 0..polls {
        let reply = client.call(Op::TakeReply {
          pty: from_pty.clone(),
          id: id.clone(),
        })?;
        if !reply.is_null() {
          let text = reply["body"].as_str().unwrap_or_default();
          if json {
            write(
              out,
              pretty(&serde_json::json!({
                "id": id,
                "state": "replied",
                "from": reply["from"],
                "reply_id": reply["id"],
                "reply": text,
              })),
            )?;
          } else {
            write(out, format!("{} replied: {text}\n", reply["from"].as_str().unwrap_or("?")))?;
          }
          return Ok(0);
        }
        sleep(POLL);
      }
      if json {
        write(out, pretty(&serde_json::json!({ "id": id, "state": "timed_out" })))?;
      } else {
        write(out, format!("timed out waiting for a reply to {id}\n"))?;
      }
      Ok(TIMED_OUT_EXIT)
    }
    Command::Reply { id, body } => {
      let data = client.call(Op::Reply {
        pty: me()?,
        id: id.clone(),
        body: body.clone(),
      })?;
      write(out, if json { pretty(&data) } else { sent_lines(&data) })?;
      Ok(0)
    }
    Command::Inbox { peek, wait, kinds } => {
      let pty = me()?;
      let polls = if *wait {
        (model::TIMEOUT.as_millis() / POLL.as_millis()).max(1)
      } else {
        1
      };
      let mut messages = Vec::new();
      for poll in 0..polls {
        let data = client.call(Op::Inbox {
          pty: pty.clone(),
          peek: *peek,
        })?;
        messages = data["messages"]
          .as_array()
          .cloned()
          .unwrap_or_default()
          .into_iter()
          .filter(|message| {
            kinds.is_empty()
              || message["kind"]
                .as_str()
                .and_then(Kind::parse)
                .is_some_and(|kind| kinds.contains(&kind))
          })
          .collect();
        if !messages.is_empty() || poll + 1 == polls {
          break;
        }
        sleep(POLL);
      }
      if json {
        write(out, pretty(&Value::Array(messages)))?;
      } else {
        write(out, inbox_lines(&messages))?;
      }
      Ok(0)
    }
    Command::Ack { ids } => {
      let data = client.call(Op::Ack {
        pty: me()?,
        ids: ids.clone(),
      })?;
      if json {
        write(out, pretty(&data))?;
      } else {
        write(out, format!("acked {}\n", data["acked"].as_u64().unwrap_or(0)))?;
      }
      Ok(0)
    }
    Command::Status { to, body } => {
      let data = client.call(Op::Status {
        pty: me()?,
        to: to.clone(),
        body: body.clone(),
      })?;
      write(out, if json { pretty(&data) } else { sent_lines(&data) })?;
      Ok(0)
    }
  }
}

/// `m-1 → 2 agents` and one line per target.
fn sent_lines(data: &Value) -> String {
  let targets = data["targets"].as_array().cloned().unwrap_or_default();
  let mut text = format!(
    "{} → {} agent{}\n",
    data["id"].as_str().unwrap_or("?"),
    targets.len(),
    if targets.len() == 1 { "" } else { "s" }
  );
  for target in &targets {
    text.push_str(&format!(
      "  {:<12} {} ({})\n",
      target["handle"].as_str().unwrap_or("?"),
      target["outcome"].as_str().unwrap_or("?"),
      target["via"].as_str().unwrap_or("?"),
    ));
  }
  text
}

fn inbox_lines(messages: &[Value]) -> String {
  if messages.is_empty() {
    return "No messages.\n".into();
  }
  let mut text = String::new();
  for message in messages {
    text.push_str(&format!(
      "{}  {} from {}\n  {}\n",
      message["id"].as_str().unwrap_or("?"),
      message["kind"].as_str().unwrap_or("message"),
      message["from"].as_str().unwrap_or("?"),
      message["body"].as_str().unwrap_or_default().replace('\n', "\n  "),
    ));
  }
  text.push_str(
    "\nAck: treq agent msg --ack <id>   Reply: treq agent msg --reply <id> \"<text>\"\n",
  );
  text
}

fn agents_table(data: &Value) -> String {
  let agents = data["agents"].as_array().cloned().unwrap_or_default();
  if agents.is_empty() {
    return "No agents to message.\n".into();
  }
  let mut text = format!(
    "{:<12} {:<8} {:<24} {:<24} {:<22} WAITING\n",
    "HANDLE", "HARNESS", "REPO", "WORKSPACE", "VIA"
  );
  for agent in &agents {
    text.push_str(&format!(
      "{:<12} {:<8} {:<24} {:<24} {:<22} {}\n",
      agent["handle"].as_str().unwrap_or("?"),
      agent["harness"].as_str().unwrap_or("?"),
      agent["repo"].as_str().unwrap_or("?"),
      agent["workspace"].as_str().unwrap_or("-"),
      agent["via"].as_str().unwrap_or("?"),
      agent["waiting"].as_u64().unwrap_or(0),
    ));
  }
  text
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cli::args;
  use serde_json::json;
  use std::cell::RefCell;

  fn parsed(argv: &[&str]) -> Result<Command, String> {
    let mut full = vec!["treq", "agent", "msg"];
    full.extend_from_slice(argv);
    let matches = args::parse(full).map_err(|e| e.to_string())?;
    let agent = matches.subcommand.expect("agent");
    let msg = agent.matches.subcommand.expect("msg");
    assert_eq!(msg.name, "msg");
    parse(&msg.matches)
  }

  #[test]
  fn parses_a_message_to_one_agent() {
    assert_eq!(
      parsed(&["claude-2", "rebase done?"]),
      Ok(Command::Send {
        to: Some("claude-2".into()),
        all: false,
        repo: None,
        body: "rebase done?".into(),
        ask: false,
        timeout: Duration::from_secs(60),
      })
    );
  }

  #[test]
  fn parses_an_ask_with_a_timeout_and_repo() {
    assert_eq!(
      parsed(&["cursor-4", "hit rate?", "--ask", "--timeout", "30s", "-r", "app"]),
      Ok(Command::Send {
        to: Some("cursor-4".into()),
        all: false,
        repo: Some("app".into()),
        body: "hit rate?".into(),
        ask: true,
        timeout: Duration::from_secs(30),
      })
    );
  }

  #[test]
  fn parses_a_broadcast_with_only_the_text() {
    assert_eq!(
      parsed(&["--all", "--repo", "app", "main moved"]),
      Ok(Command::Send {
        to: None,
        all: true,
        repo: Some("app".into()),
        body: "main moved".into(),
        ask: false,
        timeout: Duration::from_secs(60),
      })
    );
  }

  #[test]
  fn refuses_a_timeout_over_a_minute() {
    assert!(parsed(&["claude-2", "hi", "--timeout", "2m"]).is_err());
  }

  #[test]
  fn refuses_a_send_without_text_or_target() {
    assert!(parsed(&["claude-2"]).is_err());
    assert!(parsed(&[]).is_err());
    assert!(parsed(&["--all"]).is_err());
    assert!(parsed(&["claude-2", "  "]).is_err());
    assert!(parsed(&["--all", "claude-2", "hi"]).is_err());
  }

  #[test]
  fn refuses_ask_with_all() {
    assert!(parsed(&["--all", "hi", "--ask"]).is_err());
  }

  #[test]
  fn parses_reply_inbox_ack_status_and_list() {
    assert_eq!(
      parsed(&["--reply", "m-1", "done"]),
      Ok(Command::Reply {
        id: "m-1".into(),
        body: "done".into()
      })
    );
    assert_eq!(
      parsed(&["--inbox", "--peek", "--wait", "--types", "ask, status"]),
      Ok(Command::Inbox {
        peek: true,
        wait: true,
        kinds: vec![Kind::Ask, Kind::Status]
      })
    );
    assert_eq!(
      parsed(&["--ack", "m-1", "--ack", "m-2"]),
      Ok(Command::Ack {
        ids: vec!["m-1".into(), "m-2".into()]
      })
    );
    assert_eq!(
      parsed(&["--status", "done", "--note", "rebased", "codex-1"]),
      Ok(Command::Status {
        to: Some("codex-1".into()),
        body: "done: rebased".into()
      })
    );
    assert_eq!(
      parsed(&["--status", "working"]),
      Ok(Command::Status {
        to: None,
        body: "working".into()
      })
    );
    assert_eq!(parsed(&["--list", "-r", "app"]), Ok(Command::List { repo: Some("app".into()) }));
  }

  #[test]
  fn refuses_mixed_modes_and_stray_flags() {
    assert!(parsed(&["--inbox", "--list"]).is_err());
    assert!(parsed(&["--reply", "m-1", "x", "--ack", "m-2"]).is_err());
    assert!(parsed(&["--status", "sleeping"]).is_err());
    assert!(parsed(&["--inbox", "claude-2"]).is_err());
    assert!(parsed(&["--peek"]).is_err());
    assert!(parsed(&["claude-2", "hi", "--note", "x"]).is_err());
    assert!(parsed(&["--inbox", "--types", "gossip"]).is_err());
  }

  struct FakeClient {
    answers: RefCell<Vec<Result<Value, String>>>,
    calls: RefCell<Vec<Op>>,
  }

  impl FakeClient {
    fn new(answers: Vec<Result<Value, String>>) -> Self {
      Self {
        answers: RefCell::new(answers),
        calls: RefCell::new(Vec::new()),
      }
    }
  }

  impl Client for FakeClient {
    fn call(&self, op: Op) -> Result<Value, String> {
      self.calls.borrow_mut().push(op);
      self.answers.borrow_mut().remove(0)
    }
  }

  fn run_with(command: Command, json: bool, client: &FakeClient) -> (Result<i32, String>, String) {
    let mut out = Vec::new();
    let slept = RefCell::new(0);
    let code = run(&command, Some("pty-codex"), json, client, &mut out, &|_| {
      *slept.borrow_mut() += 1;
    });
    (code, String::from_utf8(out).unwrap())
  }

  fn sent(id: &str) -> Value {
    json!({
      "id": id,
      "targets": [
        { "handle": "claude-2", "repo": "/src/app", "via": "local", "outcome": "queued" },
        { "handle": "copilot-3", "repo": "/src/app", "via": "socket:127.0.0.1:7", "outcome": "not delivered: refused" }
      ]
    })
  }

  fn send(ask: bool) -> Command {
    Command::Send {
      to: Some("claude-2".into()),
      all: false,
      repo: None,
      body: "rebase done?".into(),
      ask,
      timeout: Duration::from_secs(1),
    }
  }

  #[test]
  fn send_prints_the_id_and_each_target() {
    let client = FakeClient::new(vec![Ok(sent("m-1"))]);
    let (code, out) = run_with(send(false), false, &client);
    assert_eq!(code, Ok(0));
    assert!(out.contains("m-1"), "{out}");
    assert!(out.contains("claude-2") && out.contains("queued"), "{out}");
    assert!(out.contains("not delivered: refused"), "{out}");
    let calls = client.calls.borrow();
    match &calls[0] {
      Op::Send(request) => {
        assert_eq!(request.from_pty, "pty-codex");
        assert_eq!(request.kind, Kind::Message);
        assert_eq!(request.timeout_ms, 1_000);
      }
      other => panic!("{other:?}"),
    }
  }

  #[test]
  fn ask_polls_until_the_reply_and_prints_it() {
    let client = FakeClient::new(vec![
      Ok(sent("m-1")),
      Ok(Value::Null),
      Ok(json!({ "id": "m-2", "from": "claude-2", "body": "done, head 9c41e2a", "reply_to": "m-1" })),
    ]);
    let (code, out) = run_with(send(true), false, &client);
    assert_eq!(code, Ok(0));
    assert!(out.contains("done, head 9c41e2a"), "{out}");
    let calls = client.calls.borrow();
    assert!(matches!(&calls[0], Op::Send(request) if request.kind == Kind::Ask));
    assert!(matches!(&calls[2], Op::TakeReply { id, .. } if id == "m-1"));
  }

  #[test]
  fn ask_exits_3_when_no_reply_comes_in_time() {
    let mut answers = vec![Ok(sent("m-1"))];
    answers.extend((0..10).map(|_| Ok(Value::Null)));
    let client = FakeClient::new(answers);
    let (code, out) = run_with(send(true), false, &client);
    assert_eq!(code, Ok(TIMED_OUT_EXIT));
    assert!(out.contains("timed out"), "{out}");
  }

  #[test]
  fn ask_with_json_prints_one_object() {
    let client = FakeClient::new(vec![
      Ok(sent("m-1")),
      Ok(json!({ "id": "m-2", "from": "claude-2", "body": "92%", "reply_to": "m-1" })),
    ]);
    let (code, out) = run_with(send(true), true, &client);
    assert_eq!(code, Ok(0));
    let value: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(value["id"], "m-1");
    assert_eq!(value["state"], "replied");
    assert_eq!(value["reply"], "92%");
  }

  #[test]
  fn inbox_prints_each_message_and_filters_kinds() {
    let client = FakeClient::new(vec![Ok(json!({ "messages": [
      { "id": "m-1", "kind": "ask", "from": "codex-1", "body": "rebase done?", "sent_at_ms": 0, "timeout_ms": 60000 },
      { "id": "m-2", "kind": "status", "from": "copilot-3", "body": "working", "sent_at_ms": 0, "timeout_ms": 60000 }
    ]}))]);
    let command = Command::Inbox {
      peek: false,
      wait: false,
      kinds: vec![Kind::Ask],
    };
    let (code, out) = run_with(command, false, &client);
    assert_eq!(code, Ok(0));
    assert!(out.contains("m-1") && out.contains("rebase done?"), "{out}");
    assert!(!out.contains("m-2"), "{out}");
  }

  #[test]
  fn inbox_wait_polls_until_something_arrives() {
    let client = FakeClient::new(vec![
      Ok(json!({ "messages": [] })),
      Ok(json!({ "messages": [
        { "id": "m-1", "kind": "message", "from": "codex-1", "body": "hi", "sent_at_ms": 0, "timeout_ms": 60000 }
      ]})),
    ]);
    let command = Command::Inbox {
      peek: false,
      wait: true,
      kinds: Vec::new(),
    };
    let (code, out) = run_with(command, false, &client);
    assert_eq!(code, Ok(0));
    assert!(out.contains("hi"), "{out}");
    assert_eq!(client.calls.borrow().len(), 2);
  }

  #[test]
  fn an_empty_inbox_says_so() {
    let client = FakeClient::new(vec![Ok(json!({ "messages": [] }))]);
    let command = Command::Inbox {
      peek: false,
      wait: false,
      kinds: Vec::new(),
    };
    let (_, out) = run_with(command, false, &client);
    assert!(out.contains("No messages"), "{out}");
  }

  #[test]
  fn list_prints_a_table() {
    let client = FakeClient::new(vec![Ok(json!({ "agents": [
      { "handle": "claude-2", "harness": "claude", "repo": "/src/app", "workspace": "fix/x", "via": "local", "waiting": 1 }
    ]}))]);
    let (code, out) = run_with(Command::List { repo: None }, false, &client);
    assert_eq!(code, Ok(0));
    assert!(out.contains("HANDLE") && out.contains("claude-2") && out.contains("fix/x"), "{out}");
  }

  #[test]
  fn commands_that_act_as_this_agent_need_a_treq_terminal() {
    let client = FakeClient::new(vec![]);
    let mut out = Vec::new();
    let result = run(&send(false), None, false, &client, &mut out, &|_| {});
    assert!(result.unwrap_err().contains("treq"));
  }

  #[test]
  fn list_works_outside_a_treq_terminal() {
    let client = FakeClient::new(vec![Ok(json!({ "agents": [] }))]);
    let mut out = Vec::new();
    let code = run(&Command::List { repo: None }, None, false, &client, &mut out, &|_| {});
    assert_eq!(code, Ok(0));
  }

  #[test]
  fn instance_errors_are_returned() {
    let client = FakeClient::new(vec![Err("no agent called gemini-9".into())]);
    let (code, _) = run_with(send(false), false, &client);
    assert_eq!(code, Err("no agent called gemini-9".into()));
  }
}
