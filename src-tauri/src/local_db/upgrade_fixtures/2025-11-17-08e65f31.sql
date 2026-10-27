-- Fresh-install schema as of 08e65f31 (2025-11-17), replayed from that commit's local_db.rs.
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
CREATE INDEX idx_plans_executed_at ON plans(executed_at);
CREATE INDEX idx_plans_worktree_id ON plans(worktree_id);
