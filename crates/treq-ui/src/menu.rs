//! Menu rows, for menus drawn in a view: a [`crate::Popover`] of
//! [`MenuItem`]s. Popup-window menus draw the same rows.

use gpui::{div, prelude::*, App, Div, FontWeight, Interactivity, SharedString, Stateful, Window};
use gpui_kit::base::TestSupportExt as _;

use crate::{icon, text, theme, theme::Palette, zpx, Kbd, TextSize};

/// One command: icon 16, label, then its shortcut. A description puts a
/// muted line under the label, for choices that need explaining. The current choice in a
/// menu of values gets a check. Destructive items go last, after a
/// [`MenuSeparator`].
#[derive(IntoElement)]
pub struct MenuItem {
  base: Stateful<Div>,
  palette: Palette,
  label: SharedString,
  description: Option<SharedString>,
  icon: Option<&'static str>,
  shortcut: Option<SharedString>,
  checked: Option<bool>,
  danger: bool,
  disabled: bool,
}

impl MenuItem {
  pub fn new(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    palette: &Palette,
  ) -> Self {
    let id: SharedString = id.into();
    let selector = id.to_string();
    Self {
      base: div().id(id).debug_selector(move || selector),
      palette: *palette,
      label: label.into(),
      description: None,
      icon: None,
      shortcut: None,
      checked: None,
      danger: false,
      disabled: false,
    }
  }

  /// A muted second line, such as what a mode does.
  pub fn description(mut self, description: impl Into<SharedString>) -> Self {
    self.description = Some(description.into());
    self
  }

  pub fn icon(mut self, path: &'static str) -> Self {
    self.icon = Some(path);
    self
  }

  /// The keystroke that runs it, as GPUI binds it (`secondary-c`).
  pub fn shortcut(mut self, keystroke: impl Into<SharedString>) -> Self {
    self.shortcut = Some(keystroke.into());
    self
  }

  /// For a menu of values: whether this one is current. Items in such a
  /// menu keep a check's room so their labels line up.
  pub fn checked(mut self, checked: bool) -> Self {
    self.checked = Some(checked);
    self
  }

  /// Destroys something, such as deleting a workspace.
  pub fn danger(mut self) -> Self {
    self.danger = true;
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }
}

impl InteractiveElement for MenuItem {
  fn interactivity(&mut self) -> &mut Interactivity {
    self.base.interactivity()
  }
}

impl StatefulInteractiveElement for MenuItem {}

impl RenderOnce for MenuItem {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let hover = p.hover();
    let fg = if self.disabled {
      p.muted_foreground.opacity(0.6)
    } else if self.danger {
      p.destructive
    } else {
      p.popover_foreground
    };
    let icon_color = if self.danger && !self.disabled {
      p.destructive
    } else {
      p.muted_foreground
    };
    let description = self.description;
    text(self.base, TextSize::Sm)
      .role(gpui::Role::MenuItem)
      .aria_label(self.label.clone())
      .test_support()
      .flex()
      .gap_2()
      .px_2()
      .map(|el| match description {
        Some(_) => el.items_start().py_1p5(),
        None => el.items_center().h(zpx(28.)),
      })
      .rounded(zpx(theme::RADIUS_SM))
      .text_color(fg)
      .when(!self.disabled, |el| {
        el.cursor_pointer().hover(move |s| s.bg(hover))
      })
      .when_some(self.checked, |el, checked| {
        el.child(
          div()
            .size(zpx(16.))
            .flex_none()
            .when(checked, |el| el.child(icon(theme::ICON_CHECK, 16., fg))),
        )
      })
      .when_some(self.icon, |el, path| el.child(icon(path, 16., icon_color)))
      .child(match description {
        Some(description) => div()
          .flex_1()
          .min_w_0()
          .flex()
          .flex_col()
          .gap_0p5()
          .child(div().font_weight(FontWeight::MEDIUM).child(self.label))
          .child(
            text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child(description),
          ),
        None => div().flex_1().min_w_0().truncate().child(self.label),
      })
      .when_some(self.shortcut, |el, keystroke| {
        el.child(
          text(div(), TextSize::Xs)
            .text_color(p.muted_foreground)
            .child(Kbd::label(&keystroke)),
        )
      })
  }
}

/// The line between groups of menu items.
#[derive(IntoElement)]
pub struct MenuSeparator {
  palette: Palette,
}

impl MenuSeparator {
  pub fn new(palette: &Palette) -> Self {
    Self { palette: *palette }
  }
}

impl RenderOnce for MenuSeparator {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    div().my_1().mx(zpx(-4.)).h_px().bg(self.palette.border)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_menu_item_builder() {
    let p = theme::palette(false);
    let item = MenuItem::new("delete", "Delete workspace…", &p)
      .icon(theme::ICON_X)
      .description("Removes its worktree too")
      .shortcut("secondary-backspace")
      .checked(false)
      .danger()
      .disabled(true);
    assert!(item.danger && item.disabled);
    assert_eq!(item.checked, Some(false));
    assert_eq!(
      item.description.as_deref(),
      Some("Removes its worktree too")
    );
    assert_eq!(item.shortcut.as_deref(), Some("secondary-backspace"));
  }
}
