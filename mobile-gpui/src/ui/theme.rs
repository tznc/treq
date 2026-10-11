//! Theme setup: follow the system light/dark mode, phone-sized type.

use std::borrow::Cow;

use gpui_kit::{component::Theme, px, App};

/// The Treq logo blue (sampled from `assets/logo.svg`'s PNG render), as 0xRRGGBB.
pub const BRAND_BLUE: u32 = 0x4B97E3;

// Static Inter weights. Android's only UI font is a single variable Roboto,
// which the text system treats as one regular face, so bold text would not
// render. Bundling a static family gives real weights on every platform.
const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");
const INTER_BOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-Bold.ttf");

pub fn init(cx: &mut App) {
  // Fonts first, so the family resolves when the theme is applied.
  let fonts = [INTER_REGULAR, INTER_MEDIUM, INTER_SEMIBOLD, INTER_BOLD]
    .into_iter()
    .map(Cow::Borrowed)
    .collect();
  if let Err(err) = cx.text_system().add_fonts(fonts) {
    log::error!("failed to load bundled Inter fonts: {err:#}");
  }
  Theme::sync_system_appearance(None, cx);
  let theme = Theme::global_mut(cx);
  theme.font_family = "Inter".into();
  theme.font_size = px(15.);
  theme.mono_font_size = px(12.);
  #[cfg(target_os = "android")]
  {
    theme.mono_font_family = "Droid Sans Mono".into();
  }
}
