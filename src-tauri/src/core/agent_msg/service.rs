//! The running instance's messaging: one hub and screen tracker per
//! process, the IPC entry point, and the delivery loop.

use super::coordinator::Coordinator;
use super::hub::{Hub, SessionInfo};
use super::network::{DbLedger, InstanceNetwork};
use super::protocol::{self, Request, Response};
use super::terminals::{ScreenTracker, Writer};
use crate::agent_dispatch::now_millis;
use crate::core::remote::TreqCommandRequest;
use crate::core::remote_control_plane::SshEndpoint;
use crate::lock_ext::LockExt;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often the delivery loop looks at the terminals.
const TICK: Duration = Duration::from_millis(500);

/// How often remote hosts are asked what their agents did.
const REMOTE_POLL: Duration = Duration::from_secs(2);

/// How many ticks between ledger sweeps.
const SWEEP_EVERY: u64 = 10;

static ENDPOINT: OnceLock<String> = OnceLock::new();

/// Bumped whenever what a terminal shows for its messages may have changed,
/// so the UI knows to redraw.
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn hub() -> &'static Hub {
  static HUB: OnceLock<Hub> = OnceLock::new();
  HUB.get_or_init(Hub::new)
}

pub fn tracker() -> &'static ScreenTracker {
  static TRACKER: OnceLock<ScreenTracker> = OnceLock::new();
  TRACKER.get_or_init(ScreenTracker::new)
}

pub fn generation() -> u64 {
  GENERATION.load(Ordering::Relaxed)
}

fn changed() {
  GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// A remote agent terminal: where its host keeps its mailbox.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteSession {
  pub endpoint: SshEndpoint,
  pub repo: String,
  pub workspace: String,
  pub label: String,
}

/// Runs a typed command on a host over SSH (the window code's transport).
pub type RemoteRunner =
  Arc<dyn Fn(&SshEndpoint, TreqCommandRequest) -> Result<Value, String> + Send + Sync>;

/// Each remote terminal, and the last host event taken from it.
fn remotes() -> &'static Mutex<HashMap<String, (RemoteSession, u64)>> {
  static REMOTES: OnceLock<Mutex<HashMap<String, (RemoteSession, u64)>>> = OnceLock::new();
  REMOTES.get_or_init(Mutex::default)
}

fn runner() -> &'static Mutex<Option<RemoteRunner>> {
  static RUNNER: OnceLock<Mutex<Option<RemoteRunner>>> = OnceLock::new();
  RUNNER.get_or_init(Mutex::default)
}

pub fn set_remote_runner(run: RemoteRunner) {
  *runner().lock_or_recover() = Some(run);
}

/// A remote agent terminal treq attached, `cols`×`rows`.
pub fn register_remote(session: SessionInfo, remote: RemoteSession, cols: u16, rows: u16) {
  remotes()
    .lock_or_recover()
    .insert(session.pty_id.clone(), (remote, 0));
  register(session, cols, rows);
}

/// Writes `envelope` into its host's mailbox when it is for a remote
/// terminal; nothing to do for a local one.
pub fn mirror(envelope: &super::hub::Envelope) -> Result<(), String> {
  let Some((remote, _)) = remotes().lock_or_recover().get(&envelope.to_pty).cloned() else {
    return Ok(());
  };
  let run = runner()
    .lock_or_recover()
    .clone()
    .ok_or("treq cannot reach remote hosts yet")?;
  let json =
    serde_json::to_string(envelope).map_err(|e| format!("failed to encode message: {e}"))?;
  run(
    &remote.endpoint,
    TreqCommandRequest::PtyMsgDeliver {
      repo: remote.repo,
      workspace: remote.workspace,
      label: remote.label,
      envelope: json,
    },
  )
  .map(drop)
}

/// Takes what each remote agent did on its host and acts on it.
fn poll_remotes() {
  let sessions: Vec<(String, RemoteSession, u64)> = remotes()
    .lock_or_recover()
    .iter()
    .map(|(pty, (remote, since))| (pty.clone(), remote.clone(), *since))
    .collect();
  let Some(run) = runner().lock_or_recover().clone() else {
    return;
  };
  for (pty, remote, since) in sessions {
    let answer = run(
      &remote.endpoint,
      TreqCommandRequest::PtyMsgPoll {
        repo: remote.repo.clone(),
        workspace: remote.workspace.clone(),
        label: remote.label.clone(),
        since,
      },
    );
    let events: Vec<super::mailbox::Event> = match answer.and_then(|value| {
      serde_json::from_value(value).map_err(|e| format!("invalid poll answer: {e}"))
    }) {
      Ok(events) => events,
      Err(error) => {
        log::warn!("agent msg: polling {pty} failed: {error}");
        continue;
      }
    };
    if events.is_empty() {
      continue;
    }
    let last =
      with_coordinator(|coordinator| coordinator.apply_remote(&pty, &events, since, now_millis()));
    if let (Some(last), Some(entry)) = (last, remotes().lock_or_recover().get_mut(&pty)) {
      entry.1 = last;
    }
    changed();
  }
}

/// Starts messaging for an instance reachable at `endpoint`. Once per process.
pub fn start(endpoint: String) {
  if ENDPOINT.set(endpoint).is_err() {
    return;
  }
  std::thread::spawn(|| loop {
    std::thread::sleep(REMOTE_POLL);
    poll_remotes();
  });
  std::thread::spawn(|| {
    let mut ticks: u64 = 0;
    loop {
      std::thread::sleep(TICK);
      ticks += 1;
      tick(ticks.is_multiple_of(SWEEP_EVERY));
    }
  });
}

/// How treq types into a PTY. Set by the window code, which can write to
/// local and remote terminals alike.
pub fn set_writer(writer: Writer) {
  tracker().set_writer(writer);
}

/// An agent terminal treq launched, `cols`×`rows`.
pub fn register(session: SessionInfo, cols: u16, rows: u16) {
  tracker().track(&session.pty_id, cols, rows);
  hub().register(session);
  changed();
}

pub fn unregister(pty_id: &str) {
  remotes().lock_or_recover().remove(pty_id);
  hub().unregister(pty_id);
  tracker().untrack(pty_id);
  changed();
}

fn with_coordinator<T>(run: impl FnOnce(&Coordinator) -> T) -> Option<T> {
  let endpoint = ENDPOINT.get()?;
  let network = InstanceNetwork {
    endpoint: endpoint.clone(),
  };
  Some(run(&Coordinator {
    hub: hub(),
    ledger: &DbLedger,
    network: &network,
  }))
}

/// Answers a `treq agent msg` IPC request.
pub fn handle(request: &Request) -> Response {
  let rows = |repo: &str, id: &str| {
    let rows = crate::local_db::agent_message_rows(repo, id)?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
  };
  let response =
    with_coordinator(|coordinator| protocol::handle(coordinator, &rows, request, now_millis()))
      .unwrap_or_else(|| Response::error("this treq instance is still starting"));
  changed();
  response
}

/// The banner's "Deliver": types the wake line into `pty_id` now.
pub fn deliver_now(pty_id: &str) {
  let pty_id = pty_id.to_string();
  std::thread::spawn(move || {
    let reports = hub().deliver_now(&pty_id, tracker());
    with_coordinator(|coordinator| coordinator.record(&reports, now_millis()));
    changed();
  });
}

fn tick(sweep: bool) {
  let now = now_millis();
  let reports = hub().tick(tracker(), now);
  if !reports.is_empty() {
    changed();
  }
  with_coordinator(|coordinator| coordinator.record(&reports, now));
  if sweep {
    let mut repos: Vec<String> = hub()
      .sessions()
      .into_iter()
      .map(|s| s.ledger.unwrap_or(s.repo))
      .collect();
    repos.sort();
    repos.dedup();
    for repo in repos {
      if let Err(error) = crate::local_db::sweep_agent_messages(&repo, now) {
        log::warn!("agent msg sweep failed for {repo}: {error}");
      }
    }
    changed();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  type Calls = Arc<Mutex<Vec<TreqCommandRequest>>>;

  fn fake_runner(answer: Value) -> Calls {
    let calls: Calls = Arc::default();
    let seen = calls.clone();
    set_remote_runner(Arc::new(move |_, request| {
      seen.lock().unwrap().push(request);
      Ok(answer.clone())
    }));
    calls
  }

  fn remote_session(pty: &str) -> (SessionInfo, RemoteSession) {
    (
      SessionInfo {
        pty_id: pty.into(),
        handle: "claude-agent-1".into(),
        harness: "claude".into(),
        repo: "/srv/app".into(),
        workspace: Some("main".into()),
        ledger: Some("remote:app".into()),
      },
      RemoteSession {
        endpoint: serde_json::from_value(serde_json::json!({
          "id": "ep-1",
          "instance_id": null,
          "source": { "type": "user_managed" },
          "hostname": "devbox.example",
          "port": 22,
          "username": "me",
          "host_keys": [],
          "authentication": { "type": "public_key", "key_reference": "/k.pub" },
        }))
        .unwrap(),
        repo: "/srv/app".into(),
        workspace: "main".into(),
        label: "agent-1".into(),
      },
    )
  }

  /// The runner and the remote registry are process-wide, so the remote
  /// cases run in one test.
  #[test]
  fn remote_terminals_get_their_mail_through_the_host() {
    let calls = fake_runner(serde_json::json!([]));
    let (session, remote) = remote_session("remote-pty-service-1");
    register_remote(session, remote, 80, 24);
    assert!(hub().session("remote-pty-service-1").is_some());

    let envelope = super::super::hub::Envelope {
      id: "m-1".into(),
      kind: super::super::model::Kind::Message,
      from: "codex-1".into(),
      to: "claude-agent-1".into(),
      to_pty: "remote-pty-service-1".into(),
      body: "hi".into(),
      reply_to: None,
      sent_at_ms: 0,
      timeout_ms: 60_000,
      origin: "127.0.0.1:1".into(),
      origin_repo: "/src/app".into(),
    };
    mirror(&envelope).unwrap();
    let mut local = envelope;
    local.to_pty = "pty-local-service".into();
    mirror(&local).unwrap();

    poll_remotes();
    let calls = calls.lock().unwrap().clone();
    assert_eq!(
      calls.len(),
      2,
      "one deliver for the remote terminal, one poll: {calls:?}"
    );
    match &calls[0] {
      TreqCommandRequest::PtyMsgDeliver {
        repo,
        workspace,
        label,
        envelope,
      } => {
        assert_eq!(
          (repo.as_str(), workspace.as_str(), label.as_str()),
          ("/srv/app", "main", "agent-1")
        );
        assert!(envelope.contains("\"m-1\""));
      }
      other => panic!("{other:?}"),
    }
    assert!(matches!(
      &calls[1],
      TreqCommandRequest::PtyMsgPoll { since: 0, .. }
    ));
    unregister("remote-pty-service-1");
  }

  #[test]
  fn generation_moves_when_sessions_change() {
    let before = generation();
    register(
      SessionInfo {
        pty_id: "msg-service-1".into(),
        handle: "claude-1".into(),
        harness: "claude".into(),
        repo: "/src/app".into(),
        workspace: None,
        ledger: None,
      },
      80,
      24,
    );
    assert!(generation() > before);
    assert!(hub().session("msg-service-1").is_some());
    unregister("msg-service-1");
    assert!(hub().session("msg-service-1").is_none());
  }
}
