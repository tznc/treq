//! `sessions.link_url`: what a session works on, such as a reviewed PR.

use rusqlite::Connection;

pub(super) fn apply(conn: &Connection) -> Result<(), String> {
  conn
    .execute("ALTER TABLE sessions ADD COLUMN link_url TEXT", [])
    .map(|_| ())
    .map_err(|e| format!("Failed to add sessions link_url column: {}", e))
}
