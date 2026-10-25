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

/// A forced full frame (`refresh` invalidates cached views too).
fn draw(cx: &mut TestAppContext, window: AnyWindowHandle) {
  cx.update_window(window, |_, window, cx| window.render_frame(cx))
    .unwrap();
}

/// A frame as the platform draws it after a `notify`: views not notified
/// and behind a cached boundary are reused.
fn redraw(cx: &mut TestAppContext, window: AnyWindowHandle) {
  cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
    .unwrap();
}

fn rows_env(default: &[usize]) -> Vec<usize> {
  std::env::var("ROWS")
    .ok()
    .map(|v| v.split(',').filter_map(|n| n.parse().ok()).collect())
    .unwrap_or_else(|| default.to_vec())
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
        .into_any_element()
    } else {
      self.child.clone().into_any_element()
    };
    div().size_full().child(child)
  }
}

/// Which part of a list row costs the layout time: rows rebuilt from
/// parts, `n` of them in a scrolling column like the screens use.
struct Variant {
  n: usize,
  kind: &'static str,
}

impl Render for Variant {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    use gpui_kit::component::{h_flex, v_flex};
    use gpui_kit::prelude::*;
    use gpui_kit::{ParentElement as _, StatefulInteractiveElement as _};
    let kind = self.kind;
    let rows = (0..self.n).map(|i| -> gpui_kit::AnyElement {
      match kind {
        "production-row" => super::widgets::row_full(
          gpui_kit::SharedString::from(format!("r{i}")),
          None,
          format!("Workspace number {i}"),
          Some(format!("feature/branch-{i}").into()),
          Some(h_flex().gap_2().child("3").into_any_element()),
          cx,
        )
        .into_any_element(),
        "hold-wrapped" => super::hold::hold(
          ("h", i as u64),
          super::widgets::row_full(
            gpui_kit::SharedString::from(format!("r{i}")),
            None,
            format!("Workspace number {i}"),
            Some(format!("feature/branch-{i}").into()),
            Some(h_flex().gap_2().child("3").into_any_element()),
            cx,
          ),
          |_, _| {},
        )
        .into_any_element(),
        "two-text-in-hflex" => h_flex()
          .id(("t", i))
          .px_4()
          .py_2()
          .gap_3()
          .child(v_flex().flex_1().min_w_0().child(div().truncate().child("Workspace number"))
            .child(div().truncate().child("feature/branch")))
          .into_any_element(),
        "two-text-no-truncate" => h_flex()
          .id(("n", i))
          .px_4()
          .py_2()
          .gap_3()
          .child(v_flex().flex_1().min_w_0().child(div().child("Workspace number"))
            .child(div().child("feature/branch")))
          .into_any_element(),
        "two-text-nowrap" => h_flex()
          .id(("w", i))
          .px_4()
          .py_2()
          .gap_3()
          .child(v_flex().flex_1().min_w_0().child(div().whitespace_nowrap().overflow_hidden().child("Workspace number"))
            .child(div().whitespace_nowrap().overflow_hidden().child("feature/branch")))
          .into_any_element(),
        "one-truncate" => div().id(("o", i)).h(px(56.)).child(div().truncate().child("Workspace number")).into_any_element(),
        "two-truncate-vflex-only" => v_flex().id(("v", i)).child(div().truncate().child("Workspace number"))
            .child(div().truncate().child("feature/branch")).into_any_element(),
        // Block (not flex) column for the two texts.
        "block-column-in-hflex" => h_flex()
          .id(("a", i))
          .px_4()
          .py_2()
          .gap_3()
          .child(div().flex_1().min_w_0().child(div().truncate().child("Workspace number"))
            .child(div().truncate().child("feature/branch")))
          .into_any_element(),
        // Whole row a block container; texts truncate; chevron absolute-free.
        "block-row" => div()
          .id(("b", i))
          .px_4()
          .py_2()
          .child(div().truncate().child("Workspace number"))
          .child(div().truncate().child("feature/branch"))
          .into_any_element(),
        // Fixed width texts (what a measured layout would settle on).
        "fixed-width-texts-in-hflex" => h_flex()
          .id(("c", i))
          .px_4()
          .py_2()
          .gap_3()
          .child(v_flex().flex_1().min_w_0().child(div().w(px(300.)).h(px(20.)).truncate().child("Workspace number"))
            .child(div().w(px(300.)).h(px(20.)).truncate().child("feature/branch")))
          .into_any_element(),
        // Single text node with a newline-free combined label.
        "one-text-in-hflex" => h_flex()
          .id(("d", i))
          .px_4()
          .py_2()
          .gap_3()
          .child(div().flex_1().min_w_0().truncate().child("Workspace number"))
          .into_any_element(),
        // Block text column; the chevron is out of flow.
        "absolute-chevron-row" => div()
          .id(("e", i))
          .relative()
          .px_4()
          .py_2()
          .pr(px(40.))
          .child(div().truncate().child("Workspace number"))
          .child(div().truncate().child("feature/branch"))
          .child(div().absolute().right_4().top_4().child("›"))
          .into_any_element(),
        "one-text" => div().id(("o", i)).h(px(56.)).child("Workspace number").into_any_element(),
        "fixed-height-row" => h_flex()
          .id(("f", i))
          .h(px(56.))
          .px_4()
          .gap_3()
          .child(v_flex().flex_1().min_w_0().child(div().truncate().child("Workspace number"))
            .child(div().truncate().child("feature/branch")))
          .into_any_element(),
        _ => unreachable!(),
      }
    });
    div().size_full().child(
      v_flex()
        .id("scroller")
        .size_full()
        .overflow_y_scroll()
        .child(v_flex().children(rows)),
    )
  }
}

/// The production row in a `uniform_list`: only the rows on screen are built.
struct Virtual {
  n: usize,
}

impl Render for Virtual {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    use gpui_kit::component::h_flex;
    use gpui_kit::prelude::*;
    let n = self.n;
    div().size_full().child(
      gpui_kit::uniform_list(
        "rows",
        n,
        cx.processor(move |_this, range: std::ops::Range<usize>, _window, cx| {
          range
            .map(|i| {
              super::widgets::row_full(
                gpui_kit::SharedString::from(format!("r{i}")),
                None,
                format!("Workspace number {i}"),
                Some(format!("feature/branch-{i}").into()),
                Some(h_flex().gap_2().child("3").into_any_element()),
                cx,
              )
            })
            .collect::<Vec<_>>()
        }),
      )
      .size_full(),
    )
  }
}

#[gpui_kit::test]
#[ignore]
fn mobile_render_virtual_list(cx: &mut TestAppContext) {
  for n in rows_env(&[20, 100, 400, 2000]) {
    let (window, _) = mount(cx, |_, cx| cx.new(|_| Virtual { n }));
    draw(cx, window);
    measure(&format!("uniform_list of production rows, n={n}"), frames(), || draw(cx, window));
  }
}

#[gpui_kit::test]
#[ignore]
fn mobile_render_row_variants(cx: &mut TestAppContext) {
  let only = std::env::var("KIND").ok();
  for kind in ["absolute-chevron-row", "block-column-in-hflex", "block-row", "fixed-width-texts-in-hflex", "one-text-in-hflex", "one-text", "one-truncate", "two-truncate-vflex-only", "two-text-no-truncate", "two-text-nowrap", "two-text-in-hflex", "fixed-height-row", "production-row", "hold-wrapped"] {
    if only.as_deref().is_some_and(|k| k != kind) {
      continue;
    }
    let (window, _) = mount(cx, |_, cx| cx.new(|_| Variant { n: rows_env(&[100])[0], kind }));
    draw(cx, window);
    measure(&format!("100 x {kind}"), frames(), || draw(cx, window));
  }
}

#[gpui_kit::test]
#[ignore]
fn mobile_render_workspaces_list(cx: &mut TestAppContext) {
  for n in rows_env(&[20, 100, 400]) {
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
/// A page with a spinner over it (a sheet's Create button while it runs,
/// the refresh button while loading): every animation frame redraws the
/// window, so the page below re-renders unless it sits behind a cache.
struct SpinnerOverPage {
  page: AnyView,
  cached: bool,
  spinner: Entity<SpinnerSheet>,
}

struct SpinnerSheet;

impl Render for SpinnerSheet {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    div().absolute().bottom_0().child(gpui_kit::component::spinner::Spinner::new())
  }
}

impl Render for SpinnerOverPage {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    let page = if self.cached {
      self.page.clone().cached(StyleRefinement::default().size_full()).into_any_element()
    } else {
      self.page.clone().into_any_element()
    };
    div().size_full().child(page).child(self.spinner.clone())
  }
}

#[gpui_kit::test]
#[ignore]
fn mobile_render_spinner_over_page(cx: &mut TestAppContext) {
  for cached in [false, true] {
    if std::env::var("CACHED").is_ok_and(|v| (v == "1") != cached) {
      continue;
    }
    let list = summaries(rows_env(&[100])[0]);
    let (window, _host) = mount(cx, |window, cx| {
      let page = cx.new(|cx| WorkspacesScreen::for_test(list, window, cx));
      let spinner = cx.new(|_| SpinnerSheet);
      cx.new(|_| SpinnerOverPage { page: page.into(), cached, spinner })
    });
    draw(cx, window);
    measure(&format!("spinner animation frame over n-row page, cached={cached}"), frames(), || {
      cx.update_window(window, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
      })
      .unwrap();
    });
  }
}

#[gpui_kit::test]
#[ignore]
fn mobile_render_cached_boundary(cx: &mut TestAppContext) {
  for cached in [false, true] {
    if std::env::var("CACHED").is_ok_and(|v| (v == "1") != cached) {
      continue;
    }
    let list = summaries(rows_env(&[100])[0]);
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
        redraw(cx, window);
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

/// The diff body as one styled text per hunk (colors as highlight runs)
/// instead of one `div` per line, to see what the per-line divs cost.
struct DiffAsText {
  hunks: Vec<JjDiffHunk>,
}

impl Render for DiffAsText {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    use gpui_kit::component::{v_flex, ActiveTheme as _};
    use gpui_kit::prelude::*;
    use gpui_kit::{HighlightStyle, StyledText};
    let theme = cx.theme();
    let (add, del) = (theme.success, theme.danger);
    div().size_full().child(
      v_flex().id("diff").size_full().overflow_y_scroll().children(self.hunks.iter().map(|hunk| {
        let mut text = String::new();
        let mut runs = Vec::new();
        for line in &hunk.lines {
          let start = text.len();
          text.push_str(line);
          text.push('\n');
          let color = match super::screens::diff::line_kind(line) {
            super::screens::diff::LineKind::Added => Some(add),
            super::screens::diff::LineKind::Removed => Some(del),
            super::screens::diff::LineKind::Context => None,
          };
          if let Some(color) = color {
            runs.push((start..text.len() - 1, HighlightStyle { color: Some(color), ..Default::default() }));
          }
        }
        div().px_2().whitespace_nowrap().child(StyledText::new(text).with_highlights(runs))
      })),
    )
  }
}

#[gpui_kit::test]
#[ignore]
fn mobile_render_diff_as_text(cx: &mut TestAppContext) {
  for n in rows_env(&[200, 2000, 10000]) {
    let hunks: Vec<JjDiffHunk> = (0..n / 100).map(|_| hunk(100)).collect();
    let (window, _) = mount(cx, |_, cx| cx.new(|_| DiffAsText { hunks }));
    draw(cx, window);
    measure(&format!("diff as one text per hunk, {n} lines"), frames().min(50), || draw(cx, window));
  }
}

#[gpui_kit::test]
#[ignore]
fn mobile_render_diff_screen(cx: &mut TestAppContext) {
  for n in rows_env(&[200, 2000, 10000]) {
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
      let line: String = (0..cols as usize).map(|c| (b'a' + ((c + chunk) % 26) as u8) as char).collect();
      parser.process(format!("\x1b[3{}m{line}\x1b[0m\r\n", 1 + chunk % 7).as_bytes());
    }
    println!(
      "vt100 5000-line scrollback at {cols} cols: {} KB live",
      (LIVE.load(Relaxed) - a0) / 1024
    );
  }
}

#[allow(dead_code)]
fn _unused(_: Size<Pixels>, _: &mut Context<Host>) {}

/// Touch drags reach the terminal as pixel scroll deltas; each is rounded
/// to whole lines on its own.
#[gpui_kit::test]
#[ignore]
fn mobile_render_terminal_slow_drag_scrolls_nothing(cx: &mut TestAppContext) {
  use gpui_kit::{point, ScrollDelta};
  let (tx, rx) = mpsc::unbounded();
  let (window, view) = mount(cx, |window, cx| {
    cx.new(|cx| TerminalView::new("t".into(), (51, 40), rx, |_, _, _| {}, window, cx))
  });
  let lines: String = (0..300).map(|i| format!("line {i}\r\n")).collect();
  feed(&tx, lines.into_bytes());
  cx.run_until_parked();
  draw(cx, window);
  for per_event_px in [3.0f32, 6.0, 8.0, 9.0, 17.0] {
    cx.update_window(window, |_, w, cx| {
      view.update(cx, |v, _| assert_eq!(v.scrollback_offset(), 0));
      for _ in 0..20 {
        w.dispatch_event(
          gpui_kit::PlatformInput::ScrollWheel(gpui_kit::ScrollWheelEvent {
            position: point(px(200.), px(300.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(per_event_px))),
            ..Default::default()
          }),
          cx,
        );
      }
    })
    .unwrap();
    let offset = cx.update_window(window, |_, _, cx| view.read(cx).scrollback_offset()).unwrap();
    println!(
      "20 touch-move events of {per_event_px} px (= {:.0} px total, {:.1} lines): scrolled back {offset} lines",
      20.0 * per_event_px,
      20.0 * per_event_px / 17.0
    );
    cx.update_window(window, |_, w, cx| {
      view.update(cx, |v, cx| v.write(Vec::new(), cx));
      let _ = w;
    })
    .unwrap();
    // `write` returns to the live screen only when scrolled back.
  }
}

/// `Root::save_snapshot` runs on the UI thread on every page push/pop.
#[test]
#[ignore]
fn mobile_render_micro_snapshot_write() {
  use treq_lib::core::remote_records::{self as records, SessionEndpoint, SessionSnapshot, SnapshotWorkspace};
  let dir = std::env::temp_dir().join(format!("treq-perf-{}", std::process::id()));
  std::fs::create_dir_all(&dir).unwrap();
  let db = treq_lib::db::Database::new(dir.join("treq.db")).unwrap();
  db.init().unwrap();
  let snapshot = SessionSnapshot {
    endpoint: SessionEndpoint::Managed,
    repo_path: Some("/home/dev/src/treq".into()),
    workspace: Some(SnapshotWorkspace { id: "12".into(), name: "ws-12".into() }),
  };
  let n = 300;
  let start = Instant::now();
  for _ in 0..n {
    records::save_session_snapshot(&db, Some(&snapshot));
  }
  println!("save_session_snapshot: {:.0} us/write (tmpfs-or-disk of this machine, WAL+NORMAL)", start.elapsed().as_secs_f64() * 1e6 / n as f64);
}
