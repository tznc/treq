//! One treq instance's side of messaging: the agent terminals it launched,
//! the messages waiting for each, and the delivery tick that wakes them.
//!
//! The hub never touches a PTY or the db itself. [`Terminals`] reads a
//! terminal and types into it; every state change comes back as a
//! [`Report`] for the caller to record at the message's origin.

use super::delivery::{self, Action};
use super::idle::{Harness, Screen};
use super::model::{Kind, State};
use crate::lock_ext::LockExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// An agent terminal treq launched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
  pub pty_id: String,
  /// What other agents address it by: [`handle`].
  pub handle: String,
  pub harness: String,
  pub repo: String,
  pub workspace: Option<String>,
  /// The app db its sent messages are recorded in, when not `repo`: a
  /// remote terminal's repository exists only on its host.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub ledger: Option<String>,
}

/// `<harness>-<session id>`, as in `claude-2`.
pub fn handle(harness: &str, session_id: i64) -> String {
  format!("{harness}-{session_id}")
}

/// A message on its way to one target, as the target's instance holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
  pub id: String,
  pub kind: Kind,
  pub from: String,
  pub to: String,
  pub to_pty: String,
  pub body: String,
  #[serde(default)]
  pub reply_to: Option<String>,
  pub sent_at_ms: u64,
  pub timeout_ms: u64,
  /// The endpoint of the instance that sent it, where its state is recorded.
  pub origin: String,
  /// The sender's repository, whose app db holds the ledger.
  pub origin_repo: String,
}

/// A state change to record at the message's origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
  pub origin: String,
  pub origin_repo: String,
  pub id: String,
  pub target: String,
  pub state: State,
  #[serde(default)]
  pub mode: Option<String>,
}

/// Reads and types into the terminals the hub knows.
pub trait Terminals {
  /// What the screen shows; `None` when the terminal is gone.
  fn screen(&self, pty_id: &str, harness: Harness) -> Option<Screen>;
  /// How long ago the user last typed into it; `None` if never.
  fn since_last_key(&self, pty_id: &str) -> Option<Duration>;
  /// Types `line` and presses Enter.
  fn type_line(&self, pty_id: &str, line: &str) -> Result<(), String>;
}

#[derive(Debug, Clone)]
struct Waiting {
  envelope: Envelope,
  /// Recorded as delivered at the origin (woken or badged).
  delivered: bool,
  /// The wake line was typed for it.
  woken: bool,
  read: bool,
}

impl Waiting {
  fn live(&self, now_ms: u64) -> bool {
    !super::model::timed_out(
      self.envelope.sent_at_ms,
      Duration::from_millis(self.envelope.timeout_ms),
      now_ms,
    )
  }
}

fn report(envelope: &Envelope, state: State, mode: Option<&str>) -> Report {
  Report {
    origin: envelope.origin.clone(),
    origin_repo: envelope.origin_repo.clone(),
    id: envelope.id.clone(),
    target: envelope.to.clone(),
    state,
    mode: mode.map(str::to_string),
  }
}

#[derive(Default)]
pub struct Hub {
  sessions: Mutex<Vec<SessionInfo>>,
  waiting: Mutex<HashMap<String, Vec<Waiting>>>,
  last_sender: Mutex<HashMap<String, (String, String)>>,
}

impl Hub {
  pub fn new() -> Self {
    Self::default()
  }

  /// Adds or replaces the session with this PTY id.
  pub fn register(&self, session: SessionInfo) {
    let mut sessions = self.sessions.lock_or_recover();
    sessions.retain(|known| known.pty_id != session.pty_id);
    sessions.push(session);
  }

  /// Forgets a session and the messages waiting for it.
  pub fn unregister(&self, pty_id: &str) {
    self
      .sessions
      .lock_or_recover()
      .retain(|known| known.pty_id != pty_id);
    self.waiting.lock_or_recover().remove(pty_id);
    self.last_sender.lock_or_recover().remove(pty_id);
  }

  pub fn sessions(&self) -> Vec<SessionInfo> {
    self.sessions.lock_or_recover().clone()
  }

  pub fn session(&self, pty_id: &str) -> Option<SessionInfo> {
    self
      .sessions
      .lock_or_recover()
      .iter()
      .find(|known| known.pty_id == pty_id)
      .cloned()
  }

  /// Queues `envelope` for its `to_pty`. An unknown terminal is an error.
  pub fn enqueue(&self, envelope: Envelope) -> Result<(), String> {
    if self.session(&envelope.to_pty).is_none() {
      return Err(format!("{} is not running", envelope.to));
    }
    self.last_sender.lock_or_recover().insert(
      envelope.to_pty.clone(),
      (envelope.from.clone(), envelope.origin_repo.clone()),
    );
    let mut waiting = self.waiting.lock_or_recover();
    let queue = waiting.entry(envelope.to_pty.clone()).or_default();
    queue.retain(|known| known.envelope.id != envelope.id);
    queue.push(Waiting {
      envelope,
      delivered: false,
      woken: false,
      read: false,
    });
    queue.sort_by_key(|known| known.envelope.sent_at_ms);
    Ok(())
  }

  /// One delivery pass at `now_ms`: drops what timed out, then wakes each
  /// terminal that has undelivered messages or badges it, per
  /// [`delivery::decide`].
  pub fn tick(&self, terminals: &dyn Terminals, now_ms: u64) -> Vec<Report> {
    let mut reports = Vec::new();
    for session in self.sessions() {
      let Some(harness) = Harness::parse(&session.harness) else {
        continue;
      };
      // Drop what ran out of time, then see whether anything is left to show.
      let unwoken = {
        let mut waiting = self.waiting.lock_or_recover();
        let Some(queue) = waiting.get_mut(&session.pty_id) else {
          continue;
        };
        queue.retain(|known| known.live(now_ms));
        queue.iter().any(|known| !known.read && !known.woken)
      };
      if !unwoken {
        continue;
      }
      let Some(screen) = terminals.screen(&session.pty_id, harness) else {
        continue;
      };
      let action = delivery::decide(screen, terminals.since_last_key(&session.pty_id));
      match action {
        Action::Defer => {}
        Action::Badge => reports.extend(self.mark(&session.pty_id, "badge", false)),
        Action::Wake => reports.extend(self.wake(&session.pty_id, terminals)),
      }
    }
    reports
  }

  /// Types the wake line now, whatever the screen shows: the banner's
  /// "Deliver now". Nothing happens when nothing waits.
  pub fn deliver_now(&self, pty_id: &str, terminals: &dyn Terminals) -> Vec<Report> {
    self.wake(pty_id, terminals)
  }

  /// Types the wake line for what waits unread in `pty_id`, then records
  /// the ones not yet delivered.
  fn wake(&self, pty_id: &str, terminals: &dyn Terminals) -> Vec<Report> {
    let line = {
      let waiting = self.waiting.lock_or_recover();
      let unread: Vec<&Waiting> = waiting
        .get(pty_id)
        .map(|queue| queue.iter().filter(|known| !known.read).collect())
        .unwrap_or_default();
      let Some(latest) = unread.last() else {
        return Vec::new();
      };
      delivery::wake_line(unread.len(), &latest.envelope.from, &latest.envelope.id)
    };
    if let Err(error) = terminals.type_line(pty_id, &line) {
      log::warn!("agent msg wake failed: pty={pty_id} error={error}");
      return Vec::new();
    }
    self.mark(pty_id, "wake", true)
  }

  /// Marks the unread messages of `pty_id` delivered by `mode`, returning a
  /// report for each one not delivered before.
  fn mark(&self, pty_id: &str, mode: &str, woken: bool) -> Vec<Report> {
    let mut waiting = self.waiting.lock_or_recover();
    let Some(queue) = waiting.get_mut(pty_id) else {
      return Vec::new();
    };
    let mut reports = Vec::new();
    for known in queue.iter_mut().filter(|known| !known.read) {
      known.woken |= woken;
      if !known.delivered {
        known.delivered = true;
        reports.push(report(&known.envelope, State::Delivered, Some(mode)));
      }
    }
    reports
  }

  /// The messages waiting for `pty_id`, oldest first. Unless `peek`, they
  /// are marked read.
  pub fn inbox(&self, pty_id: &str, peek: bool, now_ms: u64) -> (Vec<Envelope>, Vec<Report>) {
    let mut waiting = self.waiting.lock_or_recover();
    let Some(queue) = waiting.get_mut(pty_id) else {
      return (Vec::new(), Vec::new());
    };
    queue.retain(|known| known.live(now_ms));
    let mut reports = Vec::new();
    let mut messages = Vec::new();
    for known in queue.iter_mut() {
      if !peek && !known.read {
        known.read = true;
        reports.push(report(&known.envelope, State::Read, None));
      }
      messages.push(known.envelope.clone());
    }
    (messages, reports)
  }

  /// Marks the given messages of `pty_id` read, as a remote agent's
  /// `--inbox` did on its host.
  pub fn mark_read(&self, pty_id: &str, ids: &[String]) -> Vec<Report> {
    let mut waiting = self.waiting.lock_or_recover();
    let Some(queue) = waiting.get_mut(pty_id) else {
      return Vec::new();
    };
    let mut reports = Vec::new();
    for known in queue.iter_mut() {
      if !known.read && ids.contains(&known.envelope.id) {
        known.read = true;
        reports.push(report(&known.envelope, State::Read, None));
      }
    }
    reports
  }

  /// Ends the given messages of `pty_id` as acked (or `Replied`). Ids that
  /// are not waiting are ignored.
  pub fn finish(&self, pty_id: &str, ids: &[String], state: State) -> Vec<Report> {
    let mut waiting = self.waiting.lock_or_recover();
    let Some(queue) = waiting.get_mut(pty_id) else {
      return Vec::new();
    };
    let mut reports = Vec::new();
    queue.retain(|known| {
      if ids.contains(&known.envelope.id) {
        reports.push(report(&known.envelope, state, None));
        false
      } else {
        true
      }
    });
    reports
  }

  /// How many unread messages wait for `pty_id`: its badge.
  pub fn waiting(&self, pty_id: &str, now_ms: u64) -> usize {
    self
      .waiting
      .lock_or_recover()
      .get(pty_id)
      .map_or(0, |queue| {
        queue
          .iter()
          .filter(|known| !known.read && known.live(now_ms))
          .count()
      })
  }

  /// The newest unread message for `pty_id`: what its banner names.
  pub fn latest(&self, pty_id: &str, now_ms: u64) -> Option<Envelope> {
    self
      .waiting
      .lock_or_recover()
      .get(pty_id)?
      .iter()
      .filter(|known| !known.read && known.live(now_ms))
      .max_by_key(|known| known.envelope.sent_at_ms)
      .map(|known| known.envelope.clone())
  }

  /// Who last messaged `pty_id`, and from which repository: where a
  /// `--status` with no target goes.
  pub fn last_sender(&self, pty_id: &str) -> Option<(String, String)> {
    self.last_sender.lock_or_recover().get(pty_id).cloned()
  }

  /// A reply to message `id` waiting for `pty_id`, for a blocking `--ask`.
  /// Taking it marks it read.
  pub fn take_reply(&self, pty_id: &str, id: &str) -> Option<(Envelope, Vec<Report>)> {
    let mut waiting = self.waiting.lock_or_recover();
    let known = waiting
      .get_mut(pty_id)?
      .iter_mut()
      .find(|known| known.envelope.reply_to.as_deref() == Some(id))?;
    let mut reports = Vec::new();
    if !known.read {
      known.read = true;
      reports.push(report(&known.envelope, State::Read, None));
    }
    Some((known.envelope.clone(), reports))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::cell::RefCell;

  #[derive(Default)]
  struct FakeTerminals {
    screens: HashMap<String, Screen>,
    keys: HashMap<String, Duration>,
    typed: RefCell<Vec<(String, String)>>,
  }

  impl Terminals for FakeTerminals {
    fn screen(&self, pty_id: &str, _harness: Harness) -> Option<Screen> {
      self.screens.get(pty_id).copied()
    }
    fn since_last_key(&self, pty_id: &str) -> Option<Duration> {
      self.keys.get(pty_id).copied()
    }
    fn type_line(&self, pty_id: &str, line: &str) -> Result<(), String> {
      self.typed.borrow_mut().push((pty_id.into(), line.into()));
      Ok(())
    }
  }

  fn terminals(screen: Screen) -> FakeTerminals {
    FakeTerminals {
      screens: HashMap::from([("pty-claude".to_string(), screen)]),
      ..FakeTerminals::default()
    }
  }

  fn hub() -> Hub {
    let hub = Hub::new();
    hub.register(SessionInfo {
      pty_id: "pty-claude".into(),
      handle: "claude-2".into(),
      harness: "claude".into(),
      repo: "/repo".into(),
      workspace: Some("fix/rebase".into()),
      ledger: None,
    });
    hub
  }

  fn envelope(id: &str, sent_at_ms: u64) -> Envelope {
    Envelope {
      id: id.into(),
      kind: Kind::Ask,
      from: "codex-1".into(),
      to: "claude-2".into(),
      to_pty: "pty-claude".into(),
      body: "rebase done?".into(),
      reply_to: None,
      sent_at_ms,
      timeout_ms: 60_000,
      origin: "127.0.0.1:9".into(),
      origin_repo: "/repo".into(),
    }
  }

  fn states(reports: &[Report]) -> Vec<(String, State, Option<String>)> {
    reports
      .iter()
      .map(|r| (r.id.clone(), r.state, r.mode.clone()))
      .collect()
  }

  #[test]
  fn handles_are_harness_dash_session_id() {
    assert_eq!(handle("claude", 2), "claude-2");
    assert_eq!(handle("copilot", 31), "copilot-31");
  }

  #[test]
  fn registering_the_same_pty_again_replaces_it() {
    let hub = hub();
    hub.register(SessionInfo {
      pty_id: "pty-claude".into(),
      handle: "claude-2".into(),
      harness: "claude".into(),
      repo: "/repo".into(),
      workspace: Some("renamed".into()),
      ledger: None,
    });
    assert_eq!(hub.sessions().len(), 1);
    assert_eq!(
      hub.session("pty-claude").unwrap().workspace.as_deref(),
      Some("renamed")
    );
  }

  #[test]
  fn enqueue_to_an_unknown_terminal_is_an_error() {
    let hub = hub();
    let mut lost = envelope("m-1", 0);
    lost.to_pty = "pty-gone".into();
    assert!(hub.enqueue(lost).is_err());
  }

  #[test]
  fn an_idle_agent_is_woken_once_with_the_wake_line() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    let terms = terminals(Screen::IdleEmpty);

    let reports = hub.tick(&terms, 1_000);
    assert_eq!(
      states(&reports),
      vec![("m-1".into(), State::Delivered, Some("wake".into()))]
    );
    assert_eq!(reports[0].origin, "127.0.0.1:9");
    assert_eq!(reports[0].target, "claude-2");
    assert_eq!(
      terms.typed.borrow().as_slice(),
      &[(
        "pty-claude".to_string(),
        "[treq] 1 message from codex-1 (m-1). Read it: treq agent msg --inbox".to_string()
      )]
    );

    assert!(hub.tick(&terms, 2_000).is_empty(), "delivered messages are not typed again");
    assert_eq!(terms.typed.borrow().len(), 1);
  }

  #[test]
  fn a_terminal_with_a_draft_gets_a_badge_and_no_typing() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    let terms = terminals(Screen::IdleTyped);

    let reports = hub.tick(&terms, 1_000);
    assert_eq!(
      states(&reports),
      vec![("m-1".into(), State::Delivered, Some("badge".into()))]
    );
    assert!(terms.typed.borrow().is_empty());
    assert_eq!(hub.waiting("pty-claude", 1_000), 1);
  }

  #[test]
  fn a_badged_message_is_woken_once_the_prompt_is_empty_again() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    hub.tick(&terminals(Screen::IdleTyped), 1_000);

    let terms = terminals(Screen::IdleEmpty);
    let reports = hub.tick(&terms, 5_000);
    assert!(reports.is_empty(), "already delivered, so nothing new to record");
    assert_eq!(terms.typed.borrow().len(), 1, "but the wake line is typed");
    assert!(hub.tick(&terms, 6_000).is_empty());
    assert_eq!(terms.typed.borrow().len(), 1);
  }

  #[test]
  fn a_busy_agent_keeps_its_message_queued() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    let terms = terminals(Screen::Busy);
    assert!(hub.tick(&terms, 1_000).is_empty());
    assert!(terms.typed.borrow().is_empty());
    assert_eq!(hub.waiting("pty-claude", 1_000), 1);
  }

  #[test]
  fn recent_keystrokes_badge_an_empty_prompt() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    let mut terms = terminals(Screen::IdleEmpty);
    terms
      .keys
      .insert("pty-claude".into(), Duration::from_millis(400));
    let reports = hub.tick(&terms, 1_000);
    assert_eq!(reports[0].mode.as_deref(), Some("badge"));
    assert!(terms.typed.borrow().is_empty());
  }

  #[test]
  fn one_wake_line_covers_every_waiting_message() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    let mut second = envelope("m-2", 10);
    second.from = "copilot-3".into();
    hub.enqueue(second).unwrap();
    let terms = terminals(Screen::IdleEmpty);

    let reports = hub.tick(&terms, 1_000);
    assert_eq!(reports.len(), 2);
    assert_eq!(
      terms.typed.borrow()[0].1,
      "[treq] 2 messages, latest from copilot-3 (m-2). Read them: treq agent msg --inbox"
    );
  }

  #[test]
  fn timed_out_messages_leave_the_inbox() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    let terms = terminals(Screen::Busy);
    hub.tick(&terms, 60_000);
    assert_eq!(hub.waiting("pty-claude", 60_000), 0);
    assert!(hub.inbox("pty-claude", false, 60_000).0.is_empty());
  }

  #[test]
  fn inbox_returns_oldest_first_and_marks_read_once() {
    let hub = hub();
    hub.enqueue(envelope("m-2", 20)).unwrap();
    hub.enqueue(envelope("m-1", 10)).unwrap();

    let (messages, reports) = hub.inbox("pty-claude", false, 100);
    let ids: Vec<&str> = messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["m-1", "m-2"]);
    assert_eq!(
      states(&reports),
      vec![
        ("m-1".into(), State::Read, None),
        ("m-2".into(), State::Read, None)
      ]
    );

    let (again, reports) = hub.inbox("pty-claude", false, 200);
    assert_eq!(again.len(), 2, "read messages stay until acked");
    assert!(reports.is_empty(), "and are not reported read twice");
  }

  #[test]
  fn peek_does_not_mark_read() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    let (messages, reports) = hub.inbox("pty-claude", true, 100);
    assert_eq!(messages.len(), 1);
    assert!(reports.is_empty());
    let (_, reports) = hub.inbox("pty-claude", false, 100);
    assert_eq!(reports.len(), 1);
  }

  #[test]
  fn reading_a_message_stops_its_wake_line() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    hub.inbox("pty-claude", false, 100);
    let terms = terminals(Screen::IdleEmpty);
    hub.tick(&terms, 1_000);
    assert!(terms.typed.borrow().is_empty());
  }

  #[test]
  fn finish_removes_the_message_and_reports_its_state() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    hub.enqueue(envelope("m-2", 0)).unwrap();
    let reports = hub.finish(
      "pty-claude",
      &["m-1".to_string(), "m-9".to_string()],
      State::Acked,
    );
    assert_eq!(states(&reports), vec![("m-1".into(), State::Acked, None)]);
    let ids: Vec<String> = hub
      .inbox("pty-claude", true, 100)
      .0
      .into_iter()
      .map(|m| m.id)
      .collect();
    assert_eq!(ids, vec!["m-2".to_string()]);
  }

  #[test]
  fn latest_names_the_newest_waiting_message() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 10)).unwrap();
    hub.enqueue(envelope("m-2", 20)).unwrap();
    assert_eq!(hub.latest("pty-claude", 30).unwrap().id, "m-2");
    assert!(hub.latest("pty-other", 30).is_none());
  }

  #[test]
  fn deliver_now_types_whatever_the_screen() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    hub.tick(&terminals(Screen::IdleTyped), 1_000);
    let terms = terminals(Screen::IdleTyped);
    hub.deliver_now("pty-claude", &terms);
    assert_eq!(terms.typed.borrow().len(), 1);
  }

  #[test]
  fn take_reply_finds_the_answer_to_an_ask() {
    let hub = hub();
    let mut reply = envelope("m-2", 5);
    reply.kind = Kind::Reply;
    reply.reply_to = Some("m-1".into());
    hub.enqueue(reply).unwrap();

    assert!(hub.take_reply("pty-claude", "m-9").is_none());
    let (found, reports) = hub.take_reply("pty-claude", "m-1").unwrap();
    assert_eq!(found.id, "m-2");
    assert_eq!(states(&reports), vec![("m-2".into(), State::Read, None)]);
  }

  #[test]
  fn last_sender_outlives_the_message() {
    let hub = hub();
    assert_eq!(hub.last_sender("pty-claude"), None);
    hub.enqueue(envelope("m-1", 0)).unwrap();
    let mut later = envelope("m-2", 10);
    later.from = "copilot-3".into();
    later.origin_repo = "/other".into();
    hub.enqueue(later).unwrap();
    hub.finish("pty-claude", &["m-1".into(), "m-2".into()], State::Acked);
    assert_eq!(
      hub.last_sender("pty-claude"),
      Some(("copilot-3".to_string(), "/other".to_string()))
    );
  }

  #[test]
  fn mark_read_reports_each_message_once() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    let ids = vec!["m-1".to_string(), "m-9".to_string()];
    assert_eq!(
      states(&hub.mark_read("pty-claude", &ids)),
      vec![("m-1".into(), State::Read, None)]
    );
    assert!(hub.mark_read("pty-claude", &ids).is_empty());
    assert_eq!(hub.waiting("pty-claude", 0), 0);
  }

  #[test]
  fn reading_clears_the_badge() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    hub.inbox("pty-claude", false, 100);
    assert_eq!(hub.waiting("pty-claude", 100), 0);
    assert!(hub.latest("pty-claude", 100).is_none());
  }

  #[test]
  fn unregister_drops_the_session_and_its_messages() {
    let hub = hub();
    hub.enqueue(envelope("m-1", 0)).unwrap();
    hub.unregister("pty-claude");
    assert!(hub.sessions().is_empty());
    assert_eq!(hub.waiting("pty-claude", 0), 0);
  }
}
