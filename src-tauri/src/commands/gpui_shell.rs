use tauri::AppHandle;

/// Shows React's sidebar snapshot in the GPUI sidebar of window
/// `window_label`, or hides it for `null`. A no-op outside the GPUI shell.
#[tauri::command]
pub fn gpui_sidebar_update(
  app: AppHandle,
  window_label: String,
  snapshot: Option<serde_json::Value>,
) {
  #[cfg(all(desktop, feature = "gpui-shell"))]
  {
    if !crate::gpui_shell::is_enabled(&app) {
      return;
    }
    let snapshot = match snapshot.map(serde_json::from_value).transpose() {
      Ok(snapshot) => snapshot,
      Err(err) => {
        log::warn!("invalid GPUI sidebar snapshot: {err}");
        return;
      }
    };
    crate::gpui_shell::update_sidebar(&app, &window_label, snapshot);
  }
  #[cfg(not(all(desktop, feature = "gpui-shell")))]
  let _ = (app, window_label, snapshot);
}

/// Opens a GPUI menu of `entries` at the last right-click in the sidebar of
/// window `window_label`. Picks come back as `gpui-menu-select` events.
#[tauri::command]
pub fn gpui_menu_show(app: AppHandle, window_label: String, entries: serde_json::Value) {
  #[cfg(all(desktop, feature = "gpui-shell"))]
  {
    if !crate::gpui_shell::is_enabled(&app) {
      return;
    }
    match serde_json::from_value(entries) {
      Ok(entries) => crate::gpui_shell::show_menu(&app, &window_label, entries),
      Err(err) => log::warn!("invalid GPUI menu: {err}"),
    }
  }
  #[cfg(not(all(desktop, feature = "gpui-shell")))]
  let _ = (app, window_label, entries);
}

/// Closes an open GPUI menu.
#[tauri::command]
pub fn gpui_menu_hide(app: AppHandle) {
  #[cfg(all(desktop, feature = "gpui-shell"))]
  if crate::gpui_shell::is_enabled(&app) {
    crate::gpui_shell::hide_menus(&app);
  }
  #[cfg(not(all(desktop, feature = "gpui-shell")))]
  let _ = app;
}

/// Shows window `window_label`'s toasts (React's toast store) in GPUI, over
/// the webview. Clicks come back as `gpui-toast` events.
#[tauri::command]
pub fn gpui_toasts_update(app: AppHandle, window_label: String, toasts: serde_json::Value) {
  #[cfg(all(desktop, feature = "gpui-shell"))]
  {
    if !crate::gpui_shell::is_enabled(&app) {
      return;
    }
    match serde_json::from_value(toasts) {
      Ok(toasts) => crate::gpui_shell::update_toasts(&app, &window_label, toasts),
      Err(err) => log::warn!("invalid GPUI toasts: {err}"),
    }
  }
  #[cfg(not(all(desktop, feature = "gpui-shell")))]
  let _ = (app, window_label, toasts);
}

/// Draws React's screen snapshot in GPUI in place of the webview of window
/// `window_label`, or shows the webview again for `null`. A no-op outside
/// the GPUI shell.
#[tauri::command]
pub fn gpui_screen_update(
  app: AppHandle,
  window_label: String,
  snapshot: Option<serde_json::Value>,
) {
  #[cfg(all(desktop, feature = "gpui-shell"))]
  {
    if !crate::gpui_shell::is_enabled(&app) {
      return;
    }
    let snapshot = match snapshot.map(serde_json::from_value).transpose() {
      Ok(snapshot) => snapshot,
      Err(err) => {
        log::warn!("invalid GPUI screen snapshot: {err}");
        return;
      }
    };
    crate::gpui_shell::update_screen(&app, &window_label, snapshot);
  }
  #[cfg(not(all(desktop, feature = "gpui-shell")))]
  let _ = (app, window_label, snapshot);
}

/// Draws React's dialog in GPUI over the screen of window `window_label`
/// (the agent prompt, prompt history or stash list), or closes it for `null`.
/// A no-op outside the GPUI shell.
#[tauri::command]
pub fn gpui_modal_update(app: AppHandle, window_label: String, modal: Option<serde_json::Value>) {
  #[cfg(all(desktop, feature = "gpui-shell"))]
  {
    if !crate::gpui_shell::is_enabled(&app) {
      return;
    }
    let modal = match modal.map(serde_json::from_value).transpose() {
      Ok(modal) => modal,
      Err(err) => {
        log::warn!("invalid GPUI modal: {err}");
        return;
      }
    };
    crate::gpui_shell::update_modal(&app, &window_label, modal);
  }
  #[cfg(not(all(desktop, feature = "gpui-shell")))]
  let _ = (app, window_label, modal);
}

/// Runs the action of a GPUI screen's toast button (`NativeEvent::Toast`'s
/// `action`), once. A no-op for an unknown or used id.
#[tauri::command]
pub async fn gpui_toast_action(id: u64) {
  #[cfg(all(desktop, feature = "gpui-shell"))]
  {
    let _ =
      tauri::async_runtime::spawn_blocking(move || crate::gpui_shell::native::run_toast_action(id))
        .await;
  }
  #[cfg(not(all(desktop, feature = "gpui-shell")))]
  let _ = id;
}
