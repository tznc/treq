//! A remote agent's messages, kept on its host in a file per terminal.
//!
//! A host runs no treq app, so the instance that attached the terminal
//! coordinates for it over SSH: it writes each message here
//! (`treq pty-remote msg-deliver`) and polls what the agent did
//! (`treq pty-remote msg-poll`). The agent's own `treq agent msg` on the
//! host reads and answers this file, recording each step as an [`Event`].

use super::hub::Envelope;
use super::model::{self, Kind, State};
use super::protocol::Op;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Something the agent did, for the attached instance to act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
  /// One of its messages moved to `state`.
  State {
    seq: u64,
    at_ms: u64,
    id: String,
    state: State,
  },
  /// It sent a message, for the attached instance to route.
  Send {
    seq: u64,
    at_ms: u64,
    message: Outgoing,
  },
}

impl Event {
  pub fn seq(&self) -> u64 {
    match self {
      Event::State { seq, .. } | Event::Send { seq, .. } => *seq,
    }
  }

  fn at_ms(&self) -> u64 {
    match self {
      Event::State { at_ms, .. } | Event::Send { at_ms, .. } => *at_ms,
    }
  }
}

/// A message the remote agent sent, with the id it was given on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outgoing {
  pub id: String,
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

/// A message waiting for the remote agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Held {
  envelope: Option<Envelope>,
  #[serde(default)]
  read: bool,
}

/// The file's contents.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Mail {
  #[serde(default)]
  next_seq: u64,
  #[serde(default)]
  held: Vec<Held>,
  #[serde(default)]
  events: Vec<Event>,
  /// Who last messaged the agent, where `--status` goes by default.
  #[serde(default)]
  last_from: Option<String>,
}

impl Mail {
  fn record_state(&mut self, id: &str, state: State, now_ms: u64) {
    self.next_seq += 1;
    self.events.push(Event::State {
      seq: self.next_seq,
      at_ms: now_ms,
      id: id.to_string(),
      state,
    });
  }

  fn record_send(&mut self, message: Outgoing, now_ms: u64) {
    self.next_seq += 1;
    self.events.push(Event::Send {
      seq: self.next_seq,
      at_ms: now_ms,
      message,
    });
  }

  fn live(&mut self, now_ms: u64) -> impl Iterator<Item = &mut Held> {
    self.held.retain(|held| {
      held.envelope.as_ref().is_some_and(|envelope| {
        !model::timed_out(
          envelope.sent_at_ms,
          Duration::from_millis(envelope.timeout_ms),
          now_ms,
        )
      })
    });
    self.held.iter_mut()
  }

  fn take(&mut self, id: &str) -> Option<Envelope> {
    let index = self
      .held
      .iter()
      .position(|held| held.envelope.as_ref().is_some_and(|e| e.id == id))?;
    self.held.remove(index).envelope
  }
}

/// `session` as a file name: only letters, digits, `-`, `_` and `.`, and
/// never `.` or `..`.
fn file_stem(session: &str) -> String {
  let stem: String = session
    .chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
        c
      } else {
        '_'
      }
    })
    .collect();
  if stem.is_empty() {
    "_".into()
  } else {
    stem
  }
}

/// Opens `session`'s file under a lock, lets `change` read and change it,
/// and writes it back.
fn with_mail<T>(dir: &Path, session: &str, change: impl FnOnce(&mut Mail) -> Result<T, String>) -> Result<T, String> {
  std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
  let stem = file_stem(session);
  let lock_path = dir.join(format!("{stem}.lock"));
  let lock = std::fs::OpenOptions::new()
    .create(true)
    .truncate(false)
    .write(true)
    .open(&lock_path)
    .map_err(|e| format!("failed to open {}: {e}", lock_path.display()))?;
  lock
    .lock()
    .map_err(|e| format!("failed to lock {}: {e}", lock_path.display()))?;
  let path = dir.join(format!("{stem}.json"));
  let mut mail: Mail = match std::fs::read_to_string(&path) {
    Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Mail::default(),
    Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
  };
  let result = change(&mut mail)?;
  let text = serde_json::to_string(&mail).map_err(|e| format!("failed to encode mailbox: {e}"))?;
  let temp = dir.join(format!("{stem}.json.tmp"));
  std::fs::write(&temp, text)
    .and_then(|()| std::fs::rename(&temp, &path))
    .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
  Ok(result)
}

fn to_json<T: Serialize>(value: &T) -> Result<Value, String> {
  serde_json::to_value(value).map_err(|e| format!("failed to encode answer: {e}"))
}

/// What a send answers on the host: the id, and where the message goes
/// once the attached instance routes it.
fn queued(id: &str, to: Option<&str>) -> Value {
  serde_json::json!({
    "id": id,
    "targets": [{
      "handle": to.unwrap_or("--all"),
      "repo": "",
      "via": "treq app",
      "outcome": "queued",
    }],
  })
}

/// The default directory: `~/.treq/agent-msg`.
pub fn default_dir() -> Result<PathBuf, String> {
  let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
  Ok(Path::new(&home).join(".treq").join("agent-msg"))
}

/// Holds `envelope` for the terminal `session`. Delivering it again
/// replaces it.
pub fn deliver(dir: &Path, session: &str, envelope: &Envelope) -> Result<(), String> {
  with_mail(dir, session, |mail| {
    mail.last_from = Some(envelope.from.clone());
    mail.take(&envelope.id);
    mail.held.push(Held {
      envelope: Some(envelope.clone()),
      read: false,
    });
    Ok(())
  })
}

/// The events after `since`. The ones up to `since` were taken and are
/// dropped.
pub fn poll(dir: &Path, session: &str, since: u64, now_ms: u64) -> Result<Vec<Event>, String> {
  with_mail(dir, session, |mail| {
    mail
      .events
      .retain(|event| event.seq() > since && !model::expired(event.at_ms(), now_ms));
    Ok(mail.events.clone())
  })
}

/// Answers the remote agent's own `treq agent msg` from the file, as the
/// instance would ([`super::protocol::handle`]).
pub fn answer(dir: &Path, session: &str, op: &Op, now_ms: u64) -> Result<Value, String> {
  with_mail(dir, session, |mail| match op {
    Op::Send(request) => {
      let id = request.id.clone().unwrap_or_else(model::new_id);
      mail.record_send(
        Outgoing {
          id: id.clone(),
          to: request.to.clone(),
          all: request.all,
          repo: request.repo.clone(),
          kind: request.kind,
          body: request.body.clone(),
          reply_to: request.reply_to.clone(),
          timeout_ms: request.timeout_ms,
        },
        now_ms,
      );
      Ok(queued(&id, request.to.as_deref()))
    }
    Op::Inbox { peek, .. } => {
      let mut read = Vec::new();
      let mut messages = Vec::new();
      for held in mail.live(now_ms) {
        let Some(envelope) = &held.envelope else {
          continue;
        };
        if !peek && !held.read {
          held.read = true;
          read.push(envelope.id.clone());
        }
        messages.push(envelope.clone());
      }
      for id in read {
        mail.record_state(&id, State::Read, now_ms);
      }
      messages.sort_by_key(|envelope| envelope.sent_at_ms);
      Ok(serde_json::json!({ "messages": to_json(&messages)? }))
    }
    Op::Ack { ids, .. } => {
      let mut acked = 0;
      for id in ids {
        if mail.take(id).is_some() {
          mail.record_state(id, State::Acked, now_ms);
          acked += 1;
        }
      }
      Ok(serde_json::json!({ "acked": acked }))
    }
    Op::Reply { id, body, .. } => {
      let asked = mail
        .take(id)
        .ok_or_else(|| format!("no message {id} is waiting for you. See treq agent msg --inbox"))?;
      mail.record_state(id, State::Replied, now_ms);
      let reply_id = model::new_id();
      mail.record_send(
        Outgoing {
          id: reply_id.clone(),
          to: Some(asked.from.clone()),
          all: false,
          repo: None,
          kind: Kind::Reply,
          body: body.clone(),
          reply_to: Some(asked.id.clone()),
          timeout_ms: model::TIMEOUT.as_millis() as u64,
        },
        now_ms,
      );
      Ok(queued(&reply_id, Some(&asked.from)))
    }
    Op::Status { to, body, .. } => {
      let to = to
        .clone()
        .or_else(|| mail.last_from.clone())
        .ok_or("no one has messaged you yet: name the agent to report to")?;
      let id = model::new_id();
      mail.record_send(
        Outgoing {
          id: id.clone(),
          to: Some(to.clone()),
          all: false,
          repo: None,
          kind: Kind::Status,
          body: body.clone(),
          reply_to: None,
          timeout_ms: model::TIMEOUT.as_millis() as u64,
        },
        now_ms,
      );
      Ok(queued(&id, Some(&to)))
    }
    Op::TakeReply { id, .. } => {
      let found = mail.live(now_ms).find_map(|held| {
        held
          .envelope
          .as_ref()
          .filter(|envelope| envelope.reply_to.as_deref() == Some(id.as_str()))
          .map(|envelope| envelope.id.clone())
      });
      let Some(reply_id) = found else {
        return Ok(Value::Null);
      };
      let reply = mail.take(&reply_id);
      mail.record_state(&reply_id, State::Read, now_ms);
      mail.record_state(&reply_id, State::Acked, now_ms);
      to_json(&reply)
    }
    Op::List { .. } | Op::Rows { .. } | Op::Deliver { .. } | Op::Report { .. } => Err(
      "this runs on a remote host: --list and message logs need the treq app on your computer"
        .into(),
    ),
  })
}

#[cfg(test)]
mod tests {
  use super::super::coordinator::SendRequest;
  use super::*;
  use tempfile::TempDir;

  const SESSION: &str = "treq-app-main-agent-1";

  fn envelope(id: &str, sent_at_ms: u64) -> Envelope {
    Envelope {
      id: id.into(),
      kind: Kind::Ask,
      from: "codex-1".into(),
      to: "claude-agent-1".into(),
      to_pty: "remote-pty-agent-1".into(),
      body: "rebase done?".into(),
      reply_to: None,
      sent_at_ms,
      timeout_ms: 60_000,
      origin: "127.0.0.1:1".into(),
      origin_repo: "/src/app".into(),
    }
  }

  fn inbox(dir: &Path, peek: bool, now_ms: u64) -> Value {
    answer(
      dir,
      SESSION,
      &Op::Inbox {
        pty: SESSION.into(),
        peek,
      },
      now_ms,
    )
    .unwrap()
  }

  #[test]
  fn delivered_messages_show_in_the_inbox_and_reading_them_is_an_event() {
    let dir = TempDir::new().unwrap();
    deliver(dir.path(), SESSION, &envelope("m-1", 0)).unwrap();

    let messages = inbox(dir.path(), false, 1_000);
    assert_eq!(messages["messages"][0]["id"], "m-1");
    assert_eq!(messages["messages"][0]["body"], "rebase done?");
    let events = poll(dir.path(), SESSION, 0, 1_000).unwrap();
    assert!(matches!(
      events.as_slice(),
      [Event::State { id, state: State::Read, .. }] if id == "m-1"
    ));
  }

  #[test]
  fn delivering_the_same_message_twice_keeps_one() {
    let dir = TempDir::new().unwrap();
    deliver(dir.path(), SESSION, &envelope("m-1", 0)).unwrap();
    deliver(dir.path(), SESSION, &envelope("m-1", 0)).unwrap();
    assert_eq!(inbox(dir.path(), true, 0)["messages"].as_array().unwrap().len(), 1);
  }

  #[test]
  fn peek_records_nothing() {
    let dir = TempDir::new().unwrap();
    deliver(dir.path(), SESSION, &envelope("m-1", 0)).unwrap();
    inbox(dir.path(), true, 0);
    assert!(poll(dir.path(), SESSION, 0, 0).unwrap().is_empty());
  }

  #[test]
  fn timed_out_messages_leave_the_inbox() {
    let dir = TempDir::new().unwrap();
    deliver(dir.path(), SESSION, &envelope("m-1", 0)).unwrap();
    assert!(inbox(dir.path(), false, 60_000)["messages"]
      .as_array()
      .unwrap()
      .is_empty());
  }

  #[test]
  fn poll_drops_what_was_taken_and_returns_the_rest() {
    let dir = TempDir::new().unwrap();
    deliver(dir.path(), SESSION, &envelope("m-1", 0)).unwrap();
    deliver(dir.path(), SESSION, &envelope("m-2", 0)).unwrap();
    inbox(dir.path(), false, 10);
    let first = poll(dir.path(), SESSION, 0, 10).unwrap();
    assert_eq!(first.len(), 2);
    let last = first.last().unwrap().seq();
    let again = poll(dir.path(), SESSION, last, 10).unwrap();
    assert!(again.is_empty());
    assert!(poll(dir.path(), SESSION, 0, 10).unwrap().is_empty(), "taken events are gone");
  }

  #[test]
  fn ack_removes_the_message_and_records_it() {
    let dir = TempDir::new().unwrap();
    deliver(dir.path(), SESSION, &envelope("m-1", 0)).unwrap();
    let acked = answer(
      dir.path(),
      SESSION,
      &Op::Ack {
        pty: SESSION.into(),
        ids: vec!["m-1".into(), "m-9".into()],
      },
      5,
    )
    .unwrap();
    assert_eq!(acked["acked"], 1);
    assert!(inbox(dir.path(), true, 5)["messages"].as_array().unwrap().is_empty());
    let events = poll(dir.path(), SESSION, 0, 5).unwrap();
    assert!(matches!(&events[0], Event::State { state: State::Acked, .. }));
  }

  #[test]
  fn reply_ends_the_ask_and_sends_the_answer_back_to_its_sender() {
    let dir = TempDir::new().unwrap();
    deliver(dir.path(), SESSION, &envelope("m-1", 0)).unwrap();
    let sent = answer(
      dir.path(),
      SESSION,
      &Op::Reply {
        pty: SESSION.into(),
        id: "m-1".into(),
        body: "done".into(),
      },
      5,
    )
    .unwrap();
    assert!(sent["id"].as_str().unwrap().starts_with("m-"));
    let events = poll(dir.path(), SESSION, 0, 5).unwrap();
    assert!(matches!(&events[0], Event::State { id, state: State::Replied, .. } if id == "m-1"));
    match &events[1] {
      Event::Send { message, .. } => {
        assert_eq!(message.kind, Kind::Reply);
        assert_eq!(message.to.as_deref(), Some("codex-1"));
        assert_eq!(message.reply_to.as_deref(), Some("m-1"));
        assert_eq!(message.body, "done");
        assert_eq!(message.id, sent["id"].as_str().unwrap());
      }
      other => panic!("{other:?}"),
    }
  }

  #[test]
  fn replying_to_a_message_not_held_is_an_error() {
    let dir = TempDir::new().unwrap();
    let error = answer(
      dir.path(),
      SESSION,
      &Op::Reply {
        pty: SESSION.into(),
        id: "m-9".into(),
        body: "?".into(),
      },
      0,
    );
    assert!(error.is_err());
  }

  #[test]
  fn send_records_the_message_for_the_attached_instance() {
    let dir = TempDir::new().unwrap();
    let sent = answer(
      dir.path(),
      SESSION,
      &Op::Send(SendRequest {
        from_pty: SESSION.into(),
        to: Some("copilot-3".into()),
        all: false,
        repo: None,
        kind: Kind::Message,
        body: "hi".into(),
        reply_to: None,
        timeout_ms: 30_000,
        id: None,
      }),
      7,
    )
    .unwrap();
    assert_eq!(sent["targets"][0]["handle"], "copilot-3");
    let events = poll(dir.path(), SESSION, 0, 7).unwrap();
    match &events[0] {
      Event::Send { message, at_ms, .. } => {
        assert_eq!(message.id, sent["id"].as_str().unwrap());
        assert_eq!(message.to.as_deref(), Some("copilot-3"));
        assert_eq!(message.timeout_ms, 30_000);
        assert_eq!(*at_ms, 7);
      }
      other => panic!("{other:?}"),
    }
  }

  #[test]
  fn status_goes_to_the_last_sender() {
    let dir = TempDir::new().unwrap();
    deliver(dir.path(), SESSION, &envelope("m-1", 0)).unwrap();
    answer(
      dir.path(),
      SESSION,
      &Op::Status {
        pty: SESSION.into(),
        to: None,
        body: "working".into(),
      },
      5,
    )
    .unwrap();
    let events = poll(dir.path(), SESSION, 0, 5).unwrap();
    assert!(matches!(
      &events[0],
      Event::Send { message, .. } if message.kind == Kind::Status && message.to.as_deref() == Some("codex-1")
    ));
  }

  #[test]
  fn take_reply_finds_and_acks_the_answer() {
    let dir = TempDir::new().unwrap();
    let mut reply = envelope("m-2", 0);
    reply.kind = Kind::Reply;
    reply.reply_to = Some("m-1".into());
    deliver(dir.path(), SESSION, &reply).unwrap();

    let none = answer(
      dir.path(),
      SESSION,
      &Op::TakeReply {
        pty: SESSION.into(),
        id: "m-9".into(),
      },
      1,
    )
    .unwrap();
    assert!(none.is_null());
    let found = answer(
      dir.path(),
      SESSION,
      &Op::TakeReply {
        pty: SESSION.into(),
        id: "m-1".into(),
      },
      1,
    )
    .unwrap();
    assert_eq!(found["id"], "m-2");
    let states: Vec<State> = poll(dir.path(), SESSION, 0, 1)
      .unwrap()
      .into_iter()
      .filter_map(|e| match e {
        Event::State { state, .. } => Some(state),
        Event::Send { .. } => None,
      })
      .collect();
    assert_eq!(states, vec![State::Read, State::Acked]);
  }

  #[test]
  fn listing_needs_the_app() {
    let dir = TempDir::new().unwrap();
    let error = answer(
      dir.path(),
      SESSION,
      &Op::List {
        repo: None,
        local_only: false,
      },
      0,
    )
    .unwrap_err();
    assert!(error.contains("treq app"), "{error}");
  }

  #[test]
  fn events_older_than_an_hour_are_dropped() {
    let dir = TempDir::new().unwrap();
    deliver(dir.path(), SESSION, &envelope("m-1", 0)).unwrap();
    inbox(dir.path(), false, 10);
    assert!(poll(dir.path(), SESSION, 0, 3_600_010).unwrap().is_empty());
  }

  #[test]
  fn session_names_become_safe_file_names() {
    let dir = TempDir::new().unwrap();
    deliver(dir.path(), "a/../b c", &envelope("m-1", 0)).unwrap();
    let names: Vec<String> = std::fs::read_dir(dir.path())
      .unwrap()
      .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
      .collect();
    assert!(names.iter().all(|n| !n.contains('/') && !n.contains(' ')), "{names:?}");
    assert_eq!(inbox_of(dir.path(), "a/../b c"), 1);
  }

  fn inbox_of(dir: &Path, session: &str) -> usize {
    answer(
      dir,
      session,
      &Op::Inbox {
        pty: session.into(),
        peek: true,
      },
      0,
    )
    .unwrap()["messages"]
      .as_array()
      .unwrap()
      .len()
  }
}
