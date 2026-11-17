//! Each shell window's toasts ([`ToastQueue`]), drawn in that window by
//! `toast_layer`. Toasts (`Backend::toast`, `NativeEvent::Toast`) go straight in,
//! with their timers here.

use std::{
  collections::HashMap,
  sync::{Mutex, OnceLock},
  time::Duration,
};

use tauri::AppHandle;

use super::{
  native::ToastTone,
  popup::{Toast, ToastKind},
};
use crate::{
  core::toast_queue::{self, QueuedToast, ToastQueue},
  lock_ext::LockExt,
};

/// What a toast's action button runs.
pub type ToastActionFn = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct WindowToasts {
  queue: ToastQueue,
  actions: HashMap<String, ToastActionFn>,
}

fn windows() -> &'static Mutex<HashMap<String, WindowToasts>> {
  static WINDOWS: OnceLock<Mutex<HashMap<String, WindowToasts>>> = OnceLock::new();
  WINDOWS.get_or_init(Default::default)
}

fn kind_of(tone: ToastTone) -> toast_queue::ToastKind {
  match tone {
    ToastTone::Success => toast_queue::ToastKind::Success,
    ToastTone::Warning => toast_queue::ToastKind::Warning,
    ToastTone::Error => toast_queue::ToastKind::Error,
  }
}

fn to_popup(toast: QueuedToast) -> Toast {
  Toast {
    id: toast.id,
    title: toast.title,
    description: toast.description,
    kind: match toast.kind {
      toast_queue::ToastKind::Success => ToastKind::Success,
      toast_queue::ToastKind::Error => ToastKind::Error,
      toast_queue::ToastKind::Info => ToastKind::Info,
      toast_queue::ToastKind::Warning => ToastKind::Warning,
    },
    action: toast.action,
  }
}

/// Shows a toast in window `label` until it expires or is closed. Callable
/// from any thread.
pub fn push(
  app: &AppHandle,
  label: &str,
  tone: ToastTone,
  title: &str,
  description: &str,
  action: Option<(String, ToastActionFn)>,
) {
  let toast = {
    let mut windows = windows().lock_or_recover();
    let window = windows.entry(label.to_string()).or_default();
    let toast = window.queue.push(
      title,
      Some(description),
      kind_of(tone),
      action.as_ref().map(|(label, _)| label.as_str()),
    );
    if let Some((_, run)) = action {
      window.actions.insert(toast.id.clone(), run);
    }
    toast
  };
  redraw(app, label);
  let (app, label) = (app.clone(), label.to_string());
  tauri::async_runtime::spawn(async move {
    tokio::time::sleep(expiry(&toast)).await;
    if remove(&label, &toast.id) {
      redraw(&app, &label);
    }
  });
}

/// How long a toast shows before it dismisses itself: errors, warnings and
/// toasts with an action stay longer.
fn expiry(toast: &QueuedToast) -> Duration {
  use toast_queue::ToastKind::{Error, Warning};
  if matches!(toast.kind, Error | Warning) {
    return toast_queue::ACTION_DURATION;
  }
  match (toast.action.is_none(), toast.description.is_none()) {
    (true, true) => Duration::from_secs(4),
    _ => toast.duration(),
  }
}

fn remove(label: &str, id: &str) -> bool {
  let mut windows = windows().lock_or_recover();
  let Some(window) = windows.get_mut(label) else {
    return false;
  };
  window.actions.remove(id);
  window.queue.remove(id)
}

/// A click on a toast: its action (`action: true`) or close button.
pub fn click(app: &AppHandle, label: &str, id: &str, action: bool) {
  let run = {
    let mut windows = windows().lock_or_recover();
    let Some(window) = windows.get_mut(label) else {
      return;
    };
    window.queue.remove(id);
    window.actions.remove(id)
  };
  redraw(app, label);
  if let Some(run) = run.filter(|_| action) {
    tauri::async_runtime::spawn_blocking(run);
  }
}

fn redraw(app: &AppHandle, label: &str) {
  let toasts = windows()
    .lock_or_recover()
    .get(label)
    .map(|w| w.queue.list())
    .unwrap_or_default();
  super::draw_toasts(app, label, toasts.into_iter().map(to_popup).collect());
}

/// Forgets a destroyed window's toasts.
pub fn forget(label: &str) {
  windows().lock_or_recover().remove(label);
}

#[cfg(test)]
mod tests {
  use super::*;

  fn toast(kind: toast_queue::ToastKind, action: Option<&str>) -> QueuedToast {
    QueuedToast {
      id: "treq-1".into(),
      title: "t".into(),
      description: Some("d".into()),
      kind,
      action: action.map(str::to_string),
    }
  }

  #[test]
  fn every_toast_dismisses_itself() {
    use toast_queue::ToastKind::*;
    for kind in [Success, Info] {
      assert_eq!(expiry(&toast(kind, None)), toast_queue::PLAIN_DURATION);
      assert_eq!(
        expiry(&toast(kind, Some("Undo"))),
        toast_queue::ACTION_DURATION
      );
    }
    for kind in [Error, Warning] {
      assert_eq!(expiry(&toast(kind, None)), toast_queue::ACTION_DURATION);
      assert_eq!(
        expiry(&toast(kind, Some("Retry"))),
        toast_queue::ACTION_DURATION
      );
    }
  }
}
