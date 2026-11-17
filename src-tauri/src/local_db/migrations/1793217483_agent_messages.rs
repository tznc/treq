//! The `agent_messages` ledger that `treq agent msg` records into.

use rusqlite::Connection;

pub(super) fn apply(conn: &Connection) -> Result<(), String> {
  conn
    .execute_batch(super::super::agent_messages::CREATE_AGENT_MESSAGES)
    .map_err(|e| format!("Failed to create agent_messages table: {}", e))
}
