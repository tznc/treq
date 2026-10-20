//! Idempotency keys for user-initiated remote changes (`remote-idempotency.ts`;
//! prds/remote-development.md, "Retrying after network loss").
//!
//! The host uses the key to recognise a repeat of a command it already ran.
//! The key belongs to the user's action, not to a click: the same inputs,
//! retried before any confirmed outcome, reuse the same key, however long
//! that takes. A confirmed outcome ends the action, so the next try starts
//! a new one with a new key.

use std::{collections::HashMap, sync::Mutex};

use serde_json::Value;

use crate::lock_ext::LockExt;

/// `value` as JSON with object keys sorted, so property order never changes
/// a fingerprint.
pub fn canonical_json(value: &Value) -> String {
  fn sorted(value: &Value) -> Value {
    match value {
      Value::Object(map) => {
        let mut entries: Vec<_> = map.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let mut out = serde_json::Map::new();
        for (key, inner) in entries {
          out.insert(key.clone(), sorted(inner));
        }
        Value::Object(out)
      }
      Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
      other => other.clone(),
    }
  }
  serde_json::to_string(&sorted(value)).unwrap_or_default()
}

struct Pending {
  key: String,
  scope: String,
}

/// How an action's attempt ended, for [`ActionKeys::settle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
  Applied,
  AlreadyApplied,
  /// Whether it landed is unknown: the user may retry with the same key.
  Ambiguous,
}

/// `ActionIdempotencyKeys`: the keys of actions with no confirmed outcome.
pub struct ActionKeys {
  pending: Mutex<HashMap<String, Pending>>,
  new_id: Box<dyn Fn() -> String + Send + Sync>,
}

impl Default for ActionKeys {
  fn default() -> Self {
    Self::new(|| uuid::Uuid::new_v4().to_string())
  }
}

impl ActionKeys {
  pub fn new(new_id: impl Fn() -> String + Send + Sync + 'static) -> Self {
    Self {
      pending: Mutex::default(),
      new_id: Box::new(new_id),
    }
  }

  /// The key for an action with these inputs: the earlier key while an
  /// action with the same inputs has no confirmed outcome. `scope` names
  /// the endpoint generation or repository, so a key never crosses to
  /// another host.
  pub fn key_for(&self, prefix: &str, inputs: &[Value], scope: &str) -> String {
    let mut parts = vec![Value::from(scope), Value::from(prefix)];
    parts.extend(inputs.iter().cloned());
    let fingerprint = canonical_json(&Value::Array(parts));
    let mut pending = self.pending.lock_or_recover();
    if let Some(entry) = pending.get(&fingerprint) {
      return entry.key.clone();
    }
    let key = format!("{prefix}:{}", (self.new_id)());
    pending.insert(
      fingerprint,
      Pending {
        key: key.clone(),
        scope: scope.to_string(),
      },
    );
    key
  }

  /// Records how the attempt sent with `key` ended. Only a confirmed
  /// outcome ends the action. A dispatch that failed outright never
  /// settles, so its key is kept too: the error may hide a change that
  /// landed.
  pub fn settle(&self, key: &str, outcome: Outcome) {
    if outcome == Outcome::Ambiguous {
      return;
    }
    self.pending.lock_or_recover().retain(|_, e| e.key != key);
  }

  /// Drops the pending actions in `scope`, or all of them.
  pub fn release(&self, scope: Option<&str>) {
    self
      .pending
      .lock_or_recover()
      .retain(|_, e| scope.is_some_and(|s| e.scope != s));
  }
}

/// The app-wide keys (`remoteActionKeys`), shared by every window.
pub fn global() -> &'static ActionKeys {
  static KEYS: std::sync::OnceLock<ActionKeys> = std::sync::OnceLock::new();
  KEYS.get_or_init(ActionKeys::default)
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicUsize, Ordering};

  use serde_json::json;

  use super::*;

  fn counter() -> ActionKeys {
    let next = AtomicUsize::new(0);
    ActionKeys::new(move || format!("id{}", next.fetch_add(1, Ordering::SeqCst) + 1))
  }

  fn inputs(values: &[&str]) -> Vec<Value> {
    values.iter().map(|v| json!(v)).collect()
  }

  #[test]
  fn reuses_the_key_when_the_same_action_is_retried_after_an_ambiguous_result() {
    let keys = counter();
    let first = keys.key_for("commit", &inputs(&["/r", "ws", "msg"]), "");
    keys.settle(&first, Outcome::Ambiguous);
    assert_eq!(
      keys.key_for("commit", &inputs(&["/r", "ws", "msg"]), ""),
      first
    );
  }

  #[test]
  fn reuses_the_key_when_the_dispatch_failed_before_any_outcome() {
    let keys = counter();
    let first = keys.key_for("push", &inputs(&["/r", "ws"]), "");
    // No `settle`: the dispatch failed.
    assert_eq!(keys.key_for("push", &inputs(&["/r", "ws"]), ""), first);
  }

  #[test]
  fn starts_a_new_action_after_a_confirmed_outcome() {
    let keys = counter();
    let action = inputs(&["/r", "ws", "msg"]);
    let first = keys.key_for("commit", &action, "");
    keys.settle(&first, Outcome::Applied);
    let second = keys.key_for("commit", &action, "");
    assert_ne!(second, first);
    keys.settle(&second, Outcome::AlreadyApplied);
    assert_ne!(keys.key_for("commit", &action, ""), second);
  }

  #[test]
  fn gives_different_inputs_different_keys_and_keeps_each_pending_one() {
    let keys = counter();
    let a = keys.key_for("rebase", &inputs(&["/r", "ws", "main"]), "");
    let b = keys.key_for("rebase", &inputs(&["/r", "ws", "dev"]), "");
    assert_ne!(a, b);
    assert_eq!(
      keys.key_for("rebase", &inputs(&["/r", "ws", "main"]), ""),
      a
    );
  }

  #[test]
  fn fingerprints_object_inputs_independently_of_property_order() {
    let keys = counter();
    let a = keys.key_for("GitPush", &[json!({"repo": "/r", "workspace": "7"})], "");
    let b = keys.key_for("GitPush", &[json!({"workspace": "7", "repo": "/r"})], "");
    assert_eq!(a, b);
  }

  #[test]
  fn keeps_an_ambiguous_attempts_key_until_it_is_confirmed() {
    // Elapsed time does not show the first attempt never landed: nothing
    // here expires.
    let keys = counter();
    let first = keys.key_for("SplitCommit", &inputs(&["/r", "abc"]), "");
    keys.settle(&first, Outcome::Ambiguous);
    assert_eq!(
      keys.key_for("SplitCommit", &inputs(&["/r", "abc"]), ""),
      first
    );
    keys.settle(&first, Outcome::AlreadyApplied);
    assert_ne!(
      keys.key_for("SplitCommit", &inputs(&["/r", "abc"]), ""),
      first
    );
  }

  #[test]
  fn releases_pending_keys_by_scope_or_all_of_them() {
    let keys = counter();
    let a = keys.key_for("GitPush", &inputs(&["/r"]), "repo-a");
    let b = keys.key_for("GitPush", &inputs(&["/r"]), "repo-b");
    assert_ne!(a, b);
    keys.release(Some("repo-a"));
    assert_ne!(keys.key_for("GitPush", &inputs(&["/r"]), "repo-a"), a);
    assert_eq!(keys.key_for("GitPush", &inputs(&["/r"]), "repo-b"), b);
    keys.release(None);
    assert_ne!(keys.key_for("GitPush", &inputs(&["/r"]), "repo-b"), b);
  }

  #[test]
  fn does_not_build_keys_from_the_clock() {
    let keys = ActionKeys::new(|| "fixed".into());
    assert_eq!(
      keys.key_for("agent-start", &inputs(&["/r", "ws"]), ""),
      "agent-start:fixed"
    );
  }
}
