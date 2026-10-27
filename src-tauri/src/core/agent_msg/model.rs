//! A message's life: its kinds, the states one target moves through, and
//! the two clocks that end it.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long a message waits to be acked or replied to. Also the longest
/// `--timeout` accepts.
pub const TIMEOUT: Duration = Duration::from_secs(60);

/// How long a row stays in the app db, whatever its state. The system log
/// keeps the history after that.
pub const RETENTION: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
  /// A message to read; acked once read.
  Message,
  /// A question whose sender may block until it is replied to.
  Ask,
  /// The answer to an earlier message.
  Reply,
  /// A worker's status report.
  Status,
}

impl Kind {
  pub fn as_str(self) -> &'static str {
    match self {
      Kind::Message => "message",
      Kind::Ask => "ask",
      Kind::Reply => "reply",
      Kind::Status => "status",
    }
  }

  pub fn parse(value: &str) -> Option<Self> {
    [Kind::Message, Kind::Ask, Kind::Reply, Kind::Status]
      .into_iter()
      .find(|kind| kind.as_str() == value)
  }
}

/// Where one target's copy of a message stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
  /// Recorded; not yet shown to the agent.
  Queued,
  /// The agent was woken, or its terminal shows the waiting badge.
  Delivered,
  /// The agent fetched it with `--inbox`.
  Read,
  Acked,
  Replied,
  /// Not acked or replied to within its timeout.
  TimedOut,
}

impl State {
  pub fn as_str(self) -> &'static str {
    match self {
      State::Queued => "queued",
      State::Delivered => "delivered",
      State::Read => "read",
      State::Acked => "acked",
      State::Replied => "replied",
      State::TimedOut => "timed_out",
    }
  }

  pub fn parse(value: &str) -> Option<Self> {
    [
      State::Queued,
      State::Delivered,
      State::Read,
      State::Acked,
      State::Replied,
      State::TimedOut,
    ]
    .into_iter()
    .find(|state| state.as_str() == value)
  }

  /// Acked, replied and timed out end a message: nothing moves it after.
  pub fn is_final(self) -> bool {
    matches!(self, State::Acked | State::Replied | State::TimedOut)
  }

  fn rank(self) -> u8 {
    match self {
      State::Queued => 0,
      State::Delivered => 1,
      State::Read => 2,
      State::Acked | State::Replied | State::TimedOut => 3,
    }
  }

  /// The state after `next` happens. States only move forward, and may
  /// skip a step (an agent can read a message before its wake line lands).
  /// Repeating the current state is a no-op; a final state never changes.
  pub fn advance(self, next: State) -> Result<State, String> {
    if self == next {
      return Ok(self);
    }
    if self.is_final() {
      return Err(format!(
        "message is already {}; it cannot become {}",
        self.as_str(),
        next.as_str()
      ));
    }
    if next.rank() < self.rank() {
      return Err(format!(
        "message is {}; it cannot go back to {}",
        self.as_str(),
        next.as_str()
      ));
    }
    Ok(next)
  }
}

/// `--timeout`: seconds as `30`, `30s` or `1m`. Never longer than [`TIMEOUT`].
pub fn parse_timeout(value: &str) -> Result<Duration, String> {
  let value = value.trim();
  let (number, unit) = match value.strip_suffix('m') {
    Some(minutes) => (minutes, 60),
    None => (value.strip_suffix('s').unwrap_or(value), 1),
  };
  let seconds = number
    .trim()
    .parse::<u64>()
    .map_err(|_| format!("invalid timeout '{value}'. Use seconds like 30s, or 1m"))?
    * unit;
  if seconds == 0 {
    return Err("timeout must be at least 1s".into());
  }
  let timeout = Duration::from_secs(seconds);
  if timeout > TIMEOUT {
    return Err(format!(
      "timeout '{value}' is longer than the {}s limit",
      TIMEOUT.as_secs()
    ));
  }
  Ok(timeout)
}

/// Whether a message sent at `sent_at_ms` with `timeout` has run out at `now_ms`.
pub fn timed_out(sent_at_ms: u64, timeout: Duration, now_ms: u64) -> bool {
  now_ms.saturating_sub(sent_at_ms) >= timeout.as_millis() as u64
}

/// Whether a row sent at `sent_at_ms` is past [`RETENTION`] at `now_ms`.
pub fn expired(sent_at_ms: u64, now_ms: u64) -> bool {
  now_ms.saturating_sub(sent_at_ms) >= RETENTION.as_millis() as u64
}

/// A fresh message id: `m-` and eight hex digits, unique enough within the
/// hour a row lives.
pub fn new_id() -> String {
  format!("m-{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn states_move_forward_and_may_skip_a_step() {
    assert_eq!(State::Queued.advance(State::Delivered), Ok(State::Delivered));
    assert_eq!(State::Queued.advance(State::Read), Ok(State::Read));
    assert_eq!(State::Delivered.advance(State::Replied), Ok(State::Replied));
    assert_eq!(State::Read.advance(State::Acked), Ok(State::Acked));
    assert_eq!(State::Read.advance(State::TimedOut), Ok(State::TimedOut));
  }

  #[test]
  fn repeating_a_state_is_a_no_op() {
    assert_eq!(State::Delivered.advance(State::Delivered), Ok(State::Delivered));
    assert_eq!(State::Acked.advance(State::Acked), Ok(State::Acked));
  }

  #[test]
  fn states_never_go_back() {
    assert!(State::Read.advance(State::Delivered).is_err());
    assert!(State::Delivered.advance(State::Queued).is_err());
  }

  #[test]
  fn final_states_never_change() {
    for done in [State::Acked, State::Replied, State::TimedOut] {
      for next in [State::Read, State::Acked, State::Replied, State::TimedOut] {
        if next != done {
          assert!(done.advance(next).is_err(), "{done:?} -> {next:?}");
        }
      }
    }
  }

  #[test]
  fn states_and_kinds_round_trip_through_their_names() {
    for state in [
      State::Queued,
      State::Delivered,
      State::Read,
      State::Acked,
      State::Replied,
      State::TimedOut,
    ] {
      assert_eq!(State::parse(state.as_str()), Some(state));
    }
    for kind in [Kind::Message, Kind::Ask, Kind::Reply, Kind::Status] {
      assert_eq!(Kind::parse(kind.as_str()), Some(kind));
    }
    assert_eq!(State::parse("stale"), None);
  }

  #[test]
  fn parses_timeouts_up_to_one_minute() {
    assert_eq!(parse_timeout("30"), Ok(Duration::from_secs(30)));
    assert_eq!(parse_timeout("45s"), Ok(Duration::from_secs(45)));
    assert_eq!(parse_timeout("1m"), Ok(Duration::from_secs(60)));
    assert_eq!(parse_timeout(" 60s "), Ok(Duration::from_secs(60)));
  }

  #[test]
  fn rejects_timeouts_over_one_minute_zero_or_garbage() {
    assert!(parse_timeout("61s").is_err());
    assert!(parse_timeout("2m").is_err());
    assert!(parse_timeout("0").is_err());
    assert!(parse_timeout("soon").is_err());
  }

  #[test]
  fn a_message_times_out_after_its_timeout() {
    let timeout = Duration::from_secs(60);
    assert!(!timed_out(1_000, timeout, 60_999));
    assert!(timed_out(1_000, timeout, 61_000));
  }

  #[test]
  fn a_row_expires_after_an_hour() {
    assert!(!expired(0, 3_599_999));
    assert!(expired(0, 3_600_000));
  }

  #[test]
  fn ids_are_m_dash_eight_hex_digits() {
    let id = new_id();
    assert_eq!(id.len(), 10);
    assert!(id.starts_with("m-"));
    assert!(id[2..].chars().all(|c| c.is_ascii_hexdigit()));
  }
}
