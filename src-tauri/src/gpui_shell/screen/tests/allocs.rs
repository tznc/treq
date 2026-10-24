//! Heap allocations per frame on the main screens, and the memory the kept
//! tab views hold. A counting allocator wraps the system one for the whole
//! test binary; only the ignored benchmarks read it:
//! `cargo test --features gpui-shell --lib allocs_ -- --ignored --nocapture --test-threads=1`.

use std::{
  alloc::{GlobalAlloc, Layout, System},
  sync::atomic::{AtomicIsize, AtomicUsize, Ordering::Relaxed},
};

use gpui::TestAppContext;

use super::{
  changes_large, commits_large,
  kept_tabs::{set_workspace, workspace},
  *,
};

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicIsize = AtomicIsize::new(0);

// SAFETY: forwards every call to `System` unchanged, only counting.
unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(layout.size(), Relaxed);
    LIVE.fetch_add(layout.size() as isize, Relaxed);
    // SAFETY: the caller upholds `alloc`'s contract.
    unsafe { System.alloc(layout) }
  }

  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    LIVE.fetch_sub(layout.size() as isize, Relaxed);
    // SAFETY: the caller upholds `dealloc`'s contract.
    unsafe { System.dealloc(ptr, layout) }
  }

  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(new_size, Relaxed);
    LIVE.fetch_add(new_size as isize - layout.size() as isize, Relaxed);
    // SAFETY: the caller upholds `realloc`'s contract.
    unsafe { System.realloc(ptr, layout, new_size) }
  }
}

#[global_allocator]
static COUNTING: Counting = Counting;

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

fn live() -> isize {
  LIVE.load(Relaxed)
}

/// Heap bytes live now, for the memory benches in `memory.rs`.
pub(super) fn heap() -> isize {
  live()
}

/// Allocations and bytes allocated so far, for the memory benches.
pub(super) fn totals() -> (usize, usize) {
  (ALLOCS.load(Relaxed), BYTES.load(Relaxed))
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
