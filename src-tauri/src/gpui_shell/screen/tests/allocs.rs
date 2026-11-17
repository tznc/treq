//! Heap allocations per frame on the main screens, and the memory the kept
//! tab views hold. A counting allocator wraps the system one for the whole
//! test binary; only the ignored benchmarks read it:
//! `cargo test --features gpui-shell --lib allocs_ -- --ignored --nocapture --test-threads=1`.

use std::sync::atomic::Ordering::Relaxed;

use gpui::TestAppContext;

use crate::test_alloc::{ALLOCS, BYTES, LIVE};

use super::{
  changes_large, commits_large,
  kept_tabs::{set_workspace, workspace},
  *,
};

/// The running (allocations, bytes) totals, for the other benchmarks.
pub(super) fn counts() -> (usize, usize) {
  (ALLOCS.load(Relaxed), BYTES.load(Relaxed))
}

/// Allocations and bytes allocated by one full frame, as the median of
/// `n` frames.
fn frame_allocs(h: &mut Harness, n: usize) -> (usize, usize) {
  let mut samples: Vec<(usize, usize)> = (0..n)
    .map(|_| {
      h.cx.run_until_parked();
      h.cx.update(|window, cx| {
        window.refresh();
        let (a, b) = (ALLOCS.load(Relaxed), BYTES.load(Relaxed));
        window.draw(cx).clear(cx);
        (ALLOCS.load(Relaxed) - a, BYTES.load(Relaxed) - b)
      })
    })
    .collect();
  samples.sort();
  samples[n / 2]
}

fn print_frame(name: &str, h: &mut Harness) {
  let (allocs, bytes) = frame_allocs(h, 15);
  println!(
    "{name:<28} allocs/frame={allocs:>7}  KiB/frame={:>8.1}",
    bytes as f64 / 1024.
  );
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture --test-threads=1"]
fn allocs_changes_large(cx: &mut TestAppContext) {
  let mut h = changes_large::expanded(cx);
  print_frame("changes, large, expanded", &mut h);
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture --test-threads=1"]
fn allocs_commits_large(cx: &mut TestAppContext) {
  let mut h = commits_large::opened(cx);
  print_frame("commits, large commit open", &mut h);
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture --test-threads=1"]
fn allocs_code(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  print_frame("code tab", &mut h);
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture --test-threads=1"]
fn allocs_changes_small(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  print_frame("changes, small", &mut h);
}

pub(super) fn live() -> isize {
  LIVE.load(Relaxed)
}

pub(super) fn allocs_now() -> usize {
  ALLOCS.load(Relaxed)
}

/// Memory held after showing the large working copy in one workspace after
/// another: the kept views bound it.
#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture --test-threads=1"]
fn allocs_kept_views(cx: &mut TestAppContext) {
  let mut host = changes_large::large_host();
  host.workspaces = (1..=10).map(workspace).collect();
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  h.cx.run_until_parked();
  let base = live();
  for id in 1..=10 {
    set_workspace(&mut h, id);
    if h.drawn("view-large-changeset") {
      h.click("view-large-changeset");
    }
    h.cx.run_until_parked();
    changes_large::frame(&mut h);
    println!(
      "after workspace {id:>2}: live +{:>8.1} KiB",
      (live() - base) as f64 / 1024.
    );
  }
}
