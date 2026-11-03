//! What a list or pane shows when it has nothing, or is still loading.

use std::time::Duration;

use gpui::{div, prelude::*, Animation, AnimationExt as _, AnyElement, App, SharedString, Window};
use gpui_kit::base::TestSupportExt as _;

use crate::{icon, spinner, text, theme, theme::Palette, zpx, TextSize};

/// Why a pane is empty, and what to do about it: a 24 icon, a title, one
/// sentence and at most one action.
#[derive(IntoElement)]
pub struct EmptyState {
  id: SharedString,
  title: SharedString,
  message: Option<SharedString>,
  icon: &'static str,
  error: bool,
  action: Option<AnyElement>,
  palette: Palette,
}

impl EmptyState {
  pub fn new(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    palette: &Palette,
  ) -> Self {
    Self {
      id: id.into(),
      title: title.into(),
      message: None,
      icon: theme::ICON_INFO,
      error: false,
      action: None,
      palette: *palette,
    }
  }

  /// One sentence: why it's empty, or what failed and what to do next.
  pub fn message(mut self, message: impl Into<SharedString>) -> Self {
    self.message = Some(message.into());
    self
  }

  pub fn icon(mut self, path: &'static str) -> Self {
    self.icon = path;
    self
  }

  /// Something failed: the icon takes the danger color.
  pub fn error(mut self, error: bool) -> Self {
    self.error = error;
    self
  }

  /// The one action, such as a button to retry or to create the first item.
  pub fn action(mut self, action: impl IntoElement) -> Self {
    self.action = Some(action.into_any_element());
    self
  }
}

impl RenderOnce for EmptyState {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = &self.palette;
    let icon_color = if self.error {
      p.destructive
    } else {
      p.muted_foreground
    };
    let selector = self.id.to_string();
    let message_id = format!("{}-message", self.id);
    div()
      .id(self.id)
      .debug_selector(move || selector)
      .role(gpui::Role::Status)
      .aria_label(self.title.clone())
      .flex()
      .flex_col()
      .items_center()
      .justify_center()
      .gap_2()
      .py_8()
      .px_6()
      .text_center()
      .child(icon(self.icon, 24., icon_color))
      .child(
        text(div(), TextSize::Sm)
          .font_weight(gpui::FontWeight::MEDIUM)
          .text_color(p.foreground)
          .child(self.title),
      )
      .when_some(self.message, |el, message| {
        el.child(
          text(div(), TextSize::Sm)
            .id(message_id)
            // Readable on its own, so tests and assistive tech find the why.
            .role(gpui::Role::Paragraph)
            .aria_label(message.clone())
            .max_w(zpx(360.))
            .w_full()
            .text_color(p.muted_foreground)
            .child(message)
            .test_support(),
        )
      })
      .when_some(self.action, |el, action| {
        el.child(div().pt_2().child(action))
      })
      .test_support()
  }
}

/// A spinner where the result will appear, with an optional muted label.
#[derive(IntoElement)]
pub struct Loading {
  id: SharedString,
  label: Option<SharedString>,
  palette: Palette,
}

impl Loading {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      label: None,
      palette: *palette,
    }
  }

  pub fn label(mut self, label: impl Into<SharedString>) -> Self {
    self.label = Some(label.into());
    self
  }
}

impl RenderOnce for Loading {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = &self.palette;
    let selector = self.id.to_string();
    div()
      .id(self.id.clone())
      .debug_selector(move || selector)
      .role(gpui::Role::ProgressIndicator)
      .flex()
      .items_center()
      .justify_center()
      .gap_2()
      .text_color(p.muted_foreground)
      .child(spinner(
        format!("{}-spinner", self.id),
        16.,
        p.muted_foreground,
      ))
      .when_some(self.label, |el, label| {
        el.aria_label(label.clone())
          .child(text(div(), TextSize::Sm).child(label))
      })
      .test_support()
  }
}

/// Placeholder rows for a list slower than 300ms to load, pulsing in the
/// hover color.
#[derive(IntoElement)]
pub struct Skeleton {
  id: SharedString,
  rows: usize,
  row_height: Option<f32>,
  palette: Palette,
}

impl Skeleton {
  pub fn new(id: impl Into<SharedString>, rows: usize, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      rows,
      row_height: None,
      palette: *palette,
    }
  }

  /// Each bar centred in a row this tall, so the placeholder takes the
  /// room of the rows it stands in for.
  pub fn row_height(mut self, height: f32) -> Self {
    self.row_height = Some(height);
    self
  }
}

impl RenderOnce for Skeleton {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let fill = self.palette.hover();
    let row_height = self.row_height;
    div()
      .flex()
      .flex_col()
      .when(row_height.is_none(), |el| el.gap_2())
      .children((0..self.rows).map(move |row| {
        // Ragged widths, so it reads as text rather than a table.
        let width = [0.9, 0.7, 0.8, 0.6][row % 4];
        let bar = div()
          .h(zpx(12.))
          .w(gpui::relative(width))
          .rounded(zpx(theme::RADIUS_SM))
          .bg(fill);
        match row_height {
          Some(h) => div().h(zpx(h)).flex().items_center().child(bar),
          None => bar,
        }
      }))
      .with_animation(
        self.id,
        Animation::new(Duration::from_millis(1500))
          .repeat()
          .with_max_fps(crate::PULSE_FPS),
        |el, delta| el.opacity(0.5 + 0.5 * (delta * std::f32::consts::TAU).cos().abs()),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_empty_state_builder() {
    let p = theme::palette(false);
    let empty = EmptyState::new("prs", "No pull requests", &p)
      .message("Push a branch to open one.")
      .icon(theme::ICON_CIRCLE_DOT)
      .error(true)
      .action(div());
    assert!(empty.error);
    assert_eq!(empty.icon, theme::ICON_CIRCLE_DOT);
    assert!(Loading::new("loading", &p)
      .label("Loading commits")
      .label
      .is_some());
    assert_eq!(Skeleton::new("rows", 3, &p).rows, 3);
  }

  #[test]
  fn test_skeleton_row_height() {
    let p = theme::palette(false);
    assert_eq!(Skeleton::new("rows", 3, &p).row_height, None);
    assert_eq!(
      Skeleton::new("rows", 3, &p).row_height(56.).row_height,
      Some(56.)
    );
  }
}
