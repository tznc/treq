//! Reactions on GitHub comments: the counts under a comment, and the
//! picker of GitHub's eight reactions that adds or removes the user's own.

use tauri_plugin_gpui::gpui::{deferred, div, prelude::*, Context, Div, SharedString};

use crate::{
  github::GhReactionGroup,
  gpui_shell::{
    popup::{Side, TooltipContent},
    theme::{self, tw, Palette},
    tooltips::{HasTooltips, Tooltips},
    ui::{self, zpx, Button, DismissExt as _, Size, TextSize, Variant},
  },
};

/// GitHub's `ReactionContent`s, in the order GitHub lists them, with their
/// emoji and names.
pub(super) const REACTIONS: [(&str, &str, &str); 8] = [
  ("THUMBS_UP", "👍", "Thumbs up"),
  ("THUMBS_DOWN", "👎", "Thumbs down"),
  ("LAUGH", "😄", "Laugh"),
  ("HOORAY", "🎉", "Hooray"),
  ("CONFUSED", "😕", "Confused"),
  ("HEART", "❤️", "Heart"),
  ("ROCKET", "🚀", "Rocket"),
  ("EYES", "👀", "Eyes"),
];

fn known(content: &str) -> Option<(usize, &'static str, &'static str)> {
  REACTIONS
    .iter()
    .enumerate()
    .find(|(_, (c, _, _))| *c == content)
    .map(|(i, (_, emoji, name))| (i, *emoji, *name))
}

/// Adds the user's `content` reaction to `groups`, or takes it away when
/// they already reacted with it. Returns whether it is now on.
pub(super) fn toggle(groups: &mut Vec<GhReactionGroup>, content: &str) -> bool {
  match groups.iter().position(|g| g.content == content) {
    Some(i) if groups[i].viewer_has_reacted => {
      groups[i].count = groups[i].count.saturating_sub(1);
      groups[i].viewer_has_reacted = false;
      if groups[i].count == 0 {
        groups.remove(i);
      }
      false
    }
    Some(i) => {
      groups[i].count += 1;
      groups[i].viewer_has_reacted = true;
      true
    }
    None => {
      groups.push(GhReactionGroup {
        content: content.to_string(),
        count: 1,
        viewer_has_reacted: true,
      });
      groups.sort_by_key(|g| known(&g.content).map_or(usize::MAX, |(i, ..)| i));
      true
    }
  }
}

/// A view that draws reactions: it knows which comment's picker is open and
/// toggles a reaction on a comment.
pub(super) trait Reacts: HasTooltips + Sized + 'static {
  /// The comment whose picker is open.
  fn reaction_picker(&mut self) -> &mut Option<String>;
  /// Toggles the user's `content` reaction on comment `id`, at once and
  /// then on GitHub, undone if GitHub refuses.
  fn react(&mut self, id: &str, content: &'static str, cx: &mut Context<Self>);

  fn set_reaction_picker(&mut self, id: Option<String>, cx: &mut Context<Self>) {
    *self.reaction_picker() = id;
    cx.notify();
  }
}

/// The add-reaction button, then a chip per reaction with its count. The
/// chips of reactions the user gave are highlighted; a click toggles one.
pub(super) fn bar<V: Reacts>(
  tips: &Tooltips,
  picker_open: Option<&str>,
  id: &str,
  groups: &[GhReactionGroup],
  p: &Palette,
  cx: &mut Context<V>,
) -> Div {
  let open = picker_open == Some(id);
  let picker_id = id.to_string();
  let add = Button::new(SharedString::from(format!("reaction-add-{id}")), p)
    .variant(Variant::Ghost)
    .size(Size::IconXs)
    .rounded_full()
    .aria_label("Add reaction")
    .child(ui::icon(theme::ICON_SMILE_PLUS, 14., p.muted_foreground))
    .on_click(cx.listener(move |view: &mut V, _, _, cx| {
      let next = (!open).then(|| picker_id.clone());
      view.set_reaction_picker(next, cx);
    }));
  let add = tips.wrap(
    format!("reaction-add-{id}"),
    Side::Top,
    Some(TooltipContent::Text("Add reaction".into())),
    add,
    cx,
  );
  div()
    .flex()
    .flex_wrap()
    .items_center()
    .gap_1()
    .mt_2()
    .child(
      div()
        .relative()
        .child(add)
        .when(open, |d| d.child(picker(id, groups, p, cx))),
    )
    .children(groups.iter().filter_map(|g| {
      let (_, emoji, name) = known(&g.content)?;
      Some(chip(id, g, emoji, name, p, cx))
    }))
}

fn chip<V: Reacts>(
  id: &str,
  group: &GhReactionGroup,
  emoji: &'static str,
  name: &'static str,
  p: &Palette,
  cx: &mut Context<V>,
) -> Button {
  let sky = tw::color(tw::SKY_500);
  let mine = group.viewer_has_reacted;
  let (comment, content) = (id.to_string(), static_content(&group.content));
  Button::new(
    SharedString::from(format!("reaction-{id}-{}", group.content)),
    p,
  )
  .variant(Variant::Outline)
  .size(Size::Xs)
  .rounded_full()
  .gap_1()
  .when(mine, |b| {
    b.border_color(sky.opacity(0.6)).bg(sky.opacity(0.1))
  })
  .aria_label(format!(
    "{name} ({}{})",
    group.count,
    if mine { ", you reacted" } else { "" }
  ))
  .child(emoji)
  .child(
    ui::text(div(), TextSize::Xs)
      .text_color(if mine { sky } else { p.muted_foreground })
      .child(group.count.to_string()),
  )
  .on_click(cx.listener(move |view: &mut V, _, _, cx| {
    if let Some(content) = content {
      view.react(&comment, content, cx);
    }
  }))
}

/// GitHub's eight reactions in a row below the add button.
fn picker<V: Reacts>(
  id: &str,
  groups: &[GhReactionGroup],
  p: &Palette,
  cx: &mut Context<V>,
) -> impl IntoElement {
  let sky = tw::color(tw::SKY_500);
  deferred(
    div()
      .id(SharedString::from(format!("reaction-picker-{id}")))
      .debug_selector(|| "reaction-picker".into())
      .absolute()
      .top(zpx(28.))
      .left_0()
      .p_1()
      .flex()
      .gap_1()
      .rounded(zpx(8.))
      .border_1()
      .border_color(p.border)
      .bg(p.popover)
      .shadow_md()
      .occlude()
      .dismiss_with(format!("reaction-picker-{id}"), cx, |view: &mut V, cx| {
        let open = view.reaction_picker().is_some();
        view.set_reaction_picker(None, cx);
        open
      })
      .children(REACTIONS.iter().map(|&(content, emoji, name)| {
        let mine = groups
          .iter()
          .any(|g| g.content == content && g.viewer_has_reacted);
        let comment = id.to_string();
        Button::new(
          SharedString::from(format!("reaction-pick-{id}-{content}")),
          p,
        )
        .variant(Variant::Ghost)
        .size(Size::IconXs)
        .when(mine, |b| b.bg(sky.opacity(0.15)))
        .aria_label(name)
        .child(emoji)
        .on_click(cx.listener(move |view: &mut V, _, _, cx| {
          view.set_reaction_picker(None, cx);
          view.react(&comment, content, cx);
        }))
      })),
  )
  .with_priority(1)
}

fn static_content(content: &str) -> Option<&'static str> {
  REACTIONS
    .iter()
    .find(|(c, _, _)| *c == content)
    .map(|(c, _, _)| *c)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn group(content: &str, count: u32, mine: bool) -> GhReactionGroup {
    GhReactionGroup {
      content: content.into(),
      count,
      viewer_has_reacted: mine,
    }
  }

  #[test]
  fn toggle_adds_a_new_reaction_in_github_order() {
    let mut groups = vec![group("HEART", 2, false)];
    assert!(toggle(&mut groups, "THUMBS_UP"));
    assert_eq!(
      groups,
      vec![group("THUMBS_UP", 1, true), group("HEART", 2, false)]
    );
  }

  #[test]
  fn toggle_joins_and_leaves_others() {
    let mut groups = vec![group("HEART", 2, false)];
    assert!(toggle(&mut groups, "HEART"));
    assert_eq!(groups, vec![group("HEART", 3, true)]);
    assert!(!toggle(&mut groups, "HEART"));
    assert_eq!(groups, vec![group("HEART", 2, false)]);
  }

  #[test]
  fn toggle_drops_a_reaction_no_one_has_left() {
    let mut groups = vec![group("ROCKET", 1, true)];
    assert!(!toggle(&mut groups, "ROCKET"));
    assert!(groups.is_empty());
  }
}
