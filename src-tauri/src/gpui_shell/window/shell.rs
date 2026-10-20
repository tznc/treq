//! How a shell window and its [`WindowController`] meet: the controller is
//! made with the window, draws through the `Shell`, and receives what the
//! window's views, the backend, the menu bar and app-wide events report.
//!
//! React keeps running in the webview. While it shows a remote repository
//! (or its dialogs) it owns the window, as before the model: its pushes are
//! drawn and the actions go to it. Otherwise its pushes are ignored.

use std::{
  collections::HashMap,
  rc::Rc,
  sync::{Arc, Mutex},
};

use tauri::{AppHandle, Listener};
use tauri_plugin_gpui::gpui::{prelude::*, App, Entity, WeakEntity};

use super::{
  actions::Menu,
  controller::{Forwarder, Presenter, WindowController},
  model::Forward,
};
use crate::{
  gpui_shell::{
    defer_main, emit_to_webview,
    native::{Backend, NativeEvent},
    overlay::OverlaySnapshot,
    screen::{self, ScreenAction, ScreenSnapshot},
    sidebar::SidebarAction,
    update_shell, Shell, Shells,
  },
  lock_ext::LockExt,
};

/// The windows the model drives, by label, and whether React owns each now.
fn driven() -> &'static Mutex<HashMap<String, bool>> {
  static DRIVEN: std::sync::OnceLock<Mutex<HashMap<String, bool>>> = std::sync::OnceLock::new();
  DRIVEN.get_or_init(Default::default)
}

/// Whether the model handles window `label`'s actions now.
pub fn drives(label: &str) -> bool {
  driven().lock_or_recover().get(label) == Some(&false)
}

pub(in crate::gpui_shell) fn set_react_owns(label: &str, owns: bool) {
  if let Some(entry) = driven().lock_or_recover().get_mut(label) {
    *entry = owns;
  }
}

pub(in crate::gpui_shell) fn forget(label: &str) {
  driven().lock_or_recover().remove(label);
}

/// Whether React's `snapshot` is for what only React runs: a remote
/// repository, its dialogs, or the remote repository form.
pub fn react_owns(snapshot: &ScreenSnapshot) -> bool {
  snapshot.repository.is_some()
    || snapshot.remote.is_open()
    || matches!(&snapshot.view, screen::View::Onboarding(route) if route.remote_repo.is_some())
}

/// Gives window `label` its model, on `repo`. Main thread, outside a GPUI
/// update.
pub(in crate::gpui_shell) fn start(
  app: &AppHandle,
  label: &str,
  repo: Option<String>,
) -> Result<(), tauri_plugin_gpui::GpuiError> {
  driven().lock_or_recover().insert(label.to_string(), false);
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
    let forward: Forwarder = {
      let (app, label) = (app.clone(), label.clone());
      Rc::new(move |forward, _| match forward {
        Forward::Event(event) => emit_to_webview(&app, &label, event, ()),
      })
    };
    let host = Arc::new(Backend::new(app.clone(), label.clone()));
    let controller = cx.new(|cx| WindowController::new(host, repo, present, forward, cx));
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

/// An action sink: the model's handler, then React's `event` for what it
/// leaves.
pub(in crate::gpui_shell) fn routed(
  app: AppHandle,
  label: String,
  event: &'static str,
  handle: fn(&str, &ScreenAction, &mut App) -> bool,
) -> screen::ActionSink {
  Rc::new(move |action: ScreenAction, cx| {
    if !drives(&label) {
      return emit_to_webview(&app, &label, event, action);
    }
    let (app, label) = (app.clone(), label.clone());
    // The view that sent it is mid-update.
    cx.defer(move |cx| {
      if !handle(&label, &action, cx) {
        emit_to_webview(&app, &label, event, action);
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

/// Keeps React's overlay for window `label`; whether to draw it. Main
/// thread, outside a GPUI update.
pub(in crate::gpui_shell) fn react_overlay(label: &str, snapshot: Option<OverlaySnapshot>) -> bool {
  let label = label.to_string();
  tauri_plugin_gpui::with_app(move |cx| {
    let shell = cx
      .try_global::<Shells>()
      .and_then(|shells| shells.0.get(&label))
      .and_then(WeakEntity::upgrade);
    match shell {
      Some(shell) => shell.update(cx, |shell, _| {
        shell.react.overlay = snapshot;
        shell.react_draws()
      }),
      None => true,
    }
  })
  .unwrap_or(true)
}

/// A change the backend reports for window `label` (`Backend::notify`):
/// the model follows it. True keeps it from React, for the requests the
/// model answers with a dialog of its own. Any thread.
pub fn native_event(app: &AppHandle, label: &str, event: &NativeEvent) -> bool {
  if !drives(label) {
    return false;
  }
  let event_for_model = event.clone();
  with_controller(app, label, move |c, cx| {
    c.native_event(&event_for_model, cx)
  });
  matches!(event, NativeEvent::Open { .. })
}

/// A menu bar item for window `label`, as the event `lib.rs` would send
/// React (`payload`: the folder "Open…" picked). True when the model
/// handles it. Any thread.
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
  app.listen_any("pr-statuses-updated", move |_| {
    for label in labels() {
      with_controller(&handle, &label, |c, cx| c.prs_changed(cx));
    }
  });
  let handle = app.clone();
  app.listen_any("workspace-files-changed", move |event| {
    let payload: serde_json::Value = serde_json::from_str(event.payload()).unwrap_or_default();
    let Some(id) = payload["workspace_id"].as_i64() else {
      return;
    };
    for label in labels() {
      with_controller(&handle, &label, move |c, cx| c.files_changed(id, cx));
    }
  });
}

fn labels() -> Vec<String> {
  driven()
    .lock_or_recover()
    .iter()
    .filter(|(_, owns)| !**owns)
    .map(|(label, _)| label.clone())
    .collect()
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
