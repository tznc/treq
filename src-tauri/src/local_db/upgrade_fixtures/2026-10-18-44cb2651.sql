-- Fresh-install schema as of 44cb2651 (2026-10-18), replayed from that commit's local_db.rs.
CREATE TABLE agent_review_comments (
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
        , quoted_text TEXT, line_hash TEXT);
CREATE TABLE changed_files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            file_path TEXT NOT NULL,
            workspace_status TEXT,
            is_untracked INTEGER NOT NULL DEFAULT 0,
            hunks_json TEXT,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
            UNIQUE(workspace_id, file_path)
        );
CREATE TABLE commit_diff_stats (
            commit_id TEXT NOT NULL PRIMARY KEY,
            insertions INTEGER NOT NULL,
            deletions INTEGER NOT NULL,
            updated_at TEXT NOT NULL,
            change_id TEXT,
            description TEXT,
            author_name TEXT,
            timestamp TEXT
        );
CREATE TABLE file_browser_reviews (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER NOT NULL UNIQUE,
            comments TEXT NOT NULL,
            summary_text TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        );
CREATE TABLE instance_registry (
            instance_id TEXT NOT NULL PRIMARY KEY,
            pid INTEGER NOT NULL,
            started_at INTEGER NOT NULL,
            last_heartbeat_at INTEGER NOT NULL,
            endpoint TEXT NOT NULL,
            windows_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
CREATE TABLE "pending_reviews" (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workspace_id INTEGER NOT NULL UNIQUE,
                    comments TEXT NOT NULL,
                    viewed_files TEXT,
                    summary_text TEXT,
                    conflict_comments TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
                );
CREATE TABLE "prompt_history" (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              workspace_id INTEGER,
              session_id INTEGER,
              prompt_text TEXT NOT NULL,
              agent TEXT,
              created_at TEXT NOT NULL,
              FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
              FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
          );
CREATE TABLE repo_trust (
            id INTEGER PRIMARY KEY,
            trusted_at TEXT NOT NULL
        );
CREATE TABLE "sessions" (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workspace_id INTEGER,
                    name TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    last_accessed TEXT NOT NULL,
                    model TEXT,
                    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
                );
CREATE TABLE "stashes" (
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
CREATE TABLE supporting_repos (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            repo_path TEXT NOT NULL UNIQUE,
            added_at TEXT NOT NULL
        );
CREATE TABLE workflow_job_results (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id INTEGER NOT NULL,
            job_id TEXT NOT NULL,
            position INTEGER NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT,
            completed_at TEXT,
            steps_json TEXT NOT NULL,
            log_path TEXT
        );
CREATE TABLE workflow_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER,
            filename TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL,
            completed_at TEXT
        );
CREATE TABLE workspace_files (
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
        );
CREATE TABLE workspaces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_name TEXT NOT NULL,
            workspace_path TEXT NOT NULL UNIQUE,
            branch_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            metadata TEXT,
            target_branch TEXT
        , has_conflicts BOOLEAN DEFAULT 0, archived BOOLEAN DEFAULT 0, not_on_remote BOOLEAN DEFAULT 0, title TEXT, description TEXT, moved_files TEXT, refreshed_at TEXT, sparse_patterns TEXT);
CREATE INDEX idx_agent_review_comments_target ON agent_review_comments(repo_path, target_type, target_id);
CREATE INDEX idx_changed_files_workspace ON changed_files(workspace_id);
CREATE INDEX idx_file_browser_reviews_workspace ON file_browser_reviews(workspace_id);
CREATE INDEX idx_instance_registry_last_heartbeat_at
         ON instance_registry(last_heartbeat_at);
CREATE INDEX idx_pending_reviews_workspace ON pending_reviews(workspace_id);
CREATE INDEX idx_prompt_history_created_at ON prompt_history(created_at);
CREATE INDEX idx_prompt_history_workspace ON prompt_history(workspace_id);
CREATE INDEX idx_sessions_workspace ON sessions(workspace_id);
CREATE INDEX idx_stashes_created_at ON stashes(created_at);
CREATE INDEX idx_workflow_job_results_run
         ON workflow_job_results(run_id, position);
CREATE INDEX idx_workflow_runs_lookup
         ON workflow_runs(workspace_id, filename, id DESC);
CREATE INDEX idx_workspace_files_parent ON workspace_files(workspace_id, parent_path);
CREATE INDEX idx_workspace_files_workspace ON workspace_files(workspace_id);
CREATE INDEX idx_workspaces_branch ON workspaces(branch_name);
