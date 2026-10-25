//! A pull request's code review, as on desktop's review page: the changed
//! files, then one file's diff with its review threads inline (replies and
//! reactions), comments picked from diff lines into a pending review, and a
//! sheet that submits the review with a verdict. The rules live in
//! [`super::pr_review_logic`].

use std::collections::HashSet;

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
  *,
};
use treq_lib::{
  core::{
    remote::{GithubAction, GithubContext, GithubQuery, TreqCommandRequest},
    review::{line_at, line_label, LineRef},
  },
  github::{GhReviewComment, GhReviewThread},
};

use super::{
  diff::{self, Picked},
  mutation_message, on_epoch,
  pr_detail::{self, emoji, toggle_reaction, REACTIONS},
  pr_review_logic::{
    allowed, can_submit, collapsed, detached_threads, extends, parse_diff, pending_for,
    submit_action, suggestion, thread_count, threads_at, Event, PendingComment, PrFile,
    OWN_PR_HINT,
  },
};
use crate::{
  errors,
  remote::{self, IdempotencyKeys, Mutation},
  ui::{load, nav, review::picked_bg, sheet, widgets, Load, Location},
};

/// The smallest comfortable touch target.
const TAP: f32 = 44.;

pub fn diff_request(repo: &str, number: u64) -> TreqCommandRequest {
  TreqCommandRequest::GithubRead {
    repo: repo.to_string(),
    query: GithubQuery::PrDiff { number },
  }
}

pub fn threads_request(repo: &str, number: u64) -> TreqCommandRequest {
  TreqCommandRequest::GithubRead {
    repo: repo.to_string(),
    query: GithubQuery::PrReviewThreads { number },
  }
}

/// Pushes the review of pull request `number` of the repository at `repo`.
pub fn open(repo: String, number: u64, cx: &mut App) {
  let location = Location {
    repo: Some(repo.clone()),
    workspace: None,
  };
  nav::push(
    format!("Review #{number}"),
    location,
    cx,
    move |window, cx| cx.new(|cx| PrReviewScreen::new(repo, number, window, cx)),
  );
}

/// What the comment box at the bottom is writing.
#[derive(Debug, Clone, PartialEq)]
enum Compose {
  /// A new pending comment on the picked lines.
  New,
  /// Pending comment `i`.
  Edit(usize),
  /// A reply to a thread.
  Reply(String),
}

pub struct PrReviewScreen {
  repo: String,
  number: u64,
  files: Load<Vec<PrFile>>,
  threads: Load<Vec<GhReviewThread>>,
  /// The file whose diff shows; `None` lists the files.
  file: Option<usize>,
  picked: Option<Picked>,
  compose: Option<Compose>,
  text: Entity<TextareaState>,
  pending: Vec<PendingComment>,
  /// The review's summary, kept while the submit sheet is closed.
  summary: String,
  /// Threads shown against their default (collapsed when resolved or
  /// outdated, open otherwise).
  toggled: HashSet<String>,
  /// The comment whose reaction picker is open.
  picker: Option<String>,
  busy: Option<&'static str>,
  message: Option<(bool, String)>,
  keys: IdempotencyKeys,
  scroll: ScrollHandle,
  /// The signed-in GitHub login and the PR's author, once read.
  login: Option<String>,
  author: Option<String>,
}

impl PrReviewScreen {
  fn new(repo: String, number: u64, window: &mut Window, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let files = remote::last::<String>(&diff_request(&repo, number))
      .map_or(Load::Idle, |d| Load::Ready(parse_diff(&d)));
    let threads = remote::last(&threads_request(&repo, number)).map_or(Load::Idle, Load::Ready);
    let text = cx.new(|cx| {
      TextareaState::new(window, cx)
        .rows(3)
        .placeholder("Comment")
    });
    let mut this = Self {
      repo,
      number,
      files,
      threads,
      file: None,
      picked: None,
      compose: None,
      text,
      pending: Vec::new(),
      summary: String::new(),
      toggled: HashSet::new(),
      picker: None,
      busy: None,
      message: None,
      keys: IdempotencyKeys::default(),
      scroll: ScrollHandle::new(),
      login: None,
      author: None,
    };
    this.fetch(true, cx);
    let context = TreqCommandRequest::GithubRead {
      repo: this.repo.clone(),
      query: GithubQuery::Context,
    };
    load::run(
      cx,
      remote::read_cached::<GithubContext>(context, true),
      |this, result, _| this.login = result.ok().map(|c| c.login),
    );
    load::run(
      cx,
      remote::read_cached::<treq_lib::github::GhPullRequest>(
        pr_detail::request(&this.repo, number),
        true,
      ),
      |this, result, _| this.author = result.ok().map(|pr| pr.author.login),
    );
    this
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
  }

  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    if self.files.ready().is_none() {
      self.files = Load::Loading;
    }
    if self.threads.ready().is_none() {
      self.threads = Load::Loading;
    }
    load::run(
      cx,
      remote::read_cached::<String>(diff_request(&self.repo, self.number), join),
      |this, result, _| match result {
        Ok(diff) => this.files = Load::Ready(parse_diff(&diff)),
        Err(error) if this.files.ready().is_some() => {
          this.message = Some((
            false,
            format!("Couldn't refresh: {}", errors::describe(&error).message),
          ))
        }
        Err(error) => this.files = Load::Failed(error),
      },
    );
    self.fetch_threads(join, cx);
    cx.notify();
  }

  fn fetch_threads(&mut self, join: bool, cx: &mut Context<Self>) {
    load::run(
      cx,
      remote::read_cached::<Vec<GhReviewThread>>(threads_request(&self.repo, self.number), join),
      |this, result, _| {
        if result.is_ok() || this.threads.ready().is_none() {
          this.threads = Load::from_result(result);
        }
      },
    );
  }

  fn threads(&self) -> &[GhReviewThread] {
    self.threads.ready().map_or(&[], Vec::as_slice)
  }

  fn current(&self) -> Option<&PrFile> {
    self.files.ready()?.get(self.file?)
  }

  fn show_file(&mut self, file: Option<usize>, cx: &mut Context<Self>) {
    self.file = file;
    self.picked = None;
    if self.compose == Some(Compose::New) {
      self.compose = None;
    }
    self.scroll.set_offset(point(px(0.), px(0.)));
    cx.notify();
  }

  fn set_text(&self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
    self.text.update(cx, |state, cx| {
      state.set_value(text.to_string(), window, cx)
    });
  }

  /// A tap on diff line `at`: picks it, extends a one-line pick in the
  /// same hunk and side to a range, or drops the pick.
  fn tap_line(&mut self, at: LineRef, window: &mut Window, cx: &mut Context<Self>) {
    let Some(hunks) = self.current().map(|f| f.hunks.clone()) else {
      return;
    };
    let extend = self
      .picked
      .as_ref()
      .is_some_and(|p| p.lines.len() == 1 && (p.anchor == at || extends(&hunks, p.anchor, at)));
    let picked = if extend {
      diff::tap(self.picked.as_ref(), &hunks, at)
    } else {
      diff::tap(None, &hunks, at)
    };
    if self.picked.is_none() || !matches!(self.compose, Some(Compose::New)) {
      self.set_text("", window, cx);
    }
    self.compose = picked.as_ref().map(|_| Compose::New);
    self.picked = picked;
    cx.notify();
  }

  fn picked_comment(&self) -> Option<PendingComment> {
    let file = self.current()?;
    pending_for(&file.path, &file.hunks, &self.picked.as_ref()?.lines)
  }

  fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.compose = None;
    self.picked = None;
    self.set_text("", window, cx);
    cx.notify();
  }

  fn insert_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let Some(c) = self.picked_comment() else {
      return;
    };
    let current = self.text.read(cx).value().to_string();
    let block = suggestion(&c.lines);
    let text = if current.trim().is_empty() {
      block
    } else {
      format!("{}\n\n{block}", current.trim_end())
    };
    self.set_text(&text, window, cx);
  }

  fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let body = self.text.read(cx).value().trim().to_string();
    if body.is_empty() {
      self.message = Some((false, "Write a comment first.".into()));
      return cx.notify();
    }
    match self.compose.clone() {
      Some(Compose::New) => {
        let Some(mut c) = self.picked_comment() else {
          self.message = Some((
            false,
            "Pick lines on one side: removed lines, or added and unchanged ones.".into(),
          ));
          return cx.notify();
        };
        c.body = body;
        self.pending.push(c);
      }
      Some(Compose::Edit(i)) => {
        if let Some(c) = self.pending.get_mut(i) {
          c.body = body;
        }
      }
      Some(Compose::Reply(thread_id)) => return self.reply(thread_id, body, window, cx),
      None => return,
    }
    self.message = None;
    self.cancel(window, cx);
  }

  fn edit(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
    let Some(c) = self.pending.get(i) else { return };
    let body = c.body.clone();
    self.picked = None;
    self.compose = Some(Compose::Edit(i));
    self.set_text(&body, window, cx);
    cx.notify();
  }

  fn remove(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
    if i < self.pending.len() {
      self.pending.remove(i);
    }
    if matches!(self.compose, Some(Compose::Edit(_))) {
      self.cancel(window, cx);
    }
    cx.notify();
  }

  fn start_reply(&mut self, thread_id: String, window: &mut Window, cx: &mut Context<Self>) {
    self.picked = None;
    self.compose = Some(Compose::Reply(thread_id));
    self.set_text("", window, cx);
    cx.notify();
  }

  /// Runs `action` once per idempotency key, then calls `applied`.
  #[allow(clippy::too_many_arguments)]
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
      action,
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
          applied(this, window, cx);
        }
        this.fetch_threads(false, cx);
      },
    );
    cx.notify();
  }

  fn reply(
    &mut self,
    thread_id: String,
    body: String,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    self.write(
      "pr-review-reply",
      vec![self.repo.clone(), thread_id.clone(), body.clone()],
      GithubAction::ReplyToReviewThread { thread_id, body },
      "Reply posted.",
      window,
      cx,
      |this, window, cx| this.cancel(window, cx),
    );
  }

  /// Submits the review with every pending comment; they are cleared only
  /// once GitHub took it.
  fn submit(&mut self, event: Event, body: String, window: &mut Window, cx: &mut Context<Self>) {
    self.summary = body.clone();
    if !can_submit(event, &body, self.pending.len()) {
      self.message = Some((
        false,
        "Requesting changes needs a summary or a comment.".into(),
      ));
      return cx.notify();
    }
    let action = submit_action(self.number, event, &body, &self.pending);
    let inputs = vec![
      self.repo.clone(),
      self.number.to_string(),
      serde_json::to_string(&action).unwrap_or_default(),
    ];
    self.write(
      "pr-review-submit",
      inputs,
      action,
      "Review submitted.",
      window,
      cx,
      |this, window, cx| {
        this.pending.clear();
        this.summary.clear();
        this.cancel(window, cx);
      },
    );
  }

  fn open_submit(&mut self, cx: &mut Context<Self>) {
    let screen = cx.weak_entity();
    let (summary, pending) = (self.summary.clone(), self.pending.len());
    let (login, author) = (self.login.clone(), self.author.clone());
    sheet::open(Some("Finish your review".into()), cx, move |window, cx| {
      cx.new(|cx| {
        let body = cx.new(|cx| {
          let mut state = TextareaState::new(window, cx)
            .rows(4)
            .placeholder("Leave a summary (optional)");
          state.set_value(summary, window, cx);
          state
        });
        // Typing a summary can enable "Request changes".
        cx.observe(&body, |_, _, cx| cx.notify()).detach();
        SubmitSheet {
          screen,
          body,
          event: Event::Comment,
          pending,
          login,
          author,
        }
      })
    });
  }

  /// Toggles the user's reaction on a thread comment at once, then on
  /// GitHub; undone if GitHub refuses or the result is unknown.
  fn react(&mut self, comment_id: String, content: &'static str, cx: &mut Context<Self>) {
    self.picker = None;
    let Some(on) = self.toggle(&comment_id, content) else {
      return;
    };
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
      this.toggle(&comment_id, content);
      let text = mutation_message(&outcome, "").1;
      this.message = Some((false, errors::describe(&text).message));
    });
    cx.notify();
  }

  fn toggle(&mut self, comment_id: &str, content: &str) -> Option<bool> {
    let Load::Ready(threads) = &mut self.threads else {
      return None;
    };
    let comment = threads
      .iter_mut()
      .flat_map(|t| t.comments.iter_mut())
      .find(|c| c.id == comment_id)?;
    Some(toggle_reaction(&mut comment.reactions, content))
  }

  fn reactions(&self, comment: &GhReviewComment, cx: &mut Context<Self>) -> Div {
    let picker_open = self.picker.as_deref() == Some(comment.id.as_str());
    let id = comment.id.clone();
    let chips = h_flex()
      .gap_2()
      .flex_wrap()
      .children(comment.reactions.iter().map(|group| {
        let (id, content) = (comment.id.clone(), group.content.clone());
        let known = REACTIONS
          .iter()
          .find(|(c, _)| *c == content)
          .map(|(c, _)| *c);
        Button::new(SharedString::from(format!("re-{}-{content}", comment.id)))
          .h(px(TAP))
          .small()
          .label(format!("{} {}", emoji(&content), group.count))
          .map(|b| {
            if group.viewer_has_reacted {
              b.primary()
            } else {
              b.outline()
            }
          })
          .when_some(known, |b, content| {
            b.on_click(cx.listener(move |this, _, _, cx| this.react(id.clone(), content, cx)))
          })
      }))
      .child(
        Button::new(SharedString::from(format!("re-add-{}", comment.id)))
          .h(px(TAP))
          .w(px(TAP))
          .ghost()
          .icon(IconName::Plus)
          .on_click(cx.listener(move |this, _, _, cx| {
            this.picker = if picker_open { None } else { Some(id.clone()) };
            cx.notify();
          })),
      );
    let picker = picker_open.then(|| {
      h_flex()
        .gap_1()
        .flex_wrap()
        .children(REACTIONS.iter().map(|(content, e)| {
          let id = comment.id.clone();
          let content: &'static str = content;
          Button::new(SharedString::from(format!("pick-{}-{content}", comment.id)))
            .h(px(TAP))
            .w(px(TAP))
            .ghost()
            .label(*e)
            .on_click(cx.listener(move |this, _, _, cx| this.react(id.clone(), content, cx)))
        }))
    });
    v_flex().gap_1().child(chips).children(picker)
  }

  fn thread_card(&self, thread: &GhReviewThread, cx: &mut Context<Self>) -> AnyElement {
    let theme = cx.theme().clone();
    let open = collapsed(thread) == self.toggled.contains(&thread.id);
    let id = thread.id.clone();
    let first = thread.comments.first();
    let title = match first {
      Some(c) => format!(
        "{} · {}",
        c.author.login,
        widgets_count(thread.comments.len(), "comment")
      ),
      None => "Thread".into(),
    };
    let head = h_flex()
      .id(SharedString::from(format!("thread-{}", thread.id)))
      .min_h(px(TAP))
      .px_3()
      .gap_2()
      .items_center()
      .child(div().text_color(theme.muted_foreground).child(if open {
        IconName::ChevronDown
      } else {
        IconName::ChevronRight
      }))
      .child(
        div()
          .flex_1()
          .min_w_0()
          .font_semibold()
          .text_sm()
          .child(title),
      )
      .when(thread.is_outdated, |h| {
        h.child(Tag::warning().small().child("Outdated"))
      })
      .when(thread.is_resolved, |h| {
        h.child(Tag::secondary().small().child("Resolved"))
      })
      .on_click(cx.listener(move |this, _, _, cx| {
        if !this.toggled.remove(&id) {
          this.toggled.insert(id.clone());
        }
        cx.notify();
      }));
    let card = v_flex()
      .mx_2()
      .my_1()
      .rounded(theme.radius)
      .border_1()
      .border_color(theme.border)
      .bg(theme.secondary)
      .child(head);
    if !open {
      return card.into_any_element();
    }
    let comments: Vec<Div> = thread
      .comments
      .iter()
      .map(|c| {
        v_flex()
          .px_3()
          .py_2()
          .gap_1()
          .border_t_1()
          .border_color(theme.border)
          .child(
            h_flex()
              .gap_2()
              .child(
                div()
                  .font_semibold()
                  .text_sm()
                  .child(c.author.login.clone()),
              )
              .child(
                div()
                  .text_xs()
                  .text_color(theme.muted_foreground)
                  .child(c.created_at.get(..10).unwrap_or("").to_string()),
              ),
          )
          .child(div().text_sm().child(widgets::markdown(
            SharedString::from(format!("thread-comment-{}", c.id)),
            &c.body,
            cx,
          )))
          .child(self.reactions(c, cx))
      })
      .collect();
    let thread_id = thread.id.clone();
    card
      .children(comments)
      .child(
        h_flex().px_3().pb_2().child(
          Button::new(SharedString::from(format!("reply-{}", thread.id)))
            .h(px(TAP))
            .outline()
            .small()
            .label("Reply")
            .on_click(cx.listener(move |this, _, window, cx| {
              this.start_reply(thread_id.clone(), window, cx)
            })),
        ),
      )
      .into_any_element()
  }

  fn pending_card(&self, i: usize, c: &PendingComment, cx: &mut Context<Self>) -> Div {
    let theme = cx.theme().clone();
    v_flex()
      .mx_2()
      .my_1()
      .p_3()
      .gap_1()
      .rounded(theme.radius)
      .border_1()
      .border_color(theme.primary)
      .child(
        h_flex()
          .gap_2()
          .child(Tag::primary().small().child("Pending"))
          .child(widgets::muted(
            format!("{} {}", c.path, line_label(c.start_line, c.end_line)),
            cx,
          )),
      )
      .child(div().text_sm().child(c.body.clone()))
      .child(
        h_flex()
          .gap_2()
          .child(
            Button::new(("pending-edit", i))
              .h(px(TAP))
              .outline()
              .small()
              .label("Edit")
              .on_click(cx.listener(move |this, _, window, cx| this.edit(i, window, cx))),
          )
          .child(
            Button::new(("pending-remove", i))
              .h(px(TAP))
              .ghost()
              .small()
              .label("Remove")
              .on_click(cx.listener(move |this, _, window, cx| this.remove(i, window, cx))),
          ),
      )
  }

  fn file_list(&self, files: &[PrFile], cx: &mut Context<Self>) -> Div {
    let theme = cx.theme().clone();
    let threads = self.threads();
    let rows: Vec<AnyElement> = files
      .iter()
      .enumerate()
      .map(|(i, f)| {
        let count = thread_count(threads, &f.path);
        let pending = self.pending.iter().filter(|c| c.path == f.path).count();
        let mut detail = format!("+{} −{}", f.additions, f.deletions);
        if count > 0 {
          detail.push_str(&format!(" · {}", widgets_count(count, "thread")));
        }
        if pending > 0 {
          detail.push_str(&format!(" · {pending} pending"));
        }
        widgets::row(("pr-file", i), f.path.clone(), Some(detail.into()), cx)
          .on_click(cx.listener(move |this, _, _, cx| this.show_file(Some(i), cx)))
          .into_any_element()
      })
      .collect();
    let pending: Vec<Div> = self
      .pending
      .iter()
      .enumerate()
      .map(|(i, c)| self.pending_card(i, c, cx))
      .collect();
    v_flex()
      .gap_4()
      .child(
        widgets::section(widgets_count(files.len(), "file"), cx).child(
          widgets::card(cx)
            .when(rows.is_empty(), |c| {
              c.child(widgets::centered_message(
                "No changes in this pull request.",
                cx,
              ))
            })
            .children(rows),
        ),
      )
      .when(!pending.is_empty(), |page| {
        page.child(
          widgets::section(format!("Pending review · {}", pending.len()), cx)
            .child(v_flex().children(pending)),
        )
      })
      .when(matches!(self.threads, Load::Failed(_)), |page| {
        page.child(
          div()
            .text_sm()
            .text_color(theme.danger)
            .child("Couldn't load review threads."),
        )
      })
  }

  fn file_diff(&self, i: usize, file: &PrFile, cx: &mut Context<Self>) -> Div {
    let theme = cx.theme().clone();
    let threads = self.threads().to_vec();
    let picked: Vec<LineRef> = self
      .picked
      .as_ref()
      .map(|p| p.lines.clone())
      .unwrap_or_default();
    let detached: Vec<AnyElement> = detached_threads(&threads, &file.path, &file.hunks)
      .into_iter()
      .map(|t| self.thread_card(t, cx))
      .collect();
    let mut rows: Vec<AnyElement> = Vec::new();
    for (h, hunk) in file.hunks.iter().enumerate() {
      rows.push(
        widgets::mono(hunk.header.clone(), cx)
          .px_2()
          .py_1()
          .text_color(theme.muted_foreground)
          .bg(theme.secondary)
          .into_any_element(),
      );
      for (l, line) in hunk.lines.iter().enumerate() {
        let on = picked.contains(&(h, l));
        let (number, side) = line_at(hunk, l);
        rows.push(
          h_flex()
            .id(SharedString::from(format!("line-{h}-{l}")))
            .min_h(px(28.))
            .child(
              widgets::mono(number.to_string(), cx)
                .w(px(40.))
                .flex_none()
                .pr_1()
                .text_right()
                .text_color(if on {
                  theme.primary
                } else {
                  theme.muted_foreground
                }),
            )
            .child(diff::diff_line(line, cx).flex_1().when(on, |row| {
              row
                .bg(gpui_kit::transparent_black())
                .text_color(theme.foreground)
            }))
            .when(on, |row| row.bg(picked_bg(cx)))
            .on_click(cx.listener(move |this, _, window, cx| this.tap_line((h, l), window, cx)))
            .into_any_element(),
        );
        for t in threads_at(&threads, &file.path, number, side) {
          rows.push(self.thread_card(t, cx));
        }
        for (p, c) in self.pending.iter().enumerate() {
          if c.path == file.path && c.end_line == number && c.side == side {
            rows.push(self.pending_card(p, c, cx).into_any_element());
          }
        }
      }
    }
    let total = self.files.ready().map_or(0, Vec::len);
    v_flex()
      .gap_3()
      .child(
        v_flex()
          .gap_1()
          .child(
            div()
              .font_semibold()
              .font_family(theme.mono_font_family.clone())
              .text_sm()
              .child(file.path.clone()),
          )
          .child(widgets::muted(
            format!(
              "File {} of {total} · +{} −{}",
              i + 1,
              file.additions,
              file.deletions
            ),
            cx,
          ))
          .child(widgets::muted(
            "Tap a line to comment, then another in the same hunk for a range.",
            cx,
          )),
      )
      .when(!detached.is_empty(), |page| {
        page.child(
          widgets::section("Outdated and other threads", cx).child(v_flex().children(detached)),
        )
      })
      .child(
        widgets::card(cx).child(
          div()
            .id("pr-diff")
            .overflow_x_scroll()
            .child(v_flex().py_1().min_w_full().children(rows))
            .when(file.hunks.is_empty(), |d| {
              d.child(widgets::centered_message(
                "No text changes (binary or renamed).",
                cx,
              ))
            }),
        ),
      )
  }

  /// The bottom bar: the comment box while writing, else navigation and
  /// the submit button.
  fn footer(&self, cx: &mut Context<Self>) -> Div {
    let theme = cx.theme().clone();
    let bar = v_flex()
      .p_4()
      .gap_2()
      .border_t_1()
      .border_color(theme.border)
      .bg(theme.background);
    if let Some(compose) = &self.compose {
      let (title, action) = match compose {
        Compose::New => (
          self
            .picked_comment()
            .map(|c| format!("Comment on {}", line_label(c.start_line, c.end_line)))
            .unwrap_or_else(|| "Lines on both sides can't share a comment".into()),
          "Add to review",
        ),
        Compose::Edit(_) => ("Edit pending comment".into(), "Save"),
        Compose::Reply(_) => ("Reply to thread".into(), "Reply"),
      };
      let replying = matches!(compose, Compose::Reply(_));
      return bar
        .child(widgets::muted(title, cx))
        .child(Textarea::new(&self.text))
        .child(
          h_flex()
            .gap_2()
            .child(
              Button::new("compose-cancel")
                .h(px(TAP))
                .ghost()
                .label("Cancel")
                .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
            )
            .when(*compose == Compose::New, |row| {
              row.child(
                Button::new("compose-suggest")
                  .h(px(TAP))
                  .outline()
                  .label("Suggest")
                  .on_click(cx.listener(|this, _, window, cx| this.insert_suggestion(window, cx))),
              )
            })
            .child(
              Button::new("compose-save")
                .h(px(TAP))
                .flex_1()
                .primary()
                .label(action)
                .loading(replying && self.busy == Some("pr-review-reply"))
                .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            ),
        );
    }
    let total = self.files.ready().map_or(0, Vec::len);
    let nav_row = self.file.map(|i| {
      h_flex()
        .gap_2()
        .child(
          Button::new("file-prev")
            .h(px(TAP))
            .flex_1()
            .outline()
            .icon(IconName::ChevronLeft)
            .label("Previous")
            .disabled(i == 0)
            .on_click(cx.listener(move |this, _, _, cx| this.show_file(Some(i - 1), cx))),
        )
        .child(
          Button::new("file-list")
            .h(px(TAP))
            .outline()
            .label("Files")
            .on_click(cx.listener(|this, _, _, cx| this.show_file(None, cx))),
        )
        .child(
          Button::new("file-next")
            .h(px(TAP))
            .flex_1()
            .outline()
            .icon(IconName::ChevronRight)
            .label("Next")
            .disabled(i + 1 >= total)
            .on_click(cx.listener(move |this, _, _, cx| this.show_file(Some(i + 1), cx))),
        )
    });
    let label = match self.pending.len() {
      0 => "Submit review".to_string(),
      n => format!("Submit review ({n})"),
    };
    bar.children(nav_row).child(
      Button::new("review-submit")
        .primary()
        .large()
        .w_full()
        .h(px(widgets::CTA_H))
        .label(label)
        .loading(self.busy == Some("pr-review-submit"))
        .on_click(cx.listener(|this, _, _, cx| this.open_submit(cx))),
    )
  }
}

fn widgets_count(n: usize, word: &str) -> String {
  format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

impl Render for PrReviewScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let files = match &self.files {
      Load::Ready(files) => files.clone(),
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
                    .h(px(TAP))
                    .outline()
                    .label("Retry")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                ),
              ),
          )
          .into_any_element()
      }
      _ => {
        return widgets::page()
          .child(widgets::card(cx).child(widgets::centered_message("Loading changes…", cx)))
          .into_any_element()
      }
    };
    let body = match self.file.and_then(|i| files.get(i).map(|f| (i, f))) {
      Some((i, file)) => self.file_diff(i, file, cx),
      None => self.file_list(&files, cx),
    };
    v_flex()
      .size_full()
      .child(
        div()
          .id("pr-review")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .track_scroll(&self.scroll)
          .child(widgets::page().child(body).when_some(
            self.message.clone(),
            |page, (ok, text)| {
              page.child(if ok {
                widgets::muted(text, cx)
              } else {
                widgets::error_text(text, cx)
              })
            },
          )),
      )
      .child(self.footer(cx))
      .into_any_element()
  }
}

/// The submit sheet: a summary and the verdict.
struct SubmitSheet {
  screen: WeakEntity<PrReviewScreen>,
  body: Entity<TextareaState>,
  event: Event,
  pending: usize,
  login: Option<String>,
  author: Option<String>,
}

impl Render for SubmitSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let choices: Vec<AnyElement> = Event::ALL
      .iter()
      .map(|&event| {
        let ok = allowed(event, self.login.as_deref(), self.author.as_deref());
        widgets::choice(
          SharedString::from(format!("event-{}", event.value())),
          event.label(),
          Some(if ok { event.hint() } else { OWN_PR_HINT }.into()),
          self.event == event,
          cx,
        )
        .when(!ok, |row| row.opacity(0.5))
        .when(ok, |row| {
          row.on_click(cx.listener(move |this, _, _, cx| {
            this.event = event;
            cx.notify();
          }))
        })
        .into_any_element()
      })
      .collect();
    let ready = can_submit(self.event, &self.body.read(cx).value(), self.pending);
    v_flex()
      .px_4()
      .pt_1()
      .pb_4()
      .gap_3()
      .child(widgets::muted(
        format!(
          "{} will be posted with the review.",
          widgets_count(self.pending, "pending comment")
        ),
        cx,
      ))
      .child(Textarea::new(&self.body))
      .child(widgets::card(cx).children(choices))
      .when(!ready, |s| {
        s.child(widgets::muted(
          "Requesting changes needs a summary or a comment.",
          cx,
        ))
      })
      .child(
        Button::new("submit-review")
          .primary()
          .large()
          .w_full()
          .h(px(widgets::CTA_H))
          .label("Submit review")
          .disabled(!ready)
          .on_click(cx.listener(|this, _, window, cx| {
            let (event, body) = (this.event, this.body.read(cx).value().trim().to_string());
            this
              .screen
              .update(cx, |screen, cx| screen.submit(event, body, window, cx))
              .ok();
            sheet::close(cx);
          })),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[core::prelude::v1::test]
  fn reads_name_the_diff_and_threads() {
    assert_eq!(
      diff_request("/r", 3),
      TreqCommandRequest::GithubRead {
        repo: "/r".into(),
        query: GithubQuery::PrDiff { number: 3 }
      }
    );
    assert_eq!(
      threads_request("/r", 3),
      TreqCommandRequest::GithubRead {
        repo: "/r".into(),
        query: GithubQuery::PrReviewThreads { number: 3 }
      }
    );
    assert_eq!(widgets_count(1, "file"), "1 file");
    assert_eq!(widgets_count(2, "file"), "2 files");
  }
}
