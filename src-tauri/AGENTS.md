# Backend Guide (Rust)

- **Commands** (`src/commands/`) are thin handlers: validate input, acquire state locks, delegate to `core/`.
- **Core** (`src/core/`) holds all business logic (workspaces, commits, repo, changes, remote, ...).
- **Infrastructure:** `jj.rs` (all Jujutsu calls), `db.rs` / `local_db.rs` (SQLite), `pty.rs` (embedded terminal).
- **Thread safety:** `AppState` (`src/lib.rs`) wraps shared mutable resources in `Mutex`.
- **NAPI bridge:** the lib compiles with the `tauri-test` feature to a native addon (`src/tauri_test_bridge.rs`). Frontend tests `require` it so `invoke` runs real Rust commands without a desktop window. Run `npm run build:napi` after any Rust change.

## Rust tests

Unit tests live inline in a `#[cfg(test)] mod tests` next to the code. Integration tests against real jj repos live in `tests/`; `tests/common/mod.rs` provides `TestRepo`, a real jj repo in a `TempDir`.

- Use `tempfile::TempDir` for any filesystem operations; never hardcode paths
- Test one behaviour per `#[test]` function
- Name tests `verb_noun_condition`: `creates_workspace_with_empty_dir`, `returns_error_when_path_missing`
- Use `assert_eq!` / `assert!` directly; avoid custom assertion helpers unless shared across many tests

## Benchmarks

Add a Criterion benchmark in `benches/` for hot paths (file save, keystroke, large diffs). Use `BatchSize::PerIteration` with `TestRepo` to isolate each iteration.
