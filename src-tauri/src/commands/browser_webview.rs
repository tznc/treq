use crate::core::browser_url::is_allowed_browser_url;
use tauri::{
  AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewBuilder, WebviewUrl,
};

/// Label of the native child webview embedded inside the main window to
/// preview a page. Deliberately does not match any `windows` glob in
/// `capabilities/default.json`, so it is granted zero Tauri IPC
/// permissions — loaded page content cannot call back into the app via
/// `invoke`. Navigation is reported back through `on_navigation`.
const BROWSER_WEBVIEW_LABEL: &str = "browser-preview";

fn parse_allowed_url(url: &str) -> Result<Url, String> {
  if !is_allowed_browser_url(url) {
    return Err(format!(
            "URL not allowed in the in-app browser (only http://localhost, http://127.0.0.1 and file:// are supported): {url}"
        ));
  }
  Url::parse(url).map_err(|e| format!("Failed to parse URL: {e}"))
}

/// Opens (or navigates, if already open) the embedded page-preview webview
/// at the given position/size, overlaying a placeholder in the React
/// `BrowserPanel`. Creating a child webview must happen on the main thread,
/// hence `async` (matches Tauri's own documented pattern for this API).
///
/// Desktop-only: the child-webview APIs used below (`Webview::set_position`/
/// `set_size`/`close`, `Window::add_child`) have no mobile implementation in
/// Tauri. Mobile has no in-app browser-preview panel to back. Dispatches to
/// a cfg-gated helper rather than being cfg-gated itself (with a mobile-stub
/// twin of the same name) because this crate's `#[tauri::command]`-scanning
/// test tooling (`tauri-test-macros`) works off the source text and doesn't
/// evaluate `#[cfg]`, so two same-named command functions read as a
/// duplicate to it even though only one is ever compiled.
#[tauri::command]
pub async fn open_browser_webview(
  app: AppHandle,
  url: String,
  x: f64,
  y: f64,
  width: f64,
  height: f64,
) -> Result<(), String> {
  #[cfg(not(desktop))]
  {
    let _ = (app, url, x, y, width, height);
    return Err("Browser preview is not available on mobile".to_string());
  }
  #[cfg(desktop)]
  open_browser_webview_desktop(app, url, x, y, width, height).await
}

#[cfg(desktop)]
async fn open_browser_webview_desktop(
  app: AppHandle,
  url: String,
  x: f64,
  y: f64,
  width: f64,
  height: f64,
) -> Result<(), String> {
  let parsed = parse_allowed_url(&url)?;

  if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
    webview.navigate(parsed).map_err(|e| e.to_string())?;
    webview
      .set_position(LogicalPosition::new(x, y))
      .map_err(|e| e.to_string())?;
    webview
      .set_size(LogicalSize::new(width, height))
      .map_err(|e| e.to_string())?;
    return Ok(());
  }

  let window = app
    .get_window("main")
    .ok_or_else(|| "Main window not found".to_string())?;

  let app_for_nav = app.clone();
  let builder = WebviewBuilder::new(BROWSER_WEBVIEW_LABEL, WebviewUrl::External(parsed))
    .on_navigation(move |nav_url| {
      let _ = app_for_nav.emit("browser-url-changed", nav_url.as_str().to_string());
      true
    });

  window
    .add_child(
      builder,
      LogicalPosition::new(x, y),
      LogicalSize::new(width, height),
    )
    .map_err(|e| e.to_string())?;

  Ok(())
}

#[tauri::command]
pub fn navigate_browser_webview(app: AppHandle, url: String) -> Result<(), String> {
  #[cfg(not(desktop))]
  {
    let _ = (app, url);
    return Err("Browser preview is not available on mobile".to_string());
  }
  #[cfg(desktop)]
  {
    let parsed = parse_allowed_url(&url)?;
    let webview = app
      .get_webview(BROWSER_WEBVIEW_LABEL)
      .ok_or_else(|| "Browser preview is not open".to_string())?;
    webview.navigate(parsed).map_err(|e| e.to_string())
  }
}

#[tauri::command]
pub fn close_browser_webview(app: AppHandle) -> Result<(), String> {
  #[cfg(not(desktop))]
  {
    let _ = app;
  }
  #[cfg(desktop)]
  if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
    webview.close().map_err(|e| e.to_string())?;
  }
  Ok(())
}

#[tauri::command]
pub fn sync_browser_webview_bounds(
  app: AppHandle,
  x: f64,
  y: f64,
  width: f64,
  height: f64,
) -> Result<(), String> {
  #[cfg(not(desktop))]
  {
    let _ = (app, x, y, width, height);
    return Err("Browser preview is not available on mobile".to_string());
  }
  #[cfg(desktop)]
  {
    let webview = app
      .get_webview(BROWSER_WEBVIEW_LABEL)
      .ok_or_else(|| "Browser preview is not open".to_string())?;
    webview
      .set_position(LogicalPosition::new(x, y))
      .map_err(|e| e.to_string())?;
    webview
      .set_size(LogicalSize::new(width, height))
      .map_err(|e| e.to_string())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rejects_disallowed_url_before_touching_any_webview() {
    let err = parse_allowed_url("https://example.com").unwrap_err();
    assert!(err.contains("not allowed"));
  }

  #[test]
  fn accepts_localhost_url() {
    assert!(parse_allowed_url("http://localhost:5173/app").is_ok());
  }
}
