//! Reading ahead of a click: what the pointer rests on, or what an idle
//! moment suggests opening next, is read into the cache the click's own
//! read consults, so it draws in the first frame. A read made ahead never
//! changes what shows.

use std::{
  collections::VecDeque,
  time::{Duration, Instant},
};

use tauri_plugin_gpui::gpui::{Context, Global, Task};

use crate::jj::JjRevisionDiff;

/// How long the pointer rests on something before what it opens is read
/// ahead: passing over it on the way elsewhere reads nothing.
pub(in crate::gpui_shell) const HOVER_INTENT: Duration = Duration::from_millis(150);

/// How long a read made ahead stands in for the click's own; an older one
/// is dropped.
pub(super) const FRESH: Duration = Duration::from_secs(10);

/// The last few values read ahead, by key, with when each was read. The
/// one used last is kept longest.
pub(super) struct Lru<K, V> {
  cap: usize,
  entries: VecDeque<(K, Instant, V)>,
}

impl<K: PartialEq, V> Lru<K, V> {
  pub fn new(cap: usize) -> Self {
    Self {
      cap,
      entries: VecDeque::new(),
    }
  }

  pub fn insert(&mut self, key: K, at: Instant, value: V) {
    self.entries.retain(|(k, _, _)| *k != key);
    self.entries.push_back((key, at, value));
    while self.entries.len() > self.cap {
      self.entries.pop_front();
    }
  }

  /// `key`'s value, whatever its age, kept longest from now on.
  pub fn get(&mut self, key: &K) -> Option<&V> {
    let index = self.entries.iter().position(|(k, _, _)| k == key)?;
    let entry = self.entries.remove(index)?;
    self.entries.push_back(entry);
    self.entries.back().map(|(_, _, value)| value)
  }

  /// Takes `key`'s value out, whatever its age.
  pub fn take(&mut self, key: &K) -> Option<V> {
    let index = self.entries.iter().position(|(k, _, _)| k == key)?;
    self.entries.remove(index).map(|(_, _, value)| value)
  }

  /// `key`'s value if it was read less than [`FRESH`] before `now`.
  pub fn fresh(&self, key: &K, now: Instant) -> Option<&V> {
    self
      .entries
      .iter()
      .find(|(k, at, _)| k == key && now.duration_since(*at) < FRESH)
      .map(|(_, _, value)| value)
  }

  /// Whether `key` was read less than [`FRESH`] before `now`.
  pub fn has_fresh(&self, key: &K, now: Instant) -> bool {
    self
      .entries
      .iter()
      .any(|(k, at, _)| k == key && now.duration_since(*at) < FRESH)
  }

  /// Takes `key`'s value out if it was read less than [`FRESH`] before
  /// `now`.
  pub fn take_fresh(&mut self, key: &K, now: Instant) -> Option<V> {
    let index = self.entries.iter().position(|(k, _, _)| k == key)?;
    let (_, at, value) = self.entries.remove(index)?;
    (now.duration_since(at) < FRESH).then_some(value)
  }

  pub fn clear(&mut self) {
    self.entries.clear();
  }
}

/// What the pointer rests on, read ahead once it has rested there for
/// [`HOVER_INTENT`]; leaving it first reads nothing.
pub(in crate::gpui_shell) struct Intent<K> {
  key: Option<K>,
  _wait: Option<Task<()>>,
}

impl<K> Default for Intent<K> {
  fn default() -> Self {
    Self {
      key: None,
      _wait: None,
    }
  }
}

impl<K: PartialEq + 'static> Intent<K> {
  /// The pointer entered (`hovered`) or left `key`: `read` runs once it
  /// has rested there long enough.
  pub fn hover<V: 'static>(
    &mut self,
    key: K,
    hovered: bool,
    cx: &mut Context<V>,
    read: impl FnOnce(&mut V, &mut Context<V>) + 'static,
  ) {
    if !hovered {
      if self.key.as_ref() == Some(&key) {
        self.key = None;
        self._wait = None;
      }
      return;
    }
    if self.key.as_ref() == Some(&key) {
      return;
    }
    self.key = Some(key);
    self._wait = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(HOVER_INTENT).await;
      let _ = this.update(cx, read);
    }));
  }
}

/// Commits' diffs by repository and commit id, read ahead by hovering their
/// rows. A commit id names one tree and its parents, so its diff never goes
/// stale; the working copy's commit, which moves, is never kept.
pub(super) struct CommitDiffs(pub Lru<(String, String), JjRevisionDiff>);

impl Default for CommitDiffs {
  fn default() -> Self {
    Self(Lru::new(16))
  }
}

impl Global for CommitDiffs {}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn lru_drops_the_entry_used_least_recently() {
    let now = Instant::now();
    let mut lru = Lru::new(2);
    lru.insert("a", now, 1);
    lru.insert("b", now, 2);
    assert_eq!(lru.get(&"a"), Some(&1));
    lru.insert("c", now, 3);
    assert_eq!(lru.get(&"b"), None);
    assert_eq!(lru.get(&"a"), Some(&1));
    assert_eq!(lru.get(&"c"), Some(&3));
  }

  #[test]
  fn lru_takes_only_fresh_entries() {
    let now = Instant::now();
    let mut lru = Lru::new(2);
    lru.insert("a", now, 1);
    lru.insert("b", now, 2);
    assert!(lru.has_fresh(&"a", now + FRESH / 2));
    assert_eq!(lru.take_fresh(&"a", now + FRESH / 2), Some(1));
    assert_eq!(lru.take_fresh(&"a", now), None, "taken once");
    assert_eq!(lru.take_fresh(&"b", now + FRESH), None, "too old");
  }
}
