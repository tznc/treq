//! Treq's design system: its tokens ([`theme`]) and the components every
//! GPUI screen builds from, on gpui-bootstrap re-themed with those tokens.
//! Screens compose these; they don't draw controls from `div()`.
//!
//! `cargo run -p treq-ui --example styleguide` shows every component.

pub mod alert;
pub mod badge;
pub mod checkbox;
pub mod dialog;
pub mod empty_state;
pub mod icon_button;
pub mod kbd;
pub mod menu;
pub mod picker;
pub mod popover;
pub mod row;
pub mod split_button;
pub mod tabs;
pub mod theme;

pub use alert::{Alert, Severity};
pub use badge::{Badge, Count, Dot, Tone};
pub use checkbox::{CheckState, Checkbox};
pub use dialog::{ConfirmDialog, Dialog};
pub use empty_state::{EmptyState, Loading, Skeleton};
pub use icon_button::{IconButton, IconButtonSize};
pub use kbd::Kbd;
pub use menu::{MenuItem, MenuLabel, MenuSeparator};
pub use picker::{Picker, PickerPlacement};
pub use popover::Popover;
pub use row::{Row, RowDensity};
pub use split_button::SplitButton;
pub use tabs::{Tab, Tabs, TabsStyle};

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};

use gpui::{
  accesskit, div, prelude::*, svg, Animation, AnimationExt as _, AnyElement, App, Bounds,
  ClickEvent, Context, Div, ElementId, GlobalElementId, Hsla, InspectorElementId, Interactivity,
  LayoutId, MouseButton, MouseDownEvent, Pixels, SharedString, StyleRefinement, Transformation,
  Window,
};

use gpui_kit::component::input::{Input, InputState};

use theme::Palette;

/// The rem size at 100% `ui_zoom`, as the webview's root font size.
pub const BASE_REM: f32 = 16.;

/// The corner radii, as rems so they follow UI zoom:
/// `.rounded(radius::MD)`.
pub mod radius {
  use gpui::Rems;

  use crate::{theme, BASE_REM};

  /// Kbd, checkboxes, menu and list rows.
  pub const SM: Rems = Rems(theme::RADIUS_SM / BASE_REM);
  /// Buttons, inputs, menus, popovers and tooltips.
  pub const MD: Rems = Rems(theme::RADIUS_MD / BASE_REM);
  /// Cards, toasts and dialogs.
  pub const LG: Rems = Rems(theme::RADIUS_LG / BASE_REM);
}

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

/// A right-click handler, given where the pointer went down.
pub type OnRightClick = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;

/// Told whether the pointer is over the element, such as to show a tooltip.
pub type OnHover = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// The app's text scale (Tailwind's names, as the React UI overrode them):
/// size and line height in pixels.
#[derive(Clone, Copy)]
pub enum TextSize {
  /// Captions, badges and kbd.
  Xs,
  /// Body text and controls.
  Sm,
  /// Titles.
  Lg,
  /// Headings.
  Xl,
  /// Code, paths and aligned digits, in the mono font.
  Code,
}

impl TextSize {
  pub fn metrics(self) -> (f32, f32) {
    match self {
      TextSize::Xs => (11., 16.),
      TextSize::Sm => (13., 20.),
      TextSize::Lg => (15., 22.),
      TextSize::Xl => (20., 28.),
      TextSize::Code => (12., 20.),
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
pub fn spinner(id: impl Into<ElementId>, size: f32, color: Hsla) -> impl IntoElement {
  spin(id, icon(theme::ICON_SPINNER, size, color))
}

/// How often a spinner redraws. Each of its frames draws the whole window,
/// and half the display's rate still turns smoothly.
pub const SPIN_FPS: f32 = 30.;

/// How often a pulse redraws. Its opacity moves by half in a second, so a
/// dozen steps a second look the same as sixty, for a fifth of the frames.
pub const PULSE_FPS: f32 = 12.;

/// Spins `icon` once a second, as Tailwind's `animate-spin` does.
pub fn spin(id: impl Into<ElementId>, icon: gpui::Svg) -> impl IntoElement {
  icon.with_animation(
    id,
    Animation::new(Duration::from_secs(1))
      .repeat()
      .with_max_fps(SPIN_FPS),
    |svg, delta| svg.with_transformation(Transformation::rotate(gpui::percentage(delta))),
  )
}

/// How long a load runs before its spinner or skeleton shows: one that
/// lands sooner draws nothing rather than flashing it.
pub const SPINNER_DELAY: Duration = Duration::from_millis(150);

/// `placeholder` (a spinner, skeleton or "Loading..." line), hidden for its
/// first [`SPINNER_DELAY`], so a fast load flashes nothing. It takes its room
/// from the start, so nothing moves when it shows. Once shown it also
/// answers to the debug selector `{id}-shown`.
pub fn delayed<E>(id: impl Into<SharedString>, placeholder: E) -> gpui::AnimationElement<E>
where
  E: Styled + InteractiveElement + IntoElement + 'static,
{
  let id: SharedString = id.into();
  let shown = format!("{id}-shown");
  placeholder.with_animation(
    ElementId::Name(id),
    // One frame when the delay is up, rather than one every frame while
    // nothing shows.
    Animation::new(SPINNER_DELAY).with_max_fps(1. / SPINNER_DELAY.as_secs_f32()),
    move |el, delta| {
      if delta < 1. {
        el.opacity(0.)
      } else {
        let shown = shown.clone();
        el.debug_selector(move || shown)
      }
    },
  )
}

/// `ui/button.tsx` variants.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
  Default,
  Destructive,
  Ghost,
  Link,
  Outline,
  Secondary,
  /// Filled amber: an action that brings back something that drifted
  /// (Resync after a local edit). Not for errors; those are `Destructive`.
  Warning,
}

/// `ui/button.tsx` sizes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Size {
  Default,
  Icon,
  IconXs,
  Lg,
  Sm,
  Xs,
}

/// A [`Button`] child. Icons take the button's color when it renders, so
/// they fade with it whichever order `icon` and `disabled` were called in.
enum ButtonChild {
  Element(gpui::AnyElement),
  Icon(&'static str, f32),
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
  children: Vec<ButtonChild>,
  on_click: Option<OnClick>,
  on_right_click: Option<OnRightClick>,
  on_hover: Option<OnHover>,
  selected: bool,
  /// On or off, for a toggle.
  pressed: Option<bool>,
  style: StyleRefinement,
  hover_bg: Option<Hsla>,
  /// The accessible name: the first label, unless set with `aria_label`.
  name: Option<SharedString>,
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
      on_right_click: None,
      on_hover: None,
      selected: false,
      pressed: None,
      style: StyleRefinement::default(),
      hover_bg: None,
      name: None,
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

  /// Drawn on, in the one selected style: a toggle that is on, or the
  /// trigger of a menu that is open.
  pub fn selected(mut self, selected: bool) -> Self {
    self.selected = selected;
    self
  }

  /// A toggle, on or off: drawn in the selected style while on, and read
  /// out as pressed or not.
  pub fn pressed(mut self, on: bool) -> Self {
    self.pressed = Some(on);
    self.selected = on;
    self
  }

  /// Text in the button's own color.
  pub fn label(mut self, label: impl Into<SharedString>) -> Self {
    let label: SharedString = label.into();
    self.name.get_or_insert_with(|| label.clone());
    self.child(label)
  }

  /// The accessible name, for an icon button or one whose label is not its
  /// first text (React's `aria-label`).
  pub fn aria_label(mut self, name: impl Into<SharedString>) -> Self {
    self.name = Some(name.into());
    self
  }

  /// A Lucide icon in the button's text color.
  pub fn icon(mut self, path: &'static str, size: f32) -> Self {
    self.children.push(ButtonChild::Icon(path, size));
    self
  }

  pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_click = Some(Rc::new(f));
    self
  }

  /// Runs `f` on a right-click, such as to open a context menu. It runs
  /// while disabled too, as a menu that only copies does no harm.
  pub fn on_right_click(
    mut self,
    f: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
  ) -> Self {
    self.on_right_click = Some(Rc::new(f));
    self
  }

  /// Runs `f` as the pointer enters and leaves, such as to show a tooltip.
  /// It runs while disabled too, so a tooltip can say why.
  pub fn on_hover(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
    self.on_hover = Some(Rc::new(f));
    self
  }

  /// The text color for this variant, muted when disabled. Set `disabled`
  /// before reading it.
  pub fn foreground(&self) -> Hsla {
    if self.disabled {
      self.disabled_foreground()
    } else {
      self.variant_foreground()
    }
  }

  fn filled(&self) -> bool {
    matches!(
      self.variant,
      Variant::Default | Variant::Destructive | Variant::Warning
    )
  }

  fn variant_foreground(&self) -> Hsla {
    let p = &self.palette;
    match self.variant {
      Variant::Default => p.primary_foreground,
      Variant::Destructive => p.destructive_foreground,
      Variant::Ghost | Variant::Outline => p.foreground,
      Variant::Link => p.primary,
      Variant::Secondary => p.secondary_foreground,
      Variant::Warning => p.warning_foreground,
    }
  }
}

impl ParentElement for Button {
  fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
    self
      .children
      .extend(elements.into_iter().map(ButtonChild::Element));
  }
}

impl Styled for Button {
  fn style(&mut self) -> &mut StyleRefinement {
    &mut self.style
  }
}

impl Button {
  /// The colors of each interaction state, as gpui-bootstrap's
  /// `button-variant()` takes them. A disabled button keeps its shape: a
  /// filled one swaps its fill for the neutral muted one, whatever its
  /// variant or a caller's fill, so no state colour shows on a control that
  /// can't act; an unfilled one fades its border, and the label goes muted.
  fn colors(&self) -> gpui_bootstrap::ButtonColors {
    let p = &self.palette;
    let clear = gpui::transparent_black();
    let (bg, border, hover_bg) = match self.variant {
      Variant::Default => (p.primary, p.primary, p.primary.opacity(0.85)),
      Variant::Destructive => (p.destructive, p.destructive, p.destructive.opacity(0.85)),
      Variant::Ghost => (clear, clear, p.accent),
      Variant::Link => (clear, clear, clear),
      Variant::Outline => (p.background, p.input, p.accent),
      Variant::Secondary => (p.secondary, p.secondary, p.secondary.opacity(0.7)),
      Variant::Warning => (p.warning, p.warning, p.warning.opacity(0.85)),
    };
    let hover_bg = self.hover_bg.unwrap_or(hover_bg);
    let (bg, hover_bg) = if self.selected && !self.filled() {
      (p.selected(), p.selected())
    } else {
      (bg, hover_bg)
    };
    let color = self.variant_foreground();
    let (disabled_bg, disabled_border) = if self.filled() {
      (p.muted, p.muted)
    } else {
      (bg, border.opacity(0.5))
    };
    gpui_bootstrap::ButtonColors {
      bg,
      border,
      color,
      hover_bg,
      hover_border: border,
      hover_color: color,
      // Pressing keeps the hover look.
      active_bg: hover_bg,
      active_border: border,
      active_color: color,
      focus_ring: p.ring.opacity(0.55),
      disabled_bg,
      disabled_border,
      disabled_color: self.disabled_foreground(),
    }
  }

  /// The label color while disabled. Over the muted fill the muted color
  /// alone reads as disabled; over the page it needs fading too, or an
  /// outline button looks enabled.
  fn disabled_foreground(&self) -> Hsla {
    let muted = self.palette.muted_foreground;
    if self.filled() {
      muted
    } else {
      muted.opacity(0.6)
    }
  }
}

impl RenderOnce for Button {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let colors = self.colors();
    let fg = self.foreground();
    let disabled = self.disabled;
    let filled = self.filled();
    let style = if disabled {
      // A caller's fill or label color is muted too.
      let mut style = self.style.clone();
      if filled {
        style.background = style.background.take().map(|fill| match fill {
          gpui::Fill::Color(_) => gpui::Fill::Color(self.palette.muted.into()),
        });
      }
      style.border_color = style.border_color.map(|c| c.opacity(0.5));
      style.text.color = Some(fg);
      style.box_shadow = None;
      style
    } else {
      self.style.clone()
    };
    // Icons follow the label, including a `text_color` override.
    let icon_color = style.text.color.unwrap_or(fg);
    let selector = self.id.to_string();
    let mut el = gpui_bootstrap::button::Button::new(self.id)
      .colors(colors)
      .disabled(disabled)
      // As before the facade: a press leaves focus in the editor or list
      // the button acts on.
      .focusable(false)
      .when(self.variant == Variant::Link, |b| {
        b.button_style(gpui_bootstrap::button::ButtonStyle::Link)
      })
      .when_some(self.name, |b, name| b.accessibility_label(name))
      .when_some(self.pressed, |b, on| {
        b.aria_toggled(if on {
          gpui::accesskit::Toggled::True
        } else {
          gpui::accesskit::Toggled::False
        })
      })
      .debug_selector(move || selector)
      .whitespace_nowrap()
      .font_weight(gpui::FontWeight::MEDIUM)
      .rounded(zpx(8.))
      .p_0()
      .min_h_0()
      .when(self.variant != Variant::Outline, |b| b.border_0())
      .map(|b| match self.size {
        Size::Default => text(b.h(zpx(40.)).px_4(), TextSize::Sm),
        Size::Icon => b.size(zpx(40.)),
        Size::IconXs => b.size(zpx(24.)),
        Size::Lg => text(b.h(zpx(44.)).px_6(), TextSize::Lg),
        Size::Sm => text(b.h(zpx(32.)).px_3(), TextSize::Sm),
        Size::Xs => text(b.h(zpx(24.)).px_2(), TextSize::Sm),
      })
      .when(filled && !disabled, |b| b.shadow_sm())
      .when_some(self.on_click.filter(|_| !disabled), |b, f| {
        b.cursor_pointer().on_click(move |e, w, cx| f(e, w, cx))
      })
      .when_some(self.on_right_click, |b, f| {
        b.on_mouse_down(MouseButton::Right, move |e, w, cx| f(e, w, cx))
      })
      .when_some(self.on_hover, |b, f| {
        b.on_hover(move |e, w, cx| f(e, w, cx))
      })
      .children(self.children.into_iter().map(|child| match child {
        ButtonChild::Element(el) => el,
        ButtonChild::Icon(path, size) => icon(path, size, icon_color).into_any_element(),
      }));
    el.style().refine(&style);
    el
  }
}

/// Closes a click-to-open menu and says whether it was open.
type CloseMenu = Rc<dyn Fn(&mut App) -> bool>;

thread_local! {
  /// What closes each click-to-open menu a view draws, by the menu's key,
  /// for [`escape_menus`]. Each says whether its menu was open; one that no
  /// longer is drops out on the next Escape.
  static MENUS: RefCell<HashMap<SharedString, CloseMenu>> = RefCell::default();
}

/// Closes the open click-to-open menus for an Escape the shell window took,
/// whatever had focus: a click on a menu's button blurs the window, so the
/// menu never sees the key itself. Whether one was open.
pub fn escape_menus(cx: &mut App) -> bool {
  let menus: Vec<_> = MENUS.with(|menus| {
    menus
      .borrow()
      .iter()
      .map(|(key, close)| (key.clone(), close.clone()))
      .collect()
  });
  let mut closed = false;
  for (key, close) in menus {
    if close(cx) {
      closed = true;
    } else {
      MENUS.with(|menus| menus.borrow_mut().remove(&key));
    }
  }
  closed
}

/// A click-to-open menu that a press outside it and Escape both close, as
/// base-ui's `DropdownMenu` and `Popover` do.
pub trait DismissExt: InteractiveElement + Sized {
  /// `close` closes the menu `key` names and says whether it was open.
  fn dismiss_with<V: 'static>(
    self,
    key: impl Into<SharedString>,
    cx: &mut Context<V>,
    close: impl Fn(&mut V, &mut Context<V>) -> bool + 'static,
  ) -> Self {
    let close = Rc::new(close);
    let view = cx.weak_entity();
    let on_escape = close.clone();
    MENUS.with(|menus| {
      menus.borrow_mut().insert(
        key.into(),
        Rc::new(move |cx: &mut App| {
          view
            .update(cx, |view, cx| on_escape(view, cx))
            .unwrap_or(false)
        }),
      )
    });
    self.on_mouse_down_out(cx.listener(move |view, _, _, cx| {
      close(view, cx);
    }))
  }
}

impl<E: InteractiveElement> DismissExt for E {}

/// Whether `keystroke` is a plain Escape, the key that closes the topmost
/// layer. For a dispatcher that routes every key; a dialog or panel uses
/// [`EscapeExt::on_escape`].
pub fn is_escape(keystroke: &gpui::Keystroke) -> bool {
  let m = keystroke.modifiers;
  keystroke.key == "escape" && !(m.control || m.alt || m.shift || m.platform || m.function)
}

/// Escape for the focused dialog, sheet or panel: `f` runs on a plain
/// Escape, which stops there, so only the topmost layer closes.
pub trait EscapeExt: InteractiveElement + Sized {
  fn on_escape(self, f: impl Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_key_down(move |e: &gpui::KeyDownEvent, window, cx| {
      if is_escape(&e.keystroke) {
        cx.stop_propagation();
        f(e, window, cx);
      }
    })
  }
}

impl<E: InteractiveElement> EscapeExt for E {}

/// Marks an element's accessibility node disabled, as `aria-disabled` did:
/// GPUI has no setter for it, so a disabled button reached screen readers,
/// and the e2e suite's `toBeDisabled`, as enabled. Call it before
/// `test_support`, so the observation sees the flag.
pub trait A11yDisabledExt: Sized {
  fn a11y_disabled(self, disabled: bool) -> A11yDisabled<Self>;
}

impl<E: Element + InteractiveElement> A11yDisabledExt for E {
  fn a11y_disabled(self, disabled: bool) -> A11yDisabled<Self> {
    A11yDisabled {
      inner: self,
      disabled,
    }
  }
}

/// See [`A11yDisabledExt`]. It forwards everything to the element it wraps
/// and adds no layout node.
pub struct A11yDisabled<E> {
  inner: E,
  disabled: bool,
}

impl<E: Element> IntoElement for A11yDisabled<E> {
  type Element = Self;

  fn into_element(self) -> Self {
    self
  }
}

impl<E: Element> Element for A11yDisabled<E> {
  type RequestLayoutState = E::RequestLayoutState;
  type PrepaintState = E::PrepaintState;

  fn id(&self) -> Option<ElementId> {
    self.inner.id()
  }

  fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
    self.inner.source_location()
  }

  fn request_layout(
    &mut self,
    id: Option<&GlobalElementId>,
    inspector: Option<&InspectorElementId>,
    window: &mut Window,
    cx: &mut App,
  ) -> (LayoutId, Self::RequestLayoutState) {
    self.inner.request_layout(id, inspector, window, cx)
  }

  fn prepaint(
    &mut self,
    id: Option<&GlobalElementId>,
    inspector: Option<&InspectorElementId>,
    bounds: Bounds<Pixels>,
    layout: &mut Self::RequestLayoutState,
    window: &mut Window,
    cx: &mut App,
  ) -> Self::PrepaintState {
    self
      .inner
      .prepaint(id, inspector, bounds, layout, window, cx)
  }

  fn paint(
    &mut self,
    id: Option<&GlobalElementId>,
    inspector: Option<&InspectorElementId>,
    bounds: Bounds<Pixels>,
    layout: &mut Self::RequestLayoutState,
    prepaint: &mut Self::PrepaintState,
    window: &mut Window,
    cx: &mut App,
  ) {
    self
      .inner
      .paint(id, inspector, bounds, layout, prepaint, window, cx)
  }

  fn a11y_role(&self) -> Option<gpui::Role> {
    self.inner.a11y_role()
  }

  fn write_a11y_info(&self, node: &mut accesskit::Node) {
    self.inner.write_a11y_info(node);
    if self.disabled {
      node.set_disabled();
    }
  }

  fn a11y_synthetic_children(
    &mut self,
    prepaint: &mut Self::PrepaintState,
    builder: &mut gpui::A11ySubtreeBuilder,
  ) {
    self.inner.a11y_synthetic_children(prepaint, builder);
  }
}

impl<E: InteractiveElement> InteractiveElement for A11yDisabled<E> {
  fn interactivity(&mut self) -> &mut Interactivity {
    self.inner.interactivity()
  }
}

impl<E: StatefulInteractiveElement> StatefulInteractiveElement for A11yDisabled<E> {}

impl<E: Styled> Styled for A11yDisabled<E> {
  fn style(&mut self) -> &mut StyleRefinement {
    self.inner.style()
  }
}

impl<E: ParentElement> ParentElement for A11yDisabled<E> {
  fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
    self.inner.extend(elements);
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn escape_is_plain_escape_only() {
    let key = |s| gpui::Keystroke::parse(s).unwrap();
    assert!(is_escape(&key("escape")));
    assert!(!is_escape(&key("shift-escape")));
    assert!(!is_escape(&key("enter")));
  }

  #[test]
  fn a_pressed_button_is_a_toggle_in_the_selected_style() {
    let p = theme::palette(false);
    let off = Button::new("off", &p).pressed(false);
    assert_eq!(off.pressed, Some(false));
    assert!(!off.selected);
    let on = Button::new("on", &p)
      .variant(Variant::Outline)
      .pressed(true);
    assert_eq!(on.pressed, Some(true));
    assert_eq!(on.colors().bg, p.selected());
    // A plain button is no toggle.
    assert_eq!(Button::new("plain", &p).pressed, None);
  }

  #[test]
  fn warning_button_is_filled_with_a_dark_label() {
    let p = theme::palette(false);
    let warning = Button::new("warning", &p).variant(Variant::Warning);
    assert!(warning.filled());
    assert_eq!(warning.foreground(), p.warning_foreground);
    assert_eq!(
      theme::palette(true).warning_foreground,
      p.warning_foreground
    );
  }

  #[test]
  fn disabled_button_mutes_its_label_and_icons() {
    let p = theme::palette(false);
    let primary = Button::new("primary", &p);
    assert_eq!(primary.foreground(), p.primary_foreground);
    // An opaque label over the muted fill, so the fill can't wash it out.
    let primary = primary.disabled(true);
    assert_eq!(primary.foreground(), p.muted_foreground);
    let outline = Button::new("outline", &p)
      .variant(Variant::Outline)
      .disabled(true);
    assert_eq!(outline.foreground(), p.muted_foreground.opacity(0.6));
    // An icon added before `disabled` still takes the disabled color, since
    // it is colored when the button renders.
    let icon_first = Button::new("icon", &p)
      .icon(theme::ICON_COMPARE, 16.)
      .disabled(true);
    assert!(matches!(icon_first.children[..], [ButtonChild::Icon(..)]));
  }
}
