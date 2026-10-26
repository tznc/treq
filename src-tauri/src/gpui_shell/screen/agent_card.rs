//! `AgentReviewCommentCard` for text reviewed from a snapshot: a Linear
//! issue, project or document, and the review prompt.

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
  Dismiss,
}

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// How one screen draws the card: `id_prefix` names its parts
/// (`<prefix>-card`, `<prefix>-header`, `<prefix>-apply-<comment id>`),
/// `location` is the header's where, and `resolvable` adds Resolve for
/// text whose comments can be agreed with rather than applied.
pub struct CardSpec<'a> {
  pub id_prefix: &'a str,
  pub location: String,
  pub resolvable: bool,
  pub busy: bool,
}

/// `AgentReviewCommentCard`, condensed: the header (who, where), the
/// comment, its suggestion as a diff, then the actions pulled right with
/// Apply, the primary, last as in a dialog footer.
pub fn agent_comment_card(
  comment: &AgentReviewComment,
  spec: CardSpec<'_>,
  p: &Palette,
  mut on: impl FnMut(CardAction) -> ClickHandler,
) -> Div {
  let (id, prefix, busy) = (&comment.id, spec.id_prefix, spec.busy);
  let button = |name: &str, icon: &'static str, label: &'static str| {
    Button::new(SharedString::from(format!("{prefix}-{name}-{id}")), p)
      .size(Size::Xs)
      .gap_1()
      .disabled(busy)
      .icon(icon, 12.)
      .label(label)
  };
  let mut actions = div().flex().justify_end().gap_2();
  if spec.resolvable {
    actions = actions.child(
      button("resolve", theme::ICON_CIRCLE_CHECK, "Resolve")
        .variant(Variant::Ghost)
        .on_click(on(CardAction::Resolve)),
    );
  }
  actions = actions.child(
    button("dismiss", theme::ICON_X, "Dismiss")
      .variant(Variant::Ghost)
      .on_click(on(CardAction::Dismiss)),
  );
  if comment.suggested_replacement.is_some() {
    actions =
      actions.child(button("apply", theme::ICON_CHECK, "Apply").on_click(on(CardAction::Apply)));
  }
  let (card, header) = (format!("{prefix}-card"), format!("{prefix}-header"));
  div()
    .debug_selector(move || card)
    .rounded(radius::MD)
    .border_1()
    .border_color(ui::Tone::GitMerged.border(p))
    .bg(ui::Tone::GitMerged.tint(p))
    .p_2()
    .flex()
    .flex_col()
    .gap_1p5()
    .child(
      div()
        .debug_selector(move || header)
        .flex()
        .items_center()
        .gap_1p5()
        .child(ui::icon(theme::ICON_BOT, 12., ui::Tone::GitMerged.color(p)))
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
