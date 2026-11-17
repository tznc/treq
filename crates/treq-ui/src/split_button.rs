//! A button with a second half that opens a menu of related commands.

use std::rc::Rc;

use gpui::{div, prelude::*, App, ClickEvent, MouseDownEvent, SharedString, Window};

use crate::{theme, theme::Palette, zpx, Button, OnClick, Size, Tone, Variant};

type OnRightClick = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;
type OnHover = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// The menu half's width.
const MENU_WIDTH: f32 = 26.;

/// One silhouette, two buttons: the command and, after a 1px divider, a
/// chevron that opens its menu. Each half has its own accessible name.
/// Primary or outline, or a [`Self::pill`] in a state's tone.
///
/// The pill is the status label that is also a button, such as a pull
/// request's `Open`: fully rounded, tinted in its [`Tone`] like a
/// [`crate::Badge`], with a second half that is a related command (open on
/// the web) rather than a menu.
#[derive(IntoElement)]
pub struct SplitButton {
  id: SharedString,
  palette: Palette,
  label: SharedString,
  aria_label: Option<SharedString>,
  icon: Option<&'static str>,
  menu_label: SharedString,
  menu_icon: &'static str,
  outline: bool,
  pill: Option<Tone>,
  disabled: bool,
  open: bool,
  on_click: Option<OnClick>,
  on_menu: Option<OnClick>,
  on_right_click: Option<OnRightClick>,
  on_hover: Option<OnHover>,
  on_menu_hover: Option<OnHover>,
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
      aria_label: None,
      icon: None,
      menu_icon: theme::ICON_CHEVRON_DOWN,
      outline: false,
      pill: None,
      disabled: false,
      open: false,
      on_click: None,
      on_menu: None,
      on_right_click: None,
      on_hover: None,
      on_menu_hover: None,
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

  /// The command half's accessible name when the label alone is too terse,
  /// such as `View PR (#42, open)` for `Open`.
  pub fn aria_label(mut self, name: impl Into<SharedString>) -> Self {
    self.aria_label = Some(name.into());
    self
  }

  /// The second half's icon, a chevron by default. Give a pill's related
  /// command its own icon, such as an external link.
  pub fn menu_icon(mut self, path: &'static str) -> Self {
    self.menu_icon = path;
    self
  }

  /// A fully rounded status label in `tone`: tinted fill and border, label
  /// and icon in the tone's text color. For a state the user can act on,
  /// not for the region's primary command.
  pub fn pill(mut self, tone: Tone) -> Self {
    self.pill = Some(tone);
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

  /// Opens the menu, or runs a pill's second command.
  pub fn on_menu(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_menu = Some(Rc::new(f));
    self
  }

  /// Runs `f` on a right-click on either half, such as to copy a link.
  pub fn on_right_click(
    mut self,
    f: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
  ) -> Self {
    self.on_right_click = Some(Rc::new(f));
    self
  }

  /// Runs `f` as the pointer enters and leaves the command half.
  pub fn on_hover(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
    self.on_hover = Some(Rc::new(f));
    self
  }

  /// Runs `f` as the pointer enters and leaves the second half.
  pub fn on_menu_hover(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
    self.on_menu_hover = Some(Rc::new(f));
    self
  }
}

impl RenderOnce for SplitButton {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = &self.palette;
    let variant = if self.outline || self.pill.is_some() {
      Variant::Outline
    } else {
      Variant::Default
    };
    let divider = match self.pill {
      Some(tone) => tone.border(p),
      None if self.outline => p.input,
      None => p.primary_foreground.opacity(0.3),
    };
    // A pill paints both halves in its tone; hover deepens the tint.
    let toned = |b: Button| match self.pill {
      Some(tone) => b
        .bg(tone.tint(p))
        .hover_bg(tone.color(p).opacity(if p.dark { 0.25 } else { 0.18 }))
        .border_color(tone.border(p))
        .text_color(tone.text(p)),
      None => b,
    };
    let pill = self.pill.is_some();
    let right_click = self.on_right_click.clone();
    let main = toned(
      Button::new(self.id.clone(), p)
        .variant(variant)
        .size(Size::Xs)
        .disabled(self.disabled),
    )
    .when_some(self.icon, |b, icon| b.icon(icon, 16.))
    .label(self.label)
    .when_some(self.aria_label, |b, name| b.aria_label(name))
    .h(zpx(theme::CONTROL_MD))
    .px_3()
    .gap_1p5()
    .rounded_r_none()
    // The divider is the seam; a border beside it would double it.
    .border_r_0()
    .when(pill, |b| b.rounded_l_full().pl_2p5())
    .when_some(self.on_click, |b, f| {
      b.on_click(move |e, w, cx| f(e, w, cx))
    })
    .when_some(right_click, |b, f| {
      b.on_right_click(move |e, w, cx| f(e, w, cx))
    })
    .when_some(self.on_hover, |b, f| {
      b.on_hover(move |e, w, cx| f(e, w, cx))
    });
    let menu = toned(
      Button::new(format!("{}-menu", self.id), p)
        .variant(variant)
        .size(Size::IconXs)
        .disabled(self.disabled)
        .selected(self.open && self.outline),
    )
    .aria_label(self.menu_label)
    .icon(self.menu_icon, 14.)
    .w(zpx(if pill { MENU_WIDTH + 4. } else { MENU_WIDTH }))
    .h(zpx(theme::CONTROL_MD))
    .rounded_l_none()
    .when(pill, |b| b.rounded_r_full().pr_0p5())
    .border_l_0()
    .when_some(self.on_menu, |b, f| b.on_click(move |e, w, cx| f(e, w, cx)))
    .when_some(self.on_right_click, |b, f| {
      b.on_right_click(move |e, w, cx| f(e, w, cx))
    })
    .when_some(self.on_menu_hover, |b, f| {
      b.on_hover(move |e, w, cx| f(e, w, cx))
    });
    div()
      .flex()
      .flex_none()
      .items_center()
      .child(main)
      .child(div().w_px().h(zpx(theme::CONTROL_MD)).bg(divider))
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

  #[test]
  fn test_split_button_pill() {
    let p = theme::palette(false);
    let pill = SplitButton::new("pr", "Open", &p)
      .pill(Tone::GitOpen)
      .aria_label("View PR (#42, open)")
      .menu_label("Open PR on web")
      .menu_icon(theme::ICON_EXTERNAL_LINK);
    assert_eq!(pill.pill, Some(Tone::GitOpen));
    assert_eq!(pill.aria_label.as_deref(), Some("View PR (#42, open)"));
    assert_eq!(pill.menu_icon, theme::ICON_EXTERNAL_LINK);
  }
}
