//! The window's title bar on macOS, drawn by GPUI as VS Code draws its own.
//! The native bar is a transparent overlay (`open_window`), so only its
//! traffic lights show, over the left of this strip.

use std::rc::Rc;

use gpui_kit::base::TestSupportExt as _;
use tauri_plugin_gpui::gpui::{self, div, prelude::*, px, MouseButton, SharedString, Window};

use super::theme::Palette;

#[cfg(test)]
mod tests;

/// The native title bar's height, which the traffic lights are centred in.
/// Not zoomed: the traffic lights do not follow `ui_zoom`.
pub const HEIGHT: f32 = 28.;
/// Room left of the title for the traffic lights.
const TRAFFIC_LIGHTS_WIDTH: f32 = 78.;

/// What the title bar asks of its window.
pub trait TitleBarHost {
  /// Moves the window with the pointer until the button is released.
  fn start_move(&self, window: &mut Window);
  /// The title-bar double-click action: zoom, or what the user chose in
  /// macOS's Desktop & Dock settings.
  fn double_click(&self, window: &mut Window);
}

/// The window itself, through tauri-plugin-gpui.
pub struct NativeWindow;

impl TitleBarHost for NativeWindow {
  fn start_move(&self, window: &mut Window) {
    window.start_window_move();
  }

  fn double_click(&self, window: &mut Window) {
    window.titlebar_double_click();
  }
}

/// The strip across the top of the window, with the window's title centred.
/// Pressing it moves the window; double-clicking it zooms. In full screen
/// the traffic lights are gone, so the title needs no room for them.
pub fn title_bar(
  title: SharedString,
  p: &Palette,
  fullscreen: bool,
  host: Rc<dyn TitleBarHost>,
) -> impl IntoElement {
  div()
    .id("title-bar")
    .role(gpui::Role::TitleBar)
    .aria_label(title.clone())
    .test_support()
    .flex_none()
    .h(px(HEIGHT))
    .w_full()
    .flex()
    .items_center()
    .justify_center()
    .when(!fullscreen, |this| this.pl(px(TRAFFIC_LIGHTS_WIDTH)))
    .bg(p.sidebar)
    .border_b_1()
    .border_color(p.border)
    .on_mouse_down(MouseButton::Left, move |event, window, _| {
      match event.click_count {
        1 => host.start_move(window),
        2 => host.double_click(window),
        _ => {}
      }
    })
    // Unzoomed, as the strip is.
    .child(
      div()
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(p.muted_foreground)
        .truncate()
        .child(title),
    )
}
