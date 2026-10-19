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
  /// App settings (`set_setting` keys) changed.
  SettingsChanged,
  /// The settings of the repository at `repo_path` changed.
  #[serde(rename_all = "camelCase")]
  RepoSettingsChanged { repo_path: String },
  /// Something in the repository at `repo_path` changed (a push, sync or
  /// rebase), so everything React read from it is stale.
  #[serde(rename_all = "camelCase")]
  WorkspaceChanged { repo_path: String },
  Toast {
    title: String,
    description: String,
    tone: ToastTone,
    /// A button that runs a [`register_toast_action`] closure.
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<ToastAction>,
  },
}

/// A toast's button: `label`, running the action registered as `id`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToastAction {
  pub label: String,
  pub id: u64,
}

type PendingAction = Box<dyn FnOnce() + Send>;

/// The toast buttons not yet clicked, by id.
fn toast_actions() -> &'static std::sync::Mutex<(u64, std::collections::HashMap<u64, PendingAction>)>
{
  static ACTIONS: std::sync::OnceLock<
    std::sync::Mutex<(u64, std::collections::HashMap<u64, PendingAction>)>,
  > = std::sync::OnceLock::new();
  ACTIONS.get_or_init(Default::default)
}

/// Keeps `action` until its toast's button is clicked; returns its id for
/// [`ToastAction`].
pub fn register_toast_action(action: impl FnOnce() + Send + 'static) -> u64 {
  let mut actions = toast_actions().lock_or_recover();
  actions.0 += 1;
  let id = actions.0;
  actions.1.insert(id, Box::new(action));
  id
}

/// Runs, once, the action of the toast button `id` (`gpui_toast_action`).
/// Blocks for as long as the action does.
pub fn run_toast_action(id: u64) {
  let action = toast_actions().lock_or_recover().1.remove(&id);
  if let Some(action) = action {
    action();
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToastTone {
  Success,
  Warning,
  Error,
}

fn package_json() -> serde_json::Value {
  const PACKAGE_JSON: &str = include_str!("../../../package.json");
  serde_json::from_str(PACKAGE_JSON).unwrap_or_default()
}

/// The website the docs links open, as React's `WEB_URL`.
pub fn web_url() -> String {
  let env = if cfg!(debug_assertions) {
    "dev"
  } else {
    "prod"
  };
  package_json()["env"][env]["webUrl"]
    .as_str()
    .map_or_else(|| "https://treq.dev".to_string(), str::to_string)
}

/// Whether `flag` is on in React's `FEATURES` (package.json `featureFlags`).
pub fn feature_flag(flag: &str) -> bool {
  package_json()["featureFlags"][flag]
    .as_bool()
    .unwrap_or(false)
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
      action: None,
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
