//! The Code tab and its file browser on a large repository: a root of
//! thousands of entries, a directory as large, a source file near the 1 MB
//! limit, a minified file of one long line and a long README. The tree and
//! the open file are virtual lists, so only the rows on screen are drawn.
//! `code_files_perf` (ignored) prints the frame cost: `cargo test
//! --features gpui-shell --lib code_files_perf -- --ignored --nocapture
//! --test-threads=1`.

use std::{collections::HashMap, time::Duration};

use gpui::{Modifiers, TestAppContext};

use super::changes_large::{frame, leak, report, timed};
use super::*;

const BASE: &str = "/repo/.treq/workspaces/login";

fn env(name: &str, default: usize) -> usize {
  std::env::var(name)
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(default)
}

/// Entries in the root and in `big/`.
fn entries() -> usize {
  env("PERF_ENTRIES", 5_000)
}

/// Lines of `big.rs`, about 1 MB at the default.
fn file_lines() -> usize {
  env("PERF_FILE_LINES", 12_000)
}

/// Bytes of `min.js`'s one line.
fn minified_len() -> usize {
  env("PERF_MINIFIED", 200_000)
}

fn entry(rel: &str, is_directory: bool) -> crate::core::WorkspaceEntry {
  crate::core::WorkspaceEntry {
    name: rel.rsplit('/').next().unwrap_or(rel).into(),
    path: format!("{BASE}/{rel}"),
    is_directory,
    modified_at: None,
    submodule_pin: None,
    submodule_synced: None,
    status: None,
  }
}

fn source(lines: usize) -> String {
  (0..lines)
    .map(|i| {
      format!("    let value_{i} = compute(\"item {i}\", {i}).map(|v| v + {i}); // note {i:>6}\n")
    })
    .collect()
}

fn minified(len: usize) -> String {
  let unit = "function a(b,c){return b.map(function(d){return d+c*2})};var e=\"x\";";
  unit.repeat(len / unit.len() + 1)[..len].to_string()
}

fn readme(sections: usize) -> String {
  (0..sections)
    .map(|i| {
      format!(
        "## Section {i} :rocket:\n\nSome **bold** text and `code` with a [link](https://x.dev/{i}).\n\n\
         - item one\n- item two\n\n```rust\nfn f{i}() {{ let x = {i}; }}\n```\n\n<!-- hidden {i} -->\n\n"
      )
    })
    .collect()
}

/// A root of `entries()` files beside `big/` (as many again), `big.rs`,
/// `min.js` and a long `README.md`.
fn large_host() -> FakeHost {
  let mut host = code_host();
  let n = entries();
  let mut root: Vec<_> = (0..n)
    .map(|i| entry(&format!("zz_{i:05}.txt"), false))
    .collect();
  root.extend([
    entry("big", true),
    entry("big.rs", false),
    entry("min.js", false),
    entry("README.md", false),
  ]);
  let big: Vec<_> = (0..n)
    .map(|i| entry(&format!("big/item_{i:05}.rs"), false))
    .collect();
  let readme = readme(env("PERF_README_SECTIONS", 300));
  host.overview.entries = root.clone();
  host.overview.readme = Some(readme.clone());
  host.dirs = HashMap::from([(BASE.to_string(), root), (format!("{BASE}/big"), big)]);
  host.file_texts = HashMap::from([
    (format!("{BASE}/README.md"), readme),
    (format!("{BASE}/big.rs"), source(file_lines())),
    (format!("{BASE}/min.js"), minified(minified_len())),
  ]);
  host
}

fn route(browser: bool) -> ScreenSnapshot {
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["frameHeight"] = 120.into();
  if browser {
    route["fileBrowser"] = serde_json::json!({});
  }
  serde_json::from_value(route).expect("code route")
}

fn scroll(h: &mut Harness, at: gpui::Point<gpui::Pixels>, n: usize) -> Vec<Duration> {
  scroll_by(h, at, n, -200.)
}

fn scroll_by(h: &mut Harness, at: gpui::Point<gpui::Pixels>, n: usize, dy: f32) -> Vec<Duration> {
  (0..n)
    .map(|_| {
      timed(h, |h| {
        h.cx.simulate_event(gpui::ScrollWheelEvent {
          position: at,
          delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(dy))),
          modifiers: Modifiers::none(),
          touch_phase: gpui::TouchPhase::Moved,
        })
      })
    })
    .collect()
}

fn hover(h: &mut Harness, origin: gpui::Point<gpui::Pixels>, n: usize) -> Vec<Duration> {
  (0..n)
    .map(|i| {
      let at = gpui::point(origin.x, origin.y + gpui::px(i as f32 * 7.));
      timed(h, |h| h.cx.simulate_mouse_move(at, None, Modifiers::none()))
    })
    .collect()
}

#[gpui::test]
fn a_large_root_and_file_draw_only_the_rows_on_screen(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, large_host(), route(true));
  h.cx.run_until_parked();
  assert!(h.drawn("file-tree-big"));
  assert!(!h.drawn("file-tree-zz_04999.txt"), "far below the window");
  h.click("file-tree-big.rs");
  assert!(h.drawn("code-line-1"));
  assert!(!h.drawn(leak(format!("code-line-{}", file_lines()))));
}

#[gpui::test]
fn a_long_line_is_drawn(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, large_host(), route(true));
  h.cx.run_until_parked();
  h.click("file-tree-min.js");
  assert!(h.drawn("code-line-1"));
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn code_files_perf(cx: &mut TestAppContext) {
  // The Code tab: the root's entries and the README, in one scroll.
  let (mut h, _) = workspace_harness(cx, large_host(), route(false));
  h.cx.run_until_parked();
  assert!(h.drawn("code-readme"));
  println!("-- code tab: {} root entries, README", entries() + 4);
  let mut full: Vec<_> = (0..5).map(|_| frame(&mut h)).collect();
  report("full frame", &mut full);
  let origin = h
    .cx
    .debug_bounds("code-entry-zz_00000.txt")
    .expect("entry")
    .center();
  report("hover an entry", &mut hover(&mut h, origin, 10));
  report("scroll 200px", &mut scroll(&mut h, origin, 10));
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn file_browser_perf(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, large_host(), route(true));
  h.cx.run_until_parked();
  assert!(h.drawn("file-tree-big"));

  println!("-- tree: {} root entries", entries() + 4);
  let mut expand: Vec<Duration> = Vec::new();
  let mut collapse: Vec<Duration> = Vec::new();
  for _ in 0..5 {
    expand.push(timed(&mut h, |h| h.click("file-tree-big")));
    collapse.push(timed(&mut h, |h| h.click("file-tree-big")));
  }
  report("expand big/ (5k)", &mut expand);
  report("collapse big/", &mut collapse);

  println!("-- {} line source file", file_lines());
  let mut open = vec![timed(&mut h, |h| h.click("file-tree-big.rs"))];
  report("open", &mut open);
  let mut full: Vec<_> = (0..5).map(|_| frame(&mut h)).collect();
  report("full frame", &mut full);
  let line = h.cx.debug_bounds("code-line-1").expect("line").center();
  report("hover a line", &mut hover(&mut h, line, 20));
  report("scroll 200px", &mut scroll(&mut h, line, 20));
  h.cx.simulate_keystrokes("cmd-f");
  h.cx.simulate_input("value");
  h.cx.executor().advance_clock(Duration::from_millis(150));
  h.cx.run_until_parked();
  let mut find: Vec<_> = (0..5).map(|_| frame(&mut h)).collect();
  report("full frame, find applied", &mut find);
  report("scroll, find applied", &mut scroll(&mut h, line, 10));

  println!("-- minified: one {} byte line", minified_len());
  let mut open = vec![timed(&mut h, |h| h.click("file-tree-min.js"))];
  report("open", &mut open);
  let mut full: Vec<_> = (0..5).map(|_| frame(&mut h)).collect();
  report("full frame", &mut full);

  let tree = h
    .cx
    .debug_bounds("file-tree-big")
    .expect("tree row")
    .center();
  report("scroll tree 200px", &mut scroll(&mut h, tree, 10));

  println!("-- README preview");
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.file_browser.as_mut().expect("browser").selected_file =
        Some(format!("{BASE}/README.md"));
    }
  });
  let mut open = vec![timed(&mut h, |h| h.click("markdown-preview-tab"))];
  report("preview", &mut open);
  let mut full: Vec<_> = (0..5).map(|_| frame(&mut h)).collect();
  report("full frame", &mut full);
}
