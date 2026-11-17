//! The terminal pane under load: one maximized terminal streaming a full
//! screen of colored output, and eight terminals open at once with full
//! scrollback. `terminal_perf` (ignored) prints the frame costs:
//! `cargo test --features gpui-shell --lib terminal_perf -- --ignored
//! --nocapture --test-threads=1`.

use gpui::TestAppContext;

use super::changes_large::{frame, report, timed};
use super::*;

/// A line of build output: colors, bold, underline and wide characters,
/// different on each line so no two lines shape alike.
fn line(i: usize) -> String {
  format!(
    "\x1b[3{}m{i:>6}\x1b[0m \x1b[1;3{}m✓ passed\x1b[0m src/日本/mod_{i}.rs:{i}:7 \x1b[4mwarning\x1b[24m \x1b[4{}m{}\x1b[0m {}\r\n",
    i % 8,
    (i + 3) % 8,
    i % 8,
    "#".repeat(i % 7),
    "word ".repeat(10 + i % 30),
  )
}

fn lines(from: usize, n: usize) -> String {
  (from..from + n).map(line).collect()
}

/// The agent's terminal plus `shells` shells, in a large window.
fn pane(cx: &mut TestAppContext, shells: usize) -> (Harness, Arc<FakeHost>, Vec<String>) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  h.cx
    .simulate_resize(gpui::size(gpui::px(2560.), gpui::px(1440.)));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-body");
  for _ in 0..shells {
    h.cx.simulate_keystrokes("cmd-\\");
    h.cx.run_until_parked();
  }
  let ptys = host.ptys.lock().unwrap().clone();
  assert_eq!(ptys.len(), shells + 1);
  (h, host, ptys)
}

#[gpui::test]
#[ignore = "benchmark"]
fn terminal_perf_streaming(cx: &mut TestAppContext) {
  let (mut h, host, ptys) = pane(cx, 0);
  h.click("terminal-maximize");
  h.cx.run_until_parked();
  let pty = &ptys[0];
  host.output(pty, &lines(0, 3000));
  frame(&mut h);

  // Each burst scrolls a full screen of new output in, as `cat` of a
  // build log does, and draws it.
  let mut samples: Vec<_> = (0..40)
    .map(|n| timed(&mut h, |_| host.output(pty, &lines(10_000 + n * 80, 80))))
    .collect();
  report("stream: burst + frame", &mut samples);

  // The program wrote one character: an echo, a spinner tick.
  let mut samples: Vec<_> = (0..40)
    .map(|_| timed(&mut h, |_| host.output(pty, "x")))
    .collect();
  report("stream: one char + frame", &mut samples);

  let mut samples: Vec<_> = (0..40).map(|_| frame(&mut h)).collect();
  report("stream: full redraw", &mut samples);
}

#[gpui::test]
#[ignore = "benchmark"]
fn terminal_perf_many(cx: &mut TestAppContext) {
  let (mut h, host, ptys) = pane(cx, 7);
  for (i, pty) in ptys.iter().enumerate() {
    host.output(pty, &lines(i * 10_000, 2000));
  }
  frame(&mut h);

  // Output in one terminal while seven sit idle.
  let mut samples: Vec<_> = (0..40)
    .map(|n| {
      timed(&mut h, |_| {
        host.output(&ptys[0], &lines(100_000 + n * 40, 40))
      })
    })
    .collect();
  report("8 terms: burst in one", &mut samples);

  let mut samples: Vec<_> = (0..40)
    .map(|_| timed(&mut h, |_| host.output(&ptys[0], "x")))
    .collect();
  report("8 terms: one char in one", &mut samples);

  let mut samples: Vec<_> = (0..40).map(|_| frame(&mut h)).collect();
  report("8 terms: full redraw", &mut samples);

  // The window resized: every terminal reflows its scrollback.
  let mut samples: Vec<_> = (0..10)
    .map(|n| {
      let width = if n % 2 == 0 { 2200. } else { 2560. };
      timed(&mut h, |h| {
        h.cx
          .simulate_resize(gpui::size(gpui::px(width), gpui::px(1440.)))
      })
    })
    .collect();
  report("8 terms: resize", &mut samples);
}
