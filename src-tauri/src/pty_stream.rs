//! Where a PTY's output goes in process: straight from the reader thread to
//! whoever drew the terminal, as the bytes read. It skips the UTF-8 decode,
//! the JSON string and the Tauri event bus the output used to take.

use std::{
  collections::HashMap,
  sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, OnceLock,
  },
};

use crate::lock_ext::LockExt;

/// Receives each chunk of a session's output, on the thread that read it.
pub type Sink = Arc<dyn Fn(&[u8]) + Send + Sync>;

type Sinks = Mutex<HashMap<String, Vec<(u64, Sink)>>>;

fn sinks() -> &'static Sinks {
  static SINKS: OnceLock<Sinks> = OnceLock::new();
  SINKS.get_or_init(Mutex::default)
}

/// Stops delivery to a [`subscribe`]d sink when dropped.
pub struct Subscription {
  id: String,
  token: u64,
}

impl Drop for Subscription {
  fn drop(&mut self) {
    let mut sinks = sinks().lock_or_recover();
    if let Some(list) = sinks.get_mut(&self.id) {
      list.retain(|(token, _)| *token != self.token);
      if list.is_empty() {
        sinks.remove(&self.id);
      }
    }
  }
}

/// Delivers session `id`'s output to `sink` from now on.
pub fn subscribe(id: &str, sink: Sink) -> Subscription {
  static NEXT: AtomicU64 = AtomicU64::new(0);
  let token = NEXT.fetch_add(1, Ordering::Relaxed);
  sinks()
    .lock_or_recover()
    .entry(id.to_string())
    .or_default()
    .push((token, sink));
  Subscription {
    id: id.to_string(),
    token,
  }
}

/// Hands a chunk of session `id`'s output to its sinks. Output nobody
/// listens to is dropped, as an event nobody listened for was.
pub fn publish(id: &str, bytes: &[u8]) {
  let targets: Vec<Sink> = match sinks().lock_or_recover().get(id) {
    Some(list) => list.iter().map(|(_, sink)| sink.clone()).collect(),
    None => return,
  };
  for sink in targets {
    sink(bytes);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn delivers_until_the_subscription_drops() {
    let got = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink = got.clone();
    let subscription = subscribe(
      "stream-test",
      Arc::new(move |bytes| sink.lock().unwrap().extend_from_slice(bytes)),
    );
    publish("stream-test", b"ab");
    publish("stream-other", b"x");
    drop(subscription);
    publish("stream-test", b"cd");
    assert_eq!(got.lock().unwrap().as_slice(), b"ab");
  }
}
