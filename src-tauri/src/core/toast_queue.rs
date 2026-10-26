//! A window's toasts, as `toastStore` keeps them: in the order they came,
//! each expiring after a while, sooner without an action button.
//!
//! They show in one list, in arrival order.

use std::time::Duration;

/// How long a toast shows without an action (`addToast`).
pub const PLAIN_DURATION: Duration = Duration::from_millis(5000);
/// How long a toast with an action button shows.
pub const ACTION_DURATION: Duration = Duration::from_millis(12000);

/// The prefix of toast ids.
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
  /// Adds a toast; returns it with its new id.
  pub fn push(
    &mut self,
    title: &str,
    description: Option<&str>,
    kind: ToastKind,
    action: Option<&str>,
  ) -> QueuedToast {
    let description = description.filter(|d| !d.is_empty());
    // A retry or a second caller failing the same way shows nothing new.
    if let Some((_, showing)) = self.toasts.iter().find(|(_, t)| {
      t.title == title
        && t.description.as_deref() == description
        && t.kind == kind
        && t.action.as_deref() == action
    }) {
      return showing.clone();
    }
    self.next += 1;
    let toast = QueuedToast {
      id: format!("{OWN_PREFIX}{}", self.next),
      title: title.into(),
      description: description.map(str::to_string),
      kind,
      action: action.map(str::to_string),
    };
    self.seq += 1;
    self.toasts.push((self.seq, toast.clone()));
    toast
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

#[cfg(test)]
mod tests {
  use super::*;

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
  fn an_identical_toast_still_showing_is_not_stacked_again() {
    let mut queue = ToastQueue::default();
    let first = queue.push("Sync failed", Some("Push failed"), ToastKind::Error, None);
    let again = queue.push("Sync failed", Some("Push failed"), ToastKind::Error, None);
    assert_eq!(again.id, first.id);
    let other = queue.push("Sync failed", Some("Other"), ToastKind::Error, None);
    assert_eq!(ids(&queue), [first.id, other.id]);
  }

  #[test]
  fn keeps_toasts_in_arrival_order() {
    let mut queue = ToastQueue::default();
    let first = queue.push("Pushed", Some(""), ToastKind::Success, None);
    assert_eq!(first.description, None, "an empty description shows none");
    let second = queue.push("Synced", None, ToastKind::Success, None);
    assert_eq!(ids(&queue), [first.id.clone(), second.id.clone()]);
    assert!(queue.remove(&first.id));
    assert!(!queue.remove(&first.id));
    assert_eq!(ids(&queue), [second.id]);
  }
}
