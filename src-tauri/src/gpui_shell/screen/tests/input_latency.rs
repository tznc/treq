//! Keystroke -> frame cost in the app's text fields, and the window-wide
//! changes (resize, zoom, theme) that relayout everything. Every benchmark
//! is ignored and prints the median of its samples:
//! `cargo test --features gpui-shell --lib input_latency_ -- --ignored --nocapture --test-threads=1`.
//!
//! A sample is one typed character: the key dispatch, the effects it queues,
//! and the frame that follows. Allocations come from the counting allocator
//! in `allocs.rs`.

use std::time::{Duration, Instant};

use gpui::TestAppContext;

use super::{allocs::counts, changes_large, *};

#[derive(Clone, Copy)]
struct Sample {
  time: Duration,
  allocs: usize,
  bytes: usize,
}

/// `act`, the effects it starts, and the frame after, measured.
fn sample(h: &mut Harness, act: impl FnOnce(&mut Harness)) -> Sample {
  let (a, b) = counts();
  let start = Instant::now();
  act(h);
  h.cx.run_until_parked();
  h.cx.update(|window, cx| window.draw(cx).clear(cx));
  let time = start.elapsed();
  let (a2, b2) = counts();
  Sample {
    time,
    allocs: a2 - a,
    bytes: b2 - b,
  }
}

fn report(name: &str, mut samples: Vec<Sample>) {
  samples.sort_by_key(|s| s.time);
  let mid = samples[samples.len() / 2];
  let mut allocs: Vec<usize> = samples.iter().map(|s| s.allocs).collect();
  allocs.sort();
  let mut bytes: Vec<usize> = samples.iter().map(|s| s.bytes).collect();
  bytes.sort();
  println!(
    "{name:<44} n={:<3} p50={:>8.2}ms max={:>8.2}ms allocs(p50)={:>7} KiB(p50)={:>8.1}",
    samples.len(),
    mid.time.as_secs_f64() * 1000.,
    samples.last().map_or(0., |s| s.time.as_secs_f64() * 1000.),
    allocs[allocs.len() / 2],
    bytes[bytes.len() / 2] as f64 / 1024.,
  );
}

/// Types `n` characters one at a time, as a user would.
fn typing(h: &mut Harness, n: usize) -> Vec<Sample> {
  (0..n)
    .map(|i| {
      let ch = ["a", "b", "c", "d", "e", "f"][i % 6];
      sample(h, |h| h.cx.simulate_input(ch))
    })
    .collect()
}

/// A frame nothing asked for: what an idle redraw costs.
fn idle(h: &mut Harness, n: usize) -> Vec<Sample> {
  (0..n).map(|_| sample(h, |_| {})).collect()
}

/// A full redraw (`Window::refresh`): the ceiling of any keystroke.
fn full(h: &mut Harness, n: usize) -> Vec<Sample> {
  (0..n)
    .map(|_| {
      sample(h, |h| {
        h.cx.update(|window, _| window.refresh());
      })
    })
    .collect()
}

#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_commit_message(cx: &mut TestAppContext) {
  let mut h = changes_large::expanded(cx);
  report("changes large: idle frame", idle(&mut h, 10));
  report("changes large: full refresh", full(&mut h, 10));
  h.click("commit-message");
  report("commit message: keystroke", typing(&mut h, 30));
}

#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_find_bar(cx: &mut TestAppContext) {
  let mut h = changes_large::expanded(cx);
  h.cx.simulate_keystrokes("cmd-f");
  h.cx.run_until_parked();
  // Before the debounce lands: the bar echoes the key, nothing is matched.
  let keys = typing(&mut h, 12);
  report("find bar: keystroke (debounce pending)", keys);
  // The debounce lands: the matches are indexed and marked.
  h.cx.executor().advance_clock(Duration::from_millis(150));
  report(
    "find bar: debounce lands (index+marks)",
    vec![sample(&mut h, |_| {})],
  );
  // Typing on with a query applied: each key reuses the old index until the
  // debounce, then rebuilds it.
  let mut landed = Vec::new();
  for ch in ["v", "a", "l", "u", "e", "_", "1"] {
    sample(&mut h, |h| h.cx.simulate_input(ch));
    h.cx.executor().advance_clock(Duration::from_millis(150));
    landed.push(sample(&mut h, |_| {}));
  }
  report("find bar: debounce lands, value_1 typed", landed);
  report(
    "find bar: keystroke with matches applied",
    typing(&mut h, 12),
  );
}

#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_composer(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  report("code tab: idle frame", idle(&mut h, 10));
  report("code tab: full refresh", full(&mut h, 10));
  h.click("task-input");
  report("composer: keystroke", typing(&mut h, 30));
  // A long prompt: the whole value is copied per key.
  h.cx.simulate_input(&"lorem ipsum dolor sit amet\n".repeat(200));
  report("composer, 5 KB prompt: keystroke", typing(&mut h, 30));
}

#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_file_picker(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, workspace_route());
  let scope = file_picker::Scope {
    repo_path: "/repo".into(),
    workspace_id: None,
    workspace_path: "/repo".into(),
  };
  h.screen
    .update(&mut h.cx, |s, cx| s.set_file_picker(Some(scope), cx));
  assert!(h.drawn("file-picker"));
  report("file picker: keystroke", typing(&mut h, 30));
}

#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_settings(cx: &mut TestAppContext) {
  let mut h = Harness::new(cx, settings());
  h.click("settings-branch-pattern");
  report("settings field: keystroke", typing(&mut h, 30));
}

fn dialog(cx: &mut TestAppContext) -> Harness {
  let snapshot: ScreenSnapshot = serde_json::from_value(serde_json::json!({
    "kind": "onboarding",
    "dark": false,
    "modal": {
      "kind": "openWorkspaces",
      "repoPath": "/repo",
      "repos": ["/repo"],
      "linear": false,
      "stackOn": null,
    },
  }))
  .expect("snapshot");
  Harness::with_host(cx, snapshot, Arc::new(FakeHost::default()))
}

#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_new_workspaces(cx: &mut TestAppContext) {
  let mut h = dialog(cx);
  h.cx.run_until_parked();
  report("new workspaces, empty: keystroke", typing(&mut h, 20));
  // A pasted list: `lines()` runs several times per frame.
  let list: String = (0..200).map(|i| format!("feat/branch-{i}\n")).collect();
  h.cx.simulate_input(&list);
  h.cx.executor().advance_clock(Duration::from_millis(500));
  h.cx.run_until_parked();
  report("new workspaces, 200 lines: keystroke", typing(&mut h, 20));
}

#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_window_changes(cx: &mut TestAppContext) {
  use crate::gpui_shell::ui::{apply_zoom, UiZoom};
  let mut h = changes_large::expanded(cx);
  report("changes large: idle frame", idle(&mut h, 5));
  let sizes = [(1400., 900.), (1399., 900.), (1300., 850.), (1400., 900.)];
  let resize: Vec<_> = (0..20)
    .map(|i| {
      let (w, ht) = sizes[i % sizes.len()];
      sample(&mut h, |h| {
        h.cx.simulate_resize(gpui::size(gpui::px(w), gpui::px(ht)))
      })
    })
    .collect();
  report("resize (one event)", resize);
  let zoom: Vec<_> = (0..10)
    .map(|i| {
      sample(&mut h, |h| {
        h.cx.update(|window, cx| {
          cx.set_global(UiZoom(if i % 2 == 0 { 110 } else { 100 }));
          apply_zoom(window, cx);
        })
      })
    })
    .collect();
  report("zoom 100 <-> 110", zoom);
  let install: Vec<_> = (0..10)
    .map(|i| {
      sample(&mut h, |h| {
        h.cx
          .update(|_, cx| crate::gpui_shell::theme::set_dark(i % 2 == 1, cx));
      })
    })
    .collect();
  report("theme install only (set_dark + frame)", install);
  let theme: Vec<_> = (0..10)
    .map(|i| {
      sample(&mut h, |h| {
        let mut snapshot = h.screen.read_with(&h.cx, |s, _| s.snapshot().clone());
        snapshot.dark = i % 2 == 0;
        h.screen
          .update(&mut h.cx, |s, cx| s.set_snapshot(snapshot, cx));
      })
    })
    .collect();
  report("theme light <-> dark", theme);
}
