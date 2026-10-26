//! Colors, fonts and icons for GPUI views, matching the React app's theme
//! tokens in `src/index.css`.

use std::{borrow::Cow, rc::Rc};

use gpui_kit::component as kit;

use gpui::{hsla, rgb, App, AssetSource, Hsla, SharedString};

use crate as ui;

/// Lucide icons (ISC license), the same set the React app draws with, as
/// `icons/<name>.svg` asset paths and their bundled bytes.
macro_rules! icons {
  ($($konst:ident => $name:literal),* $(,)?) => {
    $(pub const $konst: &str = concat!("icons/", $name, ".svg");)*
    const ICONS: &[(&str, &[u8])] = &[
      $(($konst, include_bytes!(concat!("../../../src-tauri/assets/gpui-icons/", $name, ".svg"))),)*
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
  ICON_BRANCH_PLUS => "git-branch-plus",
  ICON_CHEVRON_DOWN => "chevron-down",
  ICON_CHEVRON_LEFT => "chevron-left",
  ICON_CHEVRONS_UP_DOWN => "chevrons-up-down",
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
  ICON_FILTER => "funnel",
  ICON_FOLDER => "folder",
  ICON_FOLDER_GIT => "folder-git-2",
  ICON_FOLDER_OPEN => "folder-open",
  ICON_FOLDER_PLUS => "folder-plus",
  ICON_FOLDER_SEARCH => "folder-search",
  ICON_GITHUB => "github",
  ICON_HISTORY => "history",
  ICON_HOUSE => "house",
  ICON_INFO => "info",
  ICON_LAPTOP => "laptop",
  ICON_LAYERS => "layers-2",
  ICON_LINEAR => "linear",
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
  ICON_TERMINAL_SQUARE => "terminal-square",
  ICON_TRASH => "trash-2",
  ICON_TRIANGLE_ALERT => "triangle-alert",
  ICON_UNDO => "undo-2",
  ICON_UPLOAD => "upload",
  ICON_USER => "user",
  ICON_X => "x",
  ICON_ZAP => "zap",
  ICON_ZOOM_IN => "zoom-in",
  ICON_ZOOM_OUT => "zoom-out",
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
  ICON_SMILE_PLUS => "smile-plus",
  ICON_GLOBE => "globe",
  ICON_TABLE => "table-2",
  ICON_FILE_CODE => "file-code-2",
  ICON_SQUARE_CHECK => "square-check-big",
  ICON_MAXIMIZE => "maximize-2",
  ICON_MINIMIZE => "minimize-2",
  ICON_CLOUD => "cloud",
  ICON_CREDIT_CARD => "credit-card",
  ICON_CROWN => "crown",
  ICON_LOG_OUT => "log-out",
  ICON_WRENCH => "wrench",
  ICON_APP_WINDOW => "app-window",
  ICON_FILE_SEARCH => "file-search",
  ICON_LIST_CHECKS => "list-checks",
  ICON_SHIELD_CHECK => "shield-check",
  ICON_ROTATE_CW => "rotate-cw",
  ICON_LIST_FILTER => "list-filter",
  ICON_SPARKLES => "sparkles",
  ICON_SCROLL_TEXT => "scroll-text",
  ICON_ROCKET => "rocket",
}

// Inter, as in the React app. The TTFs are shared with the mobile app.
const INTER_REGULAR: &[u8] = include_bytes!("../../../src-tauri/assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../../src-tauri/assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../src-tauri/assets/fonts/Inter-SemiBold.ttf");

/// The React app's mono face, converted from its web font
/// (`src/public/fonts/jetbrains-mono-400-latin.woff2`) with the same subset.
const JETBRAINS_MONO_REGULAR: &[u8] =
  include_bytes!("../../../src-tauri/assets/fonts/JetBrainsMono-Regular.ttf");
/// For `font-mono font-semibold` text. The web app ships no bolder mono
/// face, so the webview synthesizes one from Regular; JetBrains' Bold (same
/// subset), registered as weight 600, looks closest to that.
const JETBRAINS_MONO_SEMIBOLD: &[u8] =
  include_bytes!("../../../src-tauri/assets/fonts/JetBrainsMono-Bold.ttf");

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
const KIT_THEME: &str = include_str!("../../../src-tauri/assets/gpui-theme.json");

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
  // gpui-kit's input takes ⌘/Ctrl+Enter itself and, when multi-line, inserts
  // a newline, so the key never reaches the submit handlers in the screens.
  cx.bind_keys([gpui::KeyBinding::new(
    "secondary-enter",
    gpui::NoAction,
    Some("Input"),
  )]);
  // Each blink repaints the window; at gpui-kit's 500ms that cost was
  // noticeable while idling in a focused input.
  gpui_kit::base::input::CursorBlinkDefaults::new()
    .with_interval(CARET_BLINK)
    .install(cx);
  init_kit_theme(cx);
  install(false, 1., cx);
}

/// How long the caret in a focused input stays shown, then hidden.
const CARET_BLINK: std::time::Duration = std::time::Duration::from_secs(1);

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
  set_kit_metrics(cx);
}

/// How big gpui-kit's widgets draw text: 13 on the desktop, 15 on a phone,
/// which is read at arm's length and tapped rather than clicked.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Density {
  #[default]
  Desktop,
  Touch,
}

impl gpui::Global for Density {}

impl Density {
  /// gpui-kit's body and mono text sizes.
  pub fn font_sizes(self) -> (f32, f32) {
    match self {
      Density::Desktop => (13., 13.),
      Density::Touch => (15., 12.),
    }
  }
}

/// Sizes gpui-kit's widgets for `density`.
pub fn set_density(density: Density, cx: &mut App) {
  cx.set_global(density);
  set_kit_metrics(cx);
}

/// The fonts and radii treq gives gpui-kit's widgets over its theme file.
fn set_kit_metrics(cx: &mut App) {
  let (body, mono) = cx
    .try_global::<Density>()
    .copied()
    .unwrap_or_default()
    .font_sizes();
  let theme = kit::Theme::global_mut(cx);
  theme.font_family = SANS_FONT.into();
  theme.font_size = gpui::px(body);
  theme.mono_font_family = MONO_FONT.into();
  theme.mono_font_size = gpui::px(mono);
  theme.radius = gpui::px(RADIUS_MD);
  theme.radius_lg = gpui::px(RADIUS_LG);
}

/// The installed theme's palette.
pub fn current(cx: &App) -> Palette {
  palette(cx.try_global::<Installed>().is_some_and(|i| i.dark))
}

/// Puts the app's widgets, gpui-kit's and gpui-bootstrap's, in the light or
/// dark theme at the app's zoom. Cheap when neither changed.
pub fn set_dark(dark: bool, cx: &mut App) {
  install(dark, ui::zoom_factor(cx), cx);
}

/// Re-sizes gpui-bootstrap's theme after the app's zoom changed, as its
/// lengths are pixels rather than the rems `ui::zpx` gives.
pub fn sync_zoom(cx: &mut App) {
  let dark = cx.try_global::<Installed>().is_some_and(|i| i.dark);
  install(dark, ui::zoom_factor(cx), cx);
}

/// The light/dark mode and zoom the installed themes were built for.
#[derive(Clone, Copy, PartialEq)]
struct Installed {
  dark: bool,
  zoom: f32,
}

impl gpui::Global for Installed {}

fn install(dark: bool, zoom: f32, cx: &mut App) {
  let next = Installed { dark, zoom };
  if cx.try_global::<Installed>() == Some(&next) {
    return;
  }
  cx.set_global(next);
  // Writes its colors over gpui-kit's theme, so gpui-kit's own theme file
  // goes back on after it: kit widgets keep the colors they had.
  bootstrap(dark, zoom).apply(cx);
  let mode = if dark {
    kit::ThemeMode::Dark
  } else {
    kit::ThemeMode::Light
  };
  kit::Theme::change(mode, None, cx);
  set_kit_metrics(cx);
}

/// Corner radii: `sm` for kbd, checkboxes and menu rows; `md` for buttons,
/// inputs, menus, popovers and tooltips; `lg` for cards, toasts and dialogs.
pub const RADIUS_SM: f32 = 4.;
pub const RADIUS_MD: f32 = 8.;
pub const RADIUS_LG: f32 = 12.;

/// Control heights: `sm` inside rows, `md` in toolbars and headers (the
/// default), `lg` in dialog footers and forms.
pub const CONTROL_SM: f32 = 24.;
pub const CONTROL_MD: f32 = 28.;
pub const CONTROL_LG: f32 = 32.;

/// gpui-bootstrap's theme in treq's tokens, at `zoom` (1.0 at 100%).
///
/// Every gpui-bootstrap component the `ui` facade builds on reads it: its
/// colors come from [`palette`], its type scale is `ui::TextSize`'s, and its
/// control sizes are [`CONTROL_SM`], [`CONTROL_MD`] and [`CONTROL_LG`].
pub fn bootstrap(dark: bool, zoom: f32) -> gpui_bootstrap::Theme {
  use gpui_bootstrap::{ColorMode, ControlSize, ControlSizes, Platform, Radii, Theme, Typography};

  let p = palette(dark);
  let z = |v: f32| gpui::px(v * zoom);
  let mode = if dark {
    ColorMode::Dark
  } else {
    ColorMode::Light
  };
  let mut theme = Theme::platform(Platform::Web, mode);
  let c = &mut theme.colors;
  c.primary = p.primary;
  c.secondary = p.muted_foreground;
  c.success = p.success;
  c.info = p.primary;
  c.warning = p.warning;
  c.danger = p.destructive;
  c.light = p.muted;
  c.dark = p.foreground;
  c.body_bg = p.background;
  c.body_color = p.foreground;
  c.emphasis_color = p.foreground;
  c.secondary_color = p.muted_foreground;
  c.secondary_bg = p.muted;
  c.tertiary_color = p.muted_foreground;
  c.tertiary_bg = p.accent;
  c.border_color = p.border;
  c.border_color_translucent = p.border;
  c.link_color = p.primary;
  c.link_hover_color = p.primary;
  c.backdrop = gpui::black().opacity(if dark { 0.55 } else { 0.4 });
  theme.radius = Radii {
    sm: z(RADIUS_SM),
    base: z(RADIUS_MD),
    lg: z(RADIUS_LG),
    xl: z(RADIUS_LG),
    xxl: z(RADIUS_LG),
    pill: z(9999.),
  };
  let (body, body_line) = ui::TextSize::Sm.metrics();
  let (caption, _) = ui::TextSize::Xs.metrics();
  let (title, _) = ui::TextSize::Lg.metrics();
  theme.typography = Typography {
    font_family: SANS_FONT.into(),
    font_family_mono: MONO_FONT.into(),
    font_size_base: z(body),
    // Small controls keep body text; captions are for badges and meta.
    font_size_sm: z(body),
    font_size_lg: z(title),
    line_height: body_line / body,
    font_weight_normal: gpui::FontWeight::NORMAL,
    font_weight_medium: gpui::FontWeight::MEDIUM,
    font_weight_semibold: gpui::FontWeight::SEMIBOLD,
    font_weight_bold: gpui::FontWeight::BOLD,
    headings: [z(20.), z(title), z(body), z(body), z(body), z(caption)],
    headings_font_weight: gpui::FontWeight::SEMIBOLD,
  };
  let control = |height: f32, padding_x: f32| ControlSize {
    padding_x: z(padding_x),
    padding_y: z((height - body_line) / 2. - 1.),
    min_height: z(height),
  };
  theme.controls = ControlSizes {
    small: control(CONTROL_SM, 8.),
    medium: control(CONTROL_MD, 10.),
    large: control(CONTROL_LG, 12.),
    button_radius: Some(z(RADIUS_MD)),
  };
  theme.spacer = z(16.);
  theme.border_width = gpui::px(1.);
  theme.focus_ring_width = z(2.);
  theme.focus_ring_opacity = 0.55;
  // The `ui` facade gives disabled controls muted colors instead of fading
  // the whole element, which washed labels out over translucent fills.
  theme.disabled_opacity = 1.;
  // White on the primary blue is about 3.9:1, Bootstrap's 4.5 would pick
  // black; treq's buttons have always been white on blue.
  theme.min_contrast_ratio = 3.;
  theme
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
  /// Text on a `warning` fill: dark in both themes, as amber is light.
  pub warning_foreground: Hsla,
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
  pub ring: Hsla,
  /// Tailwind green-500, a passed check.
  pub success: Hsla,
  /// Picks the Prism theme for highlighted code: vsc-dark-plus or vs.
  pub dark: bool,
}

/// A CSS `hsl(h s% l%)` token.
impl Palette {
  /// The one hover fill: rows, ghost buttons and menu items.
  pub fn hover(&self) -> Hsla {
    self.accent
  }

  /// The one selected fill: rows, toggles and open triggers.
  pub fn selected(&self) -> Hsla {
    self.primary.opacity(if self.dark { 0.22 } else { 0.12 })
  }

  /// Behind every modal dialog, and behind controls laid over an image.
  pub fn scrim(&self) -> Hsla {
    gpui::black().opacity(if self.dark { 0.55 } else { 0.4 })
  }

  /// Behind file contents and code blocks: `--code-background`.
  pub fn code_background(&self) -> Hsla {
    if self.dark {
      token(0., 0., 14.)
    } else {
      token(220., 13., 94.)
    }
  }

  /// The GitHub button's fill, GitHub's own near-black in both themes.
  pub fn github(&self) -> Hsla {
    tw::color(tw::GITHUB)
  }
}

/// A color that arrives with the data, such as a GitHub or Linear label's,
/// or a terminal's color scheme. UI chrome uses [`Palette`] tokens instead.
pub fn data_color(hex: u32) -> Hsla {
  rgb(hex).into()
}

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
      warning_foreground: token(0., 0., 9.),
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
      warning_foreground: token(0., 0., 9.),
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
    let dir =
      std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/assets/gpui-icons");
    for entry in std::fs::read_dir(dir).expect("icon dir") {
      let name = entry.expect("entry").file_name();
      let path = format!("icons/{}", name.to_string_lossy());
      let bytes = Assets.load(&path).expect("load");
      assert!(bytes.is_some(), "{path} is not registered in ICONS");
    }
    assert!(Assets.load("icons/missing.svg").expect("load").is_none());
  }

  #[test]
  fn bootstrap_theme_uses_the_palette_at_the_zoom() {
    for dark in [false, true] {
      let p = palette(dark);
      let theme = bootstrap(dark, 1.5);
      assert_eq!(theme.colors.primary, p.primary);
      assert_eq!(theme.colors.body_bg, p.background);
      assert_eq!(theme.colors.danger, p.destructive);
      assert_eq!(theme.radius.base, gpui::px(RADIUS_MD * 1.5));
      assert_eq!(theme.controls.medium.min_height, gpui::px(CONTROL_MD * 1.5));
      // White text on the primary blue, as treq's buttons have always had.
      assert_eq!(
        theme.button(gpui_bootstrap::Variant::Primary).color,
        theme.colors.white
      );
    }
  }

  #[gpui::test]
  fn bootstrap_theme_leaves_gpui_kits_colors(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
      gpui_kit::init(cx);
      init_kit_theme(cx);
      let light = kit::Theme::global(cx).colors;
      install(false, 1.25, cx);
      assert_eq!(kit::Theme::global(cx).colors, light);
      assert_eq!(kit::Theme::global(cx).radius, gpui::px(RADIUS_MD));
      let bootstrap = gpui_bootstrap::Theme::global(cx);
      assert_eq!(bootstrap.radius.base, gpui::px(RADIUS_MD * 1.25));
      install(true, 1.25, cx);
      assert!(kit::Theme::global(cx).mode.is_dark());
      assert_eq!(
        gpui_bootstrap::Theme::global(cx).colors.body_bg,
        palette(true).background
      );
    });
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

  pub const GITHUB: u32 = 0x24292f;
  pub const BLUE_500: u32 = 0x3b82f6;
  pub const BLUE_600: u32 = 0x2563eb;
  pub const EMERALD_500: u32 = 0x10b981;
  pub const AMBER_200: u32 = 0xfde68a;
  pub const AMBER_300: u32 = 0xfcd34d;
  pub const AMBER_500: u32 = 0xf59e0b;
  pub const AMBER_700: u32 = 0xb45309;
  pub const AMBER_800: u32 = 0x92400e;
  pub const BLUE_300: u32 = 0x93c5fd;
  pub const BLUE_400: u32 = 0x60a5fa;
  pub const BLUE_700: u32 = 0x1d4ed8;
  pub const EMERALD_300: u32 = 0x6ee7b7;
  pub const EMERALD_700: u32 = 0x047857;
  pub const RED_300: u32 = 0xfca5a5;
  pub const RED_400: u32 = 0xf87171;
  pub const RED_700: u32 = 0xb91c1c;
  pub const SKY_300: u32 = 0x7dd3fc;
  pub const SKY_400: u32 = 0x38bdf8;
  pub const SKY_500: u32 = 0x0ea5e9;
  pub const SKY_600: u32 = 0x0284c7;
  pub const SKY_700: u32 = 0x0369a1;
  pub const VIOLET_400: u32 = 0xa78bfa;
  pub const VIOLET_500: u32 = 0x8b5cf6;
  pub const VIOLET_600: u32 = 0x7c3aed;
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
  pub const YELLOW_300: u32 = 0xfde047;
  pub const AMBER_400: u32 = 0xfbbf24;
  pub const AMBER_600: u32 = 0xd97706;
  pub const RED_50: u32 = 0xfef2f2;
  pub const RED_200: u32 = 0xfecaca;
  pub const RED_900: u32 = 0x7f1d1d;
  pub const RED_950: u32 = 0x450a0a;
  pub const YELLOW_950: u32 = 0x422006;
  pub const GREEN_400: u32 = 0x4ade80;
  pub const GREEN_700: u32 = 0x15803d;
  pub const YELLOW_400: u32 = 0xfacc15;
  pub const YELLOW_700: u32 = 0xa16207;
  pub const YELLOW_600: u32 = 0xca8a04;
}
