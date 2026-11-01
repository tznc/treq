---
name: app-qa
description: >-
  Screenshot-verify treq desktop UI and interaction changes by driving the
  real GPUI app with Playwright through gpui-playwright (e2e/desktop/qa), and
  fuzz screens headlessly with gpui-testing-library. Use on /app-qa, when asked
  to QA, screenshot or chaos-test a behavior, and proactively after changing
  src-tauri/src/gpui_shell/** or code a screen reads, before reporting done.
---

# App QA (screenshot-verified behavior checks)

The desktop app is Rust: GPUI draws every window (`src-tauri/src/gpui_shell/`),
and there is no webview. QA has three layers. Pick the lightest one that
proves the change, and always look at the pixels for anything visual.

| Layer | Tool | Runs | Use for |
|---|---|---|---|
| Screen tests and fuzzing | gpui-testing-library (`#[gpui::test]`) | headless, parallel, in `cargo test --lib` | one screen's behavior, invariants under random input |
| Screenshot QA | gpui-playwright, `e2e/desktop/qa/` (`npm run qa`) | the real binary under Xvfb, one app | what a change looks like, before/after captures |
| Chaos QA | gpui-playwright monkey script | the real binary under Xvfb | crashes, hangs and stale state across the whole app |

## When to use

- The user invokes `/app-qa`, optionally naming a flow ("app-qa the merge
  preview").
- Proactively, right after an edit to `src-tauri/src/gpui_shell/**`, or to a
  `core/` function a screen reads, before telling the user the change is done.

## Delegate the legwork

Hand discovery to cheaper subagents (`Agent` with `model: "haiku"`, or
`"sonnet"` when it needs reasoning), in parallel where independent: which spec
or `tests.rs` already sets up the state you need, which role and accessible
name an element has, which `*Host` method backs a screen. Ask for the specific
fact. Keep for yourself: deciding what to verify, writing the expectations,
reading the PNGs (step 5), and the report.

## Making elements findable

Both tools find elements the way gpui-kit observes them: an element id, a role
and an accessible name, then `.test_support()` (`gpui_kit::base::TestSupportExt`).
`ui::Button` does this for every button, named by its label. If a control has
no role or name, add them rather than querying by position. Observations are
only recorded in test builds.

## Layer 1: screen tests and fuzzing (gpui-testing-library)

- Write behavior tests in the `tests.rs` (or `tests/*.rs`) next to the screen,
  with the `Harness`/`FakeHost` patterns in
  `src-tauri/src/gpui_shell/screen/tests/testing_library.rs`: build a
  `Screen`, act with `screen.user()` (`click`, `type`, `keyboard`), assert with
  `expect(...)`. These go through GPUI's real pointer and keyboard dispatch, so
  never call a view's handler directly to drive a scenario.
- Fuzz with property tests in the same suites: generate random action
  sequences (clicks on visible controls by role, typed text with unicode and
  very long strings, Esc/Enter/Tab/arrows and the app's shortcuts) and check
  invariants after every step: no panic, at most one modal, Esc closes the top
  overlay, the route and selection stay valid, `FakeHost` receives no write the
  user didn't confirm. Keep case counts small so the lib tests stay fast;
  raise them for a hunting session with `PROPTEST_CASES=5000`. A failure prints
  its seed and shrunk action list: turn it into a plain regression test.
- Run only what you touched:
  `CARGO_INCREMENTAL=0 cargo test --features gpui-shell --lib gpui_shell::<module>`
  from `src-tauri/`.

## Layer 2: screenshot QA (gpui-playwright)

1. **Build the app with the Playwright server** (once per Rust change):
   `cargo build --manifest-path src-tauri/Cargo.toml --features e2e --bin treq`.
   Then `cd e2e/desktop && npm ci`.
2. **Pick or write a spec** in `e2e/desktop/qa/<area>.spec.ts`. One file per
   area; extend an existing file instead of duplicating its setup. The QA
   launcher seeds the demo repository (`landing/seed.ts`: stacked workspaces,
   a rebase conflict), a fake `gh` with fixture PRs, issues and checks
   (`landing/fake-gh.mjs`, no network or token) and a fake `claude`. Drive the
   app only through Playwright's user actions (`click`, `fill`, `press`,
   `keyboard.type`) on `getByRole`/`getByLabel`/`getByText`/`getByTestId`
   locators. If creating or renaming a workspace is part of the scenario,
   do it through the real dialog; seed only incidental background state.
3. **Capture with expectations.** `capture(page, { name, expectations })`
   (`qa/capture.ts`) writes `<name>-<theme>.png` and a `.json` with the
   expectations to `e2e/desktop/qa/.shots/` (or `QA_OUT`). `qa/helpers.ts`
   has the shared steps (`ready`, `home`, `openWorkspace`, `openTab`,
   `overlay`, `focusMain`, `dismissPopover`). Name captures `<area>-<NN>-<what-it-shows>`. `expectations` are
   at most three plain-English claims about what the picture should show;
   keep real `expect(...)` assertions in the spec too, since they prove the
   state before the capture.
   - The palette, the other overlays and the header popovers draw in the
     repo window itself: drive them through `page`.
   - Toasts, tooltips and menus are popup windows Playwright can't reach.
     Capture the whole X screen for those (`capture` with `screen: true`) and
     assert on what they change.
   - Check both themes for visual changes, and one non-default zoom when sizes
     changed.
4. **Run it** under Xvfb, from `e2e/desktop`:
   `xvfb-run --auto-servernum --server-args="-screen 0 3840x2400x24" npm run qa -- --grep <area>`
   (or `DISPLAY=:98 npm run qa` on a running Xvfb). `QA_THEME=light|dark`
   picks the theme (run both for visual changes), `QA_REUSE=1` drives an app
   that is already running, `TREQ_E2E_BIN` picks
   another binary and `TREQ_E2E_PORT` another server port, so two runs can
   share a machine on different displays and ports.
   - Remote SSH (`qa/18-ssh-remote.spec.ts`) needs a real host and is
     skipped without one: build the headless CLI
     (`cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin treq --target-dir target/cli`),
     run `e2e/desktop/qa/ssh-server/up.sh` (Docker sshd with treq, jj, git,
     tmux and a fake `claude`), export what it prints and run
     `npm run qa -- --grep ssh`. Rebuild the CLI and rerun `up.sh` after a
     change to `core/remote*.rs`: the host side runs that binary.
5. **Verify every capture against its expectations.** Read each
   `<name>.json`, then open `<name>.png` with the Read tool and confirm or
   refute each claim. A spec whose assertions passed can still render a broken
   layout, clipped text, the wrong theme colors or a stuck loading state; that
   is a bug to fix.
6. **Show the result.** Publish the before/after PNGs as an Artifact (a simple
   gallery: each image with its name and expectations) and give the user the
   link, calling out any expectation that didn't hold.

## Layer 3: chaos QA

For "chaos QA", or before landing a large UI change: write a seeded monkey
script on the same harness that repeatedly picks a visible control by role or
a shortcut from the keymap (Ctrl+K, Ctrl+P, Ctrl+N, Ctrl+Shift+N, Ctrl+I,
Ctrl+J, Ctrl+], Ctrl+\, ?, Esc, Tab, Enter, Ctrl+=/-/0), types random text,
and after each step checks the app process is alive, Playwright still answers,
and the app log has no `panicked`. Log the seed and the action trail so any
failure replays. Add targeted scenarios: double-submits, dialogs over dialogs,
fast switching while reads are in flight, the repo changing on disk behind the
app (new commits, a deleted workspace dir, a conflict), tiny and huge windows,
zoom extremes, 50+ workspaces, an empty repo, a non-git folder. Report each
bug with its repro, log excerpt and a screenshot, and pin it with a layer 1
test when it reproduces headlessly.

## Linux and Xvfb notes

- No window manager runs under Xvfb, so focus follows the pointer and a new
  window may not get keyboard focus. `focusMain()` (called by `ready()`)
  runs `xdotool windowfocus` on the repo window.
- `page.mouse` reaches only the repo window. Popups, menus and native dialogs
  need real X clicks (`xPointer`, `dismissPopover`).
- `GDK_SCALE=2` gives crisp 2x PNGs; the QA launcher sets it.
- jj-backed tests need a `jj` binary on `PATH`.

## Finish

After the behavior is verified and shown: `cargo fmt`, `cargo clippy
--features gpui-shell --lib --tests -- -D warnings`, and in `e2e/desktop`
`npm run typecheck`. Keep the QA specs: `e2e/desktop/qa/` is a growing
visual-regression library, not scratch.
