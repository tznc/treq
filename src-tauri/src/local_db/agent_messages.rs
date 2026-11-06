//! The `agent_messages` ledger: one row per target of each `treq agent msg`
//! message, kept by the instance that sent it. Rows are deleted an hour
//! after sending ([`model::RETENTION`]); every state change is also written
//! to the system log, which keeps the history after that.

use super::get_connection;
use crate::core::agent_msg::{log_line, model, model::Kind, model::State, LOG_TARGET};
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub(super) const CREATE_AGENT_MESSAGES: &str = "CREATE TABLE IF NOT EXISTS agent_messages (
      id TEXT NOT NULL,
      target TEXT NOT NULL,
      kind TEXT NOT NULL,
      from_handle TEXT NOT NULL,
      body TEXT NOT NULL,
      reply_to TEXT,
      via TEXT NOT NULL,
      state TEXT NOT NULL,
      mode TEXT,
      sent_at INTEGER NOT NULL,
      timeout_ms INTEGER NOT NULL,
      updated_at INTEGER NOT NULL,
      PRIMARY KEY (id, target)
  );
  CREATE INDEX IF NOT EXISTS idx_agent_messages_sent_at ON agent_messages(sent_at);";

/// A message as its sender records it.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAgentMessage {
  pub id: String,
  pub kind: Kind,
  pub from: String,
  pub body: String,
  pub reply_to: Option<String>,
  pub sent_at_ms: u64,
  pub timeout: Duration,
}

/// One recipient: its handle and how the sender reaches it (`local`,
/// `socket:<endpoint>` or `ssh:<host>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMessageTarget {
  pub handle: String,
  pub via: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMessageRow {
  pub id: String,
  pub target: String,
  pub kind: Kind,
  pub from_handle: String,
  pub body: String,
  pub reply_to: Option<String>,
  pub via: String,
  pub state: State,
  /// How it was delivered: `wake` or `badge`.
  pub mode: Option<String>,
  pub sent_at_ms: u64,
  pub timeout_ms: u64,
  pub updated_at_ms: u64,
}

/// What a sweep changed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AgentMessageSweep {
  /// `(id, target)` of each row that ran out of time.
  pub timed_out: Vec<(String, String)>,
  pub deleted: usize,
}

const COLUMNS: &str =
  "id, target, kind, from_handle, body, reply_to, via, state, mode, sent_at, timeout_ms, updated_at";

fn row_from_sql(row: &Row<'_>) -> rusqlite::Result<AgentMessageRow> {
  let kind: String = row.get(2)?;
  let state: String = row.get(7)?;
  let invalid = |column: usize, value: String| {
    rusqlite::Error::FromSqlConversionFailure(
      column,
      rusqlite::types::Type::Text,
      format!("unknown value '{value}'").into(),
    )
  };
  Ok(AgentMessageRow {
    id: row.get(0)?,
    target: row.get(1)?,
    kind: Kind::parse(&kind).ok_or_else(|| invalid(2, kind.clone()))?,
    from_handle: row.get(3)?,
    body: row.get(4)?,
    reply_to: row.get(5)?,
    via: row.get(6)?,
    state: State::parse(&state).ok_or_else(|| invalid(7, state.clone()))?,
    mode: row.get(8)?,
    sent_at_ms: row.get::<_, i64>(9)? as u64,
    timeout_ms: row.get::<_, i64>(10)? as u64,
    updated_at_ms: row.get::<_, i64>(11)? as u64,
  })
}

pub fn insert_agent_message(
  repo_path: &str,
  message: &NewAgentMessage,
  targets: &[AgentMessageTarget],
) -> Result<(), String> {
  let mut conn = get_connection(repo_path)?;
  let tx = conn
    .transaction()
    .map_err(|e| format!("Failed to start agent message insert: {}", e))?;
  for target in targets {
    tx.execute(
      "INSERT INTO agent_messages
         (id, target, kind, from_handle, body, reply_to, via, state, mode, sent_at, timeout_ms, updated_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9, ?10, ?9)",
      params![
        message.id,
        target.handle,
        message.kind.as_str(),
        message.from,
        message.body,
        message.reply_to,
        target.via,
        State::Queued.as_str(),
        message.sent_at_ms as i64,
        message.timeout.as_millis() as i64,
      ],
    )
    .map_err(|e| format!("Failed to record agent message: {}", e))?;
  }
  tx.commit()
    .map_err(|e| format!("Failed to commit agent message: {}", e))?;
  for target in targets {
    log::info!(
      target: LOG_TARGET,
      "{}",
      log_line(
        "queued",
        &message.id,
        &[
          ("kind", message.kind.as_str()),
          ("from", &message.from),
          ("to", &target.handle),
          ("via", &target.via),
        ],
      )
    );
  }
  Ok(())
}

/// Moves one target's row to `state`, which must follow its current one
/// ([`State::advance`]). Returns the state it is in afterwards.
pub fn update_agent_message_state(
  repo_path: &str,
  id: &str,
  target: &str,
  state: State,
  mode: Option<&str>,
  now_ms: u64,
) -> Result<State, String> {
  let conn = get_connection(repo_path)?;
  let current: Option<String> = conn
    .query_row(
      "SELECT state FROM agent_messages WHERE id = ?1 AND target = ?2",
      params![id, target],
      |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("Failed to read agent message: {}", e))?;
  let current = current
    .ok_or_else(|| format!("no message {id} for {target}"))
    .and_then(|state| State::parse(&state).ok_or_else(|| format!("unknown state '{state}'")))?;
  let next = current.advance(state)?;
  if next == current {
    return Ok(next);
  }
  conn
    .execute(
      "UPDATE agent_messages SET state = ?3, mode = COALESCE(?4, mode), updated_at = ?5
       WHERE id = ?1 AND target = ?2",
      params![id, target, next.as_str(), mode, now_ms as i64],
    )
    .map_err(|e| format!("Failed to update agent message: {}", e))?;
  let mut fields = vec![("to", target)];
  if let Some(mode) = mode {
    fields.push(("mode", mode));
  }
  log::info!(target: LOG_TARGET, "{}", log_line(next.as_str(), id, &fields));
  Ok(next)
}

/// Every target's row of message `id`.
pub fn agent_message_rows(repo_path: &str, id: &str) -> Result<Vec<AgentMessageRow>, String> {
  let conn = get_connection(repo_path)?;
  let mut statement = conn
    .prepare(&format!(
      "SELECT {COLUMNS} FROM agent_messages WHERE id = ?1 ORDER BY target"
    ))
    .map_err(|e| format!("Failed to read agent messages: {}", e))?;
  let rows = statement
    .query_map([id], row_from_sql)
    .map_err(|e| format!("Failed to read agent messages: {}", e))?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| format!("Failed to read agent messages: {}", e))
}

/// The newest rows first, for the message log.
pub fn list_agent_messages(repo_path: &str, limit: usize) -> Result<Vec<AgentMessageRow>, String> {
  let conn = get_connection(repo_path)?;
  let mut statement = conn
    .prepare(&format!(
      "SELECT {COLUMNS} FROM agent_messages ORDER BY sent_at DESC, id DESC, target LIMIT ?1"
    ))
    .map_err(|e| format!("Failed to list agent messages: {}", e))?;
  let rows = statement
    .query_map([limit as i64], row_from_sql)
    .map_err(|e| format!("Failed to list agent messages: {}", e))?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| format!("Failed to list agent messages: {}", e))
}

/// Times out the open rows past their timeout and deletes the rows past
/// [`model::RETENTION`].
pub fn sweep_agent_messages(repo_path: &str, now_ms: u64) -> Result<AgentMessageSweep, String> {
  let mut conn = get_connection(repo_path)?;
  let tx = conn
    .transaction()
    .map_err(|e| format!("Failed to start agent message sweep: {}", e))?;
  let mut sweep = AgentMessageSweep::default();
  {
    let mut statement = tx
      .prepare(
        "SELECT id, target FROM agent_messages
         WHERE state NOT IN ('acked', 'replied', 'timed_out') AND sent_at + timeout_ms <= ?1
         ORDER BY sent_at, id, target",
      )
      .map_err(|e| format!("Failed to find timed out agent messages: {}", e))?;
    let rows = statement
      .query_map([now_ms as i64], |row| Ok((row.get(0)?, row.get(1)?)))
      .map_err(|e| format!("Failed to find timed out agent messages: {}", e))?;
    sweep.timed_out = rows
      .collect::<rusqlite::Result<Vec<(String, String)>>>()
      .map_err(|e| format!("Failed to find timed out agent messages: {}", e))?;
  }
  tx.execute(
    "UPDATE agent_messages SET state = 'timed_out', updated_at = ?1
     WHERE state NOT IN ('acked', 'replied', 'timed_out') AND sent_at + timeout_ms <= ?1",
    [now_ms as i64],
  )
  .map_err(|e| format!("Failed to time out agent messages: {}", e))?;
  if let Some(cutoff) = now_ms.checked_sub(model::RETENTION.as_millis() as u64) {
    sweep.deleted = tx
      .execute(
        "DELETE FROM agent_messages WHERE sent_at <= ?1",
        [cutoff as i64],
      )
      .map_err(|e| format!("Failed to delete old agent messages: {}", e))?;
  }
  tx.commit()
    .map_err(|e| format!("Failed to commit agent message sweep: {}", e))?;
  for (id, target) in &sweep.timed_out {
    log::info!(target: LOG_TARGET, "{}", log_line("timed_out", id, &[("to", target)]));
  }
  Ok(sweep)
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn message(id: &str, sent_at_ms: u64) -> NewAgentMessage {
    NewAgentMessage {
      id: id.into(),
      kind: Kind::Ask,
      from: "codex-1".into(),
      body: "rebase done?".into(),
      reply_to: None,
      sent_at_ms,
      timeout: Duration::from_secs(60),
    }
  }

  fn target(handle: &str, via: &str) -> AgentMessageTarget {
    AgentMessageTarget {
      handle: handle.into(),
      via: via.into(),
    }
  }

  fn states(repo: &str, id: &str) -> Vec<(String, State)> {
    agent_message_rows(repo, id)
      .unwrap()
      .into_iter()
      .map(|row| (row.target, row.state))
      .collect()
  }

  #[test]
  fn records_one_queued_row_per_target() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    insert_agent_message(
      repo,
      &message("m-1", 1_000),
      &[
        target("claude-2", "local"),
        target("cursor-4", "ssh:build-box"),
      ],
    )
    .unwrap();

    let rows = agent_message_rows(repo, "m-1").unwrap();
    assert_eq!(rows.len(), 2);
    let cursor = rows.iter().find(|r| r.target == "cursor-4").unwrap();
    assert_eq!(cursor.state, State::Queued);
    assert_eq!(cursor.via, "ssh:build-box");
    assert_eq!(cursor.kind, Kind::Ask);
    assert_eq!(cursor.from_handle, "codex-1");
    assert_eq!(cursor.body, "rebase done?");
    assert_eq!((cursor.sent_at_ms, cursor.timeout_ms), (1_000, 60_000));
  }

  #[test]
  fn moves_each_target_on_its_own() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    insert_agent_message(
      repo,
      &message("m-1", 0),
      &[target("claude-2", "local"), target("copilot-3", "local")],
    )
    .unwrap();

    update_agent_message_state(repo, "m-1", "claude-2", State::Delivered, Some("wake"), 5).unwrap();
    update_agent_message_state(repo, "m-1", "claude-2", State::Replied, None, 9).unwrap();

    let rows = agent_message_rows(repo, "m-1").unwrap();
    let claude = rows.iter().find(|r| r.target == "claude-2").unwrap();
    assert_eq!(claude.state, State::Replied);
    assert_eq!(
      claude.mode.as_deref(),
      Some("wake"),
      "a later change keeps the mode"
    );
    assert_eq!(claude.updated_at_ms, 9);
    let copilot = rows.iter().find(|r| r.target == "copilot-3").unwrap();
    assert_eq!(copilot.state, State::Queued);
  }

  #[test]
  fn refuses_a_change_that_does_not_follow() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    insert_agent_message(repo, &message("m-1", 0), &[target("claude-2", "local")]).unwrap();
    update_agent_message_state(repo, "m-1", "claude-2", State::Acked, None, 1).unwrap();

    assert!(update_agent_message_state(repo, "m-1", "claude-2", State::Read, None, 2).is_err());
    assert_eq!(states(repo, "m-1"), vec![("claude-2".into(), State::Acked)]);
  }

  #[test]
  fn an_unknown_message_or_target_is_an_error() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    insert_agent_message(repo, &message("m-1", 0), &[target("claude-2", "local")]).unwrap();

    assert!(update_agent_message_state(repo, "m-9", "claude-2", State::Read, None, 1).is_err());
    assert!(update_agent_message_state(repo, "m-1", "codex-1", State::Read, None, 1).is_err());
  }

  #[test]
  fn sweep_times_out_open_rows_after_their_timeout() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    insert_agent_message(
      repo,
      &message("m-1", 0),
      &[target("claude-2", "local"), target("copilot-3", "local")],
    )
    .unwrap();
    update_agent_message_state(repo, "m-1", "claude-2", State::Acked, None, 10).unwrap();

    assert_eq!(
      sweep_agent_messages(repo, 59_999).unwrap(),
      AgentMessageSweep::default()
    );
    let sweep = sweep_agent_messages(repo, 60_000).unwrap();
    assert_eq!(sweep.timed_out, vec![("m-1".into(), "copilot-3".into())]);
    assert_eq!(sweep.deleted, 0);
    let rows = states(repo, "m-1");
    assert!(
      rows.contains(&("claude-2".into(), State::Acked)),
      "acked rows stay acked"
    );
    assert!(rows.contains(&("copilot-3".into(), State::TimedOut)));
  }

  #[test]
  fn sweep_deletes_rows_an_hour_after_sending_whatever_their_state() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    insert_agent_message(repo, &message("old", 0), &[target("claude-2", "local")]).unwrap();
    update_agent_message_state(repo, "old", "claude-2", State::Replied, None, 1).unwrap();
    insert_agent_message(
      repo,
      &message("new", 3_000_000),
      &[target("claude-2", "local")],
    )
    .unwrap();

    let sweep = sweep_agent_messages(repo, 3_600_000).unwrap();
    assert_eq!(sweep.deleted, 1);
    assert!(agent_message_rows(repo, "old").unwrap().is_empty());
    assert_eq!(agent_message_rows(repo, "new").unwrap().len(), 1);
  }

  #[test]
  fn lists_the_newest_rows_first() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    insert_agent_message(repo, &message("m-1", 1), &[target("claude-2", "local")]).unwrap();
    insert_agent_message(repo, &message("m-2", 2), &[target("claude-2", "local")]).unwrap();
    insert_agent_message(repo, &message("m-3", 3), &[target("claude-2", "local")]).unwrap();

    let ids: Vec<String> = list_agent_messages(repo, 2)
      .unwrap()
      .into_iter()
      .map(|row| row.id)
      .collect();
    assert_eq!(ids, vec!["m-3".to_string(), "m-2".to_string()]);
  }
}
