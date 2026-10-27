//! The IPC messages `treq agent msg` and the treq instances exchange, and
//! how an instance answers each one.

use super::coordinator::{Coordinator, SendRequest};
use super::hub::{Envelope, Report};
use super::model::{self, Kind, State};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The `kind` that routes an IPC payload here.
pub const KIND: &str = "agent_msg";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
  pub kind: String,
  pub request_id: String,
  /// The repository whose instance the CLI reached.
  pub repo: String,
  #[serde(flatten)]
  pub op: Op,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
  Send(SendRequest),
  Inbox {
    pty: String,
    #[serde(default)]
    peek: bool,
  },
  Ack {
    pty: String,
    ids: Vec<String>,
  },
  Reply {
    pty: String,
    id: String,
    body: String,
  },
  /// A worker's status, to `to` or to whoever messaged it last.
  Status {
    pty: String,
    #[serde(default)]
    to: Option<String>,
    body: String,
  },
  /// The reply to an `--ask`, once it arrived; taking it acks it.
  TakeReply {
    pty: String,
    id: String,
  },
  /// The ledger rows of a message this instance sent.
  Rows {
    id: String,
  },
  /// The agents of this instance, and unless `local_only` its peers'.
  List {
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    local_only: bool,
  },
  /// From another instance: hold this for one of your terminals.
  Deliver {
    envelope: Envelope,
  },
  /// From another instance: a target's state changed.
  Report {
    report: Report,
  },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
  /// `ok` or `error`.
  pub status: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<String>,
  #[serde(default)]
  pub data: Value,
}

impl Response {
  pub fn ok(data: Value) -> Self {
    Self {
      status: "ok".into(),
      reason: None,
      data,
    }
  }

  pub fn error(reason: impl Into<String>) -> Self {
    Self {
      status: "error".into(),
      reason: Some(reason.into()),
      data: Value::Null,
    }
  }
}

/// Reads the ledger rows of message `id` from the app db of `repo`.
pub type Rows<'a> = &'a dyn Fn(&str, &str) -> Result<Value, String>;

/// Answers one request at `now_ms`.
pub fn handle(coordinator: &Coordinator, rows: Rows, request: &Request, now_ms: u64) -> Response {
  match answer(coordinator, rows, request, now_ms) {
    Ok(data) => Response::ok(data),
    Err(error) => Response::error(error),
  }
}

fn to_json<T: Serialize>(value: &T) -> Result<Value, String> {
  serde_json::to_value(value).map_err(|e| format!("failed to encode response: {e}"))
}

fn answer(coordinator: &Coordinator, rows: Rows, request: &Request, now_ms: u64) -> Result<Value, String> {
  let hub = coordinator.hub;
  match &request.op {
    Op::Send(send) => to_json(&coordinator.send(send, now_ms)?),
    Op::Inbox { pty, peek } => {
      let (messages, reports) = hub.inbox(pty, *peek, now_ms);
      coordinator.record(&reports, now_ms);
      Ok(serde_json::json!({ "messages": to_json(&messages)? }))
    }
    Op::Ack { pty, ids } => {
      let reports = hub.finish(pty, ids, State::Acked);
      coordinator.record(&reports, now_ms);
      Ok(serde_json::json!({ "acked": reports.len() }))
    }
    Op::Reply { pty, id, body } => to_json(&coordinator.reply(pty, id, body, now_ms)?),
    Op::Status { pty, to, body } => {
      let (to, repo) = match to {
        Some(to) => (to.clone(), None),
        None => {
          let (to, repo) = hub
            .last_sender(pty)
            .ok_or("no one has messaged you yet: name the agent to report to")?;
          (to, Some(repo))
        }
      };
      let sent = coordinator.send(
        &SendRequest {
          from_pty: pty.clone(),
          to: Some(to),
          all: false,
          repo,
          kind: Kind::Status,
          body: body.clone(),
          reply_to: None,
          timeout_ms: model::TIMEOUT.as_millis() as u64,
        },
        now_ms,
      )?;
      to_json(&sent)
    }
    Op::TakeReply { pty, id } => {
      let Some((reply, mut reports)) = hub.take_reply(pty, id) else {
        return Ok(Value::Null);
      };
      reports.extend(hub.finish(pty, std::slice::from_ref(&reply.id), State::Acked));
      coordinator.record(&reports, now_ms);
      to_json(&reply)
    }
    Op::Rows { id } => rows(&request.repo, id),
    Op::List { repo, local_only } => {
      let peers = if *local_only {
        hub
          .sessions()
          .into_iter()
          .map(|session| super::route::Peer {
            via: super::route::Via::Local,
            session,
          })
          .collect()
      } else {
        coordinator.peers(repo.as_deref())
      };
      let agents: Vec<Value> = peers
        .into_iter()
        .filter(|peer| {
          repo
            .as_deref()
            .is_none_or(|repo| super::route::repo_matches(&peer.session.repo, repo))
        })
        .map(|peer| {
          serde_json::json!({
            "handle": peer.session.handle,
            "harness": peer.session.harness,
            "repo": peer.session.repo,
            "workspace": peer.session.workspace,
            "pty_id": peer.session.pty_id,
            "via": peer.via.label(),
            "waiting": hub.waiting(&peer.session.pty_id, now_ms),
          })
        })
        .collect();
      Ok(serde_json::json!({ "agents": agents }))
    }
    Op::Deliver { envelope } => {
      coordinator.receive(envelope.clone())?;
      Ok(Value::Null)
    }
    Op::Report { report } => {
      coordinator.record(std::slice::from_ref(report), now_ms);
      Ok(Value::Null)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::super::hub::{Hub, SessionInfo};
  use super::super::testing::{FakeLedger, FakeNetwork};
  use super::*;
  use serde_json::json;

  fn session(pty: &str, handle: &str) -> SessionInfo {
    SessionInfo {
      pty_id: pty.into(),
      handle: handle.into(),
      harness: handle.split('-').next().unwrap().into(),
      repo: "/src/app".into(),
      workspace: Some("main".into()),
    }
  }

  struct Fixture {
    hub: Hub,
    ledger: FakeLedger,
    network: FakeNetwork,
  }

  impl Fixture {
    fn new() -> Self {
      let hub = Hub::new();
      hub.register(session("pty-codex", "codex-1"));
      hub.register(session("pty-claude", "claude-2"));
      Self {
        hub,
        ledger: FakeLedger::default(),
        network: FakeNetwork::default(),
      }
    }

    fn call(&self, op: Op, now_ms: u64) -> Response {
      let coordinator = Coordinator {
        hub: &self.hub,
        ledger: &self.ledger,
        network: &self.network,
      };
      let rows = |repo: &str, id: &str| Ok(json!({ "repo": repo, "id": id }));
      let request = Request {
        kind: KIND.into(),
        request_id: "r-1".into(),
        repo: "/src/app".into(),
        op,
      };
      handle(&coordinator, &rows, &request, now_ms)
    }

    fn send(&self, kind: Kind, to: &str, body: &str) -> String {
      let response = self.call(
        Op::Send(SendRequest {
          from_pty: "pty-codex".into(),
          to: Some(to.into()),
          all: false,
          repo: None,
          kind,
          body: body.into(),
          reply_to: None,
          timeout_ms: 60_000,
        }),
        1_000,
      );
      assert_eq!(response.status, "ok", "{:?}", response.reason);
      response.data["id"].as_str().unwrap().to_string()
    }
  }

  #[test]
  fn requests_round_trip_as_tagged_json() {
    let request = Request {
      kind: KIND.into(),
      request_id: "r-1".into(),
      repo: "/src/app".into(),
      op: Op::Inbox {
        pty: "pty-claude".into(),
        peek: true,
      },
    };
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["kind"], "agent_msg");
    assert_eq!(json["op"], "inbox");
    assert_eq!(json["pty"], "pty-claude");
    assert_eq!(serde_json::from_value::<Request>(json).unwrap(), request);
  }

  #[test]
  fn send_answers_with_the_id_and_each_target() {
    let fixture = Fixture::new();
    let id = fixture.send(Kind::Message, "claude-2", "hi");
    assert!(id.starts_with("m-"));
    let (waiting, _) = fixture.hub.inbox("pty-claude", true, 1_000);
    assert_eq!(waiting[0].id, id);
  }

  #[test]
  fn a_failed_send_is_an_error_response() {
    let fixture = Fixture::new();
    let response = fixture.call(
      Op::Send(SendRequest {
        from_pty: "pty-codex".into(),
        to: Some("nobody-9".into()),
        all: false,
        repo: None,
        kind: Kind::Message,
        body: "hi".into(),
        reply_to: None,
        timeout_ms: 60_000,
      }),
      0,
    );
    assert_eq!(response.status, "error");
    assert!(response.reason.unwrap().contains("nobody-9"));
  }

  #[test]
  fn inbox_lists_messages_and_records_them_read() {
    let fixture = Fixture::new();
    let id = fixture.send(Kind::Ask, "claude-2", "rebase done?");
    let response = fixture.call(
      Op::Inbox {
        pty: "pty-claude".into(),
        peek: false,
      },
      2_000,
    );
    assert_eq!(response.data["messages"][0]["id"], id.as_str());
    assert_eq!(response.data["messages"][0]["body"], "rebase done?");
    assert_eq!(response.data["messages"][0]["kind"], "ask");
    let updates = fixture.ledger.updates.borrow();
    assert!(updates.iter().any(|u| u.1 == id && u.3 == State::Read));
  }

  #[test]
  fn ack_records_acked_and_counts_what_it_acked() {
    let fixture = Fixture::new();
    let id = fixture.send(Kind::Message, "claude-2", "hi");
    let response = fixture.call(
      Op::Ack {
        pty: "pty-claude".into(),
        ids: vec![id.clone(), "m-unknown".into()],
      },
      2_000,
    );
    assert_eq!(response.data["acked"], 1);
    assert!(fixture
      .ledger
      .updates
      .borrow()
      .iter()
      .any(|u| u.1 == id && u.3 == State::Acked));
  }

  #[test]
  fn reply_then_take_reply_closes_the_ask() {
    let fixture = Fixture::new();
    let id = fixture.send(Kind::Ask, "claude-2", "rebase done?");
    let none = fixture.call(
      Op::TakeReply {
        pty: "pty-codex".into(),
        id: id.clone(),
      },
      1_500,
    );
    assert_eq!(none.data, Value::Null, "no reply yet");

    let replied = fixture.call(
      Op::Reply {
        pty: "pty-claude".into(),
        id: id.clone(),
        body: "done".into(),
      },
      2_000,
    );
    assert_eq!(replied.status, "ok", "{:?}", replied.reason);

    let taken = fixture.call(
      Op::TakeReply {
        pty: "pty-codex".into(),
        id: id.clone(),
      },
      2_500,
    );
    assert_eq!(taken.data["body"], "done");
    assert_eq!(taken.data["reply_to"], id.as_str());
    assert_eq!(fixture.hub.waiting("pty-codex", 2_500), 0, "taking the reply acks it");
  }

  #[test]
  fn status_goes_to_whoever_messaged_last() {
    let fixture = Fixture::new();
    fixture.send(Kind::Message, "claude-2", "go");
    let response = fixture.call(
      Op::Status {
        pty: "pty-claude".into(),
        to: None,
        body: "done: rebased".into(),
      },
      2_000,
    );
    assert_eq!(response.status, "ok", "{:?}", response.reason);
    let (waiting, _) = fixture.hub.inbox("pty-codex", true, 2_000);
    assert_eq!(waiting[0].kind, Kind::Status);
    assert_eq!(waiting[0].body, "done: rebased");
  }

  #[test]
  fn status_with_no_one_to_tell_is_an_error() {
    let fixture = Fixture::new();
    let response = fixture.call(
      Op::Status {
        pty: "pty-claude".into(),
        to: None,
        body: "working".into(),
      },
      0,
    );
    assert_eq!(response.status, "error");
  }

  #[test]
  fn list_names_each_agent_with_its_harness_and_where_it_runs() {
    let fixture = Fixture::new();
    let response = fixture.call(
      Op::List {
        repo: None,
        local_only: false,
      },
      0,
    );
    let agents = response.data["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[1]["handle"], "claude-2");
    assert_eq!(agents[1]["harness"], "claude");
    assert_eq!(agents[1]["workspace"], "main");
    assert_eq!(agents[1]["via"], "local");
  }

  #[test]
  fn list_local_only_skips_the_peers() {
    let mut fixture = Fixture::new();
    fixture
      .network
      .remote
      .insert("127.0.0.1:7".into(), vec![session("pty-x", "copilot-3")]);
    let all = fixture.call(Op::List { repo: None, local_only: false }, 0);
    let local = fixture.call(Op::List { repo: None, local_only: true }, 0);
    assert_eq!(all.data["agents"].as_array().unwrap().len(), 3);
    assert_eq!(local.data["agents"].as_array().unwrap().len(), 2);
  }

  #[test]
  fn deliver_and_report_from_peers() {
    let fixture = Fixture::new();
    let envelope = Envelope {
      id: "m-1".into(),
      kind: Kind::Message,
      from: "copilot-3".into(),
      to: "claude-2".into(),
      to_pty: "pty-claude".into(),
      body: "hi".into(),
      reply_to: None,
      sent_at_ms: 0,
      timeout_ms: 60_000,
      origin: "127.0.0.1:7".into(),
      origin_repo: "/src/app".into(),
    };
    assert_eq!(fixture.call(Op::Deliver { envelope }, 0).status, "ok");
    assert_eq!(fixture.hub.waiting("pty-claude", 0), 1);

    let report = Report {
      origin: "127.0.0.1:1".into(),
      origin_repo: "/src/app".into(),
      id: "m-5".into(),
      target: "copilot-3".into(),
      state: State::Acked,
      mode: None,
    };
    assert_eq!(fixture.call(Op::Report { report }, 0).status, "ok");
    assert_eq!(fixture.ledger.updates.borrow()[0].1, "m-5");
  }

  #[test]
  fn rows_reads_the_ledger_of_the_cli_repo() {
    let fixture = Fixture::new();
    let response = fixture.call(Op::Rows { id: "m-1".into() }, 0);
    assert_eq!(response.data, json!({ "repo": "/src/app", "id": "m-1" }));
  }
}
