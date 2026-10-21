//! Property tests: random typing and keys in the command palette, the
//! workspace picker and the branch switcher, checking the filtered list and
//! its selection after every step.
//!
//! `PROPTEST_CASES` sets how many cases run and `SEED` the run; a failure
//! prints the shrunk step list.

use gpui::proptest::{collection::vec, prelude::*};

use super::*;
use crate::gpui_shell::fuzz;

#[derive(Debug, Clone)]
enum Step {
  Type(String),
  Key(&'static str),
  Advance(u64),
}

/// Keys that move around the list or edit the search, but choose nothing.
const MOVES: &[&str] = &[
  "up",
  "down",
  "home",
  "end",
  "pageup",
  "pagedown",
  "alt-up",
  "alt-down",
  "ctrl-n",
  "ctrl-p",
  "ctrl-j",
  "ctrl-k",
  "backspace",
  "delete",
  "left",
  "right",
  "secondary-a",
  "tab",
  "shift-tab",
];

fn step() -> impl Strategy<Value = Step> {
  prop_oneof![
    4 => prop_oneof![
      3 => "[a-zA-Z /:.-]{1,6}",
      1 => "\\PC{1,6}",
      1 => gpui::proptest::sample::select(vec![
        "Run", "check", "feat", "main@", "🚀", "日本", "…", "  ", "\u{200b}", "ΣΑΣ",
      ])
      .prop_map(str::to_string),
      1 => (1usize..400).prop_map(|n| "a".repeat(n)),
    ]
    .prop_map(Step::Type),
    5 => gpui::proptest::sample::select(MOVES).prop_map(Step::Key),
    1 => Just(Step::Key("escape")),
    1 => Just(Step::Key("enter")),
    1 => (0u64..1000).prop_map(Step::Advance),
  ]
}

fn overlay_json(which: u8) -> (serde_json::Value, FakeHost) {
  match which % 4 {
    0 => (palette_json(true), checks_host(None)),
    1 => (palette_json(false), FakeHost::default()),
    2 => (
      serde_json::json!({"kind": "workspacePicker", "dark": false, "repoPath": "/repo"}),
      workspace_host(),
    ),
    _ => (branch_json(), branch_host()),
  }
}

/// What the overlay asked of the backend that changes something.
fn changes(h: &Harness) -> Vec<String> {
  h.host
    .calls()
    .into_iter()
    .filter(|c| {
      !c.starts_with("checks ")
        && !c.starts_with("pr ")
        && !c.starts_with("workspaces ")
        && !c.starts_with("branches ")
    })
    .collect()
}

#[test]
fn overlays_filter_and_select_consistently() {
  fuzz::check(
    "overlays_filter_and_select_consistently",
    24,
    (vec(step(), 1..40), 0u8..4),
    |cx, (steps, which)| {
      let (json, host) = overlay_json(which);
      let mut h = Harness::new(cx, json, host);
      h.cx.run_until_parked();
      let mut log = Vec::new();
      for step in &steps {
        let changed_before = changes(&h).len();
        match step {
          Step::Type(text) => h.type_text(text),
          Step::Key(keys) => h.keys(keys),
          Step::Advance(ms) => {
            h.cx
              .executor()
              .advance_clock(std::time::Duration::from_millis(*ms));
            h.cx.run_until_parked();
          }
        }
        log.push(step.clone());

        let (entries, order, selected, search) = h.overlay.read_with(&h.cx, |o, _| {
          (o.entries(), o.order(), o.selected, o.search.clone())
        });
        // The selection is an entry the search shows, and one can be chosen
        // whenever the search shows any.
        if let Some(i) = selected {
          assert!(
            i < entries.len(),
            "selected {i} of {} entries after {log:?}",
            entries.len()
          );
          assert!(
            order.contains(&i),
            "selected {:?} is filtered out by {search:?} (shown {order:?}) after {log:?}",
            entries[i].value
          );
        } else if order.iter().any(|&i| !entries[i].disabled) {
          panic!("{search:?} shows {order:?} but nothing is selected after {log:?}");
        }

        // Only Enter chooses: nothing else changes the backend.
        if !matches!(step, Step::Key("enter") | Step::Advance(_)) {
          let now = changes(&h);
          assert_eq!(
            now.len(),
            changed_before,
            "{step:?} did {:?} after {log:?}",
            &now[changed_before..]
          );
        }
        let sent = h.sent();
        if matches!(step, Step::Key("escape")) {
          assert!(
            sent.contains(&press("overlay.close")),
            "Escape did not close the overlay after {log:?}: {sent:?}"
          );
        }
        // Anything sent closes the overlay in the window: the case ends.
        if !sent.is_empty() {
          assert!(
            matches!(step, Step::Key("enter" | "escape")) || matches!(step, Step::Advance(_)),
            "{step:?} sent {sent:?} after {log:?}"
          );
          break;
        }
      }
    },
  );
}
