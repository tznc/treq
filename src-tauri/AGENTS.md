# Backend Guide (Rust)

- **GPUI shell** (`src/gpui_shell/`) draws every window; each window's model (`gpui_shell/window/`) reads and changes state through `core/`.
- **Commands** (`src/commands/`) hold the app-state glue (PTYs, cloud, remote control) that GPUI and the CLI share; keep them thin and delegate to `core/`.
- **Core** (`src/core/`) holds all business logic (workspaces, commits, repo, changes, remote, ...).
- **Infrastructure:** `jj.rs` (all Jujutsu calls), `db.rs` / `local_db.rs` (SQLite), `pty.rs` (embedded terminal).
- **Thread safety:** `AppState` (`src/lib.rs`) wraps shared mutable resources in `Mutex`.

## Rust tests

Unit tests live inline in a `#[cfg(test)] mod tests` next to the code. Integration tests against real jj repos live in `tests/it/` (one test binary); `tests/common/mod.rs` provides `TestRepo`, a real jj repo in a `TempDir`.

- Use `tempfile::TempDir` for any filesystem operations; never hardcode paths
- Test one behaviour per `#[test]` function
- Name tests `verb_noun_condition`: `creates_workspace_with_empty_dir`, `returns_error_when_path_missing`
- Use `assert_eq!` / `assert!` directly; avoid custom assertion helpers unless shared across many tests

## Benchmarks

Add a Criterion benchmark in `benches/` for hot paths (file save, keystroke, large diffs). Use `BatchSize::PerIteration` with `TestRepo` to isolate each iteration.
