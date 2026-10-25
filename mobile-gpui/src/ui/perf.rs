//! Round 7 render measurements. All `#[ignore]`d; run with
//! `cargo test --lib mobile_render_ -- --ignored --nocapture --test-threads=1`.
//!
//! Each test prints `time/frame` (wall, secondary) and `allocs/frame` +
//! `bytes/frame` (exact, from the counting allocator below). For instruction
//! counts run one test under `valgrind --tool=callgrind` twice, with
//! `FRAMES=1` and `FRAMES=201`, and divide the difference by 200.
//!
//! The headless text system is not the phone's (no shaping cost), so these
//! numbers are element build + layout + prepaint + paint on this CPU, with
//! the crate at opt-level 1 and dependencies at opt-level 1 (the real dev
//! profile is 3 for dependencies). A phone is roughly 3-5x slower.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use futures::channel::mpsc;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
  div, px, size, AnyView, AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity,
  IntoElement, ParentElement as _, Pixels, Point, Render, Size, Styled as _, StyleRefinement,
  TestAppContext, Window, WindowBounds, WindowOptions,
};
use treq_lib::{
  core::workspaces::WorkspaceSummary,
  jj::{JjDiffHunk, JjFileChange, JjFileDiff, JjRevisionDiff},
};

use super::screens::{diff::DiffScreen, workspaces::WorkspacesScreen};
use super::terminal_view::{PtyEvent, TerminalView};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(layout.size(), Relaxed);
    LIVE.fetch_add(layout.size(), Relaxed);
    System.alloc(layout)
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    LIVE.fetch_sub(layout.size(), Relaxed);
    System.dealloc(ptr, layout)
  }
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(new, Relaxed);
    LIVE.fetch_add(new, Relaxed);
    LIVE.fetch_sub(layout.size(), Relaxed);
    System.realloc(ptr, layout, new)
  }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn frames() -> usize {
  std::env::var("FRAMES")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(50)
}

/// Runs `f` `n` times and prints per-call time and allocations.
fn measure(label: &str, n: usize, mut f: impl FnMut()) {
  // One warm-up so caches (line layout, element state) are primed.
  f();
  let (a0, b0) = (ALLOCS.load(Relaxed), BYTES.load(Relaxed));
  let start = Instant::now();
  for _ in 0..n {
    f();
  }
  let elapsed = start.elapsed();
  let (a1, b1) = (ALLOCS.load(Relaxed), BYTES.load(Relaxed));
  println!(
    "{label}: {:.1} us/frame, {} allocs/frame, {} bytes/frame ({n} frames)",
    elapsed.as_secs_f64() * 1e6 / n as f64,
    (a1 - a0) / n,
    (b1 - b0) / n
  );
}

/// A phone-sized window (Pixel-class, 412 x 892 dp) with `build` as content.
fn mount<V: Render>(
  cx: &mut TestAppContext,
  build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
  cx.update(super::theme::init);
  cx.update(|cx| {
    let bounds = Bounds {
      origin: Point::default(),
      size: size(px(412.), px(892.)),
    };
    let (window, content) = gpui_kit::open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
      },
      cx,
      build,
    )
    .expect("open test window");
    (window.into(), content)
  })
}

fn draw(cx: &mut TestAppContext, window: AnyWindowHandle) {
  cx.update_window(window, |_, window, cx| window.render_frame(cx))
    .unwrap();
}

fn summaries(n: usize) -> Vec<WorkspaceSummary> {
  (0..n)
    .map(|i| {
      let workspace = serde_json::from_value(serde_json::json!({
        "id": i as i64 + 1, "repo_path": "/r", "workspace_name": format!("ws-{i}"),
        "workspace_path": format!("ws-{i}"), "branch_name": format!("feature/branch-{i}"),
        "created_at": "2026-10-01T00:00:00Z", "refreshed_at": null,
        "metadata": null,
        // Every third workspace is stacked on the previous one.
        "target_branch": if i % 3 == 2 { Some(format!("feature/branch-{}", i - 1)) } else { None },
        "title": format!("Workspace number {i} with a realistic title"),
        "description": null, "moved_files": null,
        "not_on_remote": false, "sparse_patterns": null
      }))
      .unwrap();
      WorkspaceSummary {
        workspace,
        has_conflicts: i % 17 == 0,
        has_changes: i % 4 == 0,
        commits_ahead: i % 5,
        terminals: i % 3,
        last_activity_at: None,
      }
    })
    .collect()
}

/// A host view that redraws its child on demand, optionally behind a
/// cached boundary (what `Root` could do for the page).
struct Host {
  child: AnyView,
  cached: bool,
  ticks: usize,
}

impl Render for Host {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    let child = if self.cached {
      self
        .child
        .clone()
        .cached(StyleRefinement::default().size_full())
    } else {
      self.child.clone()
    };
    div().size_full().child(child)
  }
}

#[gpui_kit::test]
#[ignore]
fn mobile_render_workspaces_list(cx: &mut TestAppContext) {
  for n in [20usize, 100, 400] {
    let list = summaries(n);
    let (window, screen) = mount(cx, |window, cx| {
      cx.new(|cx| WorkspacesScreen::for_test(list, window, cx))
    });
    draw(cx, window);
    measure(&format!("workspaces list n={n} idle redraw"), frames(), || {
      draw(cx, window)
    });
    cx.update_window(window, |_, _, cx| {
      screen.update(cx, |s, cx| s.set_selecting([1, 2, 3], cx))
    })
    .unwrap();
    measure(&format!("workspaces list n={n} selecting"), frames(), || {
      draw(cx, window)
    });
  }
}

/// The page behind a cached boundary, when only the host changed.
#[gpui_kit::test]
#[ignore]
fn mobile_render_cached_boundary(cx: &mut TestAppContext) {
  for cached in [false, true] {
    let list = summaries(100);
    let (window, host) = mount(cx, |window, cx| {
      let screen = cx.new(|cx| WorkspacesScreen::for_test(list, window, cx));
      cx.new(|_| Host {
        child: screen.into(),
        cached,
        ticks: 0,
      })
    });
    draw(cx, window);
    measure(
      &format!("workspaces n=100 host notify, cached={cached}"),
      frames(),
      || {
        cx.update_window(window, |_, _, cx| {
          host.update(cx, |h, cx| {
            h.ticks += 1;
            cx.notify()
          })
        })
        .unwrap();
        draw(cx, window);
      },
    );
  }
}

fn hunk(lines: usize) -> JjDiffHunk {
  let lines: Vec<String> = (0..lines)
    .map(|i| match i % 4 {
      0 => format!("+    let value_{i} = compute(arg_{i}, other_{i});"),
      1 => format!("-    let old_{i} = legacy(arg_{i});"),
      _ => format!("     context line {i} of the surrounding function body"),
    })
    .collect();
  JjDiffHunk {
    id: "h".into(),
    header: "@@ -1,100 +1,100 @@ fn main()".into(),
    patch: lines.join("\n"),
    lines,
    conflict_style: Default::default(),
    conflict_regions: Vec::new(),
  }
}

#[gpui_kit::test]
#[ignore]
fn mobile_render_diff_screen(cx: &mut TestAppContext) {
  for n in [200usize, 2000, 10000] {
    let hunks: Vec<JjDiffHunk> = (0..n / 100).map(|_| hunk(100)).collect();
    let (window, _) = mount(cx, |_, cx| cx.new(|_| DiffScreen::for_test(hunks)));
    draw(cx, window);
    measure(
      &format!("diff screen {n} lines idle redraw"),
      frames().min(if n >= 10000 { 5 } else { 50 }),
      || draw(cx, window),
    );
  }
}

fn feed(tx: &mpsc::UnboundedSender<PtyEvent>, bytes: Vec<u8>) {
  tx.unbounded_send(PtyEvent::Output(bytes)).unwrap();
}

/// A full colored screen, as a TUI agent draws it.
fn colored_screen(rows: usize, cols: usize, seed: usize) -> Vec<u8> {
  let mut out = b"\x1b[H".to_vec();
  for r in 0..rows {
    let color = 31 + (r + seed) % 7;
    out.extend(format!("\x1b[{color}m").bytes());
    for c in 0..cols {
      if c % 11 == 0 {
        out.extend(format!("\x1b[{}m", 31 + (c + r + seed) % 7).bytes());
      }
      out.push(b'a' + ((r * 7 + c + seed) % 26) as u8);
    }
    out.extend(b"\x1b[0m\r\n");
  }
  out
}

#[gpui_kit::test]
#[ignore]
fn mobile_render_terminal_view(cx: &mut TestAppContext) {
  let (tx, rx) = mpsc::unbounded();
  let (window, _view) = mount(cx, |window, cx| {
    cx.new(|cx| TerminalView::new("t".into(), (51, 40), rx, |_, _, _| {}, window, cx))
  });
  feed(&tx, colored_screen(40, 51, 0));
  cx.run_until_parked();
  draw(cx, window);
  measure("terminal 51x40 full colored, unchanged redraw", frames(), || {
    draw(cx, window)
  });
  let mut seed = 0;
  measure("terminal 51x40 full repaint chunk + redraw", frames(), || {
    seed += 1;
    feed(&tx, colored_screen(40, 51, seed));
    cx.run_until_parked();
    draw(cx, window);
  });
  // A spinner tick: one row changes.
  measure("terminal 51x40 one row changed + redraw", frames(), || {
    seed += 1;
    feed(&tx, format!("\x1b[3;1H\x1b[32mtick {seed}\x1b[0m").into_bytes());
    cx.run_until_parked();
    draw(cx, window);
  });
}

// ---- Pure micro measurements (no window) ----

#[test]
#[ignore]
fn mobile_render_micro_palette() {
  let n = 100_000;
  let start = Instant::now();
  let mut sink = 0f32;
  for i in 0..n {
    sink += treq_ui::theme::palette(i % 2 == 0).primary.h;
  }
  println!(
    "Palette build: {:.0} ns/call (sink {sink})",
    start.elapsed().as_nanos() as f64 / n as f64
  );
}

fn big_diff(files: usize, lines_per_file: usize) -> JjRevisionDiff {
  let change = |i: usize| JjFileChange {
    path: format!("crates/some/deep/path/module_{i}/file_{i}.rs"),
    status: "M".into(),
    previous_path: None,
    changed_line_count: lines_per_file,
    diff_deferred: false,
    content_id: None,
  };
  JjRevisionDiff {
    committed_files: (0..files).map(change).collect(),
    hunks_by_file: (0..files)
      .map(|i| JjFileDiff {
        path: change(i).path,
        hunks: vec![hunk(lines_per_file)],
        conflict_style: Default::default(),
        conflict_regions: Vec::new(),
      })
      .collect(),
    uncommitted_files: Vec::new(),
    conflicted_files: Vec::new(),
    too_large_to_render: false,
    render_block_reason: None,
    working_copy_token: None,
  }
}

/// What `WorkspaceScreen::changes` does per committed file on every render.
#[test]
#[ignore]
fn mobile_render_micro_changes_tab_hunk_clone() {
  for (files, lines) in [(10usize, 50usize), (100, 50), (200, 50)] {
    let diff = big_diff(files, lines);
    measure(
      &format!("Changes tab hunk lookup+clone, {files} files x {lines} lines"),
      20,
      || {
        let mut total = 0;
        for change in &diff.committed_files {
          let hunks = diff
            .hunks_by_file
            .iter()
            .find(|f| f.path == change.path)
            .map(|f| f.hunks.clone())
            .unwrap_or_default();
          total += hunks.len();
        }
        std::hint::black_box(total);
      },
    );
  }
}

/// Building one terminal frame's rows (the part `Render` does per redraw).
#[test]
#[ignore]
fn mobile_render_micro_terminal_rows() {
  let mut parser = vt100::Parser::new(40, 51, 5000);
  parser.process(&colored_screen(40, 51, 0));
  measure("terminal::row x40 (51 cols, colored)", 200, || {
    for r in 0..40 {
      std::hint::black_box(crate::terminal::row(parser.screen(), r, 51, None));
    }
  });
}

/// Memory of a full scrollback at phone width.
#[test]
#[ignore]
fn mobile_render_micro_scrollback_memory() {
  for cols in [51u16, 80] {
    let mut parser = vt100::Parser::new(40, cols, 5000);
    let a0 = LIVE.load(Relaxed);
    for chunk in 0..6000 {
      parser.process(&colored_screen(1, cols as usize, chunk));
      parser.process(b"\r\n");
    }
    println!(
      "vt100 5000-line scrollback at {cols} cols: {} KB live",
      (LIVE.load(Relaxed) - a0) / 1024
    );
  }
}

#[allow(dead_code)]
fn _unused(_: Size<Pixels>, _: &mut Context<Host>) {}
