//! How a shell window and its [`WindowController`] meet: the controller is
//! made with the window, draws through the `Shell`, and receives what the
//! window's views, the backend, the menu bar and app-wide events report.

use std::{
  collections::HashSet,
  rc::Rc,
  sync::{Arc, Mutex},
};

use tauri::{AppHandle, Listener};
use tauri_plugin_gpui::gpui::{prelude::*, App, Entity, WeakEntity};

use super::{
  actions::{Menu, Shortcut},
  controller::{Presenter, WindowController},
};
use crate::{
  core::cloud::ManagedEndpointEvent,
  gpui_shell::{
    defer_main,
    native::{Backend, NativeEvent},
    remote_repo::{self, Cutoff},
    screen::{self, ScreenAction},
    sidebar::SidebarAction,
    update_shell, Shell, Shells,
  },
  lock_ext::LockExt,
};

/// The windows the model drives, by label.
fn driven() -> &'static Mutex<HashSet<String>> {
  static DRIVEN: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
  DRIVEN.get_or_init(Default::default)
}

/// Whether window `label` has its model.
fn drives(label: &str) -> bool {
  driven().lock_or_recover().contains(label)
}

pub(in crate::gpui_shell) fn forget(label: &str) {
  driven().lock_or_recover().remove(label);
}

/// Gives window `label` its model, on `repo`. Main thread, outside a GPUI
/// update.
pub(in crate::gpui_shell) fn start(
  app: &AppHandle,
  label: &str,
  repo: Option<String>,
) -> Result<(), tauri_plugin_gpui::GpuiError> {
  driven().lock_or_recover().insert(label.to_string());
  let (app, label) = (app.clone(), label.to_string());
  update_shell(&label.clone(), move |shell, cx| {
    let weak = cx.weak_entity();
    let present: Presenter = Rc::new(move |presentation, cx| {
      let weak = weak.clone();
      // The model can present from inside the shell's own update.
      cx.defer(move |cx| {
        let _ = weak.update(cx, |shell, cx| shell.present(presentation, cx));
      });
    });
    let host = Arc::new(Backend::new(app.clone(), label.clone()));
    let controller = cx.new(|cx| WindowController::new(host, repo, present, cx));
    if let Some(dark) = shell.os_dark {
      controller.update(cx, |c, cx| c.set_system_dark(dark, cx));
    }
    shell.window = Some(controller);
  })
}

/// Window `label`'s controller, while the model drives it.
fn controller(label: &str, cx: &App) -> Option<Entity<WindowController>> {
  if !drives(label) {
    return None;
  }
  let shell = cx
    .try_global::<Shells>()?
    .0
    .get(label)
    .and_then(WeakEntity::upgrade)?;
  shell.read(cx).window.clone()
}

/// Runs `f` on window `label`'s controller from any thread.
fn with_controller(
  app: &AppHandle,
  label: &str,
  f: impl FnOnce(&mut WindowController, &mut tauri_plugin_gpui::gpui::Context<WindowController>)
    + Send
    + 'static,
) {
  let label = label.to_string();
  defer_main(app, move |_| {
    let _ = tauri_plugin_gpui::with_app(move |cx| {
      if let Some(controller) = controller(&label, cx) {
        controller.update(cx, f);
      }
    });
  });
}

/// An action sink: the model's handler for window `label`.
pub(in crate::gpui_shell) fn routed(
  label: String,
  handle: fn(&str, &ScreenAction, &mut App) -> bool,
) -> screen::ActionSink {
  Rc::new(move |action: ScreenAction, cx| {
    let label = label.clone();
    // The view that sent it is mid-update.
    cx.defer(move |cx| {
      if !handle(&label, &action, cx) {
        log::debug!("window {label} has no handler for {action:?}");
      }
    });
  })
}

pub(in crate::gpui_shell) fn screen_action(
  label: &str,
  action: &ScreenAction,
  cx: &mut App,
) -> bool {
  controller(label, cx).is_some_and(|c| c.update(cx, |c, cx| c.screen_action(action, cx)))
}

pub(in crate::gpui_shell) fn overlay_action(
  label: &str,
  action: &ScreenAction,
  cx: &mut App,
) -> bool {
  controller(label, cx).is_some_and(|c| c.update(cx, |c, cx| c.overlay_action(action, cx)))
}

pub(in crate::gpui_shell) fn sidebar_action(
  label: &str,
  action: &SidebarAction,
  cx: &mut App,
) -> bool {
  controller(label, cx).is_some_and(|c| c.update(cx, |c, cx| c.sidebar_action(action, cx)))
}

/// Workspace `id` of `repo`, as window `label` last read it; `None` when
/// the window has not read it (or the model does not drive the window).
pub(in crate::gpui_shell) fn workspace(
  label: &str,
  repo: &str,
  id: i64,
  cx: &App,
) -> Option<crate::local_db::Workspace> {
  controller(label, cx)?.read(cx).workspace(repo, id).cloned()
}

/// A keyboard shortcut pressed in window `label`.
/// Window `label`'s confirmation answered: `yes` for its verb.
pub(in crate::gpui_shell) fn answer(label: &str, yes: bool, cx: &mut App) {
  if let Some(controller) = controller(label, cx) {
    controller.update(cx, |c, cx| c.answer(yes, cx));
  }
}

pub(in crate::gpui_shell) fn shortcut(label: &str, shortcut: Shortcut, cx: &mut App) {
  if let Some(controller) = controller(label, cx) {
    controller.update(cx, |c, cx| c.shortcut(shortcut, cx));
  }
}

/// A change the backend reports for window `label` (`Backend::notify`):
/// the model follows it. Any thread.
pub fn native_event(app: &AppHandle, label: &str, event: NativeEvent) {
  with_controller(app, label, move |c, cx| c.native_event(&event, cx));
}

/// The cloud workspace connected or renewed (`ManagedEndpointEvent`): a
/// connection goes to window `label` (the one that asked), a renewal to
/// every window. Any thread.
pub fn managed_endpoint(app: &AppHandle, label: Option<&str>, event: ManagedEndpointEvent) {
  let targets = match (&event, label) {
    (ManagedEndpointEvent::Connected { .. }, Some(label)) => vec![label.to_string()],
    _ => labels(),
  };
  for label in targets {
    let event = event.clone();
    with_controller(app, &label, move |c, cx| c.managed_endpoint(event, cx));
  }
}

/// An endpoint was reauthenticated: no window blocks it any more. Any
/// thread.
pub fn cutoff_cleared(app: &AppHandle, endpoint_id: &str) {
  remote_repo::clear_cutoff(endpoint_id);
  for label in labels() {
    let id = endpoint_id.to_string();
    with_controller(app, &label, move |c, cx| c.cutoff_changed(&id, None, cx));
  }
}

/// The account changed: windows follow the cloud workspace's state. Any
/// thread.
pub fn instance_state(app: &AppHandle, state: Option<String>) {
  for label in labels() {
    let state = state.clone();
    with_controller(app, &label, move |c, cx| c.account_changed(state, cx));
  }
}

/// A menu bar item for window `label`, named by the event `lib.rs` gives
/// it (`payload`: the folder "Open…" picked). True when the model handles
/// it. Any thread.
pub fn menu_event(app: &AppHandle, label: &str, event: &str, payload: Option<String>) -> bool {
  if !drives(label) {
    return false;
  }
  let item = match event {
    "navigate-to-dashboard" => Menu::Dashboard,
    "navigate-to-settings" => Menu::Settings,
    "menu-add-repository" => Menu::AddRepository,
    "menu-open-ssh" => Menu::OpenSsh,
    "menu-view-logs" => Menu::ViewLogs,
    "menu-factory-reset" => Menu::FactoryReset,
    "menu-force-rebase-workspace" => Menu::ForceRebase,
    "menu-open-in-new-window-invalid" => Menu::OpenInNewWindowInvalid,
    "menu-repository-path-selected" => {
      let Some(path) = payload else { return false };
      with_controller(app, label, move |c, cx| c.open_folder(path, cx));
      return true;
    }
    _ => return false,
  };
  with_controller(app, label, move |c, cx| c.menu(item, cx));
  true
}

/// Window `label` gained focus. Any thread.
pub fn window_focused(app: &AppHandle, label: &str) {
  with_controller(app, label, |c, cx| c.focused(cx));
}

/// Follows the app-wide events the window's data depends on. Once.
pub fn listen(app: &AppHandle) {
  let handle = app.clone();
  app.listen_any("pr-statuses-updated", move |event| {
    let payload: serde_json::Value = serde_json::from_str(event.payload()).unwrap_or_default();
    let Some(repo_path) = payload["repo_path"].as_str() else {
      return;
    };
    for label in labels() {
      let repo_path = repo_path.to_string();
      with_controller(&handle, &label, move |c, cx| c.prs_changed(&repo_path, cx));
    }
  });
  // `remoteCutoffStore`: the transport tore an endpoint's connections down.
  let handle = app.clone();
  app.listen_any(
    crate::commands::remote_control::REMOTE_CUTOFF_EVENT,
    move |event| {
      let payload: serde_json::Value = serde_json::from_str(event.payload()).unwrap_or_default();
      let (Some(id), Some(reason)) = (payload["endpoint_id"].as_str(), payload["reason"].as_str())
      else {
        return;
      };
      if let Some(cutoff) = Cutoff::parse(reason) {
        remote_repo::record_cutoff(id, cutoff);
      }
      for label in labels() {
        let (id, reason) = (id.to_string(), reason.to_string());
        with_controller(&handle, &label, move |c, cx| {
          c.cutoff_changed(&id, Some(reason), cx)
        });
      }
    },
  );
}

fn labels() -> Vec<String> {
  driven().lock_or_recover().iter().cloned().collect()
}

/// Tells the model the OS appearance the shell renders under.
pub(in crate::gpui_shell) fn follow_appearance(
  shell: &mut Shell,
  dark: bool,
  cx: &mut tauri_plugin_gpui::gpui::Context<Shell>,
) {
  if shell.os_dark == Some(dark) {
    return;
  }
  shell.os_dark = Some(dark);
  if let Some(controller) = shell.window.clone() {
    cx.defer(move |cx| controller.update(cx, |c, cx| c.set_system_dark(dark, cx)));
  }
}
