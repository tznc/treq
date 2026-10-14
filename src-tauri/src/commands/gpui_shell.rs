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
