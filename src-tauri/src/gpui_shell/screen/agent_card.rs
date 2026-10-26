//! `AgentReviewCommentCard` for text reviewed from a snapshot: a Linear
//! issue, project or document, and the review prompt. The comment, its
//! suggestion as a diff, then Apply, Resolve and Delete.

use tauri_plugin_gpui::gpui::{
  div, prelude::*, App, ClickEvent, Div, FontWeight, SharedString, Window,
};

use super::changes::threads::suggestion_box;
use crate::{
  gpui_shell::{
    theme::{self, Palette},
    ui::{self, radius, Button, Size, TextSize, Variant},
  },
  local_db::AgentReviewComment,
};

/// What a card's buttons do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardAction {
  Apply,
  Resolve,
  Delete,
}

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// How one screen draws the card: `id_prefix` starts each button's id
/// (`<prefix>-apply-<comment id>`), `apply_label` names where Apply writes.
pub struct CardSpec<'a> {
  pub id_prefix: &'a str,
  pub apply_label: &'a str,
  pub location: String,
  pub busy: bool,
}

pub fn agent_comment_card(
  comment: &AgentReviewComment,
  spec: CardSpec<'_>,
  p: &Palette,
  mut on: impl FnMut(CardAction) -> ClickHandler,
) -> Div {
  let (id, prefix, busy) = (&comment.id, spec.id_prefix, spec.busy);
  let mut actions = div().flex().flex_wrap().items_center().gap_2();
  if comment.suggested_replacement.is_some() {
    actions = actions.child(
      Button::new(SharedString::from(format!("{prefix}-apply-{id}")), p)
        .size(Size::Sm)
        .gap_1p5()
        .disabled(busy)
        .icon(theme::ICON_WAND, 12.)
        .label(spec.apply_label.to_string())
        .on_click(on(CardAction::Apply)),
    );
  }
  actions = actions
    .child(
      Button::new(SharedString::from(format!("{prefix}-resolve-{id}")), p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .gap_1p5()
        .disabled(busy)
        .icon(theme::ICON_CHECK, 12.)
        .label("Resolve")
        .on_click(on(CardAction::Resolve)),
    )
    .child(
      Button::new(SharedString::from(format!("{prefix}-delete-{id}")), p)
        .variant(Variant::Ghost)
        .size(Size::Sm)
        .gap_1p5()
        .text_color(p.muted_foreground)
        .disabled(busy)
        .icon(theme::ICON_TRASH, 12.)
        .label("Delete")
        .on_click(on(CardAction::Delete)),
    );
  div()
    .debug_selector(|| "agent-review-comment-card".into())
    .rounded(radius::MD)
    .border_1()
    .border_color(ui::Tone::GitMerged.border(p))
    .bg(ui::Tone::GitMerged.tint(p))
    .p_3()
    .flex()
    .flex_col()
    .gap_2()
    .child(
      div()
        .flex()
        .items_center()
        .gap_2()
        .child(ui::icon(theme::ICON_BOT, 14., ui::Tone::GitMerged.color(p)))
        .child(
          ui::text(div(), TextSize::Xs)
            .font_weight(FontWeight::MEDIUM)
            .text_color(ui::Tone::GitMerged.text(p))
            .child("Local"),
        )
        .child(
          ui::text(div(), TextSize::Xs)
            .text_color(p.muted_foreground)
            .child(spec.location),
        ),
    )
    .child(
      ui::text(div(), TextSize::Sm)
        .whitespace_normal()
        .child(comment.comment_text.clone()),
    )
    .when_some(comment.suggested_replacement.clone(), |this, suggested| {
      this.child(suggestion_box(
        comment.quoted_text.as_deref(),
        &suggested,
        p,
      ))
    })
    .child(actions)
}

/// What a [`suggestion_card`]'s buttons do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuggestionAction {
  Apply,
  Dismiss,
}

/// A proposed edit to text the user owns (the review prompt): the reason,
/// the suggestion as a diff, then Dismiss and Apply, the primary on the
/// right as in a dialog footer. Buttons are `<id_prefix>-apply-<id>` and
/// `<id_prefix>-dismiss-<id>`.
pub fn suggestion_card(
  comment: &AgentReviewComment,
  id_prefix: &str,
  busy: bool,
  p: &Palette,
  mut on: impl FnMut(SuggestionAction) -> ClickHandler,
) -> Div {
  let id = &comment.id;
  let card_id = format!("{id_prefix}-card");
  div()
    .debug_selector(move || card_id)
    .rounded(radius::MD)
    .border_1()
    .border_color(ui::Tone::GitMerged.border(p))
    .bg(ui::Tone::GitMerged.tint(p))
    .p_3()
    .flex()
    .flex_col()
    .gap_2()
    .child(
      ui::text(div(), TextSize::Sm)
        .whitespace_normal()
        .child(comment.comment_text.clone()),
    )
    .when_some(comment.suggested_replacement.clone(), |this, suggested| {
      this.child(suggestion_box(
        comment.quoted_text.as_deref(),
        &suggested,
        p,
      ))
    })
    .child(
      div()
        .flex()
        .justify_end()
        .gap_2()
        .child(
          Button::new(SharedString::from(format!("{id_prefix}-dismiss-{id}")), p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .gap_1p5()
            .disabled(busy)
            .icon(theme::ICON_X, 12.)
            .label("Dismiss")
            .on_click(on(SuggestionAction::Dismiss)),
        )
        .when(comment.suggested_replacement.is_some(), |this| {
          this.child(
            Button::new(SharedString::from(format!("{id_prefix}-apply-{id}")), p)
              .size(Size::Sm)
              .gap_1p5()
              .disabled(busy)
              .icon(theme::ICON_CHECK, 12.)
              .label("Apply")
              .on_click(on(SuggestionAction::Apply)),
          )
        }),
    )
}
