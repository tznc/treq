//! One GitHub issue: its body and comments with reactions, a composer, and
//! editing, closing, reopening and deleting it.

use crate::mobile::ui::widgets::TouchButton as _;
use std::cell::RefCell;
use std::rc::Rc;

use crate::{
  core::remote::{GithubAction, GithubQuery, TreqCommandRequest},
  github::{GhIssue, GhIssueComment, GhReactionGroup},
};
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Textarea, TextareaState},
    tag::Tag,
    v_flex, ActiveTheme, IconName, Sizable, StyledExt,
  },
  div, px, App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
  Render, SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window,
};

use super::{
  issues::{self, OnChanged, CLOSE_REASONS, REACTIONS},
  mutation_message, on_epoch, pull_request,
};
use crate::mobile::{
  errors,
  remote::{self, IdempotencyKeys, Mutation},
  ui::{load, nav, sheet, widgets, Load, Location},
};

fn request(repo: &str, number: u64) -> TreqCommandRequest {
  issues::read(repo, GithubQuery::Issue { number })
}

/// Pushes issue `number`'s page. `on_changed` runs after any change to it.
pub fn open(repo: String, number: u64, title: String, on_changed: OnChanged, cx: &mut App) {
  let screen: Rc<RefCell<Option<WeakEntity<IssueScreen>>>> = Default::default();
  let menu_screen = screen.clone();
  let _ = title;
  nav::push_with(
    format!("Issue #{number}"),
    nav::Header {
      on_title: None,
      more: Some(Rc::new(move |_, cx| {
        if let Some(screen) = menu_screen.borrow().as_ref() {
          screen.update(cx, |screen, cx| screen.menu(cx)).ok();
        }
      })),
    },
    Location {
      repo: Some(repo.clone()),
      workspace: None,
    },
    cx,
    move |window, cx| {
      let entity = cx.new(|cx| IssueScreen::new(repo, number, on_changed, window, cx));
      *screen.borrow_mut() = Some(entity.downgrade());
      entity
    },
  );
}

/// The date part of an ISO timestamp.
fn day(timestamp: &str) -> &str {
  timestamp.get(..10).unwrap_or(timestamp)
}

/// Shows a write the host confirmed on the loaded `issue`, so the page
/// changes at once; the refetch that follows replaces it with GitHub's copy.
pub fn apply(issue: &mut GhIssue, action: &GithubAction) {
  match action {
    GithubAction::CloseIssue { .. } => issue.state = "CLOSED".into(),
    GithubAction::ReopenIssue { .. } => issue.state = "OPEN".into(),
    GithubAction::EditIssue { title, body, .. } => {
      issue.title = title.clone();
      issue.body = Some(body.clone());
    }
    GithubAction::CommentOnIssue { body, .. } => {
      issue
        .comments
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

pub struct IssueScreen {
  repo: String,
  number: u64,
  issue: Load<GhIssue>,
  composer: Entity<TextareaState>,
  /// The change running now, by name.
  busy: Option<&'static str>,
  message: Option<(bool, String)>,
  keys: IdempotencyKeys,
  on_changed: OnChanged,
}

impl IssueScreen {
  fn new(
    repo: String,
    number: u64,
    on_changed: OnChanged,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let composer = cx.new(|cx| {
      TextareaState::new(window, cx)
        .placeholder("Leave a comment")
        .rows(3)
    });
    let issue = remote::last(&request(&repo, number)).map_or(Load::Idle, Load::Ready);
    let mut this = Self {
      repo,
      number,
      issue,
      composer,
      busy: None,
      message: None,
      keys: IdempotencyKeys::default(),
      on_changed,
    };
    this.fetch(true, cx);
    this
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
  }

  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    if self.issue.ready().is_none() {
      self.issue = Load::Loading;
    }
    load::run(
      cx,
      remote::read_cached::<GhIssue>(request(&self.repo, self.number), join),
      |this, result, _| match result {
        Ok(issue) => this.issue = Load::Ready(issue),
        Err(error) if this.issue.ready().is_some() => {
          this.message = Some((
            false,
            format!("Couldn't refresh: {}", errors::describe(&error).message),
          ));
        }
        Err(error) => this.issue = Load::Failed(error),
      },
    );
    cx.notify();
  }

  fn is_open(&self) -> bool {
    self.issue.ready().is_some_and(|i| i.state != "CLOSED")
  }

  /// Runs `action` once per idempotency key, then reports `success`,
  /// refetches, and runs `after` when it applied.
  fn write(
    &mut self,
    name: &'static str,
    inputs: Vec<String>,
    action: GithubAction,
    success: String,
    after: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    if self.busy.is_some() {
      return;
    }
    let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let key = self.keys.key_for(name, &refs);
    self.busy = Some(name);
    self.message = None;
    let request = TreqCommandRequest::GithubWrite {
      repo: self.repo.clone(),
      action: action.clone(),
      idempotency_key: key,
    };
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      this.busy = None;
      let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
      this.keys.settle(name, &refs, &outcome);
      let (ok, text) = mutation_message(&outcome, &success);
      this.message = Some((
        ok,
        if ok {
          text
        } else {
          errors::describe(&text).message
        },
      ));
      if matches!(outcome, Ok(Mutation::Applied(_))) {
        if let Load::Ready(issue) = &mut this.issue {
          apply(issue, &action);
        }
        (this.on_changed)(cx);
        after(this, cx);
      }
      this.reload(cx);
    });
    cx.notify();
  }

  fn menu(&mut self, cx: &mut Context<Self>) {
    let Some(issue) = self.issue.ready() else {
      return;
    };
    let this = cx.weak_entity();
    let url = issue.url.clone();
    let mut entries = vec![sheet::MenuEntry::new(
      "menu-edit",
      "Edit title and description",
      {
        let this = this.clone();
        move |_, cx| {
          this.update(cx, |screen, cx| screen.edit_sheet(cx)).ok();
        }
      },
    )];
    if self.is_open() {
      for (reason, label) in CLOSE_REASONS {
        let this = this.clone();
        entries.push(sheet::MenuEntry::new(
          format!("menu-close-{reason}"),
          label,
          move |_, cx| {
            this.update(cx, |screen, cx| screen.close(reason, cx)).ok();
          },
        ));
      }
    } else {
      let this = this.clone();
      entries.push(sheet::MenuEntry::new(
        "menu-reopen",
        "Reopen issue",
        move |_, cx| {
          this.update(cx, |screen, cx| screen.reopen(cx)).ok();
        },
      ));
    }
    entries.push(sheet::MenuEntry::new("menu-github", "Open on GitHub", {
      let this = this.clone();
      move |_, cx| {
        let error = pull_request::open_url(&url).err();
        this
          .update(cx, |screen, cx| {
            if let Some(error) = error {
              screen.message = Some((false, error));
              cx.notify();
            }
          })
          .ok();
      }
    }));
    entries.push(
      sheet::MenuEntry::new("menu-delete", "Delete issue", move |_, cx| {
        this.update(cx, |screen, cx| screen.confirm_delete(cx)).ok();
      })
      .danger(),
    );
    sheet::menu(format!("Issue #{}", self.number), entries, cx);
  }

  fn close_menu(&mut self, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    let entries = CLOSE_REASONS
      .iter()
      .map(|&(reason, label)| {
        let this = this.clone();
        sheet::MenuEntry::new(format!("close-{reason}"), label, move |_, cx| {
          this.update(cx, |screen, cx| screen.close(reason, cx)).ok();
        })
      })
      .collect();
    sheet::menu(format!("Close issue #{}", self.number), entries, cx);
  }

  fn close(&mut self, reason: &'static str, cx: &mut Context<Self>) {
    let number = self.number;
    self.write(
      "issue-close",
      vec![self.repo.clone(), number.to_string(), reason.into()],
      GithubAction::CloseIssue {
        number,
        reason: reason.into(),
      },
      format!("Issue #{number} closed."),
      |_, _| {},
      cx,
    );
  }

  fn reopen(&mut self, cx: &mut Context<Self>) {
    let number = self.number;
    self.write(
      "issue-reopen",
      vec![self.repo.clone(), number.to_string()],
      GithubAction::ReopenIssue { number },
      format!("Issue #{number} reopened."),
      |_, _| {},
      cx,
    );
  }

  fn edit_sheet(&mut self, cx: &mut Context<Self>) {
    let Some(issue) = self.issue.ready() else {
      return;
    };
    let this = cx.weak_entity();
    issues::form(
      "Edit issue",
      issue.title.clone(),
      issue.body.clone().unwrap_or_default(),
      "Save",
      Rc::new(move |title, body, cx| {
        this
          .update(cx, |screen, cx| screen.edit(title, body, cx))
          .ok();
      }),
      cx,
    );
  }

  fn edit(&mut self, title: String, body: String, cx: &mut Context<Self>) {
    let number = self.number;
    self.write(
      "issue-edit",
      vec![
        self.repo.clone(),
        number.to_string(),
        title.clone(),
        body.clone(),
      ],
      GithubAction::EditIssue {
        number,
        title,
        body,
      },
      "Issue updated.".into(),
      |_, _| {},
      cx,
    );
  }

  fn confirm_delete(&mut self, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    let number = self.number;
    sheet::confirm(
      format!("Delete issue #{number}?"),
      "Deletes the issue and its comments from GitHub for everyone. This can't be undone.",
      "Delete issue",
      true,
      move |_, cx| {
        this.update(cx, |screen, cx| screen.delete(cx)).ok();
      },
      cx,
    );
  }

  fn delete(&mut self, cx: &mut Context<Self>) {
    let number = self.number;
    self.write(
      "issue-delete",
      vec![self.repo.clone(), number.to_string()],
      GithubAction::DeleteIssue { number },
      format!("Issue #{number} deleted."),
      |_, cx| nav::pop(cx),
      cx,
    );
  }

  fn comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let body = self.composer.read(cx).value().trim().to_string();
    if body.is_empty() || self.busy.is_some() {
      return;
    }
    let number = self.number;
    let composer = self.composer.downgrade();
    let window_handle = window.window_handle();
    self.write(
      "issue-comment",
      vec![self.repo.clone(), number.to_string(), body.clone()],
      GithubAction::CommentOnIssue { number, body },
      "Comment added.".into(),
      move |_, cx| {
        let Some(composer) = composer.upgrade() else {
          return;
        };
        window_handle
          .update(cx, |_, window, cx| {
            composer.update(cx, |state, cx| state.set_value("", window, cx));
          })
          .ok();
      },
      cx,
    );
  }

  /// Flips the viewer's reaction at once and sends it; a failure refetches.
  fn react(&mut self, comment_id: String, content: String, cx: &mut Context<Self>) {
    let Load::Ready(issue) = &mut self.issue else {
      return;
    };
    let Some(comment) = issue
      .comments
      .iter_mut()
      .flatten()
      .find(|c| c.id == comment_id)
    else {
      return;
    };
    let on = issues::toggle_reaction(&mut comment.reactions, &content);
    let inputs = [comment_id.clone(), content.clone(), on.to_string()];
    let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let key = self.keys.key_for("reaction", &refs);
    let request = TreqCommandRequest::GithubWrite {
      repo: self.repo.clone(),
      action: GithubAction::SetReaction {
        comment_id,
        content,
        on,
      },
      idempotency_key: key,
    };
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
      this.keys.settle("reaction", &refs, &outcome);
      if !matches!(outcome, Ok(Mutation::Applied(_))) {
        let (_, text) = mutation_message(&outcome, "");
        this.message = Some((false, errors::describe(&text).message));
        this.reload(cx);
      }
    });
    cx.notify();
  }

  fn reaction_picker(&mut self, comment_id: String, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    let entries = REACTIONS
      .iter()
      .map(|&(content, emoji)| {
        let (this, comment_id) = (this.clone(), comment_id.clone());
        sheet::MenuEntry::new(
          format!("reaction-pick-{content}"),
          format!("{emoji}  {}", content.replace('_', " ").to_lowercase()),
          move |_, cx| {
            this
              .update(cx, |screen, cx| {
                screen.react(comment_id.clone(), content.into(), cx)
              })
              .ok();
          },
        )
      })
      .collect();
    sheet::menu("React", entries, cx);
  }

  fn reactions(&self, comment: &GhIssueComment, cx: &mut Context<Self>) -> impl IntoElement {
    let id = comment.id.clone();
    let groups: Vec<GhReactionGroup> = comment.reactions.clone();
    h_flex()
      .flex_wrap()
      .gap_2()
      .children(groups.into_iter().map(|group| {
        let (id, content) = (id.clone(), group.content.clone());
        Button::new(SharedString::from(format!(
          "reaction-{id}-{}",
          group.content
        )))
        .small()
        .h(px(36.))
        .min_w(px(44.))
        .label(format!("{} {}", issues::emoji(&group.content), group.count))
        .map(|b| {
          if group.viewer_has_reacted {
            b.primary()
          } else {
            b.outline()
          }
        })
        .on_click(cx.listener(move |this, _, _, cx| this.react(id.clone(), content.clone(), cx)))
        .touch()
      }))
      .child(
        Button::new(SharedString::from(format!("reaction-add-{id}")))
          .ghost()
          .small()
          .h(px(36.))
          .min_w(px(44.))
          .icon(IconName::Plus)
          .on_click(cx.listener(move |this, _, _, cx| this.reaction_picker(id.clone(), cx)))
          .touch(),
      )
  }

  fn header(&self, issue: &GhIssue, cx: &mut Context<Self>) -> impl IntoElement {
    let open = issue.state != "CLOSED";
    v_flex()
      .gap_2()
      .child(div().text_xl().font_semibold().child(issue.title.clone()))
      .child(
        h_flex()
          .flex_wrap()
          .gap_2()
          .items_center()
          .child(
            if open {
              Tag::success()
            } else {
              Tag::secondary()
            }
            .small()
            .child(issues::state_label(&issue.state)),
          )
          .child(widgets::muted(
            format!(
              "#{} · {} opened {}",
              issue.number,
              issue.author.login,
              day(&issue.created_at)
            ),
            cx,
          )),
      )
      .when(!issue.labels.is_empty(), |h| {
        h.child(
          h_flex().flex_wrap().gap_1().children(
            issue
              .labels
              .iter()
              .map(|l| Tag::secondary().small().child(l.name.clone())),
          ),
        )
      })
  }

  fn bottom_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let theme = cx.theme();
    let open = self.is_open();
    let empty = self.composer.read(cx).value().trim().is_empty();
    v_flex()
      .p_3()
      .gap_2()
      .border_t_1()
      .border_color(theme.border)
      .bg(theme.background)
      .child(Textarea::new(&self.composer))
      .child(
        h_flex()
          .gap_2()
          .child(
            Button::new("issue-state")
              .outline()
              .large()
              .h(px(widgets::CTA_H - 8.))
              .label(if open { "Close…" } else { "Reopen" })
              .loading(matches!(self.busy, Some("issue-close" | "issue-reopen")))
              .disabled(self.issue.ready().is_none())
              .on_click(cx.listener(move |this, _, _, cx| {
                if open {
                  this.close_menu(cx)
                } else {
                  this.reopen(cx)
                }
              }))
              .touch(),
          )
          .child(
            Button::new("issue-comment")
              .primary()
              .large()
              .flex_1()
              .h(px(widgets::CTA_H - 8.))
              .label("Comment")
              .loading(self.busy == Some("issue-comment"))
              .disabled(empty || self.issue.ready().is_none())
              .on_click(cx.listener(|this, _, window, cx| this.comment(window, cx)))
              .touch(),
          ),
      )
  }
}

impl Render for IssueScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let content = match &self.issue {
      Load::Idle | Load::Loading => widgets::card(cx)
        .child(widgets::centered_message("Loading issue…", cx))
        .into_any_element(),
      Load::Failed(error) => widgets::card(cx)
        .p_4()
        .gap_3()
        .child(widgets::error_text(errors::describe(error).message, cx))
        .child(
          h_flex().child(
            Button::new("retry")
              .outline()
              .large()
              .label("Retry")
              .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
              .touch(),
          ),
        )
        .into_any_element(),
      Load::Ready(issue) => {
        let issue = issue.clone();
        let body = issue.body.clone().filter(|b| !b.trim().is_empty());
        let comments = issue.comments.clone().unwrap_or_default();
        v_flex()
          .gap_4()
          .child(self.header(&issue, cx))
          .child(widgets::card(cx).p_4().child(match body {
            Some(body) => widgets::markdown("issue-body", &body, cx).into_any_element(),
            None => widgets::muted("No description provided.", cx).into_any_element(),
          }))
          .child(
            widgets::section(format!("Comments · {}", comments.len()), cx)
              .when(comments.is_empty(), |s| {
                s.child(widgets::card(cx).child(widgets::centered_message("No comments yet.", cx)))
              })
              .children(comments.iter().enumerate().map(|(i, comment)| {
                widgets::card(cx)
                  .p_4()
                  .gap_2()
                  .child(widgets::muted(
                    match day(&comment.created_at) {
                      "" => comment.author.login.clone(),
                      date => format!("{} · {date}", comment.author.login),
                    },
                    cx,
                  ))
                  .child(widgets::markdown(("issue-comment", i), &comment.body, cx))
                  .child(self.reactions(comment, cx))
              })),
          )
          .into_any_element()
      }
    };
    v_flex()
      .size_full()
      .child(
        div()
          .id("issue")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .child(
            widgets::page()
              .gap_3()
              .children(self.message.clone().map(|(ok, text)| {
                if ok {
                  widgets::muted(text, cx)
                } else {
                  widgets::error_text(text, cx)
                }
              }))
              .child(content),
          ),
      )
      .child(self.bottom_bar(cx))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::github::GhAuthor;

  fn issue(state: &str) -> GhIssue {
    GhIssue {
      number: 7,
      title: "Old".into(),
      state: state.into(),
      url: "u".into(),
      body: Some("old body".into()),
      author: GhAuthor {
        login: "alice".into(),
        avatar_url: None,
      },
      labels: vec![],
      created_at: "c".into(),
      updated_at: "u".into(),
      comments: Some(vec![]),
    }
  }

  #[core::prelude::v1::test]
  fn applied_writes_show_before_the_refetch() {
    let mut i = issue("OPEN");
    apply(
      &mut i,
      &GithubAction::CloseIssue {
        number: 7,
        reason: "completed".into(),
      },
    );
    assert_eq!(i.state, "CLOSED");
    apply(&mut i, &GithubAction::ReopenIssue { number: 7 });
    assert_eq!(i.state, "OPEN");
    apply(
      &mut i,
      &GithubAction::EditIssue {
        number: 7,
        title: "New".into(),
        body: "new body".into(),
      },
    );
    assert_eq!(
      (i.title.as_str(), i.body.as_deref()),
      ("New", Some("new body"))
    );
    apply(
      &mut i,
      &GithubAction::CommentOnIssue {
        number: 7,
        body: "hi".into(),
      },
    );
    let comments = i.comments.as_ref().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].body, "hi");
    assert_eq!(comments[0].author.login, "You");
  }
}
