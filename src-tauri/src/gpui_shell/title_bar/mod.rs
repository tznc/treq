//! The title bar on macOS, VS Code-style: the native bar is a transparent
//! overlay (`open_window`), so only its traffic lights show. They sit in a
//! strip GPUI draws at the top of the sidebar; the screen beside it runs to
//! the window's top edge, as Zen's page does.

use std::rc::Rc;

use gpui_kit::base::TestSupportExt as _;
use tauri_plugin_gpui::gpui::{self, div, prelude::*, px, MouseButton, Window};

#[cfg(test)]
mod tests;

/// The title bar exists only where the traffic lights overlay the window.
pub const SHOWN: bool = cfg!(target_os = "macos");

/// The native title bar's height, which the traffic lights are centred in.
/// Not zoomed: the traffic lights do not follow `ui_zoom`.
pub const HEIGHT: f32 = 28.;

/// What the title bar asks of its window.
pub trait TitleBarHost {
  /// Moves the window with the pointer until the button is released.
  fn start_move(&self, window: &mut Window);
  /// The title-bar double-click action: zoom, or what the user chose in
  /// macOS's Desktop & Dock settings.
  fn double_click(&self, window: &mut Window);
}

/// The window itself, through tauri-plugin-gpui.
struct NativeWindow;

impl TitleBarHost for NativeWindow {
  fn start_move(&self, window: &mut Window) {
    window.start_window_move();
  }

  fn double_click(&self, window: &mut Window) {
    window.titlebar_double_click();
  }
}

/// An empty strip, as tall as the native bar, that moves the window when
/// pressed and zooms it when double-clicked. Unstyled: it shows what is
/// behind it.
pub fn drag_area(id: &'static str) -> impl IntoElement {
  drag_area_with(id, Rc::new(NativeWindow))
}

fn drag_area_with(id: &'static str, host: Rc<dyn TitleBarHost>) -> impl IntoElement {
  div()
    .id(id)
    .role(gpui::Role::TitleBar)
    .aria_label("Title bar")
    .test_support()
    .flex_none()
    .h(px(HEIGHT))
    .w_full()
    .on_mouse_down(MouseButton::Left, move |event, window, _| {
      match event.click_count {
        1 => host.start_move(window),
        2 => host.double_click(window),
        _ => {}
      }
    })
}
