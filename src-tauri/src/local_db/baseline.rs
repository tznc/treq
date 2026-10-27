//! Migration 2, the baseline: brings a database from any earlier shape to
//! the schema as it stood when versioned migrations were introduced. Every
//! step checks the file's actual tables and columns, so it is safe to run on
//! any of them. Do not change it; add a new migration in `migrations.rs`.

use super::{Connection, ResultExt};

pub(super) fn apply(conn: &Connection) -> Result<(), String> {
  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS workspaces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_name TEXT NOT NULL,
            workspace_path TEXT NOT NULL UNIQUE,
            branch_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            metadata TEXT,
            target_branch TEXT
        )",
      [],
    )
    .map_err(|e| format!("Failed to create workspaces table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_workspaces_branch ON workspaces(branch_name)",
      [],
    )
    .map_err(|e| format!("Failed to create workspaces branch index: {}", e))?;

  conn
    .execute("ALTER TABLE workspaces ADD COLUMN target_branch TEXT", [])
    .best_effort();
  conn
    .execute(
      "ALTER TABLE workspaces ADD COLUMN has_conflicts BOOLEAN DEFAULT 0",
      [],
    )
    .best_effort();
  conn
    .execute(
      "ALTER TABLE workspaces ADD COLUMN archived BOOLEAN DEFAULT 0",
      [],
    )
    .best_effort();
  conn
    .execute(
      "ALTER TABLE workspaces ADD COLUMN not_on_remote BOOLEAN DEFAULT 0",
      [],
    )
    .best_effort();
  conn
    .execute("ALTER TABLE workspaces ADD COLUMN title TEXT", [])
    .best_effort();
  conn
    .execute("ALTER TABLE workspaces ADD COLUMN description TEXT", [])
    .best_effort();
  conn
    .execute("ALTER TABLE workspaces ADD COLUMN moved_files TEXT", [])
    .best_effort();
  conn
    .execute("ALTER TABLE workspaces ADD COLUMN refreshed_at TEXT", [])
    .best_effort();
  conn
    .execute("ALTER TABLE workspaces ADD COLUMN sparse_patterns TEXT", [])
    .best_effort();
  conn
    .execute("ALTER TABLE workspaces ADD COLUMN pr_base_branch TEXT", [])
    .best_effort();

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            last_accessed TEXT NOT NULL,
            model TEXT,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        )",
      [],
    )
    .map_err(|e| format!("Failed to create sessions table: {}", e))?;

  // Before the rebuild below, which copies `model`: sessions tables that
  // predate it still have `worktree_id`.
  conn
    .execute("ALTER TABLE sessions ADD COLUMN model TEXT", [])
    .best_effort();

  let has_worktree_col: Result<i64, _> = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name='worktree_id'",
    [],
    |row| row.get(0),
  );

  if let Ok(count) = has_worktree_col {
    if count > 0 {
      // Table rebuilds run in a transaction so a failure cannot strand `*_new`.
      let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to start sessions migration: {}", e))?;
      tx.execute(
        "CREATE TABLE sessions_new (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workspace_id INTEGER,
                    name TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    last_accessed TEXT NOT NULL,
                    model TEXT,
                    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
                )",
        [],
      )
      .map_err(|e| format!("Failed to create sessions_new table: {}", e))?;

      tx.execute(
                "INSERT INTO sessions_new SELECT id, worktree_id, name, created_at, last_accessed, model FROM sessions",
                [],
            )
            .map_err(|e| format!("Failed to migrate sessions data: {}", e))?;

      tx.execute("DROP TABLE sessions", [])
        .map_err(|e| format!("Failed to drop old sessions table: {}", e))?;

      tx.execute("ALTER TABLE sessions_new RENAME TO sessions", [])
        .map_err(|e| format!("Failed to rename sessions_new to sessions: {}", e))?;
      tx.commit()
        .map_err(|e| format!("Failed to commit sessions migration: {}", e))?;
    }
  }

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_sessions_workspace ON sessions(workspace_id)",
      [],
    )
    .map_err(|e| format!("Failed to create sessions workspace index: {}", e))?;

  conn
    .execute("DROP TABLE IF EXISTS git_file_hunks", [])
    .best_effort();
  conn
    .execute("DROP TABLE IF EXISTS git_changed_files", [])
    .best_effort();
  conn
    .execute("DROP INDEX IF EXISTS idx_git_file_hunks_workspace", [])
    .best_effort();
  conn
    .execute("DROP INDEX IF EXISTS idx_git_changed_files_workspace", [])
    .best_effort();

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS changed_files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            file_path TEXT NOT NULL,
            workspace_status TEXT,
            is_untracked INTEGER NOT NULL DEFAULT 0,
            hunks_json TEXT,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
            UNIQUE(workspace_id, file_path)
        )",
      [],
    )
    .map_err(|e| format!("Failed to create changed_files table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_changed_files_workspace ON changed_files(workspace_id)",
      [],
    )
    .map_err(|e| format!("Failed to create changed_files workspace index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS workspace_files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            file_path TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            is_directory INTEGER NOT NULL DEFAULT 0,
            parent_path TEXT,
            cached_at TEXT NOT NULL,
            mtime INTEGER,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
            UNIQUE(workspace_id, file_path)
        )",
      [],
    )
    .map_err(|e| format!("Failed to create workspace_files table: {}", e))?;

  conn
    .execute("ALTER TABLE workspace_files ADD COLUMN mtime INTEGER", [])
    .best_effort();

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_workspace_files_workspace ON workspace_files(workspace_id)",
      [],
    )
    .map_err(|e| format!("Failed to create workspace_files workspace index: {}", e))?;

  conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_files_parent ON workspace_files(workspace_id, parent_path)",
        [],
    )
    .map_err(|e| format!("Failed to create workspace_files parent index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS pending_reviews (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER NOT NULL UNIQUE,
            comments TEXT NOT NULL,
            viewed_files TEXT,
            summary_text TEXT,
            conflict_comments TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        )",
      [],
    )
    .map_err(|e| format!("Failed to create pending_reviews table: {}", e))?;

  // Migration: add conflict_comments column to pending_reviews if missing
  // (covers DBs created before conflict comments were persisted).
  let has_conflict_comments_column: Result<i64, _> = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('pending_reviews') WHERE name = 'conflict_comments'",
    [],
    |row| row.get(0),
  );
  if matches!(has_conflict_comments_column, Ok(0)) {
    conn
      .execute(
        "ALTER TABLE pending_reviews ADD COLUMN conflict_comments TEXT",
        [],
      )
      .map_err(|e| format!("Failed to add conflict_comments column: {}", e))?;
  }

  // Drop any prior schema of commit_diff_stats — keyed cache, safe to recreate.
  let needs_rebuild: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('commit_diff_stats') WHERE name IN ('tree_key', 'parent_ids')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
  if needs_rebuild {
    conn
      .execute("DROP TABLE IF EXISTS commit_diff_stats", [])
      .best_effort();
  }

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS commit_diff_stats (
            commit_id TEXT NOT NULL PRIMARY KEY,
            insertions INTEGER NOT NULL,
            deletions INTEGER NOT NULL,
            updated_at TEXT NOT NULL,
            change_id TEXT,
            description TEXT,
            author_name TEXT,
            timestamp TEXT
        )",
      [],
    )
    .map_err(|e| format!("Failed to create commit_diff_stats table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_pending_reviews_workspace ON pending_reviews(workspace_id)",
      [],
    )
    .map_err(|e| format!("Failed to create pending_reviews workspace index: {}", e))?;

  // Separate pending-review session for the FileBrowser's "review any file" flow —
  // intentionally not shared with pending_reviews (the Review/Changes-tab session).
  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS file_browser_reviews (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER NOT NULL UNIQUE,
            comments TEXT NOT NULL,
            summary_text TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        )",
      [],
    )
    .map_err(|e| format!("Failed to create file_browser_reviews table: {}", e))?;

  conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_file_browser_reviews_workspace ON file_browser_reviews(workspace_id)",
        [],
    )
    .map_err(|e| format!("Failed to create file_browser_reviews workspace index: {}", e))?;

  // Local-only review comments left by a review agent. Deliberately never
  // synced to GitHub — `source` records where a row came from so other local
  // producers can be added later, and `target_type`/`target_id` keep the table
  // generic over what was reviewed (a workspace diff, a FileBrowser file, and
  // later other content types).
  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS agent_review_comments (
            id TEXT PRIMARY KEY,
            repo_path TEXT NOT NULL,
            target_type TEXT NOT NULL,
            target_id TEXT NOT NULL,
            file_path TEXT NOT NULL,
            hunk_id TEXT,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            side TEXT,
            comment_text TEXT NOT NULL,
            suggested_replacement TEXT,
            status TEXT NOT NULL,
            source TEXT NOT NULL,
            created_at TEXT NOT NULL,
            resolved_at TEXT
        )",
      [],
    )
    .map_err(|e| format!("Failed to create agent_review_comments table: {}", e))?;

  // Migration: add `source` to agent_review_comments if missing (covers DBs
  // created before other local comment producers were anticipated).
  let has_agent_review_source_column: Result<i64, _> = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('agent_review_comments') WHERE name = 'source'",
    [],
    |row| row.get(0),
  );
  if matches!(has_agent_review_source_column, Ok(0)) {
    conn
      .execute(
        "ALTER TABLE agent_review_comments ADD COLUMN source TEXT NOT NULL DEFAULT 'local-agent'",
        [],
      )
      .map_err(|e| format!("Failed to add agent_review_comments source column: {}", e))?;
  }

  // Migration: add `quoted_text` (the exact reviewed text a comment covers).
  // Linear targets apply a suggestion by finding this text in the live
  // Linear content, since their line numbers only hold for one snapshot.
  let has_agent_review_quoted_text_column: Result<i64, _> = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('agent_review_comments') WHERE name = 'quoted_text'",
    [],
    |row| row.get(0),
  );
  if matches!(has_agent_review_quoted_text_column, Ok(0)) {
    conn
      .execute(
        "ALTER TABLE agent_review_comments ADD COLUMN quoted_text TEXT",
        [],
      )
      .map_err(|e| {
        format!(
          "Failed to add agent_review_comments quoted_text column: {}",
          e
        )
      })?;
  }

  let has_agent_review_line_hash_column: Result<i64, _> = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('agent_review_comments') WHERE name = 'line_hash'",
    [],
    |row| row.get(0),
  );
  if matches!(has_agent_review_line_hash_column, Ok(0)) {
    conn
      .execute(
        "ALTER TABLE agent_review_comments ADD COLUMN line_hash TEXT",
        [],
      )
      .map_err(|e| format!("Failed to add agent_review_comments line_hash column: {e}"))?;
  }

  conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_review_comments_target ON agent_review_comments(repo_path, target_type, target_id)",
        [],
    )
    .map_err(|e| format!("Failed to create agent_review_comments target index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS instance_registry (
            instance_id TEXT NOT NULL PRIMARY KEY,
            pid INTEGER NOT NULL,
            started_at INTEGER NOT NULL,
            last_heartbeat_at INTEGER NOT NULL,
            endpoint TEXT NOT NULL,
            windows_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
      [],
    )
    .map_err(|e| format!("Failed to create instance_registry table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_instance_registry_last_heartbeat_at
         ON instance_registry(last_heartbeat_at)",
      [],
    )
    .map_err(|e| format!("Failed to create instance_registry heartbeat index: {}", e))?;

  // What happened in each code review, kept 30 days so an agent can tune
  // the review prompt from it: every review-agent comment with what the
  // user did about it, and every review the user sent to an agent.
  conn
    .execute_batch(
      "CREATE TABLE IF NOT EXISTS review_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            comment_id TEXT,
            source TEXT,
            workspace_id INTEGER,
            target_type TEXT NOT NULL,
            target_id TEXT,
            file_path TEXT,
            start_line INTEGER,
            end_line INTEGER,
            body TEXT NOT NULL,
            suggestion TEXT,
            quoted_text TEXT,
            mode TEXT,
            outcome TEXT NOT NULL,
            created_at TEXT NOT NULL,
            decided_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_review_history_created_at ON review_history(created_at);
        CREATE INDEX IF NOT EXISTS idx_review_history_comment ON review_history(comment_id);",
    )
    .map_err(|e| format!("Failed to create review_history table: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS prompt_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            session_id INTEGER,
            prompt_text TEXT NOT NULL,
            agent TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )",
      [],
    )
    .map_err(|e| format!("Failed to create prompt_history table: {}", e))?;

  let prompt_history_foreign_keys_without_cascade: i64 = conn
    .query_row(
      "SELECT COUNT(*) FROM pragma_foreign_key_list('prompt_history') WHERE on_delete != 'CASCADE'",
      [],
      |row| row.get(0),
    )
    .map_err(|e| format!("Failed to inspect prompt_history foreign keys: {}", e))?;
  if prompt_history_foreign_keys_without_cascade > 0 {
    conn
      .execute_batch(
        "BEGIN;
          CREATE TABLE prompt_history_new (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              workspace_id INTEGER,
              session_id INTEGER,
              prompt_text TEXT NOT NULL,
              agent TEXT,
              created_at TEXT NOT NULL,
              FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
              FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
          );
          INSERT INTO prompt_history_new
            (id, workspace_id, session_id, prompt_text, agent, created_at)
            SELECT id, workspace_id, session_id, prompt_text, agent, created_at
            FROM prompt_history;
          DROP TABLE prompt_history;
          ALTER TABLE prompt_history_new RENAME TO prompt_history;
          COMMIT;",
      )
      .map_err(|e| {
        conn.execute_batch("ROLLBACK").best_effort();
        format!("Failed to migrate prompt_history cascade deletes: {}", e)
      })?;
  }

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_prompt_history_workspace ON prompt_history(workspace_id)",
      [],
    )
    .map_err(|e| format!("Failed to create prompt_history workspace index: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_prompt_history_created_at ON prompt_history(created_at)",
      [],
    )
    .map_err(|e| format!("Failed to create prompt_history created_at index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS stashes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            workspace_label TEXT NOT NULL,
            commit_id TEXT NOT NULL,
            change_id TEXT NOT NULL,
            short_commit_id TEXT NOT NULL,
            bookmark_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            additions INTEGER NOT NULL DEFAULT 0,
            deletions INTEGER NOT NULL DEFAULT 0,
            files_changed TEXT NOT NULL DEFAULT '[]',
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
        )",
      [],
    )
    .map_err(|e| format!("Failed to create stashes table: {}", e))?;

  let stash_foreign_keys_without_set_null: i64 = conn
    .query_row(
      "SELECT COUNT(*) FROM pragma_foreign_key_list('stashes')
       WHERE \"from\" = 'workspace_id' AND on_delete != 'SET NULL'",
      [],
      |row| row.get(0),
    )
    .map_err(|e| format!("Failed to inspect stashes foreign keys: {}", e))?;
  if stash_foreign_keys_without_set_null > 0 {
    conn
      .execute_batch(
        "BEGIN;
         CREATE TABLE stashes_new (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           workspace_id INTEGER,
           workspace_label TEXT NOT NULL,
           commit_id TEXT NOT NULL,
           change_id TEXT NOT NULL,
           short_commit_id TEXT NOT NULL,
           bookmark_name TEXT NOT NULL,
           created_at TEXT NOT NULL,
           additions INTEGER NOT NULL DEFAULT 0,
           deletions INTEGER NOT NULL DEFAULT 0,
           files_changed TEXT NOT NULL DEFAULT '[]',
           FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
         );
         INSERT INTO stashes_new
           (id, workspace_id, workspace_label, commit_id, change_id, short_commit_id,
            bookmark_name, created_at, additions, deletions, files_changed)
           SELECT id, workspace_id, workspace_label, commit_id, change_id, short_commit_id,
                  bookmark_name, created_at, additions, deletions, files_changed
           FROM stashes;
         DROP TABLE stashes;
         ALTER TABLE stashes_new RENAME TO stashes;
         COMMIT;",
      )
      .map_err(|e| {
        conn.execute_batch("ROLLBACK").best_effort();
        format!("Failed to migrate stashes delete behavior: {}", e)
      })?;
  }

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_stashes_created_at ON stashes(created_at)",
      [],
    )
    .map_err(|e| format!("Failed to create stashes created_at index: {}", e))?;

  // Migration: replace the old per-job workflow_runs shape with per-invocation.
  let has_legacy_workflow_runs: i64 = conn
    .query_row(
      "SELECT COUNT(*) FROM pragma_table_info('workflow_runs') WHERE name = 'job_id'",
      [],
      |row| row.get(0),
    )
    .unwrap_or(0);
  if has_legacy_workflow_runs > 0 {
    conn
      .execute("DROP TABLE workflow_runs", [])
      .map_err(|e| format!("Failed to drop legacy workflow_runs table: {}", e))?;
  }

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS workflow_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            filename TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL,
            completed_at TEXT
        )",
      [],
    )
    .map_err(|e| format!("Failed to create workflow_runs table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_workflow_runs_lookup
         ON workflow_runs(workspace_id, filename, id DESC)",
      [],
    )
    .map_err(|e| format!("Failed to create workflow_runs index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS workflow_job_results (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id INTEGER NOT NULL,
            job_id TEXT NOT NULL,
            position INTEGER NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT,
            completed_at TEXT,
            steps_json TEXT NOT NULL,
            log_path TEXT
        )",
      [],
    )
    .map_err(|e| format!("Failed to create workflow_job_results table: {}", e))?;

  conn
    .execute(
      "CREATE INDEX IF NOT EXISTS idx_workflow_job_results_run
         ON workflow_job_results(run_id, position)",
      [],
    )
    .map_err(|e| format!("Failed to create workflow_job_results index: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS workspace_pr_sync (
            workspace_id INTEGER PRIMARY KEY,
            pr_number INTEGER NOT NULL,
            title TEXT,
            description TEXT,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        )",
      [],
    )
    .map_err(|e| format!("Failed to create workspace_pr_sync table: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS repo_trust (
            id INTEGER PRIMARY KEY,
            trusted_at TEXT NOT NULL
        )",
      [],
    )
    .map_err(|e| format!("Failed to create repo_trust table: {}", e))?;

  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS supporting_repos (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            repo_path TEXT NOT NULL UNIQUE,
            added_at TEXT NOT NULL
        )",
      [],
    )
    .map_err(|e| format!("Failed to create supporting_repos table: {}", e))?;

  // Migration: rename pending_reviews columns from old schema to new schema.
  let has_old_columns: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('pending_reviews') WHERE name IN ('comments_json', 'overall_comment', 'viewed_files_json')",
        [],
        |row| row.get(0),
    );

  if let Ok(count) = has_old_columns {
    if count > 0 {
      let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to start pending_reviews migration: {}", e))?;
      // Create new table with correct schema
      tx.execute(
        "CREATE TABLE pending_reviews_new (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workspace_id INTEGER NOT NULL UNIQUE,
                    comments TEXT NOT NULL,
                    viewed_files TEXT,
                    summary_text TEXT,
                    conflict_comments TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
                )",
        [],
      )
      .map_err(|e| format!("Failed to create pending_reviews_new table: {}", e))?;

      // Migrate old pending_reviews column names to new names.
      tx.execute(
                "INSERT INTO pending_reviews_new (id, workspace_id, comments, viewed_files, summary_text, created_at, updated_at)
                 SELECT id, workspace_id, comments_json, viewed_files_json, overall_comment, created_at, updated_at
                 FROM pending_reviews",
                [],
            )
            .map_err(|e| format!("Failed to migrate pending_reviews data: {}", e))?;

      // Drop old table
      tx.execute("DROP TABLE pending_reviews", [])
        .map_err(|e| format!("Failed to drop old pending_reviews table: {}", e))?;

      // Rename new table to original name
      tx.execute(
        "ALTER TABLE pending_reviews_new RENAME TO pending_reviews",
        [],
      )
      .map_err(|e| {
        format!(
          "Failed to rename pending_reviews_new to pending_reviews: {}",
          e
        )
      })?;

      // Recreate the index (was dropped with old table)
      tx.execute(
        "CREATE INDEX IF NOT EXISTS idx_pending_reviews_workspace ON pending_reviews(workspace_id)",
        [],
      )
      .map_err(|e| format!("Failed to recreate pending_reviews workspace index: {}", e))?;
      tx.commit()
        .map_err(|e| format!("Failed to commit pending_reviews migration: {}", e))?;
    }
  }

  Ok(())
}
