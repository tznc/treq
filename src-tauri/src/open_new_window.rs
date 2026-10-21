use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Process-wide lock so a macOS File-menu redelivery cannot open another
/// directory picker after "Open in New Window" already started.
pub struct OpenNewWindowGate {
  in_progress: AtomicBool,
}

impl OpenNewWindowGate {
  pub const fn new() -> Self {
    Self {
      in_progress: AtomicBool::new(false),
    }
  }

  pub fn try_begin(&self) -> bool {
    self
      .in_progress
      .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
      .is_ok()
  }

  pub fn end(&self) {
    self.in_progress.store(false, Ordering::Release);
  }
}

pub static OPEN_NEW_WINDOW_GATE: OpenNewWindowGate = OpenNewWindowGate::new();

/// Same re-entry problem, but for the regular "Open..." action: a macOS
/// File-menu redelivery must not open a second directory picker while one
/// is already showing.
pub static OPEN_REPO_GATE: OpenNewWindowGate = OpenNewWindowGate::new();

pub fn new_repo_window_title(repo_path: &str) -> String {
  let name = Path::new(repo_path)
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or(repo_path);
  format!("Treq - {}", name)
}

pub fn new_repo_window_label() -> String {
  let millis = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_millis())
    .unwrap_or(0);
  format!("treq-{millis}")
}

/// Pick which window should receive a menu event.
/// Never falls back to a global broadcast: on macOS the menu bar unfocuses
/// every window, and a broadcast would run the folder picker in all of them.
pub fn resolve_menu_event_window_label(
  focused_labels: &[String],
  last_focused: &HashMap<String, u64>,
  existing: &[String],
) -> Option<String> {
  for label in focused_labels {
    if existing.iter().any(|l| l == label) {
      return Some(label.clone());
    }
  }
  last_focused
    .iter()
    .filter(|(label, _)| existing.iter().any(|l| l == *label))
    .max_by_key(|(_, ts)| *ts)
    .map(|(label, _)| label.clone())
    .or_else(|| {
      if existing.iter().any(|l| l == "main") {
        Some("main".to_string())
      } else {
        existing.first().cloned()
      }
    })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn gate_rejects_reentrant_begin() {
    let gate = OpenNewWindowGate::new();
    assert!(gate.try_begin());
    assert!(!gate.try_begin());
    gate.end();
    assert!(gate.try_begin());
  }

  #[test]
  fn open_repo_gate_rejects_reentrant_begin_and_recovers_after_cancellation() {
    let gate = OpenNewWindowGate::new();
    assert!(gate.try_begin());
    // Redelivered menu event while the picker is already showing must be rejected.
    assert!(!gate.try_begin());
    assert!(!gate.try_begin());
    // Cancelling the picker releases the gate for the next "Open..." action.
    gate.end();
    assert!(gate.try_begin());
    gate.end();
  }

  #[test]
  fn titles_window_with_folder_name() {
    assert_eq!(new_repo_window_title("/Users/me/My Repo"), "Treq - My Repo");
  }

  #[test]
  fn prefers_focused_window_over_last_focused() {
    let mut last = HashMap::new();
    last.insert("main".to_string(), 10);
    last.insert("treq-1".to_string(), 99);
    let label = resolve_menu_event_window_label(
      &["main".to_string()],
      &last,
      &["main".to_string(), "treq-1".to_string()],
    );
    assert_eq!(label.as_deref(), Some("main"));
  }

  #[test]
  fn uses_last_focused_when_no_window_is_focused() {
    let mut last = HashMap::new();
    last.insert("main".to_string(), 10);
    last.insert("treq-1".to_string(), 99);
    let label =
      resolve_menu_event_window_label(&[], &last, &["main".to_string(), "treq-1".to_string()]);
    assert_eq!(label.as_deref(), Some("treq-1"));
  }

  #[test]
  fn does_not_require_global_broadcast_when_unfocused() {
    let label = resolve_menu_event_window_label(&[], &HashMap::new(), &["main".to_string()]);
    assert_eq!(label.as_deref(), Some("main"));
  }
}
