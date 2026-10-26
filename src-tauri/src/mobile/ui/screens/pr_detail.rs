//! One GitHub pull request: its description, checks, comments with
//! reactions, a comment box, and the state changes (close, reopen, ready
//! for review, back to draft). [`open`] pushes it; the state changes sit in
//! the title bar's ⋯ menu ([`menu_items`]), so another one is one more
//! [`PrAction`].

use crate::mobile::ui::widgets::TouchButton as _;
use std::rc::Rc;

use crate::{
  core::remote::{GithubAction, GithubQuery, TreqCommandRequest},
  github::{GhIssueComment, GhPullRequest, GhReactionGroup, PrCiStatus},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Textarea, TextareaState},
    tag::Tag,
    v_flex, ActiveTheme, IconName, Sizable, StyledExt,
  },
  div, px, App, AppContext, ClipboardItem, Context, Div, Entity, InteractiveElement, IntoElement,
  ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
  WeakEntity, Window,
};

use super::{
  mutation_message, on_epoch,
  pull_request::{check_icon, checks_summary, duration, open_url},
  pull_requests::state_tag,
};
use crate::mobile::{
  errors,
  remote::{self, IdempotencyKeys, Mutation},
  ui::{load, nav, sheet, widgets, Load, Location},
};

/// GitHub's `ReactionContent`s in GitHub's order, with their emoji.
pub const REACTIONS: [(&str, &str); 8] = [
  ("THUMBS_UP", "👍"),
  ("THUMBS_DOWN", "👎"),
  ("LAUGH", "😄"),
  ("HOORAY", "🎉"),
  ("CONFUSED", "😕"),
  ("HEART", "❤️"),
  ("ROCKET", "🚀"),
  ("EYES", "👀"),
];

fn position(content: &str) -> usize {
  REACTIONS
    .iter()
    .position(|(c, _)| *c == content)
    .unwrap_or(usize::MAX)
}

/// The emoji for a reaction, or its name when GitHub adds one.
pub fn emoji(content: &str) -> &str {
  REACTIONS
    .iter()
    .find(|(c, _)| *c == content)
    .map_or(content, |(_, e)| e)
}

/// Adds the user's `content` reaction to `groups`, or takes it away when
/// they already gave it. Returns whether it is now on.
pub fn toggle_reaction(groups: &mut Vec<GhReactionGroup>, content: &str) -> bool {
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
      groups.sort_by_key(|g| position(&g.content));
      true
    }
  }
}

/// A change to the pull request's state, offered in the ⋯ menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrAction {
  MarkReady,
  ConvertToDraft,
  Close,
  Reopen,
}

impl PrAction {
  pub fn label(self) -> &'static str {
    match self {
      PrAction::MarkReady => "Ready for review",
      PrAction::ConvertToDraft => "Convert to draft",
      PrAction::Close => "Close",
      PrAction::Reopen => "Reopen",
    }
  }

  /// What the host runs.
  pub fn github_action(self, number: u64) -> GithubAction {
    match self {
      PrAction::MarkReady => GithubAction::SetPrDraft {
        number,
        draft: false,
      },
      PrAction::ConvertToDraft => GithubAction::SetPrDraft {
        number,
        draft: true,
      },
      PrAction::Close => GithubAction::ClosePr { number },
      PrAction::Reopen => GithubAction::ReopenPr { number },
    }
  }

  /// What the page says once it applied.
  pub fn done_message(self) -> &'static str {
    match self {
      PrAction::MarkReady => "Marked ready for review.",
      PrAction::ConvertToDraft => "Converted to draft.",
      PrAction::Close => "Pull request closed.",
      PrAction::Reopen => "Pull request reopened.",
    }
  }

  /// Whether it asks before running.
  pub fn destructive(self) -> bool {
    self == PrAction::Close
  }

  fn key(self) -> &'static str {
    match self {
      PrAction::MarkReady => "pr-ready",
      PrAction::ConvertToDraft => "pr-draft",
      PrAction::Close => "pr-close",
      PrAction::Reopen => "pr-reopen",
    }
  }
}

/// The state changes `pr` offers: none once merged.
pub fn actions(pr: &GhPullRequest) -> Vec<PrAction> {
  match pr.state.as_str() {
    "MERGED" => Vec::new(),
    "CLOSED" => vec![PrAction::Reopen],
    _ if pr.is_draft => vec![PrAction::MarkReady, PrAction::Close],
    _ => vec![PrAction::ConvertToDraft, PrAction::Close],
  }
}

/// Shows a write the host confirmed on the loaded `pr`, so the page changes
/// at once; the refetch that follows replaces it with GitHub's copy.
pub fn apply(pr: &mut GhPullRequest, action: &GithubAction) {
  match action {
    GithubAction::ClosePr { .. } => pr.state = "CLOSED".into(),
    GithubAction::ReopenPr { .. } => pr.state = "OPEN".into(),
    GithubAction::SetPrDraft { draft, .. } => pr.is_draft = *draft,
    GithubAction::CommentOnPr { body, .. } => {
      pr.comments
        .get_or_insert_with(Vec::new)
        .push(GhIssueComment {
          id: String::new(),
          body: body.clone(),
          author: crate::github::GhAuthor {
            login: "You".into(),
            avatar_url: None,
          },
          created_at: String::new(),
          reactions: Vec::new(),
        });
    }
    _ => {}
  }
}

/// An entry of the title bar's ⋯ menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
  Change(PrAction),
  OpenOnGithub,
  CopyLink,
  Refresh,
}

/// The ⋯ menu: `pr`'s state changes, then its links, then Refresh.
pub fn menu_items(pr: Option<&GhPullRequest>) -> Vec<MenuItem> {
  let Some(pr) = pr else {
    return vec![MenuItem::Refresh];
  };
  actions(pr)
    .into_iter()
    .map(MenuItem::Change)
    .chain([
      MenuItem::OpenOnGithub,
      MenuItem::CopyLink,
      MenuItem::Refresh,
    ])
    .collect()
}

pub fn request(repo: &str, number: u64) -> TreqCommandRequest {
  TreqCommandRequest::GithubRead {
    repo: repo.to_string(),
    query: GithubQuery::Pr { number },
  }
}

pub fn checks_request(repo: &str, number: u64) -> TreqCommandRequest {
  TreqCommandRequest::GithubRead {
    repo: repo.to_string(),
    query: GithubQuery::PrChecks { number },
  }
}

/// Pushes pull request `number` of the repository at `repo` on the host.
pub fn open(repo: String, number: u64, cx: &mut App) {
  let location = Location {
    repo: Some(repo.clone()),
    workspace: None,
  };
  let screen: Rc<std::cell::RefCell<Option<WeakEntity<PrDetailScreen>>>> = Default::default();
  let menu_screen = screen.clone();
  nav::push_with(
    format!("Pull request #{number}"),
    nav::Header {
      on_title: None,
      more: Some(Rc::new(move |_, cx| {
        if let Some(screen) = menu_screen.borrow().as_ref() {
          screen.update(cx, |screen, cx| screen.menu(cx)).ok();
        }
      })),
    },
    location,
    cx,
    move |window, cx| {
      let entity = cx.new(|cx| PrDetailScreen::new(repo, number, window, cx));
      *screen.borrow_mut() = Some(entity.downgrade());
      entity
    },
  );
}

pub struct PrDetailScreen {
  repo: String,
  number: u64,
  pr: Load<GhPullRequest>,
  checks: Load<Option<PrCiStatus>>,
  comment: Entity<TextareaState>,
  /// The mutation running, by its key; others wait for it.
  busy: Option<&'static str>,
  message: Option<(bool, String)>,
  keys: IdempotencyKeys,
  /// The comment whose reaction picker is open.
  picker: Option<String>,
  scroll: ScrollHandle,
}

impl PrDetailScreen {
  fn new(repo: String, number: u64, window: &mut Window, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let pr = remote::last(&request(&repo, number)).map_or(Load::Idle, Load::Ready);
    let checks = remote::last(&checks_request(&repo, number)).map_or(Load::Idle, Load::Ready);
    let comment = cx.new(|cx| {
      TextareaState::new(window, cx)
        .rows(3)
        .placeholder("Leave a comment")
    });
    let mut this = Self {
      repo,
      number,
      pr,
      checks,
      comment,
      busy: None,
      message: None,
      keys: IdempotencyKeys::default(),
      picker: None,
      scroll: ScrollHandle::new(),
    };
    this.fetch(true, cx);
    this
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
  }

  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    if self.pr.ready().is_none() {
      self.pr = Load::Loading;
    }
    if self.checks.ready().is_none() {
      self.checks = Load::Loading;
    }
    load::run(
      cx,
      remote::read_cached::<GhPullRequest>(request(&self.repo, self.number), join),
      |this, result, _| match result {
        Ok(pr) => this.pr = Load::Ready(pr),
        Err(error) if this.pr.ready().is_some() => {
          this.message = Some((
            false,
            format!("Couldn't refresh: {}", errors::describe(&error).message),
          ))
        }
        Err(error) => this.pr = Load::Failed(error),
      },
    );
    load::run(
      cx,
      remote::read_cached::<Option<PrCiStatus>>(checks_request(&self.repo, self.number), join),
      |this, result, _| {
        if result.is_ok() || this.checks.ready().is_none() {
          this.checks = Load::from_result(result);
        }
      },
    );
    cx.notify();
  }

  fn url(&self) -> Option<String> {
    self.pr.ready().map(|pr| pr.url.clone())
  }

  fn open_on_github(&mut self, cx: &mut Context<Self>) {
    if let Some(url) = self.url() {
      if let Err(error) = open_url(&url) {
        self.message = Some((false, error));
      }
    }
    cx.notify();
  }

  /// The title bar's ⋯.
  fn menu(&mut self, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    let url = self.url().unwrap_or_default();
    let entries = menu_items(self.pr.ready())
      .into_iter()
      .map(|item| {
        let this = this.clone();
        match item {
          MenuItem::Change(action) => {
            let entry = sheet::MenuEntry::new(action.key(), action.label(), move |window, cx| {
              this
                .update(cx, |screen, cx| screen.tap_action(action, window, cx))
                .ok();
            });
            if action.destructive() {
              entry.danger()
            } else {
              entry
            }
          }
          MenuItem::OpenOnGithub => {
            sheet::MenuEntry::new("menu-open", "Open on GitHub", move |_, cx| {
              this.update(cx, |screen, cx| screen.open_on_github(cx)).ok();
            })
          }
          MenuItem::CopyLink => {
            let url = url.clone();
            sheet::MenuEntry::new("menu-copy", "Copy link", move |_, cx| {
              cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))
            })
          }
          MenuItem::Refresh => sheet::MenuEntry::new("menu-refresh", "Refresh", move |_, cx| {
            this.update(cx, |screen, cx| screen.reload(cx)).ok();
          }),
        }
      })
      .collect();
    sheet::menu(format!("Pull request #{}", self.number), entries, cx);
  }

  /// Runs `action` on GitHub once per idempotency key, then reloads.
  #[expect(
    clippy::too_many_arguments,
    reason = "each argument is a distinct part of the write; a struct would only rename them"
  )]
  fn write(
    &mut self,
    key: &'static str,
    inputs: Vec<String>,
    action: GithubAction,
    success: &'static str,
    window: &mut Window,
    cx: &mut Context<Self>,
    applied: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
  ) {
    if self.busy.is_some() {
      return;
    }
    let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let idempotency_key = self.keys.key_for(key, &refs);
    self.busy = Some(key);
    self.message = None;
    let request = TreqCommandRequest::GithubWrite {
      repo: self.repo.clone(),
      action: action.clone(),
      idempotency_key,
    };
    load::run_in(
      window,
      cx,
      remote::mutate(request),
      move |this, outcome, window, cx| {
        this.busy = None;
        let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
        this.keys.settle(key, &refs, &outcome);
        let (ok, text) = mutation_message(&outcome, success);
        this.message = Some((
          ok,
          if ok {
            text
          } else {
            errors::describe(&text).message
          },
        ));
        if matches!(outcome, Ok(Mutation::Applied(_))) {
          if let Load::Ready(pr) = &mut this.pr {
            apply(pr, &action);
          }
          applied(this, window, cx);
        }
        this.reload(cx);
      },
    );
    cx.notify();
  }

  fn run_action(&mut self, action: PrAction, window: &mut Window, cx: &mut Context<Self>) {
    let number = self.number;
    self.write(
      action.key(),
      vec![self.repo.clone(), number.to_string()],
      action.github_action(number),
      action.done_message(),
      window,
      cx,
      |_, _, _| {},
    );
  }

  fn tap_action(&mut self, action: PrAction, window: &mut Window, cx: &mut Context<Self>) {
    if !action.destructive() {
      return self.run_action(action, window, cx);
    }
    let this = cx.weak_entity();
    sheet::confirm(
      format!("Close #{}?", self.number),
      "Closes the pull request on GitHub without merging it. It can be reopened.",
      "Close pull request",
      true,
      move |window, cx| {
        this
          .update(cx, |screen, cx| screen.run_action(action, window, cx))
          .ok();
      },
      cx,
    );
  }

  fn send_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let body = self.comment.read(cx).value().trim().to_string();
    if body.is_empty() {
      self.message = Some((false, "Write a comment first.".into()));
      return cx.notify();
    }
    let number = self.number;
    self.write(
      "pr-comment",
      vec![self.repo.clone(), number.to_string(), body.clone()],
      GithubAction::CommentOnPr { number, body },
      "Comment added.",
      window,
      cx,
      |this, window, cx| {
        this
          .comment
          .update(cx, |state, cx| state.set_value("", window, cx));
      },
    );
  }

  /// Toggles the user's reaction at once, then on GitHub; undone if GitHub
  /// refuses or the result is unknown.
  fn react(&mut self, comment_id: String, content: &'static str, cx: &mut Context<Self>) {
    self.picker = None;
    let Load::Ready(pr) = &mut self.pr else {
      return;
    };
    let Some(comment) = pr
      .comments
      .iter_mut()
      .flatten()
      .find(|c| c.id == comment_id)
    else {
      return;
    };
    let on = toggle_reaction(&mut comment.reactions, content);
    let inputs = [comment_id.clone(), content.to_string(), on.to_string()];
    let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let idempotency_key = self.keys.key_for("reaction", &refs);
    let request = TreqCommandRequest::GithubWrite {
      repo: self.repo.clone(),
      action: GithubAction::SetReaction {
        comment_id: comment_id.clone(),
        content: content.to_string(),
        on,
      },
      idempotency_key,
    };
    load::run(cx, remote::mutate(request), move |this, outcome, _| {
      let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
      this.keys.settle("reaction", &refs, &outcome);
      if matches!(outcome, Ok(Mutation::Applied(_))) {
        return;
      }
      if let Load::Ready(pr) = &mut this.pr {
        if let Some(comment) = pr
          .comments
          .iter_mut()
          .flatten()
          .find(|c| c.id == comment_id)
        {
          toggle_reaction(&mut comment.reactions, content);
        }
      }
      let text = mutation_message(&outcome, "").1;
      this.message = Some((false, errors::describe(&text).message));
    });
    cx.notify();
  }

  fn checks_section(&self, cx: &mut Context<Self>) -> Div {
    let title = match &self.checks {
      Load::Ready(Some(checks)) => format!("Checks · {}", checks_summary(checks)),
      _ => "Checks".into(),
    };
    let card = widgets::card(cx);
    let card = match &self.checks {
      Load::Idle | Load::Loading => card.child(widgets::centered_message("Loading checks…", cx)),
      Load::Failed(error) => card
        .p_4()
        .child(widgets::error_text(errors::describe(error).message, cx)),
      Load::Ready(None) => card.child(widgets::centered_message("No checks reported.", cx)),
      Load::Ready(Some(checks)) if checks.checks.is_empty() => {
        card.child(widgets::centered_message("No checks reported.", cx))
      }
      Load::Ready(Some(checks)) => {
        card.children(checks.checks.iter().enumerate().map(|(i, check)| {
          let link = check.link.clone();
          widgets::row_full(
            ("check", i),
            Some(check_icon(check, cx)),
            check.name.clone(),
            check.duration_secs.map(|s| duration(s).into()),
            None,
            cx,
          )
          .when(!link.is_empty(), |row| {
            row.on_click(cx.listener(move |this, _, _, cx| {
              if let Err(error) = open_url(&link) {
                this.message = Some((false, error));
              }
              cx.notify();
            }))
          })
        }))
      }
    };
    widgets::section(title, cx).child(card)
  }

  fn comment_card(&self, i: usize, comment: &GhIssueComment, cx: &mut Context<Self>) -> Div {
    let theme = cx.theme();
    let picker_open = self.picker.as_deref() == Some(comment.id.as_str());
    let chip = |id: SharedString, label: String, on: bool| {
      Button::new(id).h(px(44.)).small().label(label).map(|b| {
        if on {
          b.primary()
        } else {
          b.outline()
        }
      })
    };
    let id = comment.id.clone();
    let reactions = h_flex()
      .gap_2()
      .flex_wrap()
      .children(comment.reactions.iter().map(|group| {
        let (id, content) = (comment.id.clone(), group.content.clone());
        let known = REACTIONS
          .iter()
          .find(|(c, _)| *c == content)
          .map(|(c, _)| *c);
        chip(
          SharedString::from(format!("reaction-{i}-{content}")),
          format!("{} {}", emoji(&content), group.count),
          group.viewer_has_reacted,
        )
        .when_some(known, |b, content| {
          b.on_click(cx.listener(move |this, _, _, cx| this.react(id.clone(), content, cx)))
        })
        .touch()
      }))
      .child(
        Button::new(("reaction-add", i))
          .h(px(44.))
          .w(px(44.))
          .ghost()
          .icon(IconName::Plus)
          .on_click(cx.listener(move |this, _, _, cx| {
            this.picker = if picker_open { None } else { Some(id.clone()) };
            cx.notify();
          }))
          .touch(),
      );
    let picker = picker_open.then(|| {
      h_flex()
        .gap_1()
        .flex_wrap()
        .children(REACTIONS.iter().map(|(content, e)| {
          let id = comment.id.clone();
          let content: &'static str = content;
          Button::new(SharedString::from(format!("pick-{i}-{content}")))
            .h(px(44.))
            .w(px(44.))
            .ghost()
            .label(*e)
            .on_click(cx.listener(move |this, _, _, cx| this.react(id.clone(), content, cx)))
            .touch()
        }))
    });
    widgets::card(cx)
      .p_4()
      .gap_2()
      .child(
        h_flex()
          .gap_2()
          .child(div().font_semibold().child(comment.author.login.clone()))
          .child(
            div()
              .text_sm()
              .text_color(theme.muted_foreground)
              .child(day(&comment.created_at)),
          ),
      )
      .child(widgets::markdown(("pr-comment", i), &comment.body, cx))
      .child(reactions)
      .children(picker)
  }
}

/// The date part of an ISO 8601 timestamp.
fn day(timestamp: &str) -> String {
  timestamp.get(..10).unwrap_or(timestamp).to_string()
}

impl Render for PrDetailScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let pr = match &self.pr {
      Load::Ready(pr) => pr.clone(),
      Load::Failed(error) => {
        return widgets::page()
          .child(
            widgets::card(cx)
              .p_4()
              .gap_3()
              .child(widgets::error_text(errors::describe(error).message, cx))
              .child(
                h_flex().child(
                  Button::new("retry")
                    .outline()
                    .label("Retry")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
                    .touch(),
                ),
              ),
          )
          .into_any_element()
      }
      _ => {
        return widgets::page()
          .child(widgets::card(cx).child(widgets::centered_message("Loading pull request…", cx)))
          .into_any_element()
      }
    };
    let theme = cx.theme().clone();
    let body = pr.body.clone().filter(|b| !b.trim().is_empty());
    let comments = pr.comments.clone().unwrap_or_default();
    let header = v_flex()
      .gap_2()
      .child(div().text_lg().font_semibold().child(pr.title.clone()))
      .child(
        h_flex()
          .gap_2()
          .flex_wrap()
          .child(state_tag(&pr))
          .child(widgets::muted(
            format!("#{} by {}", pr.number, pr.author.login),
            cx,
          )),
      )
      .child(
        // Branch names can be long: they wrap rather than overflow.
        div()
          .text_sm()
          .text_color(theme.muted_foreground)
          .font_family(theme.mono_font_family.clone())
          .child(format!("{} → {}", pr.head_ref_name, pr.base_ref_name)),
      )
      .when(!pr.labels.is_empty(), |h| {
        h.child(
          h_flex().gap_1().flex_wrap().children(
            pr.labels
              .iter()
              .map(|l| Tag::secondary().small().child(l.name.clone())),
          ),
        )
      });
    let description =
      widgets::section("Description", cx).child(widgets::card(cx).p_4().child(match body {
        Some(body) => widgets::markdown("pr-body", &body, cx),
        None => widgets::muted("No description provided.", cx),
      }));
    let comment_rows: Vec<Div> = comments
      .iter()
      .enumerate()
      .map(|(i, c)| self.comment_card(i, c, cx))
      .collect();
    let comments_section = widgets::section(format!("Comments · {}", comments.len()), cx)
      .when(comment_rows.is_empty(), |s| {
        s.child(widgets::card(cx).child(widgets::centered_message("No comments yet.", cx)))
      })
      .children(comment_rows);
    let busy = self.busy;
    let composer = v_flex().gap_2().child(Textarea::new(&self.comment)).child(
      Button::new("comment")
        .outline()
        .large()
        .w_full()
        .h(px(widgets::CTA_H - 8.))
        .label("Comment")
        .loading(busy == Some("pr-comment"))
        .on_click(cx.listener(|this, _, window, cx| this.send_comment(window, cx)))
        .touch(),
    );
    let checks = self.checks_section(cx);
    v_flex()
      .size_full()
      .child(
        div()
          .id("pr-detail")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .track_scroll(&self.scroll)
          .child(
            widgets::page()
              .child(header)
              .child(description)
              .child(checks)
              .child(comments_section)
              .child(composer)
              .when_some(self.message.clone(), |page, (ok, text)| {
                page.child(if ok {
                  widgets::muted(text, cx)
                } else {
                  widgets::error_text(text, cx)
                })
              }),
          ),
      )
      .child(
        // The review is this page's main job, so it stays in thumb reach.
        div()
          .p_4()
          .border_t_1()
          .border_color(theme.border)
          .bg(theme.background)
          .child(
            Button::new("pr-review")
              .primary()
              .large()
              .w_full()
              .h(px(widgets::CTA_H))
              .label("Review changes")
              .on_click(cx.listener(|this, _, _, cx| {
                super::pr_review::open(this.repo.clone(), this.number, cx)
              }))
              .touch(),
          ),
      )
      .into_any_element()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::mobile::ui::screens::pull_requests::tests::pr;

  fn group(content: &str, count: u32, mine: bool) -> GhReactionGroup {
    GhReactionGroup {
      content: content.into(),
      count,
      viewer_has_reacted: mine,
    }
  }

  #[core::prelude::v1::test]
  fn reactions_toggle_and_keep_github_order() {
    let mut groups = vec![group("HEART", 2, false)];
    assert!(toggle_reaction(&mut groups, "THUMBS_UP"));
    assert_eq!(
      groups,
      vec![group("THUMBS_UP", 1, true), group("HEART", 2, false)]
    );
    assert!(toggle_reaction(&mut groups, "HEART"));
    assert_eq!(groups[1], group("HEART", 3, true));
    assert!(!toggle_reaction(&mut groups, "THUMBS_UP"));
    assert_eq!(groups, vec![group("HEART", 3, true)]);
    assert_eq!(emoji("ROCKET"), "🚀");
    assert_eq!(emoji("NEW_ONE"), "NEW_ONE");
  }

  #[core::prelude::v1::test]
  fn applied_writes_show_before_the_refetch() {
    let mut p = pr(5, "OPEN", false);
    apply(
      &mut p,
      &GithubAction::SetPrDraft {
        number: 5,
        draft: true,
      },
    );
    assert!(p.is_draft);
    apply(&mut p, &GithubAction::ClosePr { number: 5 });
    assert_eq!(p.state, "CLOSED");
    apply(&mut p, &GithubAction::ReopenPr { number: 5 });
    assert_eq!(p.state, "OPEN");
    apply(
      &mut p,
      &GithubAction::CommentOnPr {
        number: 5,
        body: "lgtm".into(),
      },
    );
    let comments = p.comments.as_ref().unwrap();
    assert_eq!(comments.last().unwrap().body, "lgtm");
  }

  #[core::prelude::v1::test]
  fn menu_holds_state_changes_then_links() {
    use MenuItem::*;
    assert_eq!(
      menu_items(Some(&pr(1, "OPEN", false))),
      [
        Change(PrAction::ConvertToDraft),
        Change(PrAction::Close),
        OpenOnGithub,
        CopyLink,
        Refresh
      ]
    );
    assert_eq!(
      menu_items(Some(&pr(1, "MERGED", false))),
      [OpenOnGithub, CopyLink, Refresh]
    );
    assert_eq!(menu_items(None), [Refresh]);
  }

  #[core::prelude::v1::test]
  fn actions_follow_state() {
    assert_eq!(
      actions(&pr(1, "OPEN", false)),
      [PrAction::ConvertToDraft, PrAction::Close]
    );
    assert_eq!(
      actions(&pr(1, "OPEN", true)),
      [PrAction::MarkReady, PrAction::Close]
    );
    assert_eq!(actions(&pr(1, "CLOSED", false)), [PrAction::Reopen]);
    assert!(actions(&pr(1, "MERGED", false)).is_empty());
  }

  #[core::prelude::v1::test]
  fn actions_map_to_github_writes() {
    assert_eq!(
      PrAction::MarkReady.github_action(5),
      GithubAction::SetPrDraft {
        number: 5,
        draft: false
      }
    );
    assert_eq!(
      PrAction::ConvertToDraft.github_action(5),
      GithubAction::SetPrDraft {
        number: 5,
        draft: true
      }
    );
    assert_eq!(
      PrAction::Close.github_action(5),
      GithubAction::ClosePr { number: 5 }
    );
    assert_eq!(
      PrAction::Reopen.github_action(5),
      GithubAction::ReopenPr { number: 5 }
    );
    assert!(PrAction::Close.destructive());
    assert!(!PrAction::Reopen.destructive());
    assert_eq!(PrAction::MarkReady.label(), "Ready for review");
  }

  #[core::prelude::v1::test]
  fn reads_name_the_pull_request() {
    assert_eq!(
      request("/r", 9),
      TreqCommandRequest::GithubRead {
        repo: "/r".into(),
        query: GithubQuery::Pr { number: 9 }
      }
    );
    assert_eq!(
      checks_request("/r", 9),
      TreqCommandRequest::GithubRead {
        repo: "/r".into(),
        query: GithubQuery::PrChecks { number: 9 }
      }
    );
  }
}
