-- Fresh-install schema as of 7e964062 (2025-12-08), replayed from that commit's local_db.rs.
CREATE TABLE "plans" (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workspace_id INTEGER NOT NULL,
                    title TEXT NOT NULL,
                    type TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    executed_at TEXT NOT NULL,
                    status TEXT NOT NULL
                );
CREATE TABLE "sessions" (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    workspace_id INTEGER,
                    name TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    last_accessed TEXT NOT NULL,
                    plan_title TEXT,
                    model TEXT,
                    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
                );
CREATE TABLE workspaces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_name TEXT NOT NULL,
            workspace_path TEXT NOT NULL UNIQUE,
            branch_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            metadata TEXT,
            is_pinned INTEGER DEFAULT 0
        );
CREATE INDEX idx_plans_executed_at ON plans(executed_at);
CREATE INDEX idx_plans_workspace_id ON plans(workspace_id);
CREATE INDEX idx_sessions_workspace ON sessions(workspace_id);
CREATE INDEX idx_workspaces_branch ON workspaces(branch_name);
