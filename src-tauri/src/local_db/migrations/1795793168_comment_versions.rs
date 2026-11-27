//! `agent_review_comments.version` and `.outdated`: a comment follows its
//! file's versions as GitHub carries a thread from commit to commit.

use rusqlite::Connection;

pub(super) fn apply(conn: &Connection) -> Result<(), String> {
  conn
    .execute_batch(
      "ALTER TABLE agent_review_comments ADD COLUMN version TEXT;
       ALTER TABLE agent_review_comments ADD COLUMN outdated INTEGER NOT NULL DEFAULT 0;",
    )
    .map_err(|e| format!("Failed to add agent_review_comments version columns: {}", e))
}
