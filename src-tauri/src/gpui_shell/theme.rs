//! Colors, fonts and icons for GPUI views, matching the React app's theme
//! tokens in `src/index.css`.

use std::borrow::Cow;

use tauri_plugin_gpui::gpui::{self, hsla, rgb, App, AssetSource, Hsla, SharedString};

/// Lucide icons (ISC license), the same set the React app draws with, as
/// `icons/<name>.svg` asset paths and their bundled bytes.
macro_rules! icons {
  ($($konst:ident => $name:literal),* $(,)?) => {
    $(pub const $konst: &str = concat!("icons/", $name, ".svg");)*
    const ICONS: &[(&str, &[u8])] = &[
      $(($konst, include_bytes!(concat!("../../assets/gpui-icons/", $name, ".svg"))),)*
    ];
  };
}

icons! {
  ICON_ARCHIVE => "archive",
  ICON_BOT => "bot",
  ICON_BRANCH => "git-branch",
  ICON_CHEVRON_DOWN => "chevron-down",
  ICON_CHEVRON_RIGHT => "chevron-right",
  ICON_GITHUB => "github",
  ICON_HOUSE => "house",
  ICON_LAYERS => "layers-2",
  ICON_LINEAR => "list-todo",
  ICON_SEARCH => "search",
  ICON_SETTINGS => "settings",
  ICON_SPINNER => "loader-circle",
  ICON_TERMINAL => "terminal",
}

// Inter, as in the React app. The TTFs are shared with mobile-gpui.
const INTER_REGULAR: &[u8] = include_bytes!("../../../mobile-gpui/assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../../mobile-gpui/assets/fonts/Inter-Medium.ttf");

/// The React app's mono stack starts with JetBrains Mono, which ships only as
/// a web font; use each platform's own mono face instead.
pub const MONO_FONT: &str = if cfg!(target_os = "macos") {
  "Menlo"
} else if cfg!(target_os = "windows") {
  "Consolas"
} else {
  "DejaVu Sans Mono"
};

/// Serves the bundled icons to GPUI's `svg()` element.
pub struct Assets;

impl AssetSource for Assets {
  fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
    Ok(
      ICONS
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| Cow::Borrowed(*bytes)),
    )
  }

  fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
    Ok(
      ICONS
        .iter()
        .filter(|(name, _)| name.starts_with(path))
        .map(|(name, _)| SharedString::from(*name))
        .collect(),
    )
  }
}

/// App-level GPUI setup: fonts and the default text style.
pub fn init(cx: &mut App) {
  let fonts = vec![Cow::Borrowed(INTER_REGULAR), Cow::Borrowed(INTER_MEDIUM)];
  if let Err(err) = cx.text_system().add_fonts(fonts) {
    log::error!("failed to load bundled Inter fonts: {err:#}");
  }
}

/// The font family GPUI views use for regular text.
pub const SANS_FONT: &str = "Inter";

/// Theme tokens from `src/index.css`, light or dark.
#[derive(Clone, Copy)]
pub struct Palette {
  pub background: Hsla,
  pub foreground: Hsla,
  pub sidebar: Hsla,
  pub sidebar_foreground: Hsla,
  pub accent: Hsla,
  pub muted: Hsla,
  pub muted_foreground: Hsla,
  pub border: Hsla,
  pub primary: Hsla,
  pub destructive: Hsla,
  /// Tailwind yellow-500, the uncommitted-changes dot.
  pub warning: Hsla,
  pub pr_open: Hsla,
  pub pr_merged: Hsla,
  pub pr_closed: Hsla,
  /// The line linking stacked workspaces.
  pub stack_guide: Hsla,
}

/// A CSS `hsl(h s% l%)` token.
fn token(h: f32, s: f32, l: f32) -> Hsla {
  hsla(h / 360., s / 100., l / 100., 1.)
}

pub fn palette(dark: bool) -> Palette {
  if dark {
    Palette {
      background: token(0., 0., 14.),
      foreground: token(0., 0., 95.),
      sidebar: token(0., 0., 18.),
      sidebar_foreground: token(0., 0., 95.),
      accent: token(0., 0., 25.),
      muted: token(0., 0., 25.),
      muted_foreground: token(0., 0., 60.),
      border: token(0., 0., 28.),
      primary: token(211., 100., 55.),
      destructive: token(0., 85., 58.),
      warning: rgb(0xeab308).into(),
      pr_open: rgb(0x4ade80).into(),
      pr_merged: rgb(0xc084fc).into(),
      pr_closed: rgb(0xf87171).into(),
      stack_guide: rgb(0x93c5fd).into(),
    }
  } else {
    Palette {
      background: token(0., 0., 98.),
      foreground: token(0., 0., 13.),
      sidebar: token(220., 13., 95.),
      sidebar_foreground: token(0., 0., 13.),
      accent: token(220., 13., 91.),
      muted: token(220., 13., 95.),
      muted_foreground: token(0., 0., 45.),
      border: token(220., 13., 88.),
      primary: token(211., 100., 50.),
      destructive: token(0., 85., 60.),
      warning: rgb(0xeab308).into(),
      pr_open: rgb(0x16a34a).into(),
      pr_merged: rgb(0x9333ea).into(),
      pr_closed: rgb(0xdc2626).into(),
      stack_guide: rgb(0x1e3a8a).into(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn serves_every_bundled_icon() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/gpui-icons");
    for entry in std::fs::read_dir(dir).expect("icon dir") {
      let name = entry.expect("entry").file_name();
      let path = format!("icons/{}", name.to_string_lossy());
      let bytes = Assets.load(&path).expect("load");
      assert!(bytes.is_some(), "{path} is not registered in ICONS");
    }
    assert!(Assets.load("icons/missing.svg").expect("load").is_none());
  }
}
