---
name: design-audit
description: >-
  Scan treq's desktop UI for drift from the design system (crates/treq-ui),
  in code and in screenshots, and write a drift report; or run a full design
  review (Krug's Don't Make Me Think principles, per component and screen).
  Use on /design-audit, when asked whether the UI is consistent, follows the
  design system or is usable, for a design review, and before landing a
  change that adds or restyles UI in src-tauri/src/gpui_shell/**.
---

# Design audit

The design system is `crates/treq-ui`: tokens in `theme.rs`, components in
the crate root, and the rules each component's doc comment and the
styleguide (`cargo run -p treq-ui --example styleguide`) state. The full
proposal, with the reasoning behind each rule, is the "Treq Design System"
artifact (source in `/mnt/project-files/design-system/`).

Drift is anything a screen draws that the system would draw differently. An
audit finds it two ways, then reports both:

| Half | Finds | Tool |
|---|---|---|
| Code scan | raw colours, text sizes and spacing; hand-rolled controls; Escape handled outside the layer stack; Yes/No copy | `npm run design:audit` (the design lints with their "not migrated" lists set aside) |
| Screenshot scan | what the code scan can't see: hierarchy, state colours, chevrons, selected styles, dialog footers, copy on screen | styleguide snapshots, /app-qa captures, judged with gpui-visual-test |

An audit answers "does the UI follow the system?". A **design review**
(section 4) answers "is the system itself right, and does the app use it?";
run it when asked for a design review, a usability pass, or before changing
the system's rules.

Scope it to what was asked: a whole-app audit, one area ("audit the merge
dialog"), or the diff of the current branch. For a branch, run both halves
on `main` and on the branch, and report only what the branch changed.

## 1. Code scan

From the repo root:

```sh
npm run design:audit            # Markdown tables
npm run design:audit -- --json  # to compare two runs
```

For each design lint it reports:

- **New drift**: a violation in a file the rule doesn't ignore. CI fails on
  these (`npm run lint`), so any here is a bug in the branch.
- **Debt**: violations in files on the rule's `# Not migrated yet` list.
  This is the migration backlog; it should only go down.
- **Stale ignores**: listed files that already pass. Remove them from the
  rule's `ignores:` in the same change.

Then **component adoption**: how many times screens construct each `ui`
component. A component at 0 that a screen clearly needs (a hand-drawn tab
strip, a local `badge` fn) is a migration candidate.

The lints can't see everything. Also grep the scope for:

- `.bg(` or `.text_color(` with `p.destructive`, `p.warning`, `p.success` or
  `pr_*` on something that isn't that state (a count, a label, chrome);
- `.shadow_` on a selected row;
- `.hover(` with anything but `p.hover()`/`p.accent`, and selected fills
  other than `p.selected()`;
- a `Button` labelled with a noun where a verb fits, or `…` missing on one
  that opens more UI;
- `cursor_default` on something clickable.

## 2. Screenshot scan

### Styleguide (components)

```sh
cargo build -p treq-ui --example styleguide
cd crates/treq-ui && ../../target/debug/examples/styleguide check
```

A mismatch means a component changed. Open the actual image under
`target/styleguide-check/` beside `styleguide/snapshots/`. If the change is
intended, `styleguide render` and commit the snapshots with it; otherwise it
is drift in the component itself.

### The app (screens)

1. Capture the screens in scope with the /app-qa skill (layer 2): build the
   `e2e` binary and run the QA specs for the areas in scope, in both themes,
   with `QA_OUT` pointing at one directory. Read that skill for the
   commands.
2. Install the gpui-visual-test CLI once:
   `cargo install --git https://github.com/Ziinc/gpui-visual-test cargo-visual-test --locked`.
3. Check every capture against the rules below, plus its own
   `what_to_check` from the `.json` beside it. Keep this check separate from
   the captures:

   ```sh
   export GPUI_VISUAL_DIR=$QA_OUT/audit
   cargo visual-test check "$QA_OUT/09-delete-workspace-confirm-light.png" \
     -e "At most one filled button is visible in the dialog" \
     -e "The buttons are verbs, not Yes/No/OK" \
     --should "The dialog has no X, because it has an action pair" \
     --diff 0.001
   ```

   `--diff` compares the capture with its approved baseline. Use it when
   auditing a branch: capture `main` first, approve those captures as
   baselines (`cargo visual-test approve --all`), then capture and check
   the branch. Keep baselines out of the repo, as app screenshots are
   generated, never committed: `export GPUI_VISUAL_BASELINE_DIR=$QA_OUT/baselines`.
4. Judge the pending checks yourself, by looking:
   `cargo visual-test pending --json`, open each image, then
   `cargo visual-test verdict <name> <index> pass|fail --reason "<what you saw>" --hash <hash>`.
   With `GPUI_VISUAL_JUDGE=anthropic:<model>` (and a key) a model judges
   instead, and you review its failures. Never pass a check without
   looking at the current image.
5. `cargo visual-test status` lists what failed. `cargo visual-test report`
   prints the path of an HTML index of every shot, failures first.

### Rules to check on every screen

Phrase each as an expectation only when the screen shows the thing it
names.

- **One primary per region**: at most one filled (primary or danger) button
  per panel or dialog.
- **Values look like values**: a control showing a current value (target
  branch, agent, model) is an outline field with a trailing chevron. A
  command that opens a menu ("Add") has no chevron.
- **Colour means state**: counts are neutral grey pills. Red, amber and
  green appear only where that state is true, once per object. Status is a
  small dot after the name, never the name's colour.
- **One selected style**: a selected row or toggle uses the light accent
  tint, with no shadow and no coloured text.
- **Dialogs**: a dialog with Cancel and a verb has no X. One without an
  action pair has an X top-right. The title is a question or a name, and the
  buttons are verbs, never Yes/No/OK.
- **Alerts**: the icon and tint carry the severity, the title is in body
  colour, and there is at most one outline action.
- **Empty states** say why the area is empty and offer at most one action.
- **Shortcuts** read `⌘ + K` on macOS and `Ctrl + K` elsewhere: the
  platform's key names with " + " between keys, rendered by `ui::Kbd`.
- **Type and spacing**: text comes in the four sizes (11, 13, 15, 20) and
  nothing is clipped or overlapping. Controls in a row share one height.
- **Copy**: sentence case, except the sidebar's group labels. No "successfully" and no
  "Are you sure".

## 3. Report (audit)

Write `design-audit.md` into `QA_OUT` (or the scratchpad) with:

1. **Summary**: new drift (must be zero), debt per rule and its trend
   against the last run if there is one, and screenshot checks failed.
2. **Findings**, worst first. Each finding gets:
   - the rule it breaks;
   - the screen and file (`path:line` when the code scan found it);
   - the capture name, with its diff image when there is one;
   - the fix, naming the `ui` component or token to use.
3. **Stale ignores** to remove.
4. **What wasn't covered**: areas without a QA spec, the theme not
   captured, popups and menus that only a full-screen capture shows.

Publish the report with the failing captures as an Artifact (before/after
pairs when auditing a branch) and give the user the link. Fixes go in their
own change: an audit reports, it doesn't migrate.

## 4. Design review

A review judges the UI against Krug's test: can a first-time user tell,
without thinking, what this is, what they can do here, and what just
happened? Score it on six principles, and use these exact names and
abbreviations, which `review/krug.json` and the walkthrough page use:

| Key | Principle | Asks |
|---|---|---|
| SE | Self-evident | Is it obvious what this is and does? |
| CV | Conventions | Does it work like macOS, Windows and GitHub taught? |
| HI | Clear hierarchy | Is the most important thing the most prominent? |
| CL | Obvious clickability | Does what's clickable look clickable, and only that? |
| NO | Less noise | Does each signal mean one thing? |
| WH | What just happened | Are waiting, emptiness and results explained? |

Work through these steps in order. Each one can stop the review with a
finding.

1. **Inventory.** List the controls the screens draw, grouped by job
   (buttons, value pickers, counts and status, tabs, rows, menus, dialogs,
   alerts, empty and loading states, toasts, shortcuts). Use the code scan's
   component adoption counts and grep for hand-drawn versions. Count the
   variants of each job (for example, "three checkboxes, five kbd styles").
   Every extra variant is a finding.
2. **Screens, per principle.** Capture the app in both themes (/app-qa).
   For each principle, write what holds up and what breaks, citing captures.
   The output is a two-column "holds up / breaks" table per principle.
3. **Before/after mocks.** For each finding worth fixing, show the real
   capture beside the same UI redrawn under the proposed rule, and state the
   rule in one or two sentences. Rules must be testable: they become
   styleguide `.describe(..)` text, lint rules, and the "Rules to check on
   every screen" list above.
4. **Components, per principle.** For every `ui` component and every
   compound example in the styleguide (an example built from several `ui`
   parts, such as Alert with a Button or Row with Dot, Count and
   IconButton), write how it satisfies each principle that applies. Put this
   in `review/krug.json`: one entry per component, with its styleguide
   example ids, a `compound` note, `notes` (principle key and how), and a
   `gap` for anything a screen needs that the component can't express. Then
   build the page:

   ```sh
   cargo run -p treq-ui --example styleguide -- render   # if components changed
   python3 .claude/skills/design-audit/review/walkthrough.py "$OUT/walkthrough.html"
   ```

   A gap is a finding. Close it in `crates/treq-ui`, with a builder test and
   a styleguide example, before migrating the screens that need it.
5. **Adoption gate.** A rule only counts as applied when screens construct
   the component that carries it. Re-run the adoption counts. Any component
   at 0, or used by one screen when several draw that job, is a finding,
   even if every lint passes. Tokens alone don't change what users see.
   Moving screens onto tokens is lint debt, not a review fix.
6. **Visible-change check.** After screens adopt the fixes, capture the same
   QA specs on the base and on the branch. For every review item, put the
   before and after captures side by side and look at them. If an item's
   screen looks the same, the fix didn't reach it: go back to step 5. Don't
   present the result as done.

Deliver the review as an Artifact with these parts:

1. The verdict.
2. The per-principle table.
3. The before/after mocks.
4. The design language and tokens.
5. Interaction rules.
6. The walkthrough link.
7. Decisions for the user, each with a recommended option.

After approval, land it as a short stack:

1. Rules and lints.
2. Components and the styleguide.
3. The audit.
4. The screens adopting the components, one commit per review item.

Re-run steps 5 and 6 on the screens PR before calling it done.
