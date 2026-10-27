//! What a treq instance does with each `treq agent msg` request: send as the
//! initiator, hold and wake as a target, and record what its targets report.

use super::hub::{Envelope, Hub, Report, SessionInfo};
use super::model::{self, Kind, State};
use super::route::{self, Peer, Via};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The app db, as the initiator records its messages
/// (`local_db::agent_messages`).
pub trait Ledger {
  fn insert(&self, repo: &str, message: &Envelope, targets: &[(String, String)]) -> Result<(), String>;
  fn update(
    &self,
    repo: &str,
    id: &str,
    target: &str,
    state: State,
    mode: Option<&str>,
    now_ms: u64,
  ) -> Result<(), String>;
}

/// The other treq instances on this machine.
pub trait Network {
  /// This instance's IPC endpoint: the `origin` of what it sends.
  fn own_endpoint(&self) -> String;
  /// Endpoints of the other running instances that share `repos`.
  fn peers(&self, repos: &[String]) -> Vec<String>;
  fn list(&self, endpoint: &str) -> Result<Vec<SessionInfo>, String>;
  fn deliver(&self, endpoint: &str, envelope: &Envelope) -> Result<(), String>;
  fn report(&self, endpoint: &str, report: &Report) -> Result<(), String>;
  /// Copies a message queued for one of this instance's terminals to where
  /// its agent reads it, when that is not here: a remote terminal's host.
  fn mirror(&self, envelope: &Envelope) -> Result<(), String> {
    let _ = envelope;
    Ok(())
  }
}

/// `treq agent msg` as the sending agent asks it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendRequest {
  pub from_pty: String,
  #[serde(default)]
  pub to: Option<String>,
  #[serde(default)]
  pub all: bool,
  #[serde(default)]
  pub repo: Option<String>,
  pub kind: Kind,
  pub body: String,
  #[serde(default)]
  pub reply_to: Option<String>,
  pub timeout_ms: u64,
  /// The id to give the message; a fresh one when `None`. A remote agent
  /// picks its own on the host, so it can wait for the reply.
  #[serde(default)]
  pub id: Option<String>,
}

/// One target of a sent message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sent {
  pub handle: String,
  pub repo: String,
  pub via: String,
  /// `queued`, or why it could not be handed over.
  pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendResult {
  pub id: String,
  pub targets: Vec<Sent>,
}

pub struct Coordinator<'a> {
  pub hub: &'a Hub,
  pub ledger: &'a dyn Ledger,
  pub network: &'a dyn Network,
}

impl Coordinator<'_> {
  /// Every agent this instance and its peers run.
  pub fn peers(&self, repo: Option<&str>) -> Vec<Peer> {
    let local = self.hub.sessions();
    let mut repos: Vec<String> = local.iter().map(|session| session.repo.clone()).collect();
    repos.extend(repo.map(str::to_string));
    repos.sort();
    repos.dedup();
    let own = self.network.own_endpoint();
    let mut peers: Vec<Peer> = local
      .into_iter()
      .map(|session| Peer {
        via: Via::Local,
        session,
      })
      .collect();
    for endpoint in self.network.peers(&repos) {
      if endpoint == own {
        continue;
      }
      match self.network.list(&endpoint) {
        Ok(sessions) => peers.extend(sessions.into_iter().map(|session| Peer {
          via: Via::Socket(endpoint.clone()),
          session,
        })),
        Err(error) => log::warn!("agent msg: instance {endpoint} did not list: {error}"),
      }
    }
    peers
  }

  pub fn send(&self, request: &SendRequest, now_ms: u64) -> Result<SendResult, String> {
    let timeout = Duration::from_millis(request.timeout_ms);
    if timeout.is_zero() || timeout > model::TIMEOUT {
      return Err(format!(
        "timeout must be between 1s and {}s",
        model::TIMEOUT.as_secs()
      ));
    }
    let from = self.hub.session(&request.from_pty).ok_or_else(|| {
      "only agents treq launched can send messages: run this in a treq agent terminal".to_string()
    })?;
    let peers = self.peers(request.repo.as_deref());
    let targets = route::resolve(
      &peers,
      &request.from_pty,
      request.to.as_deref(),
      request.all,
      request.repo.as_deref(),
    )?;
    let ledger_repo = from.ledger.clone().unwrap_or_else(|| from.repo.clone());
    let message = Envelope {
      id: request.id.clone().unwrap_or_else(model::new_id),
      kind: request.kind,
      from: from.handle.clone(),
      to: String::new(),
      to_pty: String::new(),
      body: request.body.clone(),
      reply_to: request.reply_to.clone(),
      sent_at_ms: now_ms,
      timeout_ms: request.timeout_ms,
      origin: self.network.own_endpoint(),
      origin_repo: ledger_repo.clone(),
    };
    let rows: Vec<(String, String)> = targets
      .iter()
      .map(|peer| (peer.session.handle.clone(), peer.via.label()))
      .collect();
    self.ledger.insert(&ledger_repo, &message, &rows)?;

    let mut sent = Vec::new();
    for peer in targets {
      let envelope = Envelope {
        to: peer.session.handle.clone(),
        to_pty: peer.session.pty_id.clone(),
        ..message.clone()
      };
      let handed = match &peer.via {
        Via::Local => self
          .network
          .mirror(&envelope)
          .and_then(|()| self.hub.enqueue(envelope)),
        Via::Socket(endpoint) => self.network.deliver(endpoint, &envelope),
      };
      let outcome = match handed {
        Ok(()) => "queued".to_string(),
        Err(error) => {
          if let Err(error) = self.ledger.update(
            &ledger_repo,
            &message.id,
            &peer.session.handle,
            State::TimedOut,
            None,
            now_ms,
          ) {
            log::warn!("agent msg: could not record {}: {error}", message.id);
          }
          format!("not delivered: {error}")
        }
      };
      sent.push(Sent {
        handle: peer.session.handle,
        repo: peer.session.repo,
        via: peer.via.label(),
        outcome,
      });
    }
    Ok(SendResult {
      id: message.id,
      targets: sent,
    })
  }

  /// Holds a message another instance sent to one of this one's terminals.
  pub fn receive(&self, envelope: Envelope) -> Result<(), String> {
    self.network.mirror(&envelope)?;
    self.hub.enqueue(envelope)
  }

  /// Records `reports` where each message started: in this instance's
  /// ledger, or by handing them to the instance that sent it.
  pub fn record(&self, reports: &[Report], now_ms: u64) {
    let own = self.network.own_endpoint();
    for report in reports {
      let recorded = if report.origin == own {
        self.ledger.update(
          &report.origin_repo,
          &report.id,
          &report.target,
          report.state,
          report.mode.as_deref(),
          now_ms,
        )
      } else {
        self.network.report(&report.origin, report)
      };
      if let Err(error) = recorded {
        log::warn!(
          "agent msg: could not record {} {} for {}: {error}",
          report.id,
          report.state.as_str(),
          report.target
        );
      }
    }
  }

  /// Acts on what a remote agent did on its host since the last poll
  /// ([`super::mailbox::poll`]), as `pty`'s agent. Returns the last event's
  /// number, for the next poll.
  pub fn apply_remote(&self, pty: &str, events: &[super::mailbox::Event], since: u64, now_ms: u64) -> u64 {
    use super::mailbox::Event;
    let mut last = since;
    for event in events {
      last = last.max(event.seq());
      match event {
        Event::State { id, state, .. } => {
          let ids = std::slice::from_ref(id);
          let reports = match state {
            State::Read => self.hub.mark_read(pty, ids),
            State::Acked | State::Replied => self.hub.finish(pty, ids, *state),
            State::Queued | State::Delivered | State::TimedOut => Vec::new(),
          };
          self.record(&reports, now_ms);
        }
        Event::Send { message, .. } => {
          let request = SendRequest {
            from_pty: pty.to_string(),
            to: message.to.clone(),
            all: message.all,
            repo: message.repo.clone(),
            kind: message.kind,
            body: message.body.clone(),
            reply_to: message.reply_to.clone(),
            timeout_ms: message.timeout_ms,
            id: Some(message.id.clone()),
          };
          if let Err(error) = self.send(&request, now_ms) {
            log::warn!("agent msg: could not route {} from {pty}: {error}", message.id);
          }
        }
      }
    }
    last
  }

  /// `--reply <id>`: ends message `id` for `pty` as replied and sends the
  /// answer back to whoever asked.
  pub fn reply(&self, pty: &str, id: &str, body: &str, now_ms: u64) -> Result<SendResult, String> {
    let (waiting, _) = self.hub.inbox(pty, true, now_ms);
    let asked = waiting
      .into_iter()
      .find(|envelope| envelope.id == id)
      .ok_or_else(|| format!("no message {id} is waiting for you. See treq agent msg --inbox"))?;
    let reports = self.hub.finish(pty, &[id.to_string()], State::Replied);
    self.record(&reports, now_ms);
    self.send(
      &SendRequest {
        from_pty: pty.to_string(),
        to: Some(asked.from.clone()),
        all: false,
        repo: Some(asked.origin_repo.clone()),
        kind: Kind::Reply,
        body: body.to_string(),
        reply_to: Some(asked.id.clone()),
        timeout_ms: model::TIMEOUT.as_millis() as u64,
        id: None,
      },
      now_ms,
    )
  }
}

#[cfg(test)]
mod tests {
  use super::super::mailbox::{Event, Outgoing};
  use super::super::testing::{FakeLedger, FakeNetwork};
  use super::*;
  use std::collections::HashMap;

  fn session(pty: &str, handle: &str, repo: &str) -> SessionInfo {
    SessionInfo {
      pty_id: pty.into(),
      handle: handle.into(),
      harness: handle.split('-').next().unwrap().into(),
      repo: repo.into(),
      workspace: None,
      ledger: None,
    }
  }

  fn hub() -> Hub {
    let hub = Hub::new();
    hub.register(session("pty-codex", "codex-1", "/src/app"));
    hub.register(session("pty-claude", "claude-2", "/src/app"));
    hub
  }

  fn network() -> FakeNetwork {
    FakeNetwork {
      remote: HashMap::from([(
        "127.0.0.1:7".to_string(),
        vec![session("pty-copilot", "copilot-3", "/src/app")],
      )]),
      ..FakeNetwork::default()
    }
  }

  fn ask(to: Option<&str>, all: bool) -> SendRequest {
    SendRequest {
      from_pty: "pty-codex".into(),
      to: to.map(str::to_string),
      all,
      repo: None,
      kind: Kind::Ask,
      body: "rebase done?".into(),
      reply_to: None,
      timeout_ms: 60_000,
      id: None,
    }
  }

  #[test]
  fn peers_are_local_sessions_then_each_instances() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let peers = coordinator.peers(None);
    let found: Vec<(String, Via)> = peers
      .into_iter()
      .map(|p| (p.session.handle, p.via))
      .collect();
    assert_eq!(
      found,
      vec![
        ("codex-1".into(), Via::Local),
        ("claude-2".into(), Via::Local),
        ("copilot-3".into(), Via::Socket("127.0.0.1:7".into())),
      ]
    );
  }

  #[test]
  fn an_unreachable_instance_is_left_out_of_peers() {
    let hub = hub();
    let ledger = FakeLedger::default();
    let mut network = network();
    network.remote.clear();
    network.down.push("127.0.0.1:7".into());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    assert_eq!(coordinator.peers(None).len(), 2);
  }

  #[test]
  fn sending_records_every_target_in_the_senders_repo_and_queues_local_ones() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let result = coordinator.send(&ask(Some("claude-2"), false), 1_000).unwrap();

    assert!(result.id.starts_with("m-"));
    assert_eq!(
      result.targets,
      vec![Sent {
        handle: "claude-2".into(),
        repo: "/src/app".into(),
        via: "local".into(),
        outcome: "queued".into(),
      }]
    );
    let inserted = ledger.inserted.borrow();
    let (repo, message, targets) = &inserted[0];
    assert_eq!(repo, "/src/app");
    assert_eq!(message.from, "codex-1");
    assert_eq!(message.sent_at_ms, 1_000);
    assert_eq!(targets, &vec![("claude-2".to_string(), "local".to_string())]);

    let (waiting, _) = hub.inbox("pty-claude", true, 1_000);
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].origin, "127.0.0.1:1");
    assert_eq!(waiting[0].origin_repo, "/src/app");
    assert_eq!(waiting[0].body, "rebase done?");
  }

  #[test]
  fn broadcast_reaches_other_instances_through_their_socket() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let result = coordinator.send(&ask(None, true), 1_000).unwrap();

    let handles: Vec<&str> = result.targets.iter().map(|t| t.handle.as_str()).collect();
    assert_eq!(handles, vec!["claude-2", "copilot-3"]);
    let delivered = network.delivered.borrow();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].0, "127.0.0.1:7");
    assert_eq!(delivered[0].1.to_pty, "pty-copilot");
    assert_eq!(delivered[0].1.id, result.id);
  }

  #[test]
  fn a_target_that_cannot_be_reached_is_reported_and_timed_out() {
    let hub = hub();
    let ledger = FakeLedger::default();
    let mut network = network();
    network.down.push("127.0.0.1:7".into());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let result = coordinator.send(&ask(Some("copilot-3"), false), 1_000).unwrap();

    assert!(result.targets[0].outcome.contains("connection refused"));
    let updates = ledger.updates.borrow();
    assert_eq!(updates[0].2, "copilot-3");
    assert_eq!(updates[0].3, State::TimedOut);
  }

  #[test]
  fn only_agents_treq_launched_can_send() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let mut request = ask(Some("claude-2"), false);
    request.from_pty = "pty-unknown".into();
    let error = coordinator.send(&request, 0).unwrap_err();
    assert!(error.contains("treq"), "{error}");
  }

  #[test]
  fn a_remote_sender_records_in_its_ledger_repo() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    hub.register(SessionInfo {
      ledger: Some("remote:app".into()),
      ..session("pty-remote", "claude-agent-1", "/srv/app")
    });
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let mut request = ask(Some("codex-1"), false);
    request.from_pty = "pty-remote".into();
    coordinator.send(&request, 0).unwrap();
    assert_eq!(ledger.inserted.borrow()[0].0, "remote:app");
    let (waiting, _) = hub.inbox("pty-codex", true, 0);
    assert_eq!(waiting[0].origin_repo, "remote:app");
  }

  #[test]
  fn local_deliveries_are_mirrored_for_remote_terminals() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    coordinator.send(&ask(Some("claude-2"), false), 0).unwrap();
    assert_eq!(network.mirrored.borrow()[0].to_pty, "pty-claude");
  }

  #[test]
  fn a_failed_mirror_is_not_delivered() {
    let hub = hub();
    let ledger = FakeLedger::default();
    let mut network = network();
    network.mirror_down = true;
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let result = coordinator.send(&ask(Some("claude-2"), false), 0).unwrap();
    assert!(result.targets[0].outcome.starts_with("not delivered"), "{:?}", result.targets);
    assert_eq!(hub.waiting("pty-claude", 0), 0);
    assert_eq!(ledger.updates.borrow()[0].3, State::TimedOut);
  }

  fn state(seq: u64, id: &str, state: State) -> Event {
    Event::State {
      seq,
      at_ms: 0,
      id: id.into(),
      state,
    }
  }

  #[test]
  fn remote_reads_acks_and_replies_are_recorded_at_the_origin() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let id = coordinator.send(&ask(Some("claude-2"), false), 0).unwrap().id;

    let last = coordinator.apply_remote(
      "pty-claude",
      &[state(1, &id, State::Read), state(2, &id, State::Acked)],
      0,
      5,
    );
    assert_eq!(last, 2);
    let states: Vec<State> = ledger.updates.borrow().iter().map(|u| u.3).collect();
    assert_eq!(states, vec![State::Read, State::Acked]);
    assert_eq!(hub.waiting("pty-claude", 5), 0);
  }

  #[test]
  fn a_remote_send_is_routed_with_its_own_id() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let send = Event::Send {
      seq: 4,
      at_ms: 0,
      message: Outgoing {
        id: "m-host0001".into(),
        to: Some("codex-1".into()),
        all: false,
        repo: None,
        kind: Kind::Status,
        body: "done".into(),
        reply_to: None,
        timeout_ms: 60_000,
      },
    };
    assert_eq!(coordinator.apply_remote("pty-claude", &[send], 3, 5), 4);
    let (waiting, _) = hub.inbox("pty-codex", true, 5);
    assert_eq!(waiting[0].id, "m-host0001");
    assert_eq!(waiting[0].from, "claude-2");
    assert_eq!(waiting[0].kind, Kind::Status);
  }

  #[test]
  fn nothing_new_keeps_the_poll_where_it_was() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    assert_eq!(coordinator.apply_remote("pty-claude", &[], 7, 0), 7);
  }

  #[test]
  fn a_given_id_is_kept() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let mut request = ask(Some("claude-2"), false);
    request.id = Some("m-host0001".into());
    assert_eq!(coordinator.send(&request, 0).unwrap().id, "m-host0001");
  }

  #[test]
  fn a_timeout_over_a_minute_is_refused() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let mut request = ask(Some("claude-2"), false);
    request.timeout_ms = 61_000;
    assert!(coordinator.send(&request, 0).is_err());
    assert!(ledger.inserted.borrow().is_empty());
  }

  #[test]
  fn receive_queues_for_a_known_terminal_only() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let envelope = |pty: &str| Envelope {
      id: "m-1".into(),
      kind: Kind::Message,
      from: "copilot-3".into(),
      to: "claude-2".into(),
      to_pty: pty.into(),
      body: "hi".into(),
      reply_to: None,
      sent_at_ms: 0,
      timeout_ms: 60_000,
      origin: "127.0.0.1:7".into(),
      origin_repo: "/src/app".into(),
    };
    coordinator.receive(envelope("pty-claude")).unwrap();
    assert!(coordinator.receive(envelope("pty-gone")).is_err());
    assert_eq!(hub.waiting("pty-claude", 0), 1);
  }

  #[test]
  fn reports_are_recorded_here_or_sent_to_their_origin() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let report = |origin: &str| Report {
      origin: origin.into(),
      origin_repo: "/src/app".into(),
      id: "m-1".into(),
      target: "claude-2".into(),
      state: State::Delivered,
      mode: Some("wake".into()),
    };
    coordinator.record(&[report("127.0.0.1:1"), report("127.0.0.1:7")], 5);

    assert_eq!(
      ledger.updates.borrow().as_slice(),
      &[(
        "/src/app".to_string(),
        "m-1".to_string(),
        "claude-2".to_string(),
        State::Delivered,
        Some("wake".to_string())
      )]
    );
    assert_eq!(network.reported.borrow()[0].0, "127.0.0.1:7");
  }

  #[test]
  fn reply_ends_the_ask_and_sends_the_answer_to_the_asker() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    let asked = coordinator.send(&ask(Some("claude-2"), false), 1_000).unwrap();

    let replied = coordinator
      .reply("pty-claude", &asked.id, "done, head 9c41e2a", 2_000)
      .unwrap();
    assert_eq!(replied.targets[0].handle, "codex-1");

    let updates = ledger.updates.borrow();
    assert!(updates
      .iter()
      .any(|u| u.1 == asked.id && u.2 == "claude-2" && u.3 == State::Replied));
    assert_eq!(hub.waiting("pty-claude", 2_000), 0, "the ask leaves the inbox");
    let (answer, _) = hub.take_reply("pty-codex", &asked.id).unwrap();
    assert_eq!(answer.body, "done, head 9c41e2a");
    assert_eq!(answer.kind, Kind::Reply);
  }

  #[test]
  fn replying_to_a_message_not_waiting_is_an_error() {
    let (hub, ledger, network) = (hub(), FakeLedger::default(), network());
    let coordinator = Coordinator { hub: &hub, ledger: &ledger, network: &network };
    assert!(coordinator.reply("pty-claude", "m-9", "?", 0).is_err());
  }
}
