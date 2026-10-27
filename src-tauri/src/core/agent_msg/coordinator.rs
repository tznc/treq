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
    let message = Envelope {
      id: model::new_id(),
      kind: request.kind,
      from: from.handle.clone(),
      to: String::new(),
      to_pty: String::new(),
      body: request.body.clone(),
      reply_to: request.reply_to.clone(),
      sent_at_ms: now_ms,
      timeout_ms: request.timeout_ms,
      origin: self.network.own_endpoint(),
      origin_repo: from.repo.clone(),
    };
    let rows: Vec<(String, String)> = targets
      .iter()
      .map(|peer| (peer.session.handle.clone(), peer.via.label()))
      .collect();
    self.ledger.insert(&from.repo, &message, &rows)?;

    let mut sent = Vec::new();
    for peer in targets {
      let envelope = Envelope {
        to: peer.session.handle.clone(),
        to_pty: peer.session.pty_id.clone(),
        ..message.clone()
      };
      let handed = match &peer.via {
        Via::Local => self.hub.enqueue(envelope),
        Via::Socket(endpoint) => self.network.deliver(endpoint, &envelope),
      };
      let outcome = match handed {
        Ok(()) => "queued".to_string(),
        Err(error) => {
          if let Err(error) = self.ledger.update(
            &from.repo,
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
      },
      now_ms,
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::cell::RefCell;
  use std::collections::HashMap;

  #[derive(Default)]
  struct FakeLedger {
    inserted: RefCell<Vec<(String, Envelope, Vec<(String, String)>)>>,
    updates: RefCell<Vec<(String, String, String, State, Option<String>)>>,
  }

  impl Ledger for FakeLedger {
    fn insert(&self, repo: &str, message: &Envelope, targets: &[(String, String)]) -> Result<(), String> {
      self
        .inserted
        .borrow_mut()
        .push((repo.into(), message.clone(), targets.to_vec()));
      Ok(())
    }
    fn update(
      &self,
      repo: &str,
      id: &str,
      target: &str,
      state: State,
      mode: Option<&str>,
      _now_ms: u64,
    ) -> Result<(), String> {
      self.updates.borrow_mut().push((
        repo.into(),
        id.into(),
        target.into(),
        state,
        mode.map(str::to_string),
      ));
      Ok(())
    }
  }

  #[derive(Default)]
  struct FakeNetwork {
    remote: HashMap<String, Vec<SessionInfo>>,
    down: Vec<String>,
    delivered: RefCell<Vec<(String, Envelope)>>,
    reported: RefCell<Vec<(String, Report)>>,
  }

  impl Network for FakeNetwork {
    fn own_endpoint(&self) -> String {
      "127.0.0.1:1".into()
    }
    fn peers(&self, _repos: &[String]) -> Vec<String> {
      let mut endpoints: Vec<String> = self.remote.keys().cloned().collect();
      endpoints.sort();
      endpoints
    }
    fn list(&self, endpoint: &str) -> Result<Vec<SessionInfo>, String> {
      self
        .remote
        .get(endpoint)
        .cloned()
        .ok_or_else(|| "unreachable".to_string())
    }
    fn deliver(&self, endpoint: &str, envelope: &Envelope) -> Result<(), String> {
      if self.down.iter().any(|down| down == endpoint) {
        return Err("connection refused".into());
      }
      self
        .delivered
        .borrow_mut()
        .push((endpoint.into(), envelope.clone()));
      Ok(())
    }
    fn report(&self, endpoint: &str, report: &Report) -> Result<(), String> {
      self
        .reported
        .borrow_mut()
        .push((endpoint.into(), report.clone()));
      Ok(())
    }
  }

  fn session(pty: &str, handle: &str, repo: &str) -> SessionInfo {
    SessionInfo {
      pty_id: pty.into(),
      handle: handle.into(),
      harness: handle.split('-').next().unwrap().into(),
      repo: repo.into(),
      workspace: None,
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
