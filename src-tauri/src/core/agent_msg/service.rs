//! The running instance's messaging: one hub and screen tracker per
//! process, the IPC entry point, and the delivery loop.

use super::coordinator::Coordinator;
use super::hub::{Hub, SessionInfo};
use super::network::{DbLedger, InstanceNetwork};
use super::protocol::{self, Request, Response};
use super::terminals::{ScreenTracker, Writer};
use crate::agent_dispatch::now_millis;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

/// How often the delivery loop looks at the terminals.
const TICK: Duration = Duration::from_millis(500);

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

/// Starts messaging for an instance reachable at `endpoint`. Once per process.
pub fn start(endpoint: String) {
  if ENDPOINT.set(endpoint).is_err() {
    return;
  }
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
  let response = with_coordinator(|coordinator| {
    protocol::handle(coordinator, &rows, request, now_millis())
  })
  .unwrap_or_else(|| Response::error("this treq instance is still starting"));
  changed();
  response
}

/// The banner's "Deliver now": types the wake line into `pty_id` now.
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
    let mut repos: Vec<String> = hub().sessions().into_iter().map(|s| s.repo).collect();
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
