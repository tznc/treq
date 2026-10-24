//! A debug-build timeline of startup: with `TREQ_STARTUP_TRACE=1`, each
//! [`mark`] prints the milliseconds since the process started and since the
//! last mark, to stderr. Release builds compile it away.

#[cfg(debug_assertions)]
pub fn mark(step: &str) {
  use std::{
    sync::{Mutex, OnceLock},
    time::Instant,
  };
  static START: OnceLock<Instant> = OnceLock::new();
  static LAST: Mutex<Option<Instant>> = Mutex::new(None);
  static ON: OnceLock<bool> = OnceLock::new();
  let start = *START.get_or_init(Instant::now);
  if !*ON.get_or_init(|| std::env::var_os("TREQ_STARTUP_TRACE").is_some()) {
    return;
  }
  let now = Instant::now();
  let since = LAST
    .lock()
    .ok()
    .and_then(|mut last| last.replace(now))
    .unwrap_or(start);
  #[allow(clippy::print_stderr, reason = "the trace is its output")]
  {
    eprintln!(
      "[startup] {:>7.1} ms  (+{:>6.1})  {step}",
      (now - start).as_secs_f64() * 1e3,
      (now - since).as_secs_f64() * 1e3
    );
  }
}

#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn mark(_step: &str) {}
