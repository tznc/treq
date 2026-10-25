//! Last-known results of host reads, so a screen opened again shows them at
//! once while it refetches (stale-while-revalidate), and a read already in
//! flight (a prefetch, or another screen's) is joined instead of sent twice.
//! Every read is an SSH exec that starts treq on the host, so each one saved
//! is a round trip and a process start.
//!
//! Keys are built by the caller and must name the machine and the request,
//! so one machine's, repository's or workspace's data never stands in for
//! another's.

use std::{
  any::Any,
  collections::HashMap,
  future::Future,
  sync::{Arc, Mutex},
};

use futures::future::{BoxFuture, FutureExt, Shared};

type Value = Arc<dyn Any + Send + Sync>;
type Fetch = Shared<BoxFuture<'static, Result<Value, String>>>;

/// Entries kept; the least recently used goes first.
const CAPACITY: usize = 64;

#[derive(Default)]
pub struct Reads {
  state: Mutex<State>,
}

#[derive(Default)]
struct State {
  entries: HashMap<String, Entry>,
  /// Orders uses (for eviction) and fetches (so an older fetch finishing
  /// late never replaces a newer result).
  tick: u64,
}

#[derive(Default)]
struct Entry {
  value: Option<Value>,
  /// The tick of the fetch `value` came from.
  value_tick: u64,
  used: u64,
  in_flight: Option<(u64, Fetch)>,
}

impl Reads {
  fn state(&self) -> std::sync::MutexGuard<'_, State> {
    self.state.lock().unwrap_or_else(|e| e.into_inner())
  }

  /// The last successful result for `key`, if any.
  pub fn last<T: Clone + 'static>(&self, key: &str) -> Option<T> {
    let state = self.state();
    state
      .entries
      .get(key)?
      .value
      .as_ref()?
      .downcast_ref::<T>()
      .cloned()
  }

  /// Runs `fetch` and remembers its result. With `join`, a fetch for `key`
  /// already in flight is awaited instead; without, a new one starts (a
  /// reload after a change must not get a result read before it).
  pub fn fetch<T, Fut>(
    &'static self,
    key: String,
    join: bool,
    fetch: impl FnOnce() -> Fut,
  ) -> impl Future<Output = Result<T, String>> + Send + 'static
  where
    T: Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<T, String>> + Send + 'static,
  {
    let shared = {
      let mut state = self.state();
      state.tick += 1;
      let tick = state.tick;
      let entry = state.entries.entry(key.clone()).or_default();
      entry.used = tick;
      match entry.in_flight.as_ref().filter(|_| join) {
        Some((_, running)) => running.clone(),
        None => {
          let run = fetch();
          let running = async move {
            let result = run.await.map(|value| Arc::new(value) as Value);
            self.finish(&key, tick, &result);
            result
          }
          .boxed()
          .shared();
          entry.in_flight = Some((tick, running.clone()));
          state.evict();
          running
        }
      }
    };
    async move {
      let value = shared.await?;
      value
        .downcast_ref::<T>()
        .cloned()
        .ok_or_else(|| "read cached with another type".to_string())
    }
  }

  fn finish(&self, key: &str, tick: u64, result: &Result<Value, String>) {
    let mut state = self.state();
    let Some(entry) = state.entries.get_mut(key) else {
      return;
    };
    if entry.in_flight.as_ref().is_some_and(|(t, _)| *t == tick) {
      entry.in_flight = None;
    }
    if let Ok(value) = result {
      if tick > entry.value_tick {
        entry.value = Some(value.clone());
        entry.value_tick = tick;
      }
    }
  }

  /// Forgets everything (sign-out, a forgotten host).
  pub fn clear(&self) {
    self.state().entries.clear();
  }
}

impl State {
  fn evict(&mut self) {
    while self.entries.len() > CAPACITY {
      let oldest = self
        .entries
        .iter()
        .filter(|(_, e)| e.in_flight.is_none())
        .min_by_key(|(_, e)| e.used)
        .map(|(k, _)| k.clone());
      match oldest {
        Some(key) => self.entries.remove(&key),
        None => break,
      };
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicUsize, Ordering};

  use futures::{channel::oneshot, executor::block_on};

  use super::*;

  fn reads() -> &'static Reads {
    Box::leak(Box::default())
  }

  #[test]
  fn remembers_the_last_result_and_joins_a_fetch_in_flight() {
    let reads = reads();
    let calls = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = oneshot::channel::<u32>();
    let counted = calls.clone();
    let first = reads.fetch("k".into(), true, move || {
      counted.fetch_add(1, Ordering::SeqCst);
      async move { rx.await.map_err(|e| e.to_string()) }
    });
    // A screen opening while the prefetch runs joins it.
    let counted = calls.clone();
    let second = reads.fetch::<u32, _>("k".into(), true, move || {
      counted.fetch_add(1, Ordering::SeqCst);
      async { Ok(0) }
    });
    assert_eq!(reads.last::<u32>("k"), None);
    tx.send(7).unwrap();
    assert_eq!(block_on(first), Ok(7));
    assert_eq!(block_on(second), Ok(7));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(reads.last::<u32>("k"), Some(7));
    assert_eq!(reads.last::<u32>("other"), None);
  }

  #[test]
  fn a_reload_does_not_join_and_an_older_fetch_never_wins() {
    let reads = reads();
    let (old_tx, old_rx) = oneshot::channel::<u32>();
    let old = reads.fetch("k".into(), true, move || async move {
      old_rx.await.map_err(|e| e.to_string())
    });
    let new = reads.fetch::<u32, _>("k".into(), false, || async { Ok(2) });
    assert_eq!(block_on(new), Ok(2));
    old_tx.send(1).unwrap();
    assert_eq!(block_on(old), Ok(1));
    assert_eq!(reads.last::<u32>("k"), Some(2));
  }

  #[test]
  fn failures_keep_the_last_result() {
    let reads = reads();
    assert_eq!(
      block_on(reads.fetch::<u32, _>("k".into(), true, || async { Ok(1) })),
      Ok(1)
    );
    let failed = reads.fetch::<u32, _>("k".into(), true, || async { Err("down".into()) });
    assert_eq!(block_on(failed), Err("down".into()));
    assert_eq!(reads.last::<u32>("k"), Some(1));
    reads.clear();
    assert_eq!(reads.last::<u32>("k"), None);
  }

  #[test]
  fn keeps_at_most_capacity_entries() {
    let reads = reads();
    for i in 0..CAPACITY + 10 {
      block_on(reads.fetch::<usize, _>(i.to_string(), true, move || async move { Ok(i) })).unwrap();
    }
    assert_eq!(reads.state().entries.len(), CAPACITY);
    assert_eq!(reads.last::<usize>("0"), None);
    assert_eq!(
      reads.last::<usize>(&(CAPACITY + 9).to_string()),
      Some(CAPACITY + 9)
    );
  }
}
