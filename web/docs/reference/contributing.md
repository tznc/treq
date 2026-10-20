---
sidebar_position: 4
---

# Contributing

_Developer documentation for working on Treq locally._

The Treq desktop app is Rust throughout: GPUI draws the windows, and a core runtime owns the repository logic. Start any change by working out which layer owns the behavior, then write the smallest test that proves your change.

## Repository Layout

| Path | Purpose |
|---|---|
| `src-tauri/src/gpui_shell/` | The windows: GPUI views and each window's model. |
| `src-tauri/src/commands/` | Thin app-state handlers. Keep business logic out of this layer. |
| `src-tauri/src/core/` | Workspace, commit, repository, and change-management logic. |
| `src-tauri/src/jj.rs` | Jujutsu and Git repository integration. |
| `test/` | Vitest unit tests for the Supabase Edge Functions. |
| `web/` | Documentation site. |

## Development Setup

Install dependencies from the repository root.

```bash
npm install
```

Run the desktop app in development mode.

```bash
npm start
```

Run the documentation site from `web/`.

```bash
npm start
```

## Test Strategy

Backend changes are test-first. Write a failing Rust test, make the smallest change that passes it, then run it again.

Do not mock repository behavior when the feature leans on workspace state, file status, commits, or rebasing: the Rust tests run against real jj repositories.

| Change type | Test location |
|---|---|
| Core Rust behavior | Inline tests next to the Rust module. |
| Window and screen behavior | The `tests.rs` next to the view in `src-tauri/src/gpui_shell/`. |
| Documentation | Markdown lint and local docs preview. |

## Common Commands

```bash
cargo test --manifest-path src-tauri/Cargo.toml
npm test
```

The overview shot (`assets/screenshots/code.png`) is committed, and the site build copies it to `/img/code.png`. The landing images (`/img/landing/*`) are generated, not committed: run `npm run landing` in `e2e/desktop` to write them into `web/static/img/landing/`. A production build fails if any listed image is missing.

## Implementation Rules

Keep command handlers thin. Check the input, take the state you need, and hand the work to `src-tauri/src/core/`.

Call repository libraries instead of shelling out from Rust. A repository function that takes a path must cope with a workspace path that is not there, returning an empty or default result wherever the app promises that.

Do not fold unrelated tidying into feature work. Treq has several layers with different owners, and a wide refactor makes a regression much harder to spot.

## Documentation Rules

Docs follow the style guide in `web/STYLE_GUIDE.md`. Write plain technical prose, skip the marketing language, and use tables for dense reference data.

Public docs describe macOS, Windows, and Linux behavior. Call out platform-specific behavior when it differs. Leave out implementation details a reader cannot act on.

## Learn More

- [Overview](/docs/)
- [CLI](/docs/reference/cli)
- [Workspaces](/docs/concepts/workspaces)
