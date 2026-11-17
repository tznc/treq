//! `@` mentions in the comment box: the issue's or pull request's
//! participants, suggested as a login is typed after `@`.

use gpui_kit::component::input::{Enter, Escape, MoveDown, MoveUp};
use tauri_plugin_gpui::gpui::{self, div, prelude::*, Context, Div, SharedString, Window};

use super::GithubView;
use crate::{
  core::pr_view,
  github::GhAuthor,
  gpui_shell::{
    screen::code_data::{mention_at, replace_range},
    theme::Palette,
    ui::{self, radius, TextSize},
  },
};

/// The `@` being typed in the comment box and the logins it can become.
pub(super) struct CommentMention {
  anchor: usize,
  logins: Vec<String>,
}

impl GithubView {
  /// Everyone on the open thread, most recent first.
  fn participants(&self) -> Vec<&GhAuthor> {
    let (author, comments, reviews) = match (&self.detail.issue, &self.detail.pr) {
      (Some(issue), _) => (&issue.author, &issue.comments, &[][..]),
      (None, Some(pr)) => (&pr.author, &pr.comments, &pr.reviews[..]),
      (None, None) => return Vec::new(),
    };
    let comments = comments.as_deref().unwrap_or_default();
    comments
      .iter()
      .rev()
      .map(|c| &c.author)
      .chain(reviews.iter().rev().map(|r| &r.author))
      .chain(std::iter::once(author))
      .collect()
  }

  /// The open suggestions for the comment box with its cursor at `cursor`.
  pub(super) fn comment_mention(&self, cursor: usize) -> Option<CommentMention> {
    let (anchor, query) = mention_at(&self.comment, cursor)?;
    if self.mention_dismissed == Some(anchor) {
      return None;
    }
    let logins = pr_view::mention_candidates(self.participants(), &query, 5);
    (!logins.is_empty()).then_some(CommentMention { anchor, logins })
  }

  /// Replaces `@query` with `@login `.
  fn select_mention(
    &mut self,
    anchor: usize,
    login: &str,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let comment = self.comment.clone();
    let state = self.textarea("github.comment", &comment, "", 3, window, cx);
    let cursor = state.read(cx).cursor();
    let inserted = format!("@{login} ");
    let text = replace_range(&comment, anchor, cursor, &inserted);
    let cursor = anchor + inserted.len();
    self.comment = text.clone();
    self.mention_index = 0;
    state.update(cx, |s, cx| {
      s.set_value(text, window, cx);
      s.set_selected_range(cursor..cursor, cx);
      s.focus(window, cx);
    });
    cx.notify();
  }

  /// Arrow keys, Enter and Escape steer the suggestions while they show.
  pub(super) fn mention_keys(&self, box_: Div, cursor: usize, cx: &mut Context<Self>) -> Div {
    let open = move |view: &Self| view.comment_mention(cursor);
    box_
      .capture_action(cx.listener(move |view, _: &MoveDown, _, cx| {
        if let Some(m) = open(view) {
          view.mention_index = (view.mention_index + 1) % m.logins.len();
          cx.stop_propagation();
          cx.notify();
        }
      }))
      .capture_action(cx.listener(move |view, _: &MoveUp, _, cx| {
        if let Some(m) = open(view) {
          let n = m.logins.len();
          view.mention_index = (view.mention_index + n - 1) % n;
          cx.stop_propagation();
          cx.notify();
        }
      }))
      .capture_action(cx.listener(move |view, action: &Enter, window, cx| {
        if action.secondary {
          return;
        }
        if let Some(m) = open(view) {
          let login = m.logins[view.mention_index % m.logins.len()].clone();
          cx.stop_propagation();
          view.select_mention(m.anchor, &login, window, cx);
        }
      }))
      .capture_action(cx.listener(move |view, _: &Escape, _, cx| {
        if let Some(m) = open(view) {
          view.mention_dismissed = Some(m.anchor);
          cx.stop_propagation();
          cx.notify();
        }
      }))
  }

  /// The suggestions under the comment box.
  pub(super) fn mention_list(
    &self,
    mention: CommentMention,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let selected = self.mention_index % mention.logins.len();
    let anchor = mention.anchor;
    div()
      .debug_selector(|| "github-mentions".into())
      .rounded(radius::LG)
      .border_1()
      .border_color(p.border)
      .bg(p.muted.opacity(0.5))
      .overflow_hidden()
      .children(mention.logins.into_iter().enumerate().map(|(i, login)| {
        let id = SharedString::from(format!("github-mention-{i}"));
        ui::Row::new(id, p)
          .role(gpui::Role::ListBoxOption)
          .label(login.clone())
          .selected(i == selected)
          .w_full()
          .px_3()
          .rounded_none()
          .child(ui::text(div(), TextSize::Sm).child(format!("@{login}")))
          .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |view, _, window, cx| {
              cx.stop_propagation();
              window.prevent_default();
              view.select_mention(anchor, &login, window, cx);
            }),
          )
      }))
  }
}
