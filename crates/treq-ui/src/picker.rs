//! The control that shows a value and opens a menu to change it.

use std::rc::Rc;

use gpui::{div, prelude::*, App, ClickEvent, SharedString, Window};

use crate::{icon, text, theme, theme::Palette, zpx, Button, OnClick, Size, TextSize, Variant};

/// Where a [`Picker`] sits, which sets its height.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PickerPlacement {
  /// A form field, 32 tall to line up with inputs.
  #[default]
  Form,
  /// A toolbar or composer row, 28 tall.
  Toolbar,
}

impl PickerPlacement {
  pub fn height(self) -> f32 {
    match self {
      PickerPlacement::Form => 32.,
      PickerPlacement::Toolbar => 28.,
    }
  }
}

/// A select: the current value with a trailing chevron, in an outline
/// button, so it reads as a value that can change. A command that opens a
/// menu is a [`Button`] with no chevron instead.
#[derive(IntoElement)]
pub struct Picker {
  id: SharedString,
  palette: Palette,
  value: Option<SharedString>,
  placeholder: SharedString,
  label: Option<SharedString>,
  icon: Option<&'static str>,
  placement: PickerPlacement,
  open: bool,
  disabled: bool,
  on_click: Option<OnClick>,
}

impl Picker {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      value: None,
      placeholder: "Select…".into(),
      label: None,
      icon: None,
      placement: PickerPlacement::default(),
      open: false,
      disabled: false,
      on_click: None,
    }
  }

  /// The current value.
  pub fn value(mut self, value: impl Into<SharedString>) -> Self {
    self.value = Some(value.into());
    self
  }

  /// What shows while nothing is picked.
  pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
    self.placeholder = placeholder.into();
    self
  }

  /// What the value is, such as "Target branch": the accessible name.
  pub fn label(mut self, label: impl Into<SharedString>) -> Self {
    self.label = Some(label.into());
    self
  }

  /// A leading icon inside the control, such as the agent's logo.
  pub fn icon(mut self, path: &'static str) -> Self {
    self.icon = Some(path);
    self
  }

  pub fn placement(mut self, placement: PickerPlacement) -> Self {
    self.placement = placement;
    self
  }

  /// Whether its menu is open, which draws it pressed.
  pub fn open(mut self, open: bool) -> Self {
    self.open = open;
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }

  /// Opens the menu.
  pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_click = Some(Rc::new(f));
    self
  }

  /// The accessible name: what the value is, then the value.
  pub fn accessible_name(&self) -> SharedString {
    let value = self.value.as_ref().unwrap_or(&self.placeholder);
    match &self.label {
      Some(label) => format!("{label}: {value}").into(),
      None => value.clone(),
    }
  }
}

impl RenderOnce for Picker {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let name = self.accessible_name();
    let empty = self.value.is_none();
    let value = self.value.unwrap_or(self.placeholder);
    let toolbar = self.placement == PickerPlacement::Toolbar;
    Button::new(self.id, &p)
      .variant(Variant::Outline)
      .size(Size::Xs)
      .disabled(self.disabled)
      .selected(self.open)
      .aria_label(name)
      .h(zpx(self.placement.height()))
      .px(zpx(if toolbar { 8. } else { 10. }))
      .gap_1p5()
      .justify_start()
      .font_weight(gpui::FontWeight::NORMAL)
      .when_some(self.icon, |b, path| b.icon(path, 16.))
      .child(
        text(div(), TextSize::Sm)
          .flex_1()
          .min_w_0()
          .truncate()
          .when(empty, |el| el.text_color(p.muted_foreground))
          .child(value),
      )
      .child(icon(theme::ICON_CHEVRON_DOWN, 14., p.muted_foreground))
      .when_some(self.on_click, |b, f| {
        b.on_click(move |e, w, cx| f(e, w, cx))
      })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_picker_builder() {
    let p = theme::palette(false);
    let picker = Picker::new("target", &p)
      .label("Target branch")
      .placeholder("Pick a branch")
      .placement(PickerPlacement::Toolbar)
      .icon(theme::ICON_CHEVRON_DOWN)
      .open(true)
      .disabled(true);
    assert_eq!(
      picker.accessible_name().as_ref(),
      "Target branch: Pick a branch"
    );
    let picker = picker.value("main");
    assert_eq!(picker.accessible_name().as_ref(), "Target branch: main");
    assert_eq!(PickerPlacement::Toolbar.height(), 28.);
  }
}
