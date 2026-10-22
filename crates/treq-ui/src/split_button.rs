//! A button with a second half that opens a menu of related commands.

use std::rc::Rc;

use gpui::{div, prelude::*, App, ClickEvent, SharedString, Window};

use crate::{theme, theme::Palette, zpx, Button, OnClick, Size, Variant};

/// The menu half's width.
const MENU_WIDTH: f32 = 26.;

/// One silhouette, two buttons: the command and, after a 1px divider, a
/// chevron that opens its menu. Each half has its own accessible name.
/// Primary or outline only.
#[derive(IntoElement)]
pub struct SplitButton {
  id: SharedString,
  palette: Palette,
  label: SharedString,
  icon: Option<&'static str>,
  menu_label: SharedString,
  outline: bool,
  disabled: bool,
  open: bool,
  on_click: Option<OnClick>,
  on_menu: Option<OnClick>,
}

impl SplitButton {
  pub fn new(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    palette: &Palette,
  ) -> Self {
    let label = label.into();
    Self {
      id: id.into(),
      palette: *palette,
      menu_label: format!("More {label} options").into(),
      label,
      icon: None,
      outline: false,
      disabled: false,
      open: false,
      on_click: None,
      on_menu: None,
    }
  }

  /// A leading icon, in the label's color.
  pub fn icon(mut self, path: &'static str) -> Self {
    self.icon = Some(path);
    self
  }

  /// The menu half's accessible name, `More <label> options` by default.
  pub fn menu_label(mut self, label: impl Into<SharedString>) -> Self {
    self.menu_label = label.into();
    self
  }

  /// The outline style, for a split button that isn't the region's primary.
  pub fn outline(mut self) -> Self {
    self.outline = true;
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }

  /// Whether the menu is open, which draws the menu half pressed.
  pub fn open(mut self, open: bool) -> Self {
    self.open = open;
    self
  }

  pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_click = Some(Rc::new(f));
    self
  }

  /// Opens the menu.
  pub fn on_menu(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_menu = Some(Rc::new(f));
    self
  }
}

impl RenderOnce for SplitButton {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = &self.palette;
    let variant = if self.outline {
      Variant::Outline
    } else {
      Variant::Default
    };
    let divider = if self.outline {
      p.input
    } else {
      p.primary_foreground.opacity(0.3)
    };
    let main = Button::new(self.id.clone(), p)
      .variant(variant)
      .size(Size::Xs)
      .disabled(self.disabled)
      .when_some(self.icon, |b, icon| b.icon(icon, 16.))
      .label(self.label)
      .h(zpx(28.))
      .px_3()
      .gap_1p5()
      .rounded_r_none()
      .when_some(self.on_click, |b, f| {
        b.on_click(move |e, w, cx| f(e, w, cx))
      });
    let menu = Button::new(format!("{}-menu", self.id), p)
      .variant(variant)
      .size(Size::IconXs)
      .disabled(self.disabled)
      .selected(self.open && self.outline)
      .aria_label(self.menu_label)
      .icon(theme::ICON_CHEVRON_DOWN, 14.)
      .w(zpx(MENU_WIDTH))
      .h(zpx(28.))
      .rounded_l_none()
      .when(self.outline, |b| b.border_l_0())
      .when_some(self.on_menu, |b, f| b.on_click(move |e, w, cx| f(e, w, cx)));
    div()
      .flex()
      .flex_none()
      .items_center()
      .child(main)
      .child(div().w_px().h(zpx(28.)).bg(divider))
      .child(menu)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_split_button_builder() {
    let p = theme::palette(false);
    let button = SplitButton::new("push", "Push", &p)
      .icon(theme::ICON_CHEVRON_UP)
      .outline()
      .open(true)
      .disabled(true);
    assert_eq!(button.menu_label.as_ref(), "More Push options");
    let named = SplitButton::new("push", "Push", &p).menu_label("Push options");
    assert_eq!(named.menu_label.as_ref(), "Push options");
  }
}
