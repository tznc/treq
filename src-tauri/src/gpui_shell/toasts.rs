//! Each shell window's toasts ([`ToastQueue`]), drawn by `popup` over the
//! window. The app's own toasts (`Backend::toast`, `NativeEvent::Toast`)
//! go straight in, with their timers here; React's toast store still sends
//! its own list (`gpui_toasts_update`), shown with them.

use std::{
  collections::HashMap,
  sync::{Mutex, OnceLock},
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
    ToastTone::Info => toast_queue::ToastKind::Info,
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

fn from_popup(toast: Toast) -> QueuedToast {
  QueuedToast {
    id: toast.id,
    title: toast.title,
    description: toast.description,
    kind: match toast.kind {
      ToastKind::Success => toast_queue::ToastKind::Success,
      ToastKind::Error => toast_queue::ToastKind::Error,
      ToastKind::Info => toast_queue::ToastKind::Info,
      ToastKind::Warning => toast_queue::ToastKind::Warning,
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
    tokio::time::sleep(toast.duration()).await;
    if remove(&label, &toast.id) {
      redraw(&app, &label);
    }
  });
}

/// React's toast store changed: its whole list for window `label`.
pub fn set_react(app: &AppHandle, label: &str, toasts: Vec<Toast>) {
  windows()
    .lock_or_recover()
    .entry(label.to_string())
    .or_default()
    .queue
    .set_react(toasts.into_iter().map(from_popup).collect());
  redraw(app, label);
}

fn remove(label: &str, id: &str) -> bool {
  let mut windows = windows().lock_or_recover();
  let Some(window) = windows.get_mut(label) else {
    return false;
  };
  window.actions.remove(id);
  window.queue.remove(id)
}

/// A click on one of the app's own toasts: its action (`action: true`) or
/// close button. False for React's toasts, which React handles.
pub fn click(app: &AppHandle, label: &str, id: &str, action: bool) -> bool {
  if !toast_queue::is_own(id) {
    return false;
  }
  let run = {
    let mut windows = windows().lock_or_recover();
    let Some(window) = windows.get_mut(label) else {
      return true;
    };
    window.queue.remove(id);
    window.actions.remove(id)
  };
  redraw(app, label);
  if let Some(run) = run.filter(|_| action) {
    tauri::async_runtime::spawn_blocking(run);
  }
  true
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
