-- Fresh-install schema as of e9c93152 (2025-12-01), replayed from that commit's local_db.rs.
CREATE TABLE plans (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            worktree_id INTEGER NOT NULL,
            title TEXT NOT NULL,
            type TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL,
            executed_at TEXT NOT NULL,
            status TEXT NOT NULL
        );
CREATE TABLE sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            worktree_id INTEGER,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            last_accessed TEXT NOT NULL,
            plan_title TEXT,
            model TEXT,
            FOREIGN KEY (worktree_id) REFERENCES worktrees(id) ON DELETE CASCADE
        );
CREATE TABLE worktrees (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            worktree_path TEXT NOT NULL UNIQUE,
            branch_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            metadata TEXT,
            is_pinned INTEGER DEFAULT 0
        );
CREATE INDEX idx_plans_executed_at ON plans(executed_at);
CREATE INDEX idx_plans_worktree_id ON plans(worktree_id);
CREATE INDEX idx_sessions_worktree ON sessions(worktree_id);
CREATE INDEX idx_worktrees_branch ON worktrees(branch_name);
