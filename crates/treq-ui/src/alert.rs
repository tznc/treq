//! Inline alerts: a fact and what to do about it, where it applies.

use gpui::{div, prelude::*, AnyElement, App, SharedString, Window};

use crate::{icon, text, theme, theme::Palette, zpx, TextSize};

/// How serious an [`Alert`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Severity {
  #[default]
  Info,
  Warning,
  Danger,
}

/// One red (or amber, or blue) signal: the icon and a tinted surface carry
/// the severity, the text stays in body color, and the actions are normal
/// [`crate::Button`]s, aligned to the bottom right under the text. The
/// title states the fact; the message says what to do.
///
/// One action is an outline button. With more, add them in reading order:
/// the dismissive one first (ghost), the one that resolves it last
/// (outline), so it sits in the corner.
#[derive(IntoElement)]
pub struct Alert {
  id: SharedString,
  title: SharedString,
  message: Option<SharedString>,
  severity: Severity,
  content: Option<AnyElement>,
  actions: Vec<AnyElement>,
  palette: Palette,
}

impl Alert {
  pub fn new(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    palette: &Palette,
  ) -> Self {
    Self {
      id: id.into(),
      title: title.into(),
      message: None,
      severity: Severity::default(),
      content: None,
      actions: Vec::new(),
      palette: *palette,
    }
  }

  pub fn message(mut self, message: impl Into<SharedString>) -> Self {
    self.message = Some(message.into());
    self
  }

  pub fn severity(mut self, severity: Severity) -> Self {
    self.severity = severity;
    self
  }

  /// Detail under the message, such as a log the alert is about.
  pub fn content(mut self, content: impl IntoElement) -> Self {
    self.content = Some(content.into_any_element());
    self
  }

  /// Adds an action, a button whose label is a verb. Call again for more;
  /// they lay out left to right, ending in the bottom-right corner.
  pub fn action(mut self, action: impl IntoElement) -> Self {
    self.actions.push(action.into_any_element());
    self
  }

  /// Adds several actions at once, in order.
  pub fn actions<E: IntoElement>(mut self, actions: impl IntoIterator<Item = E>) -> Self {
    self
      .actions
      .extend(actions.into_iter().map(IntoElement::into_any_element));
    self
  }
}

impl RenderOnce for Alert {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = &self.palette;
    let (color, icon_path) = match self.severity {
      Severity::Info => (p.primary, theme::ICON_INFO),
      Severity::Warning => (p.warning, theme::ICON_TRIANGLE_ALERT),
      Severity::Danger => (p.destructive, theme::ICON_CIRCLE_ALERT),
    };
    let tint = if p.dark { 0.14 } else { 0.08 };
    let selector = self.id.to_string();
    div()
      .id(self.id)
      .debug_selector(move || selector)
      .role(gpui::Role::Alert)
      .flex()
      .items_start()
      .gap_2()
      .px_3()
      .py_2()
      .rounded(zpx(theme::RADIUS_MD))
      .border_1()
      .border_color(color.opacity(0.3))
      .bg(color.opacity(tint))
      .text_color(p.foreground)
      .child(div().pt(zpx(2.)).child(icon(icon_path, 16., color)))
      .child(
        div()
          .flex_1()
          .min_w_0()
          .flex()
          .flex_col()
          .child(
            text(div(), TextSize::Sm)
              .font_weight(gpui::FontWeight::MEDIUM)
              .child(self.title),
          )
          .when_some(self.message, |el, message| {
            el.child(
              text(div(), TextSize::Sm)
                .text_color(p.muted_foreground)
                .child(message),
            )
          })
          .when_some(self.content, |el, content| el.child(div().mt_2().child(content)))
          .when(!self.actions.is_empty(), |el| {
            el.child(
              div()
                .mt_2()
                .flex()
                .flex_wrap()
                .justify_end()
                .gap_2()
                .children(self.actions),
            )
          }),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_alert_builder() {
    let p = theme::palette(false);
    let alert = Alert::new("conflicts", "1 file has conflicts", &p)
      .message("Resolve them before you push or merge.")
      .severity(Severity::Danger)
      .action(div());
    assert_eq!(alert.severity, Severity::Danger);
    assert_eq!(alert.actions.len(), 1);
    let two = Alert::new("conflicts", "1 file has conflicts", &p)
      .actions([div(), div()])
      .action(div());
    assert_eq!(two.actions.len(), 3);
    assert!(two.content.is_none());
    assert!(Alert::new("setup", "Setup failed", &p).content(div()).content.is_some());
  }
}
