//! A pending review: comments on lines and files, batched with a summary
//! into one message for an agent's terminal, as on desktop. The host keeps
//! it per page (`ReviewScope`), so it survives leaving the page, relaunches
//! and dropped connections. Shared by the Changes tab and the file viewer.

use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState, Textarea, TextareaState},
    tag::Tag,
    v_flex, ActiveTheme, Icon, IconName, Sizable,
  },
  *,
};
use treq_lib::{
  core::{
    remote::{ReviewScope, TreqCommandRequest},
    remote_pty::RemoteAgentId,
    review::{
      commented_text, comments_under, count_label, is_outdated, line_at, parse_suggestion,
      review_markdown, suggestion_block, LineComment, LineRef, Pending, Side, Stored,
    },
  },
  jj::JjDiffHunk,
};

use super::{
  load,
  screens::{
    terminal::{agent_name, Program, Sessions},
    workspace::Target,
  },
  sheet, widgets, Load,
};
use crate::{errors, remote};

/// The review a page is writing, kept on the host.
pub struct Review {
  target: Target,
  scope: ReviewScope,
  stored: Load<Stored>,
  /// A save is on its way; `dirty` means another edit came after it.
  saving: bool,
  dirty: bool,
  error: Option<String>,
}

impl Review {
  pub fn new(target: Target, scope: ReviewScope, cx: &mut Context<Self>) -> Self {
    let mut this = Self {
      target,
      scope,
      stored: Load::Loading,
      saving: false,
      dirty: false,
      error: None,
    };
    this.load(cx);
    this
  }

  fn load(&mut self, cx: &mut Context<Self>) {
    let request = TreqCommandRequest::ReviewLoad {
      repo: self.target.repo.clone(),
      workspace: self.target.id.clone(),
      scope: self.scope,
    };
    load::run(cx, remote::read::<Stored>(request), |this, result, _| {
      // Edits made while it loaded stay; an older host just has none.
      if this.stored.ready().is_none() {
        this.stored = Load::Ready(result.unwrap_or_default());
      }
    });
  }

  pub fn comments(&self) -> &[LineComment] {
    self.stored.ready().map_or(&[], |s| &s.comments)
  }

  pub fn summary(&self) -> &str {
    self.stored.ready().map_or("", |s| &s.summary)
  }

  pub fn is_empty(&self) -> bool {
    self.stored.ready().is_none_or(Stored::is_empty)
  }

  /// The comments on file `path`.
  pub fn count_for(&self, path: &str) -> usize {
    self
      .comments()
      .iter()
      .filter(|c| c.file_path == path)
      .count()
  }

  pub fn error(&self) -> Option<&str> {
    self.error.as_deref()
  }

  /// The message the agent gets.
  pub fn markdown(&self) -> String {
    review_markdown(self.comments(), self.summary(), &[], |_| None, &[])
  }

  fn edit(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Stored)) {
    if self.stored.ready().is_none() {
      self.stored = Load::Ready(Stored::default());
    }
    if let Load::Ready(stored) = &mut self.stored {
      change(stored);
    }
    self.save(cx);
    cx.notify();
  }

  pub fn add(&mut self, pending: &Pending, text: &str, cx: &mut Context<Self>) {
    let comment = pending.comment(
      text,
      uuid::Uuid::new_v4().to_string(),
      chrono::Utc::now().to_rfc3339(),
    );
    self.edit(cx, |s| s.comments.push(comment));
  }

  pub fn update(&mut self, id: &str, text: &str, cx: &mut Context<Self>) {
    self.edit(cx, |s| {
      if let Some(c) = s.comments.iter_mut().find(|c| c.id == id) {
        c.text = text.to_string();
      }
    });
  }

  pub fn delete(&mut self, id: &str, cx: &mut Context<Self>) {
    self.edit(cx, |s| s.comments.retain(|c| c.id != id));
  }

  pub fn set_summary(&mut self, summary: String, cx: &mut Context<Self>) {
    if summary != self.summary() {
      self.edit(cx, |s| s.summary = summary);
    }
  }

  /// Drops everything, once sent or discarded.
  pub fn clear(&mut self, cx: &mut Context<Self>) {
    self.edit(cx, |s| *s = Stored::default());
  }

  /// Writes the review to the host, one save at a time.
  fn save(&mut self, cx: &mut Context<Self>) {
    if self.saving {
      self.dirty = true;
      return;
    }
    let Some(review) = self.stored.ready().cloned() else {
      return;
    };
    self.saving = true;
    let request = TreqCommandRequest::ReviewSave {
      repo: self.target.repo.clone(),
      workspace: self.target.id.clone(),
      scope: self.scope,
      review,
    };
    load::run(cx, remote::mutate(request), |this, outcome, cx| {
      this.saving = false;
      let (ok, message) = super::screens::mutation_message(&outcome, "");
      this.error = (!ok).then(|| {
        format!(
          "Your review isn't saved on the host yet: {}",
          errors::describe(&message).message
        )
      });
      if std::mem::take(&mut this.dirty) {
        this.save(cx);
      }
    });
  }
}

/// `Line 41` or `Lines 41–42`.
pub fn lines_label(start: u32, end: u32) -> String {
  if start == end {
    format!("Line {start}")
  } else {
    format!("Lines {start}–{end}")
  }
}

/// Where a comment is: its lines, or "File" for a file comment.
pub fn place(comment: &LineComment) -> String {
  if comment.is_file() {
    "File".into()
  } else {
    lines_label(comment.start_line, comment.end_line)
  }
}

/// Comments on a file's own lines (the file viewer) carry this instead of
/// a diff hunk's id, as desktop's file browser does.
pub const FILE_LINES: &str = "browser";

/// The comment box for lines `start..=end` of file `path`, whose text is
/// `lines`, in a viewer with no diff.
pub fn lines_pending(path: &str, start: u32, end: u32, lines: Vec<String>) -> Pending {
  Pending {
    file_path: path.into(),
    hunk_id: FILE_LINES.into(),
    at: Some((0, 0)),
    start_line: start,
    end_line: end,
    line_content: lines,
    side: Side::New,
    quote: None,
  }
}

/// The commented lines without diff markers (file lines have none).
fn plain_lines(hunk_id: &str, line_content: &[String]) -> Vec<String> {
  if hunk_id == FILE_LINES {
    line_content.to_vec()
  } else {
    commented_text(line_content)
  }
}

/// The ```` ```suggestion ```` block "Suggest change" starts with.
fn suggestion_for(pending: &Pending) -> Option<String> {
  if pending.hunk_id == FILE_LINES {
    return (!pending.line_content.is_empty())
      .then(|| format!("```suggestion\n{}\n```\n", pending.line_content.join("\n")));
  }
  suggestion_block(pending)
}

/// Opens the comment box for `pending`, or for editing comment `id`.
pub fn compose(
  review: Entity<Review>,
  pending: Pending,
  editing: Option<LineComment>,
  cx: &mut App,
) {
  let title = if pending.at.is_none() {
    format!("Comment on {}", pending.file_path)
  } else {
    format!(
      "Comment on {}",
      lines_label(pending.start_line, pending.end_line).to_lowercase()
    )
  };
  sheet::open(Some(title.into()), cx, move |window, cx| {
    cx.new(|cx| Composer::new(review, pending, editing, window, cx))
  });
}

struct Composer {
  review: Entity<Review>,
  pending: Pending,
  editing: Option<String>,
  text: Entity<TextareaState>,
}

impl Composer {
  fn new(
    review: Entity<Review>,
    pending: Pending,
    editing: Option<LineComment>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let initial = editing.as_ref().map(|c| c.text.clone()).unwrap_or_default();
    let text = cx.new(|cx| {
      TextareaState::new(window, cx)
        .auto_grow(3, 10)
        .placeholder("Leave a comment")
        .default_value(initial)
    });
    text.update(cx, |state, cx| state.focus(window, cx));
    Self {
      review,
      pending,
      editing: editing.map(|c| c.id),
      text,
    }
  }

  fn suggest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let Some(block) = suggestion_for(&self.pending) else {
      return;
    };
    self.text.update(cx, |state, cx| {
      let value = state.value().to_string();
      let joined = if value.trim().is_empty() {
        block
      } else {
        format!("{}\n{block}", value.trim_end())
      };
      state.set_value(joined, window, cx);
    });
  }

  fn submit(&mut self, cx: &mut Context<Self>) {
    let text = self.text.read(cx).value().trim().to_string();
    if text.is_empty() {
      return;
    }
    let (pending, editing) = (self.pending.clone(), self.editing.clone());
    self.review.update(cx, |review, cx| match editing {
      Some(id) => review.update(&id, &text, cx),
      None => review.add(&pending, &text, cx),
    });
    sheet::close(cx);
  }
}

/// Lines of the picked range the composer previews, so it fits above the
/// keyboard.
const PREVIEW_LINES: usize = 6;

fn more_lines_label(hidden: usize) -> String {
  if hidden == 1 {
    "1 more line".into()
  } else {
    format!("{hidden} more lines")
  }
}

impl Render for Composer {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let can_suggest = suggestion_for(&self.pending).is_some();
    let empty = self.text.read(cx).value().trim().is_empty();
    let hidden = self
      .pending
      .line_content
      .len()
      .saturating_sub(PREVIEW_LINES);
    v_flex()
      .px_4()
      .gap_3()
      .when(!self.pending.line_content.is_empty(), |c| {
        c.child(
          widgets::card(cx)
            .py_1()
            .children(
              self
                .pending
                .line_content
                .iter()
                .take(PREVIEW_LINES)
                .map(|line| {
                  if self.pending.hunk_id == FILE_LINES {
                    widgets::mono(line.clone(), cx).px_2()
                  } else {
                    super::screens::diff::diff_line(line, cx)
                  }
                }),
            )
            .when(hidden > 0, |c| {
              c.child(widgets::muted(more_lines_label(hidden), cx).px_2())
            }),
        )
      })
      .child(Textarea::new(&self.text))
      .child(
        h_flex()
          .gap_2()
          .when(can_suggest, |row| {
            row.child(
              Button::new("suggest")
                .outline()
                .icon(IconName::Plus)
                .label("Suggest change")
                .on_click(cx.listener(|this, _, window, cx| this.suggest(window, cx))),
            )
          })
          .child(div().flex_1())
          .child(
            Button::new("add-comment")
              .primary()
              .label(if self.editing.is_some() {
                "Save"
              } else {
                "Add comment"
              })
              .disabled(empty)
              .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
          ),
      )
  }
}

/// A comment as it shows under its lines: a suggestion as a small diff.
pub fn comment_card(
  review: &Entity<Review>,
  comment: &LineComment,
  outdated: bool,
  cx: &App,
) -> AnyElement {
  let theme = cx.theme();
  let body = match parse_suggestion(&comment.text) {
    Some(s) => v_flex()
      .gap_1()
      .when(!s.before.is_empty(), |c| {
        c.child(div().child(s.before.clone()))
      })
      .child(
        widgets::card(cx)
          .py_1()
          .children(
            plain_lines(&comment.hunk_id, &comment.line_content)
              .into_iter()
              .map(|l| super::screens::diff::diff_line(&format!("-{l}"), cx)),
          )
          .children(
            s.suggested
              .lines()
              .map(|l| super::screens::diff::diff_line(&format!("+{l}"), cx)),
          ),
      )
      .when(!s.after.is_empty(), |c| {
        c.child(div().child(s.after.clone()))
      }),
    None => v_flex().child(div().child(comment.text.clone())),
  };
  let (edit, delete) = (review.clone(), review.clone());
  let (for_edit, id) = (comment.clone(), comment.id.clone());
  v_flex()
    .mx_2()
    .my_1()
    .p_3()
    .gap_2()
    .rounded(theme.radius)
    .border_1()
    .border_color(theme.border)
    .bg(theme.secondary)
    .text_sm()
    .child(
      h_flex()
        .gap_2()
        .child(widgets::muted(place(comment), cx))
        .when(outdated, |row| {
          row.child(Tag::warning().small().child("Outdated"))
        }),
    )
    .child(body)
    .child(
      h_flex()
        .gap_2()
        .child(
          Button::new(SharedString::from(format!("edit-{}", comment.id)))
            .ghost()
            .small()
            .label("Edit")
            .on_click(move |_, _, cx| {
              let pending = pending_of(&for_edit);
              compose(edit.clone(), pending, Some(for_edit.clone()), cx);
            }),
        )
        .child(
          Button::new(SharedString::from(format!("delete-{}", comment.id)))
            .ghost()
            .small()
            .label("Delete")
            .on_click(move |_, _, cx| {
              delete.update(cx, |review, cx| review.delete(&id, cx));
            }),
        ),
    )
    .into_any_element()
}

/// The box a comment was written in, to edit it again.
fn pending_of(comment: &LineComment) -> Pending {
  Pending {
    file_path: comment.file_path.clone(),
    hunk_id: comment.hunk_id.clone(),
    at: (!comment.is_file()).then_some((0, 0)),
    start_line: comment.start_line,
    end_line: comment.end_line,
    line_content: comment.line_content.clone(),
    side: comment
      .line_side
      .unwrap_or(treq_lib::core::review::Side::New),
    quote: None,
  }
}

/// Something a page does on a tap, given the window and app.
pub type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// What a tap on diff line `(hunk, line)` does.
pub type OnTap = Rc<dyn Fn(LineRef, &mut Window, &mut App)>;

/// The selected rows' background, as the file viewer uses.
pub fn picked_bg(cx: &App) -> Hsla {
  cx.theme().primary.opacity(0.15)
}

/// File `path`'s diff, each line tappable, with its comments under their
/// last line. `picked` rows show selected.
pub fn diff_hunks(
  path: &str,
  hunks: &[JjDiffHunk],
  picked: &[LineRef],
  review: &Entity<Review>,
  on_tap: OnTap,
  cx: &App,
) -> Div {
  let comments = review.read(cx).comments().to_vec();
  let base = path.to_string();
  v_flex()
    .py_1()
    .children(hunks.iter().enumerate().map(|(h, hunk)| {
      v_flex()
        .child(
          widgets::mono(hunk.header.clone(), cx)
            .px_2()
            .text_color(cx.theme().muted_foreground),
        )
        .children(hunk.lines.iter().enumerate().map(|(l, line)| {
          let on = picked.contains(&(h, l));
          let tap = on_tap.clone();
          let under: Vec<AnyElement> = comments_under(&comments, path, hunk, l, hunks)
            .into_iter()
            .map(|c| comment_card(review, c, false, cx))
            .collect();
          let number = line_at(hunk, l).0;
          v_flex()
            .child(
              h_flex()
                .id(SharedString::from(format!("{base}#{h}:{l}")))
                .child(
                  widgets::mono(number.to_string(), cx)
                    .w(px(36.))
                    .flex_none()
                    .pr_1()
                    .text_right()
                    .text_color(cx.theme().muted_foreground)
                    .when(on, |n| n.text_color(cx.theme().primary)),
                )
                .child(
                  super::screens::diff::diff_line(line, cx)
                    .flex_1()
                    .when(on, |row| {
                      row
                        .bg(gpui_kit::transparent_black())
                        .text_color(cx.theme().foreground)
                    }),
                )
                .when(on, |row| row.bg(picked_bg(cx)))
                .on_click(move |_, window, cx| tap((h, l), window, cx)),
            )
            .children(under)
        }))
    }))
}

/// Cards for file `path` that don't sit under a line: file comments, and
/// line comments whose lines are gone from `hunks` (when loaded).
pub fn file_cards(
  review: &Entity<Review>,
  path: &str,
  hunks: Option<&[JjDiffHunk]>,
  cx: &App,
) -> Vec<AnyElement> {
  let shown: Vec<(LineComment, bool)> = review
    .read(cx)
    .comments()
    .iter()
    .filter(|c| c.file_path == path)
    .filter_map(|c| {
      if c.is_file() {
        Some((c.clone(), false))
      } else if hunks.is_some() && c.hunk_id != FILE_LINES && is_outdated(c, hunks) {
        Some((c.clone(), true))
      } else {
        None
      }
    })
    .collect();
  shown
    .iter()
    .map(|(c, outdated)| comment_card(review, c, *outdated, cx))
    .collect()
}

/// The icon button that adds a comment on file `path`.
pub fn file_comment_button(review: &Entity<Review>, path: &str) -> Button {
  let (review, path) = (review.clone(), path.to_string());
  Button::new(SharedString::from(format!("comment-file-{path}")))
    .ghost()
    .icon(Icon::from(LucideIcon::MessageSquarePlus))
    .tooltip("Comment on file")
    .on_click(move |_, _, cx| compose(review.clone(), Pending::file(&path), None, cx))
}

/// The bar while lines are picked: which, and the button that comments on
/// them. `pending` is `None` when they can't take one comment.
pub fn selection_bar(
  review: &Entity<Review>,
  pending: Option<Pending>,
  many: bool,
  clear: Handler,
  cx: &App,
) -> AnyElement {
  let theme = cx.theme();
  let label = match &pending {
    Some(p) => lines_label(p.start_line, p.end_line),
    None if many => "Pick only added or only removed lines".into(),
    None => "Pick a line".into(),
  };
  let (review, on_clear, after) = (review.clone(), clear.clone(), clear);
  h_flex()
    .border_t_1()
    .border_color(theme.border)
    .bg(theme.background)
    .px_4()
    .py_3()
    .gap_2()
    .items_center()
    .child(div().flex_1().child(widgets::muted(label, cx)))
    .child(
      Button::new("clear-picked")
        .outline()
        .label("Clear")
        .on_click(move |_, window, cx| on_clear(window, cx)),
    )
    .child(
      Button::new("comment-picked")
        .primary()
        .label("Comment")
        .disabled(pending.is_none())
        .on_click(move |_, window, cx| {
          if let Some(p) = pending.clone() {
            compose(review.clone(), p, None, cx);
            after(window, cx);
          }
        }),
    )
    .into_any_element()
}

/// `2 comments in 1 file`.
pub fn size_label(review: &Review) -> String {
  let mut paths: Vec<&str> = review
    .comments()
    .iter()
    .map(|c| c.file_path.as_str())
    .collect();
  paths.sort_unstable();
  paths.dedup();
  format!(
    "{} in {}",
    count_label(review.comments().len(), "comment"),
    count_label(paths.len(), "file")
  )
}

/// The bar at the bottom of a page with a review: its size on a line of
/// its own, and the button that sends it. Nothing while it is empty.
pub fn bar(review: &Entity<Review>, sessions: &Entity<Sessions>, cx: &App) -> Option<AnyElement> {
  let r = review.read(cx);
  if r.is_empty() {
    return None;
  }
  let theme = cx.theme();
  let size = size_label(r);
  let (review, sessions) = (review.clone(), sessions.clone());
  Some(
    v_flex()
      .border_t_1()
      .border_color(theme.border)
      .bg(theme.background)
      .px_4()
      .py_3()
      .gap_2()
      .children(r.error().map(|e| widgets::error_text(e.to_string(), cx)))
      .child(
        h_flex()
          .gap_3()
          .items_center()
          .child(
            v_flex()
              .flex_1()
              .min_w_0()
              .child(div().font_weight(FontWeight::SEMIBOLD).child("Your review"))
              .child(widgets::muted(size, cx)),
          )
          .child(
            Button::new("send-review")
              .primary()
              .icon(Icon::from(LucideIcon::Send))
              .label("Send review")
              .on_click(move |_, _, cx| send(review.clone(), sessions.clone(), cx)),
          ),
      )
      .into_any_element(),
  )
}

/// Opens the sheet that sends the review to an agent.
pub fn send(review: Entity<Review>, sessions: Entity<Sessions>, cx: &mut App) {
  sessions.update(cx, |sessions, cx| sessions.reload(cx));
  sheet::open(Some("Send review".into()), cx, move |window, cx| {
    cx.new(|cx| SendSheet::new(review, sessions, window, cx))
  });
}

/// Where a review goes.
#[derive(Clone, Debug, PartialEq)]
enum Destination {
  /// A running agent session, by label.
  Running(String),
  /// A new session of this agent; `plan` asks Claude Code to plan first.
  New(RemoteAgentId),
}

struct SendSheet {
  review: Entity<Review>,
  sessions: Entity<Sessions>,
  summary: Entity<InputState>,
  /// `None` until the user picks; then the first offered.
  picked: Option<Destination>,
  plan: bool,
  sending: bool,
  error: Option<String>,
}

impl SendSheet {
  fn new(
    review: Entity<Review>,
    sessions: Entity<Sessions>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let initial = review.read(cx).summary().to_string();
    let summary = cx.new(|cx| {
      InputState::new(window, cx)
        .placeholder("Summary (optional)")
        .default_value(initial)
    });
    cx.observe(&sessions, |_, _, cx| cx.notify()).detach();
    Self {
      review,
      sessions,
      summary,
      picked: None,
      plan: false,
      sending: false,
      error: None,
    }
  }

  fn destinations(&self, cx: &App) -> Vec<Destination> {
    let running = self.sessions.read(cx).running_agents();
    running
      .into_iter()
      .rev()
      .map(|(label, _)| Destination::Running(label))
      .chain(
        Sessions::agents_offered(cx)
          .into_iter()
          .map(Destination::New),
      )
      .collect()
  }

  fn chosen(&self, cx: &App) -> Option<Destination> {
    let offered = self.destinations(cx);
    self
      .picked
      .clone()
      .filter(|p| offered.contains(p))
      .or_else(|| offered.into_iter().next())
  }

  fn send(&mut self, cx: &mut Context<Self>) {
    let Some(destination) = self.chosen(cx) else {
      return;
    };
    let summary = self.summary.read(cx).value().trim().to_string();
    self
      .review
      .update(cx, |review, cx| review.set_summary(summary, cx));
    let markdown = self.review.read(cx).markdown();
    let review = self.review.clone();
    match destination {
      Destination::Running(label) => {
        self.sending = true;
        self.error = None;
        let this = cx.weak_entity();
        self.sessions.update(cx, |sessions, cx| {
          sessions.paste_into(
            label,
            markdown,
            move |result, cx| match result {
              Ok(()) => {
                review.update(cx, |review, cx| review.clear(cx));
                sheet::close(cx);
              }
              Err(error) => {
                this
                  .update(cx, |this, cx| {
                    this.sending = false;
                    this.error = Some(error);
                    cx.notify();
                  })
                  .ok();
              }
            },
            cx,
          )
        });
      }
      Destination::New(agent) => {
        let mut args = Vec::new();
        if agent == RemoteAgentId::Claude {
          let mode = if self.plan { "plan" } else { "acceptEdits" };
          args.extend(["--permission-mode".to_string(), mode.to_string()]);
        }
        args.push(markdown);
        sheet::close(cx);
        self
          .sessions
          .update(cx, |sessions, cx| sessions.start_agent(agent, args, cx));
        review.update(cx, |review, cx| review.clear(cx));
      }
    }
    cx.notify();
  }
}

impl Render for SendSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let chosen = self.chosen(cx);
    let size = size_label(self.review.read(cx));
    let label = match &chosen {
      Some(Destination::Running(label)) => format!("Send to {label}"),
      Some(Destination::New(agent)) => format!("Start {}", agent_name(*agent)),
      None => "Send".into(),
    };
    let offered = self.destinations(cx);
    v_flex()
      .gap_3()
      .child(
        v_flex()
          .px_4()
          .gap_2()
          .child(Input::new(&self.summary).large())
          .child(widgets::muted(size, cx)),
      )
      .child(
        v_flex().children(offered.into_iter().enumerate().map(|(i, d)| {
          let (title, detail) = match &d {
            Destination::Running(label) => (
              Program::of_label(label).name().to_string(),
              format!("{label} · running"),
            ),
            Destination::New(agent) => (
              format!("New {}", agent_name(*agent)),
              "Starts a new session".to_string(),
            ),
          };
          let selected = chosen.as_ref() == Some(&d);
          widgets::choice(("destination", i), title, Some(detail.into()), selected, cx)
            .on_click(cx.listener(move |this, _, _, cx| {
              this.picked = Some(d.clone());
              cx.notify();
            }))
        })),
      )
      .when(
        matches!(chosen, Some(Destination::New(RemoteAgentId::Claude))),
        |c| {
          c.child(
            h_flex()
              .px_4()
              .gap_2()
              .child(
                Button::new("mode-edit")
                  .label("Edit")
                  .when(!self.plan, |b| b.primary())
                  .when(self.plan, |b| b.outline())
                  .on_click(cx.listener(|this, _, _, cx| {
                    this.plan = false;
                    cx.notify();
                  })),
              )
              .child(
                Button::new("mode-plan")
                  .label("Plan")
                  .when(self.plan, |b| b.primary())
                  .when(!self.plan, |b| b.outline())
                  .on_click(cx.listener(|this, _, _, cx| {
                    this.plan = true;
                    cx.notify();
                  })),
              ),
          )
        },
      )
      .child(
        v_flex()
          .px_4()
          .gap_2()
          .children(self.error.clone().map(|e| widgets::error_text(e, cx)))
          .child(
            Button::new("send")
              .primary()
              .large()
              .w_full()
              .h(px(widgets::CTA_H - 8.))
              .label(label)
              .loading(self.sending)
              .disabled(chosen.is_none() || self.sending)
              .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
          )
          .when(chosen.is_none(), |c| {
            c.child(widgets::muted(
              "No agents on this host. Install Claude Code, Codex or Cursor Agent there, then reconnect.",
              cx,
            ))
          }),
      )
  }
}
