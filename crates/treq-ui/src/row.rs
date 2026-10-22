//! List rows: the sidebar, file trees, dialog lists, commits and PRs.

use gpui::{div, prelude::*, AnyElement, App, Div, Interactivity, SharedString, Stateful, Window};

use crate::{icon, text, theme, theme::Palette, zpx, TextSize};

/// How tall a [`Row`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RowDensity {
  /// 28: the sidebar, file trees and menus.
  #[default]
  Compact,
  /// 32: lists in dialogs and settings.
  Comfortable,
  /// At least 48, with the meta on a second line: commits, PRs, issues.
  TwoLine,
}

impl RowDensity {
  pub fn min_height(self) -> f32 {
    match self {
      RowDensity::Compact => 28.,
      RowDensity::Comfortable => 32.,
      RowDensity::TwoLine => 48.,
    }
  }
}

/// A leading icon, a title, meta, trailing status (a [`crate::Dot`] or
/// [`crate::Count`]) and a fixed-width lane for actions, so titles don't
/// shift when the actions change. Hover and selected use the palette's one
/// fill each.
#[derive(IntoElement)]
pub struct Row {
  base: Stateful<Div>,
  palette: Palette,
  density: RowDensity,
  icon: Option<&'static str>,
  title: Option<SharedString>,
  meta: Option<SharedString>,
  trailing: Vec<AnyElement>,
  actions: Option<(AnyElement, f32)>,
  selected: bool,
  disabled: bool,
}

impl Row {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    let id: SharedString = id.into();
    let selector = id.to_string();
    Self {
      base: div().id(id).debug_selector(move || selector),
      palette: *palette,
      density: RowDensity::default(),
      icon: None,
      title: None,
      meta: None,
      trailing: Vec::new(),
      actions: None,
      selected: false,
      disabled: false,
    }
  }

  pub fn density(mut self, density: RowDensity) -> Self {
    self.density = density;
    self
  }

  pub fn icon(mut self, path: &'static str) -> Self {
    self.icon = Some(path);
    self
  }

  pub fn title(mut self, title: impl Into<SharedString>) -> Self {
    self.title = Some(title.into());
    self
  }

  /// Secondary text: after the title, or under it in a two-line row.
  pub fn meta(mut self, meta: impl Into<SharedString>) -> Self {
    self.meta = Some(meta.into());
    self
  }

  /// A status dot or count after the title.
  pub fn trailing(mut self, element: impl IntoElement) -> Self {
    self.trailing.push(element.into_any_element());
    self
  }

  /// The row's actions, in a lane `width` wide that is kept whether or not
  /// they show.
  pub fn actions(mut self, actions: impl IntoElement, width: f32) -> Self {
    self.actions = Some((actions.into_any_element(), width));
    self
  }

  pub fn selected(mut self, selected: bool) -> Self {
    self.selected = selected;
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }
}

impl InteractiveElement for Row {
  fn interactivity(&mut self) -> &mut Interactivity {
    self.base.interactivity()
  }
}

impl StatefulInteractiveElement for Row {}

impl Styled for Row {
  fn style(&mut self) -> &mut gpui::StyleRefinement {
    self.base.style()
  }
}

impl RenderOnce for Row {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let two_line = self.density == RowDensity::TwoLine;
    let hover = p.hover();
    let fg = if self.disabled {
      p.muted_foreground.opacity(0.6)
    } else {
      p.foreground
    };
    let title = self.title.map(|title| {
      text(div(), TextSize::Sm)
        .min_w_0()
        .truncate()
        .text_color(fg)
        .child(title)
    });
    let meta = self.meta.map(|meta| {
      text(div(), if two_line { TextSize::Xs } else { TextSize::Sm })
        .min_w_0()
        .truncate()
        .text_color(p.muted_foreground)
        .child(meta)
    });
    self
      .base
      .role(gpui::Role::ListItem)
      .aria_selected(self.selected)
      .flex()
      .items_center()
      .gap_2()
      .px_2()
      .min_h(zpx(self.density.min_height()))
      .when(two_line, |el| el.py_1p5())
      .rounded(zpx(theme::RADIUS_SM))
      .when(self.selected, |el| el.bg(p.selected()))
      .when(!self.selected && !self.disabled, |el| {
        el.hover(move |s| s.bg(hover))
      })
      .when_some(self.icon, |el, path| {
        el.child(icon(path, 16., p.muted_foreground))
      })
      .child(
        div()
          .flex_1()
          .min_w_0()
          .flex()
          .when(two_line, |el| el.flex_col())
          .when(!two_line, |el| el.items_center().gap_2())
          .children(title)
          .children(meta),
      )
      .children(self.trailing)
      .when_some(self.actions, |el, (actions, width)| {
        el.child(
          div()
            .flex_none()
            .w(zpx(width))
            .flex()
            .justify_end()
            .child(actions),
        )
      })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_row_builder() {
    let p = theme::palette(false);
    let row = Row::new("commit-1", &p)
      .density(RowDensity::TwoLine)
      .icon(theme::ICON_CIRCLE_DOT)
      .title("feat: keyvalues cache")
      .meta("just now")
      .trailing(div())
      .actions(div(), 56.)
      .selected(true)
      .disabled(false);
    assert_eq!(row.density.min_height(), 48.);
    assert_eq!(row.trailing.len(), 1);
    assert!(row.actions.is_some_and(|(_, width)| width == 56.));
  }
}
