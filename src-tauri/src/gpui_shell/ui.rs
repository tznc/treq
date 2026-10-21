//! Building blocks shared by GPUI screens, drawn to match the React app's
//! `src/components/ui` primitives (sizes and colors from its Tailwind
//! classes).

use std::{rc::Rc, time::Duration};

use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, svg, Animation, AnimationExt as _, App, ClickEvent, Div, ElementId, Hsla,
  SharedString, StyleRefinement, Transformation, Window,
};

use gpui_kit::component::input::{Input, InputState};

use super::theme::{self, Palette};

/// The rem size at 100% `ui_zoom`, as the webview's root font size.
pub const BASE_REM: f32 = 16.;

/// `v` CSS pixels as rems, so the length follows the window's rem size,
/// which `ui_zoom` scales the way React's CSS `zoom` scaled every length.
pub fn zpx(v: f32) -> gpui::Rems {
  gpui::rems(v / BASE_REM)
}

/// `v` CSS pixels at the window's zoom, for what needs `Pixels`: painting,
/// bounds and viewport math.
pub fn zoomed(v: f32, window: &Window) -> gpui::Pixels {
  window.rem_size() * (v / BASE_REM)
}

/// `ui_zoom` in percent, app-wide, for every GPUI window: the shell's, its
/// dialogs' and its popups'.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct UiZoom(pub u32);

impl gpui::Global for UiZoom {}

/// The zoom as a factor (1.0 at 100%).
pub fn zoom_factor(cx: &App) -> f32 {
  cx.try_global::<UiZoom>().map_or(100, |z| z.0) as f32 / 100.
}

/// Sizes `window`'s rems at the app's zoom. Call at the top of a window's
/// root `render`.
pub fn apply_zoom(window: &mut Window, cx: &App) {
  window.set_rem_size(gpui::px(BASE_REM * zoom_factor(cx)));
}

/// A click handler that GPUI elements can share.
pub type OnClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Tailwind's text scale as `tailwind.config.js` overrides it: size and line
/// height in pixels.
#[derive(Clone, Copy)]
pub enum TextSize {
  Xs,
  Sm,
  Lg,
}

impl TextSize {
  pub fn metrics(self) -> (f32, f32) {
    match self {
      TextSize::Xs => (11., 16.),
      TextSize::Sm => (13., 20.),
      TextSize::Lg => (15., 22.),
    }
  }
}

/// Sets the font size and line height of `el` from the Tailwind scale.
pub fn text<E: Styled>(el: E, size: TextSize) -> E {
  let (font, line) = size.metrics();
  el.text_size(zpx(font)).line_height(zpx(line))
}

/// A single-line input at `text-sm`. gpui-kit's own medium size is 14px,
/// Tailwind's `text-sm` here 13px, so placeholders and text came out larger.
pub fn input(state: &gpui::Entity<InputState>) -> Input {
  Input::new(state).text_size(zpx(13.))
}

/// A Lucide icon from the bundled set.
pub fn icon(path: &'static str, size: f32, color: Hsla) -> gpui::Svg {
  svg()
    .path(path)
    .size(zpx(size))
    .flex_none()
    .text_color(color)
}

/// The spinning `loader-circle` (`animate-spin`).
#[allow(dead_code)] // The full set, as in React; screens still to port use the rest.
pub fn spinner(id: impl Into<ElementId>, size: f32, color: Hsla) -> impl IntoElement {
  spin(id, icon(theme::ICON_SPINNER, size, color))
}

/// Spins `icon` once a second, as Tailwind's `animate-spin` does.
pub fn spin(id: impl Into<ElementId>, icon: gpui::Svg) -> impl IntoElement {
  icon.with_animation(
    id,
    Animation::new(Duration::from_secs(1)).repeat(),
    |svg, delta| svg.with_transformation(Transformation::rotate(gpui::percentage(delta))),
  )
}

/// `ui/button.tsx` variants.
#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // The full set, as in React; screens still to port use the rest.
pub enum Variant {
  Default,
  Destructive,
  Ghost,
  Link,
  Outline,
  Secondary,
}

/// `ui/button.tsx` sizes.
#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // The full set, as in React; screens still to port use the rest.
pub enum Size {
  Default,
  Icon,
  IconXs,
  Lg,
  Sm,
  Xs,
}

/// `ui/button.tsx`. Its id is also its debug selector, which tests find it
/// by. Style overrides (a `className` in React) go through
/// [`Styled`] and win over the variant's own styles.
#[derive(IntoElement)]
pub struct Button {
  id: SharedString,
  palette: Palette,
  variant: Variant,
  size: Size,
  disabled: bool,
  children: Vec<gpui::AnyElement>,
  on_click: Option<OnClick>,
  style: StyleRefinement,
  hover_bg: Option<Hsla>,
}

impl Button {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      variant: Variant::Default,
      size: Size::Default,
      disabled: false,
      children: Vec::new(),
      on_click: None,
      style: StyleRefinement::default(),
      hover_bg: None,
    }
  }

  /// The background on hover, in place of the variant's (`hover:bg-…`).
  pub fn hover_bg(mut self, color: Hsla) -> Self {
    self.hover_bg = Some(color);
    self
  }

  pub fn variant(mut self, variant: Variant) -> Self {
    self.variant = variant;
    self
  }

  pub fn size(mut self, size: Size) -> Self {
    self.size = size;
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }

  /// Text in the button's own color.
  pub fn label(self, label: impl Into<SharedString>) -> Self {
    let label: SharedString = label.into();
    self.child(label)
  }

  /// A Lucide icon in the button's text color.
  pub fn icon(self, path: &'static str, size: f32) -> Self {
    let color = self.foreground();
    self.child(icon(path, size, color))
  }

  pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_click = Some(Rc::new(f));
    self
  }

  /// The text color for this variant.
  pub fn foreground(&self) -> Hsla {
    let p = &self.palette;
    match self.variant {
      Variant::Default => p.primary_foreground,
      Variant::Destructive => p.destructive_foreground,
      Variant::Ghost | Variant::Outline => p.foreground,
      Variant::Link => p.primary,
      Variant::Secondary => p.secondary_foreground,
    }
  }
}

impl ParentElement for Button {
  fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
    self.children.extend(elements);
  }
}

impl Styled for Button {
  fn style(&mut self) -> &mut StyleRefinement {
    &mut self.style
  }
}

impl RenderOnce for Button {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let fg = self.foreground();
    let base = div()
      .flex()
      .flex_none()
      .items_center()
      .justify_center()
      .gap_2()
      .whitespace_nowrap()
      .rounded(zpx(8.))
      .font_weight(gpui::FontWeight::MEDIUM)
      .text_color(fg);
    let base = match self.size {
      Size::Default => text(base.h(zpx(40.)).px_4().py_2(), TextSize::Sm),
      Size::Icon => base.size(zpx(40.)),
      Size::IconXs => base.size(zpx(24.)),
      Size::Lg => text(base.h(zpx(44.)).px_6(), TextSize::Lg),
      Size::Sm => text(base.h(zpx(32.)).px_3(), TextSize::Sm),
      Size::Xs => text(base.h(zpx(24.)).px_2(), TextSize::Sm),
    };
    let disabled = self.disabled;
    let base = match self.variant {
      Variant::Default => base.bg(p.primary).shadow_sm(),
      Variant::Destructive => base.bg(p.destructive).shadow_sm(),
      Variant::Ghost | Variant::Link => base,
      Variant::Outline => base.border_1().border_color(p.input).bg(p.background),
      Variant::Secondary => base.bg(p.secondary),
    };
    let hover_bg = self.hover_bg.or(match self.variant {
      Variant::Default => Some(p.primary.opacity(0.85)),
      Variant::Destructive => Some(p.destructive.opacity(0.85)),
      Variant::Ghost | Variant::Outline => Some(p.accent),
      Variant::Link => None,
      Variant::Secondary => Some(p.secondary.opacity(0.7)),
    });
    let link = self.variant == Variant::Link;
    let selector = self.id.to_string();
    let mut el = base.id(self.id).debug_selector(move || selector);
    el.style().refine(&self.style);
    // GPUI has no text decoration on hover for a whole subtree, so a link
    // keeps its color and only underlines its text.
    el.when(!disabled, |b| {
      b.hover(move |s| match hover_bg {
        Some(bg) => s.bg(bg),
        None if link => s.underline(),
        None => s,
      })
    })
    .when(disabled, |b| b.opacity(0.4))
    .when_some(self.on_click.filter(|_| !disabled), |b, f| {
      b.cursor_pointer().on_click(move |e, w, cx| f(e, w, cx))
    })
    .children(self.children)
  }
}

/// `ui/card.tsx`'s `Card`: a bordered, rounded surface.
pub fn card(p: &Palette) -> Div {
  div()
    .rounded(zpx(12.))
    .border_1()
    .border_color(p.border.opacity(0.5))
    .bg(p.card)
    .text_color(p.card_foreground)
}
