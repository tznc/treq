//! The file browser's review: comment cards and boxes, the Cmd+F bar, the
//! Finish review popover and the Discard dialog.

use super::*;
use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;

impl FileBrowserView {
  /// `FileCommentSection`.
  pub(super) fn comment_section(
    &self,
    comments: &[&LineComment],
    input: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let mut section = ui::text(div(), TextSize::Sm)
      .debug_selector(|| "file-comment-section".into())
      .border_b_1()
      .border_color(p.border)
      .bg(comment_section_fill(p))
      .px_4()
      .py_3()
      .flex()
      .flex_col()
      .gap_3()
      .font_family(theme::SANS_FONT);
    for c in comments {
      section = section.child(self.comment_card(c, p, cx));
    }
    if input {
      section = section.child(self.comment_input(p, cx));
    }
    section
  }

  pub(super) fn comment_card(
    &self,
    c: &LineComment,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> gpui::AnyElement {
    let box_ = || {
      div()
        .rounded(radius::MD)
        .p_3()
        .border_1()
        .border_color(p.border.opacity(0.6))
    };
    if self.editing.as_deref() == Some(c.id.as_str()) {
      let discard = c.id.clone();
      return box_()
        .bg(p.background)
        .child(self.edit_box(
          p,
          cx.listener(move |view, _, _, cx| view.delete_comment(&discard, cx)),
          cx,
        ))
        .into_any_element();
    }
    let (edit, delete, body) = (c.id.clone(), c.id.clone(), c.text.clone());
    let muted = p.muted_foreground;
    box_()
      .id(SharedString::from(format!("comment-{}", c.id)))
      .debug_selector(|| "file-comment-card".into())
      .group("comment-card")
      .cursor_pointer()
      .hover(|s| s.bg(p.hover()))
      .on_click(cx.listener(move |view, _, _, cx| view.start_edit(&edit, &body, cx)))
      .child(
        div()
          .flex()
          .items_start()
          .gap_2()
          .child(
            div()
              .mt_0p5()
              .flex_none()
              .opacity(0.)
              .group_hover("comment-card", |s| s.opacity(1.))
              .child(ui::icon(theme::ICON_PENCIL, 12., muted)),
          )
          .child(div().flex_1().whitespace_normal().child(c.text.clone()))
          .child(
            self.tip(
              format!("delete-comment-tip-{}", c.id),
              "Delete comment",
              Side::Top,
              div()
                .id(SharedString::from(format!("delete-comment-{}", c.id)))
                .debug_selector(|| "delete-comment".into())
                .flex_none()
                .p_1()
                .rounded(radius::SM)
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover()))
                .child(ui::icon(theme::ICON_X, 12., muted))
                .on_click(cx.listener(move |view, _, _, cx| {
                  cx.stop_propagation();
                  view.delete_comment(&delete, cx);
                })),
              cx,
            ),
          ),
      )
      .into_any_element()
  }

  /// `CommentEditInput`: Discard, Cancel, Save.
  pub(super) fn edit_box(
    &self,
    p: &Palette,
    on_discard: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let empty = self.edit_text.trim().is_empty();
    div()
      .debug_selector(|| "comment-edit".into())
      .flex()
      .flex_col()
      .gap_2()
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        let k = &e.keystroke;
        if ui::is_escape(k) {
          cx.stop_propagation();
          view.cancel_edit(cx);
        } else if k.key == "enter"
          && (crate::gpui_shell::keys::held(k.modifiers).platform
            || crate::gpui_shell::keys::held(k.modifiers).control)
        {
          cx.stop_propagation();
          view.save_edit(cx);
        }
      }))
      .children(self.edit_input.as_ref().map(|i| Textarea::new(&i.state)))
      .child(
        div()
          .flex()
          .justify_between()
          .gap_2()
          .child(
            Button::new("comment-discard", p)
              .variant(Variant::Ghost)
              .size(Size::Sm)
              .text_color(p.destructive)
              .label("Discard")
              .on_click(on_discard),
          )
          .child(
            div()
              .flex()
              .gap_2()
              .child(
                Button::new("comment-edit-cancel", p)
                  .variant(Variant::Ghost)
                  .size(Size::Sm)
                  .label("Cancel")
                  .on_click(cx.listener(|view, _, _, cx| view.cancel_edit(cx))),
              )
              .child(
                Button::new("comment-save", p)
                  .size(Size::Sm)
                  .disabled(empty)
                  .label("Save")
                  .on_click(cx.listener(|view, _, _, cx| view.save_edit(cx))),
              ),
          ),
      )
  }

  /// `CommentInput`.
  pub(super) fn comment_input(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let empty = self.draft_text.trim().is_empty();
    div()
      .debug_selector(|| "comment-input".into())
      .bg(p.muted)
      .border_1()
      .border_color(p.border)
      .rounded(radius::MD)
      .px_4()
      .py_3()
      .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        let k = &e.keystroke;
        if ui::is_escape(k) {
          cx.stop_propagation();
          view.cancel_draft(cx);
        } else if k.key == "enter"
          && (crate::gpui_shell::keys::held(k.modifiers).platform
            || crate::gpui_shell::keys::held(k.modifiers).control)
        {
          cx.stop_propagation();
          view.submit_draft(cx);
        }
      }))
      .children(
        self
          .draft_input
          .as_ref()
          .map(|i| div().mb_2().child(Textarea::new(&i.state))),
      )
      .child(
        div()
          .flex()
          .justify_end()
          .gap_2()
          .child(
            Button::new("comment-cancel", p)
              .variant(Variant::Ghost)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.cancel_draft(cx))),
          )
          .child(
            Button::new("comment-submit", p)
              .disabled(empty)
              .label("Add comment")
              .on_click(cx.listener(|view, _, _, cx| view.submit_draft(cx))),
          ),
      )
  }

  /// `SearchOverlay`, top right of the code.
  pub(super) fn search_bar(&self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    if !self.search.open {
      return None;
    }
    let input = self.search.input.clone()?;
    let empty = self.search.query.trim().is_empty();
    let total = self.search.matches.len();
    let shown = if total > 0 {
      self.search.current + 1
    } else {
      0
    };
    let muted = p.muted_foreground;
    Some(
      div()
        .id("file-search")
        .debug_selector(|| "file-search".into())
        .absolute()
        .top_2()
        .right_2()
        .bg(p.background)
        .border_1()
        .border_color(p.border)
        .rounded(radius::MD)
        .shadow_lg()
        .p_0p5()
        .flex()
        .items_center()
        .gap_0p5()
        .font_family(theme::SANS_FONT)
        .capture_action(cx.listener(|view, _: &Escape, _, cx| {
          view.close_search(cx);
          cx.stop_propagation();
        }))
        .child(
          ui::text(div(), TextSize::Sm)
            .debug_selector(|| "file-search-input".into())
            .h_6()
            .w_48()
            .px_2()
            .flex()
            .items_center()
            .rounded(radius::MD)
            .border_1()
            .border_color(p.input)
            .child(ui::input(&input).appearance(false).bordered(false)),
        )
        .child(
          ui::text(div(), TextSize::Xs)
            .debug_selector(|| "file-search-count".into())
            .px_2()
            .whitespace_nowrap()
            .text_color(muted)
            .child(format!("{shown} of {total}")),
        )
        .child(
          self.tip(
            "file-search-prev-tip",
            "Previous (Shift+Enter)",
            Side::Top,
            ui::IconButton::new(
              "file-search-prev",
              theme::ICON_CHEVRON_UP,
              "Previous match",
              p,
            )
            .size(ui::IconButtonSize::Small)
            .disabled(empty)
            .on_click(cx.listener(|view, _, _, cx| view.step_match(-1, cx))),
            cx,
          ),
        )
        .child(
          self.tip(
            "file-search-next-tip",
            "Next (Enter)",
            Side::Top,
            ui::IconButton::new(
              "file-search-next",
              theme::ICON_CHEVRON_DOWN,
              "Next match",
              p,
            )
            .size(ui::IconButtonSize::Small)
            .disabled(empty)
            .on_click(cx.listener(|view, _, _, cx| view.step_match(1, cx))),
            cx,
          ),
        )
        .child(
          self.tip(
            "file-search-close-tip",
            "Close (Esc)",
            Side::Top,
            ui::IconButton::new("file-search-close", theme::ICON_X, "Close search", p)
              .size(ui::IconButtonSize::Small)
              .on_click(cx.listener(|view, _, _, cx| view.close_search(cx))),
            cx,
          ),
        ),
    )
  }

  /// The Finish review popover, under its button.
  pub(super) fn finish_popover(
    &self,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    if !self.finish_open || self.comments.is_empty() {
      return None;
    }
    let anchor = self.anchors.borrow().get("finish-review").copied()?;
    let n = self.comments.len();
    let width = 320.;
    let at = point(anchor.right() - px(width), anchor.bottom() + px(4.));
    let checked = self.use_idle;
    let checkbox = div()
      .id("use-idle-agent")
      .debug_selector(|| "use-idle-agent".into())
      .flex()
      .items_center()
      .gap_1p5()
      .cursor_pointer()
      .child(
        div()
          .size(zpx(14.))
          .rounded(radius::SM)
          .border_1()
          .border_color(if checked { p.primary } else { p.border })
          .bg(if checked { p.primary } else { p.background })
          .flex()
          .items_center()
          .justify_center()
          .when(checked, |this| {
            this.child(ui::icon(theme::ICON_CHECK, 10., p.primary_foreground))
          }),
      )
      .child(
        ui::text(div(), TextSize::Xs)
          .text_color(p.muted_foreground)
          .child("Send to idle agent if one is available"),
      )
      .on_click(cx.listener(|view, _, _, cx| {
        view.use_idle = !view.use_idle;
        cx.notify();
      }));
    let close = |view: &mut Self, cx: &mut Context<Self>| {
      view.finish_open = false;
      cx.notify();
    };
    let panel = div()
      .id("finish-review-popover")
      .debug_selector(|| "finish-review-popover".into())
      .w(zpx(width))
      .relative()
      .p_4()
      .rounded(radius::MD)
      .border_1()
      .border_color(p.border)
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_md()
      .occlude()
      .on_mouse_down_out(cx.listener(move |view, _, _, cx| close(view, cx)))
      .child(
        div().absolute().top_2().right_2().child(
          Button::new("finish-review-close", p)
            .aria_label("Close")
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .px_1()
            .icon(theme::ICON_X, 16.)
            .on_click(cx.listener(move |view, _, _, cx| close(view, cx))),
        ),
      )
      .child(
        div()
          .flex()
          .flex_col()
          .gap_3()
          .child(
            div()
              .child(
                div()
                  .font_weight(FontWeight::MEDIUM)
                  .mb_1()
                  .child("Finish your review"),
              )
              .child(
                div()
                  .text_color(p.muted_foreground)
                  .child(format!("{} will be submitted.", count_label(n, "comment"))),
              ),
          )
          .children(self.summary_input.as_ref().map(|i| Textarea::new(&i.state)))
          .child(checkbox)
          .child(
            div()
              .flex()
              .justify_between()
              .gap_2()
              .child(ui::copy_feedback::copyable("copy-review-feedback", {
                let (p, copy) = (*p, cx.listener(|view, _, _, cx| view.copy_review(cx)));
                move |copied| {
                  Button::new("copy-review", &p)
                    .variant(Variant::Outline)
                    .size(Size::Sm)
                    .gap_2()
                    .copy_icon(copied, 12.)
                    .label(if copied.copied { "Copied" } else { "Copy" })
                    .on_click(copy)
                    .into_any_element()
                }
              }))
              .child(
                div()
                  .flex()
                  .gap_2()
                  .child(
                    Button::new("review-plan", p)
                      .variant(Variant::Secondary)
                      .size(Size::Sm)
                      .label("Plan")
                      .on_click(cx.listener(|view, _, _, cx| view.send_review(true, cx))),
                  )
                  .child(
                    Button::new("review-edit", p)
                      .size(Size::Sm)
                      .label("Edit")
                      .on_click(cx.listener(|view, _, _, cx| view.send_review(false, cx))),
                  ),
              ),
          ),
      );
    Some(deferred(anchored().position(at).child(panel)).with_priority(1))
  }

  /// "Discard review?".
  pub(super) fn discard_dialog(
    &self,
    window: &mut Window,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    if !self.discard_dialog {
      return None;
    }
    let n = self.comments.len();
    let close = |view: &mut Self, cx: &mut Context<Self>| {
      view.discard_dialog = false;
      cx.notify();
    };
    Some(
      ui::Dialog::new("discard-review-dialog", p)
        .overlay(window.viewport_size())
        .width(512.)
        .title("Discard review?")
        .description(format!(
          "This will discard all {n} pending {}. This action cannot be undone.",
          if n == 1 { "comment" } else { "comments" }
        ))
        .on_close(ui::handler(cx, move |view, _, cx| close(view, cx)))
        .on_primary(ui::handler(cx, |view, _, cx| view.discard_review(cx)))
        .footer([
          Button::new("keep-reviewing", p)
            .variant(Variant::Outline)
            .label("Keep reviewing")
            .on_click(cx.listener(move |view, _, _, cx| close(view, cx)))
            .into_any_element(),
          Button::new("confirm-discard-review", p)
            .variant(Variant::Destructive)
            .label("Discard")
            .on_click(cx.listener(|view, _, _, cx| view.discard_review(cx)))
            .into_any_element(),
        ]),
    )
  }
}
