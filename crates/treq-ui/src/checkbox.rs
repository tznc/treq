//! A checkbox: 13 square, the primary color when on.

use gpui::{
  div, prelude::*, App, ClickEvent, Div, Interactivity, KeyboardButton, MouseButton, Role,
  SharedString, Stateful, Window,
};

use crate::{icon, radius, theme, theme::Palette, zpx, OnClick};

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

/// The box, with its [`Checkbox::label`] beside it. Given
/// [`Checkbox::on_click`], the box and label are one control: a tab stop
/// named by the label that Space toggles. Without it the box is drawn for a
/// row that handles the click (a [`crate::Row`] with [`Role::CheckBox`]).
#[derive(IntoElement)]
pub struct Checkbox {
  id: SharedString,
  base: Stateful<Div>,
  palette: Palette,
  state: CheckState,
  disabled: bool,
  label: Option<SharedString>,
  on_click: Option<OnClick>,
}

impl Checkbox {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    let id: SharedString = id.into();
    let selector = id.to_string();
    Self {
      id: id.clone(),
      base: div().id(id).debug_selector(move || selector),
      palette: *palette,
      state: CheckState::Off,
      disabled: false,
      label: None,
      on_click: None,
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

  /// The text beside the box, and its accessible name.
  pub fn label(mut self, label: impl Into<SharedString>) -> Self {
    self.label = Some(label.into());
    self
  }

  /// Toggles it, from a click on the box or its label or Space while it
  /// has focus. A click leaves focus where it was.
  pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_click = Some(std::rc::Rc::new(f));
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
  fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
    let p = self.palette;
    let mark = match self.state {
      CheckState::Off => None,
      CheckState::On => Some(theme::ICON_CHECK),
      CheckState::Mixed => Some(theme::ICON_MINUS),
    };
    let on_click = self.on_click.filter(|_| !self.disabled);
    let focus = on_click.as_ref().map(|_| {
      let key = SharedString::from(format!("{}-focus", self.id));
      crate::tab_stop(key, window, cx)
    });
    let focused = focus.as_ref().is_some_and(|f| f.is_focused(window));
    let square = div()
      .size(zpx(13.))
      .flex_none()
      .flex()
      .items_center()
      .justify_center()
      .rounded(radius::SM)
      .when(focused, |this| {
        this.shadow(vec![crate::focus_ring(&p, false)])
      })
      .map(|this| match mark {
        Some(mark) => this
          .bg(p.primary)
          .child(icon(mark, 11., p.primary_foreground)),
        None => this
          .border_1()
          .border_color(p.muted_foreground)
          .bg(p.background),
      });
    let toggled = match self.state {
      CheckState::Off => gpui::accesskit::Toggled::False,
      CheckState::On => gpui::accesskit::Toggled::True,
      CheckState::Mixed => gpui::accesskit::Toggled::Mixed,
    };
    let control = on_click.is_some() || self.label.is_some();
    // Its defaults go first, so styles set on it (a gap, a text size) win.
    let user: gpui::StyleRefinement = std::mem::take(self.base.style());
    let mut base = self.base.flex_none().flex().items_center().gap_2();
    base.style().refine(&user);
    base
      .when(self.disabled, |this| this.opacity(0.5))
      .when(control, |this| {
        this
          .role(Role::CheckBox)
          .aria_toggled(toggled)
          .when_some(self.label.clone(), |this, label| this.aria_label(label))
      })
      .when_some(focus, |this, focus| {
        this
          .track_focus(&focus)
          .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
      })
      .when_some(on_click, |this, f| {
        this.cursor_pointer().on_click(move |event, window, cx| {
          // Space toggles a checkbox; Enter is left to submit the form.
          if !crate::keyboard_click(event, KeyboardButton::Enter) {
            f(event, window, cx)
          }
        })
      })
      .child(square)
      .when_some(self.label, |this, label| this.child(div().child(label)))
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
