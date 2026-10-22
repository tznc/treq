//! Modal dialogs and confirmations.

use std::rc::Rc;

use gpui::{
  anchored, deferred, div, point, prelude::*, px, AnyElement, App, FocusHandle, KeyDownEvent,
  Pixels, SharedString, Size, Window,
};

use crate::{text, theme, theme::Palette, zpx, Button, IconButton, TextSize, Variant};

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A dialog on the scrim. It fills its parent, as at the root of a modal
/// window, or with [`Dialog::overlay`] covers the whole window from
/// anywhere in a view. With an action pair the
/// footer holds `[Cancel] [Verb]` and there is no X; without one, an X
/// sits top-right. Escape and a click on the scrim close it; with a focus
/// handle, Enter runs the primary action.
#[derive(IntoElement)]
pub struct Dialog {
  id: SharedString,
  palette: Palette,
  viewport: Option<Size<Pixels>>,
  width: f32,
  title: Option<SharedString>,
  description: Option<SharedString>,
  children: Vec<AnyElement>,
  footer: Vec<AnyElement>,
  focus: Option<FocusHandle>,
  on_close: Option<Handler>,
  on_primary: Option<Handler>,
}

impl Dialog {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      viewport: None,
      width: 440.,
      title: None,
      description: None,
      children: Vec::new(),
      footer: Vec::new(),
      focus: None,
      on_close: None,
      on_primary: None,
    }
  }

  pub fn width(mut self, width: f32) -> Self {
    self.width = width;
    self
  }

  /// Draws it above everything, its scrim covering `viewport`, the
  /// window's size.
  pub fn overlay(mut self, viewport: Size<Pixels>) -> Self {
    self.viewport = Some(viewport);
    self
  }

  pub fn title(mut self, title: impl Into<SharedString>) -> Self {
    self.title = Some(title.into());
    self
  }

  pub fn description(mut self, description: impl Into<SharedString>) -> Self {
    self.description = Some(description.into());
    self
  }

  /// The footer's buttons, Cancel first and the verb last.
  pub fn footer(mut self, buttons: impl IntoIterator<Item = AnyElement>) -> Self {
    self.footer.extend(buttons);
    self
  }

  /// Takes Escape and Enter while `focus`, or something in the dialog, is
  /// focused.
  pub fn focus(mut self, focus: &FocusHandle) -> Self {
    self.focus = Some(focus.clone());
    self
  }

  /// Closes it: Escape, the scrim, and the X when there is no footer.
  pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
    self.on_close = Some(Rc::new(f));
    self
  }

  /// What Enter runs: the footer's verb.
  pub fn on_primary(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
    self.on_primary = Some(Rc::new(f));
    self
  }
}

impl ParentElement for Dialog {
  fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
    self.children.extend(elements);
  }
}

impl RenderOnce for Dialog {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let selector = self.id.to_string();
    let close_on_scrim = self.on_close.clone();
    let close_on_key = self.on_close.clone();
    let primary_on_key = self.on_primary.clone();
    let x = self
      .on_close
      .clone()
      .filter(|_| self.footer.is_empty())
      .map(|close| {
        IconButton::new(format!("{}-close", self.id), theme::ICON_X, "Close", &p)
          .on_click(move |_, window, cx| close(window, cx))
      });
    let panel = text(div(), TextSize::Sm)
      .id(SharedString::from(format!("{}-panel", self.id)))
      .debug_selector(move || selector)
      .role(gpui::Role::Dialog)
      .when_some(self.title.clone(), |el, title| el.aria_label(title))
      .relative()
      .w(zpx(self.width))
      .max_w_full()
      .max_h_full()
      .p_5()
      .flex()
      .flex_col()
      .gap_4()
      .rounded(zpx(theme::RADIUS_LG))
      .border_1()
      .border_color(p.border)
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_xl()
      // A click inside doesn't reach the scrim.
      .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .when(self.title.is_some() || self.description.is_some(), |el| {
        el.child(
          div()
            .flex()
            .flex_col()
            .gap_1()
            .when(x.is_some(), |el| el.pr_8())
            .when_some(self.title, |el, title| {
              el.child(
                text(div(), TextSize::Lg)
                  .font_weight(gpui::FontWeight::SEMIBOLD)
                  .child(title),
              )
            })
            .when_some(self.description, |el, description| {
              el.child(
                text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .child(description),
              )
            }),
        )
      })
      .children(self.children)
      .when(!self.footer.is_empty(), |el| {
        el.child(div().flex().justify_end().gap_2().children(self.footer))
      })
      .when_some(x, |el, x| {
        el.child(div().absolute().top_3().right_3().child(x))
      });
    let viewport = self.viewport;
    let scrim = div()
      .id(SharedString::from(format!("{}-scrim", self.id)))
      .map(|el| match viewport {
        Some(size) => el.w(size.width).h(size.height),
        None => el.size_full(),
      })
      .p_4()
      .flex()
      .items_center()
      .justify_center()
      .bg(p.scrim())
      .occlude()
      .when_some(self.focus, |el, focus| {
        el.track_focus(&focus)
          .on_key_down(move |event: &KeyDownEvent, window, cx| {
            let handler = match event.keystroke.key.as_str() {
              "escape" => close_on_key.as_ref(),
              "enter" => primary_on_key.as_ref(),
              _ => None,
            };
            if let Some(handler) = handler {
              cx.stop_propagation();
              handler(window, cx);
            }
          })
      })
      .when_some(close_on_scrim, |el, close| {
        el.on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
          close(window, cx)
        })
      })
      .child(panel);
    match viewport {
      Some(_) => deferred(anchored().position(point(px(0.), px(0.))).child(scrim))
        .with_priority(2)
        .into_any_element(),
      None => scrim.into_any_element(),
    }
  }
}

/// A confirmation: the title is the question, the message says what will
/// happen, and the buttons are `[Cancel] [Verb]`, never Yes/No/OK. Enter
/// confirms and Escape cancels.
#[derive(IntoElement)]
pub struct ConfirmDialog {
  id: SharedString,
  palette: Palette,
  viewport: Option<Size<Pixels>>,
  question: SharedString,
  message: Option<SharedString>,
  verb: SharedString,
  destructive: bool,
  focus: Option<FocusHandle>,
  on_confirm: Option<Handler>,
  on_cancel: Option<Handler>,
}

impl ConfirmDialog {
  /// `verb` labels the confirm button with what it does, such as
  /// "Initialize" or "Delete workspace".
  pub fn new(
    id: impl Into<SharedString>,
    question: impl Into<SharedString>,
    verb: impl Into<SharedString>,
    palette: &Palette,
  ) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      viewport: None,
      question: question.into(),
      message: None,
      verb: verb.into(),
      destructive: false,
      focus: None,
      on_confirm: None,
      on_cancel: None,
    }
  }

  pub fn message(mut self, message: impl Into<SharedString>) -> Self {
    self.message = Some(message.into());
    self
  }

  /// See [`Dialog::overlay`].
  pub fn overlay(mut self, viewport: Size<Pixels>) -> Self {
    self.viewport = Some(viewport);
    self
  }

  /// The verb destroys something: the danger variant.
  pub fn destructive(mut self) -> Self {
    self.destructive = true;
    self
  }

  pub fn focus(mut self, focus: &FocusHandle) -> Self {
    self.focus = Some(focus.clone());
    self
  }

  pub fn on_confirm(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
    self.on_confirm = Some(Rc::new(f));
    self
  }

  pub fn on_cancel(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
    self.on_cancel = Some(Rc::new(f));
    self
  }
}

impl RenderOnce for ConfirmDialog {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let cancel = self.on_cancel.clone();
    let confirm = self.on_confirm.clone();
    let cancel_button = Button::new(format!("{}-cancel", self.id), &p)
      .variant(Variant::Outline)
      .label("Cancel")
      .h(zpx(theme::CONTROL_LG))
      .when_some(cancel.clone(), |b, f| b.on_click(move |_, w, cx| f(w, cx)));
    let confirm_button = Button::new(format!("{}-confirm", self.id), &p)
      .variant(if self.destructive {
        Variant::Destructive
      } else {
        Variant::Default
      })
      .label(self.verb)
      .h(zpx(theme::CONTROL_LG))
      .when_some(confirm.clone(), |b, f| b.on_click(move |_, w, cx| f(w, cx)));
    Dialog::new(self.id, &p)
      .when_some(self.viewport, |d, viewport| d.overlay(viewport))
      .width(400.)
      .title(self.question)
      .when_some(self.message, |d, message| d.description(message))
      .footer([
        cancel_button.into_any_element(),
        confirm_button.into_any_element(),
      ])
      .when_some(self.focus, |d, focus| d.focus(&focus))
      .when_some(cancel, |d, f| d.on_close(move |w, cx| f(w, cx)))
      .when_some(confirm, |d, f| d.on_primary(move |w, cx| f(w, cx)))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn viewport() -> Size<Pixels> {
    gpui::size(px(800.), px(600.))
  }

  #[test]
  fn test_dialog_builder() {
    let p = theme::palette(false);
    let dialog = Dialog::new("settings", &p)
      .overlay(viewport())
      .width(520.)
      .title("Settings")
      .description("Changes apply at once.")
      .child("Body")
      .on_close(|_, _| {});
    assert_eq!(dialog.width, 520.);
    assert!(dialog.footer.is_empty() && dialog.on_close.is_some());
  }

  #[test]
  fn test_confirm_dialog_builder() {
    let p = theme::palette(true);
    let confirm = ConfirmDialog::new(
      "init-repo",
      "Initialize a Git repository here?",
      "Initialize",
      &p,
    )
    .overlay(viewport())
    .message("Treq needs one to create workspaces.")
    .destructive()
    .on_confirm(|_, _| {})
    .on_cancel(|_, _| {});
    assert!(confirm.destructive);
    assert_eq!(confirm.verb.as_ref(), "Initialize");
  }
}
