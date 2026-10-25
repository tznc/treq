//! What a workspace window costs while nobody touches it: frames drawn and
//! host reads over a stretch of idle time, focused and in
//! the background. The display is modelled as the platform runs it: an
//! animation frame is delivered at 60 Hz while the window is active and at
//! GPUI's 30 Hz cap while it is not. `idle_perf` (ignored) prints them:
//! `cargo test --features gpui-shell --lib idle_perf -- --ignored --nocapture`.

use std::{cell::Cell, time::Duration};

use gpui::{div, TestAppContext};

use super::*;

/// Wraps the screen to count the window's frames: the root is not cached,
/// so every frame renders it. The screen inside is cached, as in the shell.
struct Counted {
  screen: Entity<Screen>,
  frames: Rc<Cell<usize>>,
}

impl Render for Counted {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    self.frames.set(self.frames.get() + 1);
    div().size_full().child(Screen::element(&self.screen))
  }
}

pub(super) fn counted(
  cx: &mut TestAppContext,
  host: FakeHost,
  snapshot: ScreenSnapshot,
) -> (Harness, Arc<FakeHost>, Rc<Cell<usize>>) {
  let host = Arc::new(host);
  let sent = Rc::new(RefCell::new(Vec::new()));
  let sink: ActionSink = {
    let sent = sent.clone();
    Rc::new(move |action, _| sent.borrow_mut().push(action))
  };
  cx.update(crate::gpui_shell::theme::init);
  let popups = Rc::new(RefCell::new(Vec::new()));
  let popup_sink: crate::gpui_shell::popup::PopupSink = {
    let popups = popups.clone();
    Rc::new(move |request, _| popups.borrow_mut().push(request))
  };
  let frames = Rc::new(Cell::new(0));
  let (root, cx) = cx.add_window_view({
    let (host, frames) = (host.clone(), frames.clone());
    move |_, cx| Counted {
      screen: cx.new(|cx| Screen::new(snapshot, sink, popup_sink, host, cx)),
      frames,
    }
  });
  let screen = root.read_with(cx, |root, _| root.screen.clone());
  let mut h = Harness {
    cx: cx.clone(),
    screen,
    sent,
    popups,
  };
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
  h.cx.update(|window, _| window.activate_window());
  h.cx.run_until_parked();
  assert!(h.cx.update(|window, _| window.is_window_active()));
  (h, host, frames)
}

/// What `secs` of idle time cost.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct Idle {
  pub frames: usize,
  pub reads: usize,
}

impl Idle {
  fn per_sec(self, secs: u64) -> String {
    let s = secs as f64;
    format!(
      "frames/s={:>6.2}  reads/s={:>5.2}",
      self.frames as f64 / s,
      self.reads as f64 / s
    )
  }
}

/// Lets `secs` pass untouched, delivering animation frames as the display
/// would, and counts what ran.
pub(super) fn idle_for(h: &mut Harness, host: &FakeHost, frames: &Cell<usize>, secs: u64) -> Idle {
  let active = h.cx.update(|window, _| window.is_window_active());
  let tick = Duration::from_micros(16_667);
  let every = if active { 1 } else { 2 };
  let (frames_before, reads_before) = (frames.get(), host.calls().len());
  for step in 0..secs * 60 {
    h.cx.executor().advance_clock(tick);
    h.cx.run_until_parked();
    if step % every == 0 {
      h.cx.update(|window, cx| {
        window.simulate_next_frame(cx);
      });
      h.cx.run_until_parked();
    }
  }
  Idle {
    frames: frames.get() - frames_before,
    reads: host.calls().len() - reads_before,
  }
}

fn settle(h: &mut Harness, host: &FakeHost, frames: &Cell<usize>) {
  // The idle prefetch and first polls run in the first seconds.
  idle_for(h, host, frames, 10);
}

/// The Changes tab of a workspace ahead of its remote, whose header's sync
/// dot pulses.
fn changes(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>, Rc<Cell<usize>>) {
  let (mut h, host, frames) = counted(cx, changes_host(), changes_route());
  assert!(h.drawn("file-card-a.rs"));
  settle(&mut h, &host, &frames);
  (h, host, frames)
}

/// The agent's terminal, focused, in a workspace in sync with its remote,
/// so nothing else on screen animates.
fn terminal(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>, Rc<Cell<usize>>) {
  let mut host = terminal_host();
  if let Some(status) = host.loaded.status.as_mut() {
    status.remote_sync = crate::core::RemoteSyncStatus::InSync;
  }
  let (mut h, host, frames) = counted(cx, host, terminal_route(serde_json::json!({})));
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  host.output(&agent_pty(), "$ ");
  h.click("terminal-body");
  settle(&mut h, &host, &frames);
  (h, host, frames)
}

/// A pulsing dot needs no more than a few frames a second to look smooth.
#[gpui::test]
fn an_idle_workspace_draws_a_few_frames_a_second(cx: &mut TestAppContext) {
  let (mut h, host, frames) = changes(cx);
  let idle = idle_for(&mut h, &host, &frames, 10);
  assert!(idle.frames <= 10 * 15, "{idle:?}");
}

/// A focused terminal in a window in the background shows a steady cursor,
/// and nothing wakes for its blink.
#[gpui::test]
fn a_background_terminal_does_not_blink(cx: &mut TestAppContext) {
  let (mut h, host, frames) = terminal(cx);
  h.cx.deactivate_window();
  idle_for(&mut h, &host, &frames, 2);
  let idle = idle_for(&mut h, &host, &frames, 10);
  assert_eq!(idle.frames, 0, "{idle:?}");
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn idle_perf(cx: &mut TestAppContext) {
  const SECS: u64 = 30;
  let (mut h, host, frames) = changes(cx);
  let focused = idle_for(&mut h, &host, &frames, SECS);
  println!("changes, focused     {}", focused.per_sec(SECS));
  h.cx.deactivate_window();
  let background = idle_for(&mut h, &host, &frames, SECS);
  println!("changes, background  {}", background.per_sec(SECS));
}

#[gpui::test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn idle_terminal_perf(cx: &mut TestAppContext) {
  const SECS: u64 = 30;
  let (mut h, host, frames) = terminal(cx);
  let focused = idle_for(&mut h, &host, &frames, SECS);
  println!("terminal, focused    {}", focused.per_sec(SECS));
  h.cx.deactivate_window();
  let background = idle_for(&mut h, &host, &frames, SECS);
  println!("terminal, background {}", background.per_sec(SECS));
}
