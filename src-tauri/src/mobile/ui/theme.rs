//! Theme setup: treq's design system (`treq-ui`) at touch density, following
//! the system light/dark mode.

use std::borrow::Cow;

use gpui_kit::{App, Window, WindowAppearance};
use treq_ui::theme::{self, Density};

// Inter Bold, for the wordmark. treq-ui bundles the other weights. Android's
// only UI font is a single variable Roboto, which the text system treats as
// one regular face, so bold text would not render without it.
const INTER_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-Bold.ttf");

/// Fonts, gpui-kit (initialized here) and both themes in treq's tokens.
pub fn init(cx: &mut App) {
  theme::init(cx);
  if let Err(err) = cx.text_system().add_fonts(vec![Cow::Borrowed(INTER_BOLD)]) {
    log::error!("failed to load bundled Inter Bold: {err:#}");
  }
  theme::set_density(Density::Touch, cx);
}

/// Puts the app in the window's light or dark mode.
pub fn sync_appearance(window: &Window, cx: &mut App) {
  let dark = matches!(
    window.appearance(),
    WindowAppearance::Dark | WindowAppearance::VibrantDark
  );
  theme::set_dark(dark, cx);
}
