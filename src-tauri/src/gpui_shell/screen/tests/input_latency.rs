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
  /// What the frame after the dispatch cost (the rest is the dispatch and
  /// the effects it queued, which may draw by itself in the test platform).
  frame: Duration,
  frame_allocs: usize,
}

/// `act`, the effects it starts, and the frame after, measured.
fn sample(h: &mut Harness, act: impl FnOnce(&mut Harness)) -> Sample {
  let (a, b) = counts();
  let start = Instant::now();
  act(h);
  h.cx.run_until_parked();
  let (fa, fstart) = (counts().0, Instant::now());
  h.cx.update(|window, cx| window.draw(cx).clear(cx));
  let frame = fstart.elapsed();
  let time = start.elapsed();
  let (a2, b2) = counts();
  Sample {
    time,
    allocs: a2 - a,
    bytes: b2 - b,
    frame,
    frame_allocs: a2 - fa,
  }
}

fn report(name: &str, mut samples: Vec<Sample>) {
  samples.sort_by_key(|s| s.time);
  let mid = samples[samples.len() / 2];
  let mut allocs: Vec<usize> = samples.iter().map(|s| s.allocs).collect();
  allocs.sort();
  let mut bytes: Vec<usize> = samples.iter().map(|s| s.bytes).collect();
  bytes.sort();
  let mut frame: Vec<(Duration, usize)> =
    samples.iter().map(|s| (s.frame, s.frame_allocs)).collect();
  frame.sort();
  println!(
    "{name:<44} n={:<3} p50={:>8.2}ms max={:>8.2}ms allocs(p50)={:>7} KiB(p50)={:>8.1} | last draw p50={:>6.2}ms allocs={:>6}",
    samples.len(),
    mid.time.as_secs_f64() * 1000.,
    samples.last().map_or(0., |s| s.time.as_secs_f64() * 1000.),
    allocs[allocs.len() / 2],
    bytes[bytes.len() / 2] as f64 / 1024.,
    frame[frame.len() / 2].0.as_secs_f64() * 1000.,
    frame[frame.len() / 2].1,
  );
}

/// Types `n` characters one at a time, as a user would.
fn typing(h: &mut Harness, n: usize) -> Vec<Sample> {
  (0..n)
    .map(|i| {
      let ch = ["a", "b", "c", "d", "e", "f"][i % 6];
      // One keystroke: `simulate_input` dispatches an empty keystroke
      // before and after each character, which triples the dispatch work.
      sample(h, |h| h.cx.simulate_keystrokes(ch))
    })
    .collect()
}

/// The text of the focused field, to check the keystrokes landed.
fn focused_value(h: &mut Harness) -> String {
  use gpui_testing_library::prelude::*;
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  screen
    .elements()
    .into_iter()
    .find(|node| {
      node.is_focused()
        && matches!(
          node.role(),
          Some(gpui::Role::TextInput | gpui::Role::MultilineTextInput | gpui::Role::SearchInput)
        )
    })
    .and_then(|node| node.value())
    .unwrap_or_default()
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
  assert!(focused_value(&mut h).starts_with("abcdef"));
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
    sample(&mut h, |h| h.cx.simulate_keystrokes(ch));
    h.cx.executor().advance_clock(Duration::from_millis(150));
    landed.push(sample(&mut h, |_| {}));
  }
  report("find bar: debounce lands, value_1 typed", landed);
  report(
    "find bar: keystroke with matches applied",
    typing(&mut h, 12),
  );
}

/// Pastes `text` into the focused field, in one edit.
fn paste(h: &mut Harness, text: &str) {
  h.cx
    .write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
  h.cx.simulate_keystrokes("cmd-v");
  h.cx.run_until_parked();
}

#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_composer(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  report("code tab: idle frame", idle(&mut h, 10));
  report("code tab: full refresh", full(&mut h, 10));
  let mut modal = agent_modal();
  modal["initialPrompt"] = "".into();
  h.set_modal(Some(modal));
  h.click("task-input");
  report("composer: keystroke", typing(&mut h, 30));
  assert!(focused_value(&mut h).starts_with("abcdef"));
  // A long prompt (a pasted log): the cost per key with the prompt's size.
  let line = "lorem ipsum dolor sit amet, consectetur adipiscing elit sed do\n";
  let mut have = 0;
  for kb in [5, 50, 200] {
    let more = kb * 1024 / line.len() - have;
    have += more;
    paste(&mut h, &line.repeat(more));
    report(
      &format!("composer, {kb} KB prompt: keystroke"),
      typing(&mut h, 15),
    );
  }
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

/// `n` small files, each one hunk of one replaced line.
fn many_files_host(n: usize) -> FakeHost {
  let mut host = code_host();
  let files: Vec<_> = (0..n)
    .map(|f| {
      serde_json::json!({
        "path": format!("src/dir_{}/module_{f}.rs", f % 40),
        "status": "M",
        "previous_path": null,
        "changed_line_count": 2,
        "diff_deferred": false,
      })
    })
    .collect();
  let hunks: Vec<_> = (0..n)
    .map(|f| {
      serde_json::json!({
        "path": format!("src/dir_{}/module_{f}.rs", f % 40),
        "hunks": [{
          "id": "h",
          "header": "@@ -1,1 +1,1 @@",
          "lines": [format!("-let a = {f};"), format!("+let a = {};", f + 1)],
          "patch": "",
        }],
      })
    })
    .collect();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": files,
    "hunks_by_file": hunks,
    "too_large_to_render": false
  }))
  .ok();
  host
}

/// A find-bar keystroke notifies the whole Changes view; with a long file
/// list the sidebar is part of that.
#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_find_bar_many_files(cx: &mut TestAppContext) {
  let n = std::env::var("FILES")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(1000);
  let (mut h, _) = workspace_harness(cx, many_files_host(n), changes_route());
  if h.drawn("view-large-changeset") {
    h.click("view-large-changeset");
  }
  h.cx.run_until_parked();
  report("many files: idle frame", idle(&mut h, 10));
  report("many files: full refresh", full(&mut h, 5));
  h.click("commit-message");
  report("many files: commit message keystroke", typing(&mut h, 12));
  h.cx.simulate_keystrokes("cmd-f");
  h.cx.run_until_parked();
  report("many files: find keystroke", typing(&mut h, 12));
}

/// Typing in an inline review comment under the large diff.
#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_review_comment(cx: &mut TestAppContext) {
  let mut h = changes_large::expanded_with(cx, changes_large::reviewed_host());
  assert!(h.drawn("inline-comment-card"));
  report("reviewed diff: idle frame", idle(&mut h, 10));
  report("reviewed diff: full refresh", full(&mut h, 5));
  // A new comment on the first line of the first file (hover shows the +).
  h.click("add-comment-src/module_0.rs-0-1");
  assert!(h.drawn("comment-input"));
  h.click("comment-input");
  report("review comment draft: keystroke", typing(&mut h, 20));
}

/// A focused text field blinks its caret: frames drawn while it is focused
/// or not, in the active window and in the background (the terminal stops
/// its blink there; `a_background_terminal_does_not_blink`). The workspace
/// is in sync with its remote, so nothing else animates.
#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_caret_blink_idle(cx: &mut TestAppContext) {
  let mut host = changes_host();
  if let Some(status) = host.loaded.status.as_mut() {
    status.remote_sync = crate::core::RemoteSyncStatus::InSync;
  }
  let (mut h, host, frames) = super::idle::counted(cx, host, changes_route());
  assert!(h.drawn("commit-message"));
  let idle = super::idle::idle_for;
  idle(&mut h, &host, &frames, 10);
  let blurred = idle(&mut h, &host, &frames, 20);
  h.click("commit-message");
  idle(&mut h, &host, &frames, 3);
  let focused = idle(&mut h, &host, &frames, 20);
  h.cx.deactivate_window();
  idle(&mut h, &host, &frames, 3);
  let background = idle(&mut h, &host, &frames, 20);
  println!(
    "no focus, active window:           {:>3} frames in 20 s",
    blurred.frames
  );
  println!(
    "commit box focused, active window: {:>3} frames in 20 s",
    focused.frames
  );
  println!(
    "commit box focused, background:    {:>3} frames in 20 s",
    background.frames
  );
}

/// Frames the window draws per typed character.
#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_frames_per_keystroke(cx: &mut TestAppContext) {
  let mut host = changes_host();
  if let Some(status) = host.loaded.status.as_mut() {
    status.remote_sync = crate::core::RemoteSyncStatus::InSync;
  }
  let (mut h, _host, frames) = super::idle::counted(cx, host, changes_route());
  h.click("commit-message");
  h.cx.run_until_parked();
  let before = frames.get();
  for ch in ["a", "b", "c", "d", "e"] {
    h.cx.simulate_keystrokes(ch);
  }
  let after_keys = frames.get();
  h.cx.run_until_parked();
  h.cx.update(|window, cx| window.draw(cx).clear(cx));
  println!(
    "draws during 5 keystrokes: {} (+{} on an explicit draw after)",
    after_keys - before,
    frames.get() - after_keys
  );
}

/// Five full frames of the many-files Changes tab and nothing else, for
/// callgrind: `FILES=300 valgrind --tool=callgrind --toggle-collect='*Window*::draw' <test binary> input_latency_profile --ignored`.
#[gpui::test]
#[ignore = "benchmark"]
fn input_latency_profile_many_files(cx: &mut TestAppContext) {
  let n = std::env::var("FILES")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(300);
  let (mut h, _) = workspace_harness(cx, many_files_host(n), changes_route());
  if h.drawn("view-large-changeset") {
    h.click("view-large-changeset");
  }
  h.cx.run_until_parked();
  for _ in 0..5 {
    h.cx.update(|window, cx| {
      window.refresh();
      window.draw(cx).clear(cx);
    });
  }
}

/// Does a notify in one cached view re-render its cached sibling? GPUI marks
/// the notifier's ancestors dirty, and an outer view that re-renders draws
/// its subtree with `refreshing` set, which a nested cached view honours by
/// rendering again. `cargo test --lib input_latency_nested_cache -- --nocapture`.
mod nested_cache {
  use std::{cell::Cell, rc::Rc};

  use gpui::{
    div, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    StyleRefinement, Styled as _, TestAppContext, Window,
  };

  struct Heavy(Rc<Cell<usize>>);
  impl Render for Heavy {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      self.0.set(self.0.get() + 1);
      div().size_full()
    }
  }

  struct Light(Rc<Cell<usize>>);
  impl Render for Light {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      self.0.set(self.0.get() + 1);
      div().size_full()
    }
  }

  struct Parent {
    heavy: Entity<Heavy>,
    light: Entity<Light>,
    own: Rc<Cell<usize>>,
  }
  impl Render for Parent {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      self.own.set(self.own.get() + 1);
      div()
        .size_full()
        .flex()
        .child(
          self
            .heavy
            .clone()
            .cached(StyleRefinement::default().size_full()),
        )
        .child(
          self
            .light
            .clone()
            .cached(StyleRefinement::default().size_full()),
        )
    }
  }

  struct Root(Entity<Parent>);
  impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      div().size_full().child(
        self
          .0
          .clone()
          .cached(StyleRefinement::default().size_full()),
      )
    }
  }

  #[gpui::test]
  #[ignore = "benchmark"]
  fn input_latency_nested_cache(cx: &mut TestAppContext) {
    let (heavy_n, light_n, own_n) = (
      Rc::new(Cell::new(0)),
      Rc::new(Cell::new(0)),
      Rc::new(Cell::new(0)),
    );
    let (hn, ln, on) = (heavy_n.clone(), light_n.clone(), own_n.clone());
    let (root, cx) = cx.add_window_view(move |_, cx| {
      let parent = cx.new(|cx| Parent {
        heavy: cx.new(|_| Heavy(hn)),
        light: cx.new(|_| Light(ln)),
        own: on,
      });
      Root(parent)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let before = (own_n.get(), heavy_n.get(), light_n.get());
    // Notify the light sibling only, as a keystroke notifies its input.
    let light = root.read_with(cx, |root, cx| root.0.read(cx).light.clone());
    light.update(cx, |_, cx| cx.notify());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let after = (own_n.get(), heavy_n.get(), light_n.get());
    println!(
      "renders after the first frame: parent={} heavy={} light={}; after notifying `light`: parent={} heavy={} light={}",
      before.0, before.1, before.2, after.0, after.1, after.2
    );
  }
}
