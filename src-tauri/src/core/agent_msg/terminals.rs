//! The live state of each agent terminal the hub delivers to: its screen,
//! kept from the PTY's output stream, and when the user last typed in it.

use super::delivery::SUBMIT_DELAY;
use super::hub::Terminals;
use super::idle::{self, Harness, Screen};
use crate::lock_ext::LockExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Types text into a PTY: `(pty_id, text)`.
pub type Writer = Box<dyn Fn(&str, &str) -> Result<(), String> + Send + Sync>;

struct Tracked {
  parser: Arc<Mutex<vt100::Parser>>,
  last_key: Mutex<Option<Instant>>,
  _output: crate::pty_stream::Subscription,
}

#[derive(Default)]
pub struct ScreenTracker {
  tracked: Mutex<HashMap<String, Arc<Tracked>>>,
  writer: OnceLock<Writer>,
  /// Waits between the wake line and its Enter; a test sets it to zero.
  submit_delay: Mutex<Option<Duration>>,
}

impl ScreenTracker {
  pub fn new() -> Self {
    Self::default()
  }

  /// Starts reading `pty_id`'s output into a `cols`×`rows` screen. Tracking
  /// a PTY again keeps its screen.
  pub fn track(&self, pty_id: &str, cols: u16, rows: u16) {
    let mut tracked = self.tracked.lock_or_recover();
    if tracked.contains_key(pty_id) {
      return;
    }
    let parser = Arc::new(Mutex::new(crate::core::pty_daemon::new_screen(cols, rows)));
    let sink = parser.clone();
    let output = crate::pty_stream::subscribe(
      pty_id,
      Arc::new(move |bytes: &[u8]| sink.lock_or_recover().process(bytes)),
    );
    tracked.insert(
      pty_id.to_string(),
      Arc::new(Tracked {
        parser,
        last_key: Mutex::new(None),
        _output: output,
      }),
    );
  }

  pub fn untrack(&self, pty_id: &str) {
    self.tracked.lock_or_recover().remove(pty_id);
  }

  pub fn resize(&self, pty_id: &str, cols: u16, rows: u16) {
    let Some(tracked) = self.get(pty_id) else {
      return;
    };
    let nonzero =
      |value: u16| std::num::NonZeroU16::new(value).unwrap_or(std::num::NonZeroU16::MIN);
    tracked
      .parser
      .lock_or_recover()
      .set_size(nonzero(rows), nonzero(cols));
  }

  fn get(&self, pty_id: &str) -> Option<Arc<Tracked>> {
    self.tracked.lock_or_recover().get(pty_id).cloned()
  }

  /// The user typed into `pty_id` just now.
  pub fn note_key(&self, pty_id: &str) {
    if let Some(tracked) = self.get(pty_id) {
      *tracked.last_key.lock_or_recover() = Some(Instant::now());
    }
  }

  /// How treq types into PTYs; set once by the app.
  pub fn set_writer(&self, writer: Writer) {
    if self.writer.set(writer).is_err() {
      log::warn!("agent msg: the PTY writer was already set");
    }
  }

  #[cfg(test)]
  fn set_submit_delay(&self, delay: Duration) {
    *self.submit_delay.lock_or_recover() = Some(delay);
  }
}

impl Terminals for ScreenTracker {
  fn screen(&self, pty_id: &str, harness: Harness) -> Option<Screen> {
    let tracked = self.get(pty_id)?;
    let parser = tracked.parser.lock_or_recover();
    Some(idle::classify(harness, parser.screen()))
  }

  fn since_last_key(&self, pty_id: &str) -> Option<Duration> {
    let tracked = self.get(pty_id)?;
    let last = *tracked.last_key.lock_or_recover();
    last.map(|at| at.elapsed())
  }

  fn type_line(&self, pty_id: &str, line: &str) -> Result<(), String> {
    let writer = self
      .writer
      .get()
      .ok_or("treq cannot type into terminals yet")?;
    writer(pty_id, line)?;
    let delay = (*self.submit_delay.lock_or_recover()).unwrap_or(SUBMIT_DELAY);
    std::thread::sleep(delay);
    writer(pty_id, "\r")
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn idle_claude() -> &'static [u8] {
    b"\x1b[2J\x1b[H\xe2\x95\xad\xe2\x94\x80\xe2\x95\xae\r\n\xe2\x94\x82 > \r\n\xe2\x95\xb0\xe2\x94\x80\xe2\x95\xaf\r\n  ? for shortcuts\r\n"
  }

  #[test]
  fn reads_the_screen_from_the_pty_output() {
    let tracker = ScreenTracker::new();
    tracker.track("msg-track-1", 80, 24);
    assert_eq!(
      tracker.screen("msg-track-1", Harness::Claude),
      Some(Screen::Busy)
    );
    crate::pty_stream::publish("msg-track-1", idle_claude());
    assert_eq!(
      tracker.screen("msg-track-1", Harness::Claude),
      Some(Screen::IdleEmpty)
    );
    crate::pty_stream::publish("msg-track-1", b"\x1b[2;5Hdraft");
    assert_eq!(
      tracker.screen("msg-track-1", Harness::Claude),
      Some(Screen::IdleTyped)
    );
  }

  #[test]
  fn an_untracked_terminal_has_no_screen() {
    let tracker = ScreenTracker::new();
    assert_eq!(tracker.screen("msg-track-2", Harness::Claude), None);
    tracker.track("msg-track-2", 80, 24);
    tracker.untrack("msg-track-2");
    crate::pty_stream::publish("msg-track-2", idle_claude());
    assert_eq!(tracker.screen("msg-track-2", Harness::Claude), None);
  }

  #[test]
  fn tracking_again_keeps_the_screen() {
    let tracker = ScreenTracker::new();
    tracker.track("msg-track-3", 80, 24);
    crate::pty_stream::publish("msg-track-3", idle_claude());
    tracker.track("msg-track-3", 80, 24);
    assert_eq!(
      tracker.screen("msg-track-3", Harness::Claude),
      Some(Screen::IdleEmpty)
    );
  }

  #[test]
  fn remembers_the_last_keystroke() {
    let tracker = ScreenTracker::new();
    tracker.track("msg-track-4", 80, 24);
    assert_eq!(tracker.since_last_key("msg-track-4"), None);
    tracker.note_key("msg-track-4");
    assert!(tracker.since_last_key("msg-track-4").unwrap() < Duration::from_secs(1));
    tracker.note_key("msg-untracked");
    assert_eq!(tracker.since_last_key("msg-untracked"), None);
  }

  #[test]
  fn types_the_line_then_enter_through_the_writer() {
    let tracker = ScreenTracker::new();
    tracker.set_submit_delay(Duration::ZERO);
    let typed = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let sink = typed.clone();
    tracker.set_writer(Box::new(move |pty, text| {
      sink.lock().unwrap().push((pty.into(), text.into()));
      Ok(())
    }));
    tracker.type_line("msg-track-5", "[treq] hi").unwrap();
    assert_eq!(
      typed.lock().unwrap().as_slice(),
      &[
        ("msg-track-5".to_string(), "[treq] hi".to_string()),
        ("msg-track-5".to_string(), "\r".to_string())
      ]
    );
  }

  #[test]
  fn typing_without_a_writer_is_an_error() {
    let tracker = ScreenTracker::new();
    assert!(tracker.type_line("msg-track-6", "x").is_err());
  }

  #[test]
  fn resize_changes_the_screen_size() {
    let tracker = ScreenTracker::new();
    tracker.track("msg-track-7", 80, 24);
    tracker.resize("msg-track-7", 120, 40);
    let tracked = tracker.tracked.lock().unwrap();
    let size = tracked["msg-track-7"]
      .parser
      .lock()
      .unwrap()
      .screen()
      .size();
    assert_eq!((u16::from(size.0), u16::from(size.1)), (40, 120));
  }
}
