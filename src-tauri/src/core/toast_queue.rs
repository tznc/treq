//! A window's toasts, as `toastStore` keeps them: in the order they came,
//! each expiring after a while, sooner without an action button.
//!
//! Toasts come from two places while React still runs: the app's own
//! (pushed here) and React's store (replaced wholesale on each change).
//! Both show in one list, in arrival order.

use std::time::Duration;

/// How long a toast shows without an action (`addToast`).
pub const PLAIN_DURATION: Duration = Duration::from_millis(5000);
/// How long a toast with an action button shows.
pub const ACTION_DURATION: Duration = Duration::from_millis(12000);

/// The prefix of the ids the app's own toasts get; React's never have it.
pub const OWN_PREFIX: &str = "treq-";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
  Success,
  Error,
  Info,
  Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedToast {
  pub id: String,
  pub title: String,
  pub description: Option<String>,
  pub kind: ToastKind,
  /// The action button's label.
  pub action: Option<String>,
}

impl QueuedToast {
  /// How long it shows before it expires.
  pub fn duration(&self) -> Duration {
    if self.action.is_some() {
      ACTION_DURATION
    } else {
      PLAIN_DURATION
    }
  }
}

/// One window's toasts.
#[derive(Debug, Clone, Default)]
pub struct ToastQueue {
  next: u64,
  seq: u64,
  /// Each toast and when it arrived.
  toasts: Vec<(u64, QueuedToast)>,
}

impl ToastQueue {
  /// Adds one of the app's own toasts; returns it with its new id.
  pub fn push(
    &mut self,
    title: &str,
    description: Option<&str>,
    kind: ToastKind,
    action: Option<&str>,
  ) -> QueuedToast {
    self.next += 1;
    let toast = QueuedToast {
      id: format!("{OWN_PREFIX}{}", self.next),
      title: title.into(),
      description: description.filter(|d| !d.is_empty()).map(str::to_string),
      kind,
      action: action.map(str::to_string),
    };
    self.seq += 1;
    self.toasts.push((self.seq, toast.clone()));
    toast
  }

  /// Takes React's whole list: React's toasts not in it go, new ones join
  /// at the end, and the app's own stay where they are.
  pub fn set_react(&mut self, react: Vec<QueuedToast>) {
    self
      .toasts
      .retain(|(_, t)| is_own(&t.id) || react.iter().any(|r| r.id == t.id));
    for toast in react {
      match self.toasts.iter_mut().find(|(_, t)| t.id == toast.id) {
        Some((_, existing)) => *existing = toast,
        None => {
          self.seq += 1;
          self.toasts.push((self.seq, toast));
        }
      }
    }
  }

  /// Drops toast `id` (expired or closed); false when it was gone.
  pub fn remove(&mut self, id: &str) -> bool {
    let before = self.toasts.len();
    self.toasts.retain(|(_, t)| t.id != id);
    self.toasts.len() != before
  }

  /// Every toast, oldest first.
  pub fn list(&self) -> Vec<QueuedToast> {
    self.toasts.iter().map(|(_, t)| t.clone()).collect()
  }
}

/// Whether toast `id` is the app's own, rather than React's.
pub fn is_own(id: &str) -> bool {
  id.starts_with(OWN_PREFIX)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn react(id: &str) -> QueuedToast {
    QueuedToast {
      id: id.into(),
      title: id.into(),
      description: None,
      kind: ToastKind::Info,
      action: None,
    }
  }

  fn ids(queue: &ToastQueue) -> Vec<String> {
    queue.list().into_iter().map(|t| t.id).collect()
  }

  #[test]
  fn shows_toasts_with_an_action_for_longer() {
    let mut queue = ToastQueue::default();
    let plain = queue.push("Saved", None, ToastKind::Success, None);
    let action = queue.push(
      "Created",
      Some("#4"),
      ToastKind::Success,
      Some("Open in Web"),
    );
    assert_eq!(plain.duration(), Duration::from_secs(5));
    assert_eq!(action.duration(), Duration::from_secs(12));
    assert_eq!(action.description.as_deref(), Some("#4"));
  }

  #[test]
  fn keeps_own_and_react_toasts_in_arrival_order() {
    let mut queue = ToastQueue::default();
    let own = queue.push("Pushed", Some(""), ToastKind::Success, None);
    assert_eq!(own.description, None, "an empty description shows none");
    queue.set_react(vec![react("a")]);
    let second = queue.push("Synced", None, ToastKind::Success, None);
    queue.set_react(vec![react("a"), react("b")]);
    assert_eq!(
      ids(&queue),
      [own.id.clone(), "a".into(), second.id.clone(), "b".into()]
    );

    // React dropped "a": only it goes.
    queue.set_react(vec![react("b")]);
    assert_eq!(ids(&queue), [own.id.clone(), second.id.clone(), "b".into()]);
    assert!(queue.remove(&own.id));
    assert!(!queue.remove(&own.id));
    assert_eq!(ids(&queue), [second.id, "b".to_string()]);
  }

  #[test]
  fn tells_own_toasts_from_reacts() {
    let mut queue = ToastQueue::default();
    let own = queue.push("x", None, ToastKind::Error, None);
    assert!(is_own(&own.id));
    assert!(!is_own("k3j9x"));
  }
}
