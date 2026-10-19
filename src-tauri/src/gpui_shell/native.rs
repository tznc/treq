//! What GPUI screens call to read and change the app's state directly, in
//! place of React's handlers.
//!
//! React still draws the parts of the window GPUI does not yet, and caches
//! what it read from the backend. So after a screen changes something, it
//! tells React with a [`NativeEvent`], and React updates its own views (it
//! never repeats the change).

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{db::Database, lock_ext::LockExt, AppState};

/// Event React listens to on its own webview window.
pub const EVENT: &str = "gpui-native-event";

/// What a GPUI screen did, for React to show, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NativeEvent {
  /// The window now shows the repository at `path`.
  RepoOpened { path: String },
  Toast {
    title: String,
    description: String,
    tone: ToastTone,
  },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToastTone {
  Success,
  Error,
}

/// The backend, as seen from window `label`'s screens.
#[derive(Clone)]
pub struct Backend {
  app: AppHandle,
  label: String,
}

impl Backend {
  pub fn new(app: AppHandle, label: String) -> Self {
    Self { app, label }
  }

  pub fn app(&self) -> &AppHandle {
    &self.app
  }

  /// Runs `f` with the app database. Blocks on its lock, so call it off
  /// the main thread for anything slow.
  pub fn with_db<T>(&self, f: impl FnOnce(&Database) -> T) -> T {
    let state = self.app.state::<AppState>();
    let db = state.db.lock_or_recover();
    f(&db)
  }

  /// Records which repository this window shows, as `set_window_repo_path`.
  pub fn set_window_repo_path(&self, path: &str) {
    let state = self.app.state::<AppState>();
    state
      .window_repo_paths
      .lock_or_recover()
      .insert(self.label.clone(), path.to_string());
  }

  /// Tells this window's React app what a screen did. Tauri calls must not
  /// run inside a GPUI update, so it sends from a task.
  pub fn notify(&self, event: NativeEvent) {
    let (app, label) = (self.app.clone(), self.label.clone());
    tauri::async_runtime::spawn(async move {
      let target = tauri::EventTarget::webview_window(&label);
      if let Err(err) = tauri::Emitter::emit_to(&app, target, EVENT, event) {
        log::warn!("failed to send a GPUI native event: {err}");
      }
    });
  }

  pub fn toast(&self, tone: ToastTone, title: impl Into<String>, description: impl Into<String>) {
    self.notify(NativeEvent::Toast {
      title: title.into(),
      description: description.into(),
      tone,
    });
  }

  /// Opens `url` in the default browser, from a task like `notify`.
  pub fn open_url(&self, url: &str) {
    let (app, url) = (self.app.clone(), url.to_string());
    tauri::async_runtime::spawn(async move {
      use tauri_plugin_opener::OpenerExt;
      if let Err(err) = app.opener().open_url(&url, None::<&str>) {
        log::warn!("failed to open {url}: {err}");
      }
    });
  }
}
