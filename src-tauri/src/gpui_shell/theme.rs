//! Colors, fonts and icons for GPUI views, matching the React app's theme
//! tokens in `src/index.css`.

use std::{borrow::Cow, rc::Rc};

use gpui_kit::component as kit;

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
  ICON_AGENT_CLAUDE => "agent-claude",
  ICON_AGENT_CODEX => "agent-codex",
  ICON_AGENT_COPILOT => "agent-copilot",
  ICON_AGENT_CURSOR => "agent-cursor",
  ICON_ARCHIVE => "archive",
  ICON_ARROW_DOWN => "arrow-down",
  ICON_ARROW_LEFT => "arrow-left",
  ICON_ARROW_RIGHT => "arrow-right",
  ICON_ARROW_RIGHT_LEFT => "arrow-right-left",
  ICON_BOOK_OPEN => "book-open",
  ICON_BOT => "bot",
  ICON_BOX => "box",
  ICON_BRANCH => "git-branch",
  ICON_CHECK => "check",
  // At lucide's default stroke, as React's header draws it; the sidebar's
  // is thinner.
  ICON_BRANCH_BOLD => "git-branch-bold",
  ICON_CHEVRON_DOWN => "chevron-down",
  ICON_CHEVRON_RIGHT => "chevron-right",
  ICON_CIRCLE => "circle",
  ICON_CIRCLE_ALERT => "circle-alert",
  ICON_CIRCLE_CHECK => "circle-check",
  ICON_CIRCLE_DOT => "circle-dot",
  ICON_CIRCLE_HELP => "circle-help",
  ICON_CIRCLE_X => "circle-x",
  ICON_CLOCK => "clock",
  ICON_CODE => "code-xml",
  ICON_COMMIT => "git-commit-horizontal",
  ICON_COMPARE => "git-compare-arrows",
  ICON_COPY => "copy",
  ICON_DATABASE => "database",
  ICON_EXTERNAL_LINK => "external-link",
  ICON_FILE => "file",
  ICON_FILE_DIFF => "file-diff",
  ICON_FILE_TEXT => "file-text",
  ICON_FLASK => "flask-conical",
  ICON_FOLDER => "folder",
  ICON_FOLDER_GIT => "folder-git-2",
  ICON_FOLDER_OPEN => "folder-open",
  ICON_FOLDER_PLUS => "folder-plus",
  ICON_FOLDER_SEARCH => "folder-search",
  ICON_GITHUB => "github",
  ICON_HOUSE => "house",
  ICON_INFO => "info",
  ICON_LAPTOP => "laptop",
  ICON_LAYERS => "layers-2",
  ICON_LINEAR => "list-todo",
  ICON_LINK => "link",
  ICON_MERGE => "git-merge",
  ICON_MORE => "ellipsis-vertical",
  ICON_PAPERCLIP => "paperclip",
  ICON_PENCIL => "pencil",
  ICON_PLAY => "play",
  ICON_PLUG => "plug",
  ICON_PLUS => "plus",
  ICON_PR => "git-pull-request",
  ICON_PR_CLOSED => "git-pull-request-closed",
  ICON_PR_DRAFT => "git-pull-request-draft",
  ICON_REFRESH => "refresh-cw",
  ICON_ROTATE_CCW => "rotate-ccw",
  ICON_SEARCH => "search",
  ICON_SERVER => "server",
  ICON_SETTINGS => "settings",
  ICON_SPINNER => "loader-circle",
  ICON_TERMINAL => "terminal",
  ICON_TRASH => "trash-2",
  ICON_TRIANGLE_ALERT => "triangle-alert",
  ICON_UNDO => "undo-2",
  ICON_UPLOAD => "upload",
  ICON_USER => "user",
  ICON_X => "x",
  ICON_ZAP => "zap",
  ICON_MINUS => "minus",
  ICON_EYE => "eye",
  ICON_EYE_OFF => "eye-off",
  ICON_CHEVRON_UP => "chevron-up",
  ICON_SQUARE => "square",
  ICON_MESSAGE => "message-square",
  ICON_MESSAGE_PLUS => "message-square-plus",
  ICON_SEND => "send",
  ICON_WAND => "wand-sparkles",
  ICON_QUOTE => "quote",
  ICON_GLOBE => "globe",
}

// Inter, as in the React app. The TTFs are shared with mobile-gpui.
const INTER_REGULAR: &[u8] = include_bytes!("../../../mobile-gpui/assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../../mobile-gpui/assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] =
  include_bytes!("../../../mobile-gpui/assets/fonts/Inter-SemiBold.ttf");

/// The React app's mono face, converted from its web font
/// (`src/public/fonts/jetbrains-mono-400-latin.woff2`) with the same subset.
const JETBRAINS_MONO_REGULAR: &[u8] =
  include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
/// For `font-mono font-semibold` text. The web app ships no bolder mono
/// face, so the webview synthesizes one from Regular; JetBrains' Bold (same
/// subset), registered as weight 600, looks closest to that.
const JETBRAINS_MONO_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf");

pub const MONO_FONT: &str = "JetBrains Mono";

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

/// gpui-kit's theme in the React app's colors, light and dark (generated
/// from its default theme with the tokens of `src/index.css`).
const KIT_THEME: &str = include_str!("../../assets/gpui-theme.json");

/// App-level GPUI setup: fonts, the default text style, and gpui-kit with
/// the app's theme.
pub fn init(cx: &mut App) {
  let fonts = vec![
    Cow::Borrowed(INTER_REGULAR),
    Cow::Borrowed(INTER_MEDIUM),
    Cow::Borrowed(INTER_SEMIBOLD),
    Cow::Borrowed(JETBRAINS_MONO_REGULAR),
    Cow::Borrowed(JETBRAINS_MONO_SEMIBOLD),
  ];
  if let Err(err) = cx.text_system().add_fonts(fonts) {
    log::error!("failed to load bundled fonts: {err:#}");
  }
  gpui_kit::init(cx);
  init_kit_theme(cx);
}

fn init_kit_theme(cx: &mut App) {
  let set: kit::ThemeSet = match serde_json::from_str(KIT_THEME) {
    Ok(set) => set,
    Err(err) => {
      log::error!("invalid gpui-kit theme: {err}");
      return;
    }
  };
  kit::Theme::change(kit::ThemeMode::Light, None, cx);
  let theme = kit::Theme::global_mut(cx);
  for config in set.themes {
    if config.mode.is_dark() {
      theme.dark_theme = Rc::new(config);
    } else {
      theme.light_theme = Rc::new(config);
    }
  }
  kit::Theme::change(kit::ThemeMode::Light, None, cx);
  let theme = kit::Theme::global_mut(cx);
  theme.font_family = SANS_FONT.into();
  theme.font_size = gpui::px(13.);
  theme.mono_font_family = MONO_FONT.into();
  theme.mono_font_size = gpui::px(13.);
  theme.radius = gpui::px(8.);
  theme.radius_lg = gpui::px(12.);
}

/// Puts gpui-kit's widgets in React's light or dark theme.
pub fn set_kit_dark(dark: bool, cx: &mut App) {
  let mode = if dark {
    kit::ThemeMode::Dark
  } else {
    kit::ThemeMode::Light
  };
  if kit::Theme::global(cx).mode != mode {
    kit::Theme::change(mode, None, cx);
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
  /// Tooltips and menus.
  pub popover: Hsla,
  pub popover_foreground: Hsla,
  /// `--accent`, a menu row's hover; `accent` above is `--sidebar-accent`.
  pub menu_accent: Hsla,
  pub primary: Hsla,
  pub destructive: Hsla,
  /// Tailwind yellow-500, the uncommitted-changes dot.
  pub warning: Hsla,
  pub pr_open: Hsla,
  pub pr_merged: Hsla,
  pub pr_closed: Hsla,
  /// The line linking stacked workspaces.
  pub stack_guide: Hsla,
  /// Toast icons: `text-green-500`, and `text-yellow-600 dark:text-yellow-400`.
  pub toast_success: Hsla,
  pub toast_warning: Hsla,
  pub card: Hsla,
  pub card_foreground: Hsla,
  pub primary_foreground: Hsla,
  pub secondary: Hsla,
  pub secondary_foreground: Hsla,
  pub destructive_foreground: Hsla,
  /// `--input`, the border of outline buttons and text fields.
  pub input: Hsla,
  /// `--ring`, the focus outline.
  #[allow(dead_code)]
  pub ring: Hsla,
  /// Tailwind green-500, a passed check.
  pub success: Hsla,
  /// Picks the Prism theme for highlighted code: vsc-dark-plus or vs.
  pub dark: bool,
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
      popover: token(0., 0., 22.),
      popover_foreground: token(0., 0., 95.),
      menu_accent: token(0., 0., 25.),
      primary: token(211., 100., 55.),
      destructive: token(0., 85., 58.),
      warning: rgb(0xeab308).into(),
      pr_open: rgb(0x4ade80).into(),
      pr_merged: rgb(0xc084fc).into(),
      pr_closed: rgb(0xf87171).into(),
      stack_guide: rgb(0x93c5fd).into(),
      toast_success: rgb(0x22c55e).into(),
      toast_warning: rgb(0xfacc15).into(),
      card: token(0., 0., 20.),
      card_foreground: token(0., 0., 95.),
      primary_foreground: token(0., 0., 100.),
      secondary: token(0., 0., 25.),
      secondary_foreground: token(0., 0., 95.),
      destructive_foreground: token(0., 0., 100.),
      input: token(0., 0., 25.),
      ring: token(211., 100., 55.),
      success: rgb(0x22c55e).into(),
      dark: true,
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
      popover: token(0., 0., 100.),
      popover_foreground: token(0., 0., 13.),
      menu_accent: token(220., 13., 95.),
      primary: token(211., 100., 50.),
      destructive: token(0., 85., 60.),
      warning: rgb(0xeab308).into(),
      pr_open: rgb(0x16a34a).into(),
      pr_merged: rgb(0x9333ea).into(),
      pr_closed: rgb(0xdc2626).into(),
      stack_guide: rgb(0x1e3a8a).into(),
      toast_success: rgb(0x22c55e).into(),
      toast_warning: rgb(0xca8a04).into(),
      card: token(0., 0., 100.),
      card_foreground: token(0., 0., 13.),
      primary_foreground: token(0., 0., 100.),
      secondary: token(220., 13., 95.),
      secondary_foreground: token(0., 0., 13.),
      destructive_foreground: token(0., 0., 100.),
      input: token(220., 13., 91.),
      ring: token(211., 100., 50.),
      success: rgb(0x22c55e).into(),
      dark: false,
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

  #[test]
  fn kit_theme_has_a_light_and_a_dark_theme() {
    let set: kit::ThemeSet = serde_json::from_str(KIT_THEME).expect("parse");
    let modes: Vec<bool> = set.themes.iter().map(|t| t.mode.is_dark()).collect();
    assert_eq!(modes.len(), 2);
    assert!(modes.contains(&true) && modes.contains(&false));
  }

  #[test]
  fn bundles_the_mono_face_by_its_family_name() {
    // TrueType, with MONO_FONT in its (UTF-16BE) name table.
    assert_eq!(&JETBRAINS_MONO_REGULAR[..4], &[0, 1, 0, 0]);
    let family: Vec<u8> = MONO_FONT
      .encode_utf16()
      .flat_map(u16::to_be_bytes)
      .collect();
    assert!(JETBRAINS_MONO_REGULAR
      .windows(family.len())
      .any(|w| w == family.as_slice()));
  }
}

/// Tailwind palette colors views use outside the theme tokens.
pub mod tw {
  use super::{rgb, Hsla};

  pub fn color(hex: u32) -> Hsla {
    rgb(hex).into()
  }

  pub const BLUE_500: u32 = 0x3b82f6;
  pub const BLUE_600: u32 = 0x2563eb;
  pub const EMERALD_500: u32 = 0x10b981;
  pub const AMBER_300: u32 = 0xfcd34d;
  pub const AMBER_500: u32 = 0xf59e0b;
  pub const AMBER_700: u32 = 0xb45309;
  pub const BLUE_300: u32 = 0x93c5fd;
  pub const BLUE_400: u32 = 0x60a5fa;
  pub const BLUE_700: u32 = 0x1d4ed8;
  pub const EMERALD_300: u32 = 0x6ee7b7;
  pub const EMERALD_700: u32 = 0x047857;
  pub const RED_300: u32 = 0xfca5a5;
  pub const RED_400: u32 = 0xf87171;
  pub const RED_700: u32 = 0xb91c1c;
  pub const SKY_300: u32 = 0x7dd3fc;
  pub const SKY_500: u32 = 0x0ea5e9;
  pub const SKY_700: u32 = 0x0369a1;
  pub const ZINC_300: u32 = 0xd4d4d8;
  pub const ZINC_500: u32 = 0x71717a;
  pub const ZINC_700: u32 = 0x3f3f46;
  pub const GREEN_500: u32 = 0x22c55e;
  pub const GREEN_600: u32 = 0x16a34a;
  pub const RED_500: u32 = 0xef4444;
  pub const RED_600: u32 = 0xdc2626;
  pub const YELLOW_50: u32 = 0xfefce8;
  pub const YELLOW_100: u32 = 0xfef9c3;
  pub const YELLOW_500: u32 = 0xeab308;
  pub const YELLOW_950: u32 = 0x422006;
}
