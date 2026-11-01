# Treq Agent Guide

Treq is a **Tauri v2** Stacking Agent Development Environment (ADE) built on Jujutsu (jj). The whole desktop app is Rust in `src-tauri/`: GPUI draws every window (`src-tauri/src/gpui_shell/`), and there is no webview frontend. `web/` is the website and `supabase/` the hosted services; `test/` holds the Supabase-side unit tests.

Area guides (read the one for the area you are changing):
- `src-tauri/AGENTS.md` — backend architecture, Rust tests, benchmarks

## Workflow Rules

- Use **npm** (not pnpm) for all JS commands
- Keep `commands/*.rs` thin: no business logic there — delegate to `core/`
- Prefer **jj-lib** or **gix** over subprocess calls; never shell out to `jj` or `git` from Rust
- `jj.rs` functions that accept a workspace path must validate the path exists and return an empty/default value on missing path rather than propagating an IO error
- GitHub MCP tools: pass `minimal_output: true` unless you need full fields, and paginate lists 5–10 at a time

## TDD (Required)

All Rust work **must** follow TDD.

1. Write a failing test that captures the desired behaviour
2. Run it — confirm the new test fails (and only the new test)
3. Write the minimum code to make it pass
4. Run tests — confirm all pass
5. Refactor while keeping tests green

Never write implementation code before the test exists. For a bug, reproduce it in a test first. Do not add tests for styling-only changes (spacing, color); check those in the running app.

| Work type | Test location |
|---|---|
| Rust core function | `#[cfg(test)]` in the same `.rs` file, or `src-tauri/tests/` |
| GPUI screen or window behaviour | the `tests.rs` next to it in `src-tauri/src/gpui_shell/` |
| Supabase Edge Function logic | `test/*.test.ts` (`npm test`) |
| Performance-critical path | `src-tauri/benches/` |

## Running Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
npm test                                     # Supabase-side unit tests
```

### Disk space

Rust's incremental cache (`target/debug/incremental`) grows by gigabytes per build and fills small disks such as cloud sessions. When a build fails with "no space left on device" or `df` shows the disk nearly full, delete it: `rm -rf target/debug/incremental` (cargo rebuilds it; the next build is slower). Setting `CARGO_INCREMENTAL=0` avoids writing it at all, as CI does. Git worktrees can share one `CARGO_TARGET_DIR` to save disk only while they sit on the same base commit: the in-repo crates (`crates/*`) compile to the same artifact in every worktree, so worktrees on different bases overwrite each other's copy and fail with errors in files they never changed. After switching to a worktree on another base, `touch crates/*/src/*.rs` before building.
