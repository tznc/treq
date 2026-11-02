//! The `e2e` feature: a Playwright server (gpui-playwright) inside the app,
//! which `e2e/desktop` drives with stock `@playwright/test`. It listens only
//! when `GPUI_PLAYWRIGHT` is set (see `gpui_playwright::Server::from_env`),
//! and release builds leave the feature out.
//!
//! `context.newPage()` adopts a repo window. Dialogs and popovers draw in
//! the repo window itself, so one page drives them. A second repo window
//! gets a page of its own (`windowPage` in the suite): with the first one
//! taken, a page adopts the active window. Adopted windows stay open when a
//! test's context closes.

use std::cell::RefCell;

use tauri_plugin_gpui::gpui::{AnyWindowHandle, App, Window};

thread_local! {
  /// The windows a `Shell` draws: the repo windows.
  static SHELLS: RefCell<Vec<AnyWindowHandle>> = const { RefCell::new(Vec::new()) };
}

/// Notes that `window` is a repo window. `Shell` calls it as it renders.
pub(super) fn shell_window(window: AnyWindowHandle) {
  SHELLS.with(|shells| {
    let mut shells = shells.borrow_mut();
    if !shells.contains(&window) {
      shells.push(window);
    }
  });
}

/// Forgets the text `window` painted in its previous frame. gpui-kit's
/// `Root` does this as it renders; the app's windows have no `Root`, so
/// each window's root view calls it at the top of `render`. Without it the
/// painted text of every frame (a caret blink repaints twice a second) piles
/// up, and `getByText` finds the same text many times.
pub fn begin_frame(window: &Window) {
  gpui_kit::base::test_support::begin_frame(window);
}

/// The repository the first window opens on, in place of the saved one.
pub const REPO_ENV: &str = "TREQ_E2E_REPO";

/// The repository `REPO_ENV` names, when the e2e suite runs the app.
pub fn repo() -> Option<String> {
  std::env::var(REPO_ENV).ok().filter(|r| !r.is_empty())
}

/// Starts the server when `GPUI_PLAYWRIGHT` asks for it.
pub fn start(cx: &mut App) {
  let Some(server) = gpui_playwright::Server::from_env() else {
    return;
  };
  // A steady caret: under Xvfb's software rendering every blink's repaint
  // kept an idle window at ~15% CPU.
  gpui_kit::base::input::CursorBlinkDefaults::new()
    .with_interval(None)
    .install(cx);
  if let Err(err) = server.on_adopt_page(page_window).start(cx) {
    log::error!("gpui-playwright failed to start: {err}");
  }
}

/// Of the windows no page holds: a repo window, else the active window,
/// else the largest, so a tooltip or menu window is never picked.
fn page_window(free: &[AnyWindowHandle], cx: &mut App) -> Option<AnyWindowHandle> {
  if let Some(shell) = SHELLS.with(|shells| {
    let shells = shells.borrow();
    free.iter().find(|w| shells.contains(w)).copied()
  }) {
    return Some(shell);
  }
  let windows: Vec<(AnyWindowHandle, bool, f32)> = free
    .iter()
    .filter_map(|w| {
      w.update(cx, |_, window, _| {
        let size = window.viewport_size();
        let area = f32::from(size.width) * f32::from(size.height);
        (*w, window.is_window_active(), area)
      })
      .ok()
    })
    .filter(|(_, _, area)| *area >= 300. * 300.)
    .collect();
  windows
    .iter()
    .find(|(_, active, _)| *active)
    .or_else(|| windows.iter().max_by(|a, b| a.2.total_cmp(&b.2)))
    .map(|(w, _, _)| *w)
}
