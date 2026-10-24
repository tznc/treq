use chrono::Utc;
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

// Import Session type from local_db for internal use
use crate::local_db::Session;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileView {
  pub id: i64,
  pub workspace_path: String,
  pub file_path: String,
  pub viewed_at: String,
  pub content_hash: String,
}

pub struct Database {
  conn: Connection,
}

impl Database {
  pub fn new(db_path: PathBuf) -> Result<Self> {
    let conn = Connection::open(&db_path)?;
    // Several handles (and treq instances) share this file. As in
    // `local_db::get_connection`, WAL lets reads run beside a write and
    // busy_timeout retries a write conflict instead of failing at once with
    // "database is locked".
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // Crash-safe under WAL; avoids an fsync per commit.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(Database { conn })
  }

  pub fn init(&self) -> Result<()> {
    self.conn.execute(
      "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
      [],
    )?;

    self.conn.execute(
      "CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_id INTEGER,
                type TEXT NOT NULL,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                last_accessed TEXT NOT NULL,
                model TEXT,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            )",
      [],
    )?;

    // Migration: Add model column if it doesn't exist
    let _ = self
      .conn
      .execute("ALTER TABLE sessions ADD COLUMN model TEXT", []);

    let _ = self.conn.execute(
      "DELETE FROM sessions WHERE type IS NULL OR type <> 'session'",
      [],
    );

    self.conn.execute(
      "CREATE TABLE IF NOT EXISTS git_cache (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_path TEXT NOT NULL,
                file_path TEXT,
                cache_type TEXT NOT NULL,
                data TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(workspace_path, file_path, cache_type)
            )",
      [],
    )?;

    // Migration: rename worktree_path to workspace_path when old column exists.
    let has_worktree_col: Result<i64, _> = self.conn.query_row(
      "SELECT COUNT(*) FROM pragma_table_info('git_cache') WHERE name='worktree_path'",
      [],
      |row| row.get(0),
    );

    if let Ok(count) = has_worktree_col {
      if count > 0 {
        // Old schema exists, need to migrate
        self.conn.execute(
          "CREATE TABLE git_cache_new (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        workspace_path TEXT NOT NULL,
                        file_path TEXT,
                        cache_type TEXT NOT NULL,
                        data TEXT NOT NULL,
                        updated_at TEXT NOT NULL,
                        UNIQUE(workspace_path, file_path, cache_type)
                    )",
          [],
        )?;

        self.conn.execute(
                    "INSERT INTO git_cache_new (id, workspace_path, file_path, cache_type, data, updated_at)
                     SELECT id, worktree_path, file_path, cache_type, data, updated_at FROM git_cache",
                    [],
                )?;

        self.conn.execute("DROP TABLE git_cache", [])?;
        self
          .conn
          .execute("ALTER TABLE git_cache_new RENAME TO git_cache", [])?;
      }
    }

    self.conn.execute(
      "CREATE INDEX IF NOT EXISTS idx_git_cache_workspace ON git_cache(workspace_path)",
      [],
    )?;

    self.conn.execute(
      "CREATE TABLE IF NOT EXISTS file_views (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_path TEXT NOT NULL,
                file_path TEXT NOT NULL,
                viewed_at TEXT NOT NULL,
                content_hash TEXT NOT NULL DEFAULT '',
                UNIQUE(workspace_path, file_path)
            )",
      [],
    )?;

    // Migration: Rename worktree_path to workspace_path in file_views if needed
    let has_worktree_col_fv: Result<i64, _> = self.conn.query_row(
      "SELECT COUNT(*) FROM pragma_table_info('file_views') WHERE name='worktree_path'",
      [],
      |row| row.get(0),
    );

    if let Ok(count) = has_worktree_col_fv {
      if count > 0 {
        // Old schema exists, need to migrate
        self.conn.execute(
          "CREATE TABLE file_views_new (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        workspace_path TEXT NOT NULL,
                        file_path TEXT NOT NULL,
                        viewed_at TEXT NOT NULL,
                        content_hash TEXT NOT NULL DEFAULT '',
                        UNIQUE(workspace_path, file_path)
                    )",
          [],
        )?;

        // Check if content_hash exists in old table
        let has_content_hash: Result<i64, _> = self.conn.query_row(
          "SELECT COUNT(*) FROM pragma_table_info('file_views') WHERE name='content_hash'",
          [],
          |row| row.get(0),
        );

        if let Ok(1) = has_content_hash {
          self.conn.execute(
                        "INSERT INTO file_views_new (id, workspace_path, file_path, viewed_at, content_hash)
                         SELECT id, worktree_path, file_path, viewed_at, content_hash FROM file_views",
                        [],
                    )?;
        } else {
          self.conn.execute(
            "INSERT INTO file_views_new (id, workspace_path, file_path, viewed_at, content_hash)
                         SELECT id, worktree_path, file_path, viewed_at, '' FROM file_views",
            [],
          )?;
        }

        self.conn.execute("DROP TABLE file_views", [])?;
        self
          .conn
          .execute("ALTER TABLE file_views_new RENAME TO file_views", [])?;
      }
    }

    // Migration: Add content_hash column if it doesn't exist
    let _ = self.conn.execute(
      "ALTER TABLE file_views ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
      [],
    );

    self.conn.execute(
      "CREATE INDEX IF NOT EXISTS idx_file_views_workspace ON file_views(workspace_path)",
      [],
    )?;

    // Migration: Rename repo_path setting to last_opened_repo_path
    if let Ok(Some(repo_path_val)) = self.get_setting("repo_path") {
      let _ = self.set_setting("last_opened_repo_path", &repo_path_val);
      let _ = self
        .conn
        .execute("DELETE FROM settings WHERE key = 'repo_path'", []);
    }

    Ok(())
  }

  pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
    let mut stmt = self
      .conn
      .prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query([key])?;

    if let Some(row) = rows.next()? {
      Ok(Some(row.get(0)?))
    } else {
      Ok(None)
    }
  }

  pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
    self.conn.execute(
      "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
      [key, value],
    )?;
    Ok(())
  }

  pub fn get_settings_batch(
    &self,
    keys: &[String],
  ) -> Result<std::collections::HashMap<String, Option<String>>> {
    use std::collections::HashMap;

    let mut result = HashMap::new();

    if keys.is_empty() {
      return Ok(result);
    }

    // Build placeholders for IN clause
    let placeholders = keys.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let query = format!(
      "SELECT key, value FROM settings WHERE key IN ({})",
      placeholders
    );

    let mut stmt = self.conn.prepare(&query)?;
    let params: Vec<&dyn rusqlite::ToSql> =
      keys.iter().map(|k| k as &dyn rusqlite::ToSql).collect();
    let mut rows = stmt.query(&params[..])?;

    // First, initialize all keys with None
    for key in keys {
      result.insert(key.clone(), None);
    }

    // Then populate with actual values
    while let Some(row) = rows.next()? {
      let key: String = row.get(0)?;
      let value: String = row.get(1)?;
      result.insert(key, Some(value));
    }

    Ok(result)
  }

  // Helper function to create composite key for repo-specific settings
  fn make_repo_key(repo_path: &str, key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo_path.as_bytes());
    let hash = hasher.finalize();
    // First 16 hex chars of the hash (8 bytes)
    let hash_hex: String = hash
      .iter()
      .take(8)
      .map(|byte| format!("{:02x}", byte))
      .collect();
    format!("repo_{}_{}", hash_hex, key)
  }

  pub fn get_repo_setting(&self, repo_path: &str, key: &str) -> Result<Option<String>> {
    let composite_key = Self::make_repo_key(repo_path, key);
    self.get_setting(&composite_key)
  }

  pub fn set_repo_setting(&self, repo_path: &str, key: &str, value: &str) -> Result<()> {
    let composite_key = Self::make_repo_key(repo_path, key);
    self.set_setting(&composite_key, value)
  }

  pub fn add_session(&self, session: &Session) -> Result<i64> {
    self.conn.execute(
      "INSERT INTO sessions (workspace_id, type, name, created_at, last_accessed, model)
             VALUES (?1, 'session', ?2, ?3, ?4, ?5)",
      (
        &session.workspace_id,
        &session.name,
        &session.created_at,
        &session.last_accessed,
        &session.model,
      ),
    )?;
    Ok(self.conn.last_insert_rowid())
  }

  pub fn get_sessions(&self) -> Result<Vec<Session>> {
    let mut stmt = self.conn.prepare(
      "SELECT id, workspace_id, name, created_at, last_accessed, model
             FROM sessions ORDER BY created_at ASC",
    )?;

    let sessions = stmt.query_map([], |row| {
      Ok(Session {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        name: row.get(2)?,
        created_at: row.get(3)?,
        last_accessed: row.get(4)?,
        model: row.get(5)?,
      })
    })?;

    sessions.collect()
  }

  pub fn update_session_access(&self, id: i64, last_accessed: &str) -> Result<()> {
    self.conn.execute(
      "UPDATE sessions SET last_accessed = ?1 WHERE id = ?2",
      params![last_accessed, id],
    )?;
    Ok(())
  }

  pub fn update_session_name(&self, id: i64, name: &str) -> Result<()> {
    self.conn.execute(
      "UPDATE sessions SET name = ?1 WHERE id = ?2",
      params![name, id],
    )?;
    Ok(())
  }

  pub fn delete_session(&self, id: i64) -> Result<()> {
    self
      .conn
      .execute("DELETE FROM sessions WHERE id = ?1", [id])?;
    Ok(())
  }

  // File view tracking methods
  pub fn mark_file_viewed(
    &self,
    workspace_path: &str,
    file_path: &str,
    content_hash: &str,
  ) -> Result<()> {
    let viewed_at = Utc::now().to_rfc3339();
    self.conn.execute(
      "INSERT INTO file_views (workspace_path, file_path, viewed_at, content_hash)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(workspace_path, file_path)
             DO UPDATE SET viewed_at = excluded.viewed_at, content_hash = excluded.content_hash",
      params![workspace_path, file_path, viewed_at, content_hash],
    )?;
    Ok(())
  }

  pub fn unmark_file_viewed(&self, workspace_path: &str, file_path: &str) -> Result<()> {
    self.conn.execute(
      "DELETE FROM file_views WHERE workspace_path = ?1 AND file_path = ?2",
      params![workspace_path, file_path],
    )?;
    Ok(())
  }

  pub fn get_viewed_files(&self, workspace_path: &str) -> Result<Vec<FileView>> {
    let mut stmt = self.conn.prepare(
      "SELECT id, workspace_path, file_path, viewed_at, content_hash
             FROM file_views
             WHERE workspace_path = ?1
             ORDER BY viewed_at DESC",
    )?;

    let views = stmt.query_map([workspace_path], |row| {
      Ok(FileView {
        id: row.get(0)?,
        workspace_path: row.get(1)?,
        file_path: row.get(2)?,
        viewed_at: row.get(3)?,
        content_hash: row.get(4)?,
      })
    })?;

    views.collect()
  }

  pub fn clear_all_viewed_files(&self, workspace_path: &str) -> Result<()> {
    self.conn.execute(
      "DELETE FROM file_views WHERE workspace_path = ?1",
      [workspace_path],
    )?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_write_waits_for_another_handles_transaction() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let path = temp.path().join("treq.db");
    let first = Database::new(path.clone()).expect("open first");
    first.init().expect("init");
    let second = Database::new(path).expect("open second");

    first.conn.execute_batch("BEGIN IMMEDIATE").expect("begin");
    let release = std::thread::spawn(move || {
      std::thread::sleep(std::time::Duration::from_millis(200));
      first.conn.execute_batch("COMMIT").expect("commit");
    });

    second
      .set_setting("theme", "dark")
      .expect("write retries while the other handle holds the lock");
    release.join().expect("release thread");
    assert_eq!(
      second.get_setting("theme").expect("read").as_deref(),
      Some("dark")
    );
  }
}
