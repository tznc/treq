//! The comment composer: a sheet with Write and Preview over a screen's
//! own text state, so closing the sheet keeps what was typed.

use std::rc::Rc;

use gpui_kit::component::{
  button::{Button, ButtonVariants as _},
  h_flex,
  input::{Textarea, TextareaState},
  v_flex, Disableable as _, Selectable as _, Sizable as _,
};
use gpui_kit::{
  div, px, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
  SharedString, Styled as _, Window,
};

use super::widgets::TouchButton as _;
use super::{sheet, widgets};

/// Sends the text; the screen clears its state once GitHub has it.
pub type OnSend = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct ComposerSheet {
  state: Entity<TextareaState>,
  preview: bool,
  on_send: OnSend,
}

/// Opens the composer over `state`, titled `title` ("Comment on #77").
pub fn open(
  title: impl Into<SharedString>,
  state: Entity<TextareaState>,
  on_send: OnSend,
  cx: &mut App,
) {
  sheet::open(Some(title.into()), cx, move |window, cx| {
    state.update(cx, |state, cx| state.focus(window, cx));
    cx.new(|cx| {
      cx.observe(&state, |_, _, cx| cx.notify()).detach();
      ComposerSheet {
        state,
        preview: false,
        on_send,
      }
    })
  });
}

impl Render for ComposerSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let text = self.state.read(cx).value().to_string();
    let empty = text.trim().is_empty();
    let tab = |id: &'static str, label: &'static str, on: bool| {
      Button::new(id)
        .outline()
        .small()
        .label(label)
        .selected(on)
        .h(px(36.))
    };
    let on_send = self.on_send.clone();
    v_flex()
      .gap_3()
      .px_4()
      .pb_3()
      .child(
        h_flex()
          .gap_1()
          .child(
            tab("composer-write", "Write", !self.preview)
              .on_click(cx.listener(|this, _, _, cx| {
                this.preview = false;
                cx.notify();
              }))
              .touch(),
          )
          .child(
            tab("composer-preview", "Preview", self.preview)
              .on_click(cx.listener(|this, _, _, cx| {
                this.preview = true;
                cx.notify();
              }))
              .touch(),
          ),
      )
      .child(if self.preview {
        div()
          .min_h(px(120.))
          .child(if empty {
            widgets::muted("Nothing to preview yet.", cx).into_any_element()
          } else {
            widgets::markdown("composer-preview-body", &text, cx).into_any_element()
          })
          .into_any_element()
      } else {
        div()
          .min_h(px(120.))
          .child(Textarea::new(&self.state))
          .into_any_element()
      })
      .child(
        Button::new("composer-send")
          .primary()
          .large()
          .w_full()
          .h(px(widgets::CTA_H - 8.))
          .label("Comment")
          .disabled(empty)
          .on_click(move |_, window, cx| {
            sheet::close(cx);
            on_send(window, cx);
          })
          .touch(),
      )
  }
}
