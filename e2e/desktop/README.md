# Desktop end-to-end tests

Stock `@playwright/test` driving the GPUI desktop app. The app is built with
the `e2e` cargo feature, which embeds a Playwright server
([gpui-playwright](https://github.com/Ziinc/gpui-playwright)) that listens
only when `GPUI_PLAYWRIGHT` is set. No browser is launched: each app window
is a Playwright page.

`launch.ts` (Playwright's `webServer`) gives the app a fresh home and app data
dir, a git repository to open (`TREQ_E2E_REPO`), and a fake `claude` agent on
`PATH` that records its arguments. The tests read the fixture from
`.fixture/fixture.json` and share one app, so they run in file order with one
worker.

## Run

```sh
# From the repository root: build the app with the Playwright server.
cargo build --manifest-path src-tauri/Cargo.toml --features e2e --bin treq

cd e2e/desktop
npm ci
# Headless, under Xvfb (Linux):
xvfb-run --auto-servernum --server-args="-screen 0 1920x1080x24" npm test
# Or on an existing display:
DISPLAY=:98 npm test
```

`TREQ_E2E_BIN` points at another binary (default:
`$CARGO_TARGET_DIR/debug/treq`, else `target/debug/treq`), and
`TREQ_E2E_PORT` changes the server port (default 9333).

## Writing tests

Elements are found the way gpui-kit observes them: give an element an id, a
role and an accessible name, then `.test_support()` (`gpui_kit::base::
TestSupportExt`). `ui::Button` does this for every button, named by its
label. Query with `getByRole`, `getByLabel`, `getByText` (labels and
painted text) and `getByTestId` (the GPUI element id).

The command palette and the other overlays open in a dialog window over the
repo window: once one shows, `dialogPage(context)` returns a page for it.
Toasts, tooltips and menus are popup windows, so assert on what they change
instead.

Rust UI tests of single screens use
[gpui-testing-library](https://github.com/Ziinc/gpui-testing-library) on the
same observations (`src-tauri/src/gpui_shell/**/testing_library*.rs`).
