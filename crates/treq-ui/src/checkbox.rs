//! A checkbox: 13 square, the primary color when on.

use gpui::{div, prelude::*, App, Div, Interactivity, SharedString, Stateful, Window};

use crate::{icon, radius, theme, theme::Palette, zpx};

/// What a [`Checkbox`] shows. `Mixed` is a parent whose children are only
/// partly checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CheckState {
  #[default]
  Off,
  On,
  Mixed,
}

impl From<bool> for CheckState {
  fn from(checked: bool) -> Self {
    if checked {
      CheckState::On
    } else {
      CheckState::Off
    }
  }
}

/// The box alone; put its label beside it in the row that handles the
/// click, so the label toggles it too.
#[derive(IntoElement)]
pub struct Checkbox {
  base: Stateful<Div>,
  palette: Palette,
  state: CheckState,
  disabled: bool,
}

impl Checkbox {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    let id: SharedString = id.into();
    let selector = id.to_string();
    Self {
      base: div().id(id).debug_selector(move || selector),
      palette: *palette,
      state: CheckState::Off,
      disabled: false,
    }
  }

  pub fn checked(mut self, state: impl Into<CheckState>) -> Self {
    self.state = state.into();
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }
}

impl InteractiveElement for Checkbox {
  fn interactivity(&mut self) -> &mut Interactivity {
    self.base.interactivity()
  }
}

impl StatefulInteractiveElement for Checkbox {}

impl Styled for Checkbox {
  fn style(&mut self) -> &mut gpui::StyleRefinement {
    self.base.style()
  }
}

impl RenderOnce for Checkbox {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let mark = match self.state {
      CheckState::Off => None,
      CheckState::On => Some(theme::ICON_CHECK),
      CheckState::Mixed => Some(theme::ICON_MINUS),
    };
    self
      .base
      .size(zpx(13.))
      .flex_none()
      .flex()
      .items_center()
      .justify_center()
      .rounded(radius::SM)
      .when(self.disabled, |this| this.opacity(0.5))
      .map(|this| match mark {
        Some(mark) => this
          .bg(p.primary)
          .child(icon(mark, 11., p.primary_foreground)),
        None => this
          .border_1()
          .border_color(p.muted_foreground)
          .bg(p.background),
      })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_checkbox_builder() {
    let p = theme::palette(false);
    let checkbox = Checkbox::new("agree", &p)
      .checked(CheckState::Mixed)
      .disabled(true);
    assert_eq!(checkbox.state, CheckState::Mixed);
    assert!(checkbox.disabled);
    assert_eq!(CheckState::from(true), CheckState::On);
  }
}
