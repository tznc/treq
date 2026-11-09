//! A square button that shows only an icon.

use gpui::{prelude::*, App, ClickEvent, MouseDownEvent, SharedString, Window};

use crate::{theme::Palette, zpx, Button, Size, Tone, Variant};

/// How big an [`IconButton`] is: 24 inside rows, 28 in toolbars and headers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum IconButtonSize {
  Small,
  #[default]
  Medium,
}

impl IconButtonSize {
  /// The button's side and its icon's, in pixels.
  pub fn metrics(self) -> (f32, f32) {
    match self {
      IconButtonSize::Small => (24., 14.),
      IconButtonSize::Medium => (28., 16.),
    }
  }

  /// [`IconButtonSize::metrics`] at `density`: on a phone every icon button
  /// is a 44 target with a 20 icon.
  pub fn metrics_at(self, density: crate::theme::Density) -> (f32, f32) {
    match density {
      crate::theme::Density::Touch => (crate::theme::TOUCH_TARGET, 20.),
      crate::theme::Density::Desktop => self.metrics(),
    }
  }
}

/// An icon-only [`Button`]: ghost in toolbars, outline beside fields. The
/// label is required, as it is the button's accessible name and the text
/// its tooltip shows.
#[derive(IntoElement)]
pub struct IconButton {
  button: Button,
  icon: &'static str,
  size: IconButtonSize,
  selected: bool,
  copied: Option<crate::copy_feedback::Copied>,
  tone: Option<Tone>,
  palette: Palette,
}

impl IconButton {
  pub fn new(
    id: impl Into<SharedString>,
    icon: &'static str,
    label: impl Into<SharedString>,
    palette: &Palette,
  ) -> Self {
    Self {
      button: Button::new(id, palette)
        .variant(Variant::Ghost)
        .size(Size::IconXs)
        .aria_label(label),
      icon,
      size: IconButtonSize::default(),
      selected: false,
      copied: None,
      tone: None,
      palette: *palette,
    }
  }

  /// A filled button in the primary colour: the one control that sends or
  /// submits, such as a prompt's Send.
  pub fn primary(mut self) -> Self {
    self.button = self.button.variant(Variant::Default);
    self
  }

  /// A bordered button, for one that sits beside a field.
  pub fn outline(mut self) -> Self {
    self.button = self.button.variant(Variant::Outline);
    self
  }

  pub fn size(mut self, size: IconButtonSize) -> Self {
    self.size = size;
    self
  }

  /// A toggle that is on, such as the panel it shows being open.
  pub fn selected(mut self, selected: bool) -> Self {
    self.selected = selected;
    self
  }

  /// A copy button: its copy icon turns into the green check while
  /// `copied`. Pair it with [`crate::copy_feedback::copyable`].
  pub fn copied(mut self, copied: impl Into<crate::copy_feedback::Copied>) -> Self {
    self.copied = Some(copied.into());
    self
  }

  /// Colours the icon with a state, such as danger for a command that
  /// deletes. Selected keeps the primary colour.
  pub fn tone(mut self, tone: Tone) -> Self {
    self.tone = Some(tone);
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.button = self.button.disabled(disabled);
    self
  }

  pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.button = self.button.on_click(f);
    self
  }

  pub fn on_right_click(
    mut self,
    f: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
  ) -> Self {
    self.button = self.button.on_right_click(f);
    self
  }

  /// Runs `f` as the pointer enters and leaves, to show the tooltip.
  pub fn on_hover(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
    self.button = self.button.on_hover(f);
    self
  }
}

impl RenderOnce for IconButton {
  fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
    let (side, icon) = self.size.metrics_at(crate::theme::Density::current(cx));
    let p = self.palette;
    self
      .button
      .selected(self.selected)
      .map(|b| match self.copied {
        Some(copied) => b.copy_icon(copied, icon),
        None => b.icon(self.icon, icon),
      })
      .w(zpx(side))
      .h(zpx(side))
      .when_some(self.tone, |b, tone| b.text_color(tone.color(&p)))
      .when(self.selected, |b| b.text_color(p.primary))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::theme::Density;

  #[test]
  fn a_touch_icon_button_is_a_44_target() {
    assert_eq!(
      IconButtonSize::Medium.metrics_at(Density::Touch),
      (44., 20.)
    );
    assert_eq!(IconButtonSize::Small.metrics_at(Density::Touch), (44., 20.));
    assert_eq!(
      IconButtonSize::Small.metrics_at(Density::Desktop),
      (24., 14.)
    );
  }
  use crate::theme;

  #[test]
  fn test_icon_button_builder() {
    let p = theme::palette(false);
    let button = IconButton::new("close", theme::ICON_X, "Close", &p)
      .outline()
      .size(IconButtonSize::Small)
      .selected(true)
      .tone(Tone::Danger)
      .disabled(true);
    assert_eq!(button.size.metrics(), (24., 14.));
    assert!(button.selected);
    assert_eq!(button.tone, Some(Tone::Danger));
    assert_eq!(IconButtonSize::default().metrics(), (28., 16.));
  }

  #[test]
  fn test_icon_button_primary_fills_with_the_primary_variant() {
    let p = theme::palette(false);
    let button = IconButton::new("send", theme::ICON_SEND, "Send", &p).primary();
    assert!(button.button.variant == Variant::Default);
  }
}
