//! The pull request's GitHub review threads and the review agent's local
//! comments, drawn in the Changes tab's diff (`GithubCommentCard`,
//! `GithubThreadsInlineList`, `AgentReviewCommentCard`).

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use std::time::Duration;

use gpui::{AnyElement, StrikethroughStyle, StyledText, Task};

use super::*;
use crate::{
  github::{GhReviewComment, GhReviewThread},
  gpui_shell::screen::{
    changes_review::{count_label, number_and_side, Pending, Quote, Side},
    changes_threads::{
      agent_by_line, comment_date, original_text, place_threads, preview, suggestion_diff, Placed,
      RowKind,
    },
    commits_data::LineRef,
    reactions::{self, Reacts},
  },
  local_db::AgentReviewComment,
};

/// How often the agent's comments are read again (`refreshMs`).
const AGENT_POLL: Duration = Duration::from_secs(10);
/// The GitHub threads are read every this many agent polls (2 minutes).
const GITHUB_EVERY: u32 = 12;

#[derive(Default)]
pub(in crate::gpui_shell::screen) struct Threads {
  github: Vec<GhReviewThread>,
  /// Threads folded; resolved ones start folded the first time seen.
  collapsed: HashSet<String>,
  seen: HashSet<String>,
  pub(in crate::gpui_shell::screen) agent: Vec<AgentReviewComment>,
  /// Agent comments with an action running.
  busy: HashSet<String>,
  placed: Placed,
  /// Indices into `agent`.
  agent_lines: HashMap<(String, u32, Side), Vec<usize>>,
  /// Bumped when `github` or `agent` reads back changed.
  pub(in crate::gpui_shell::screen) revision: u64,
  /// The rows' and the threads' revisions `placed` was derived from.
  placed_for: Option<(u64, u64)>,
  poll: Option<Task<()>>,
  /// The review comment whose reaction picker is open.
  reaction_picker: Option<String>,
}

impl ChangesView {
  /// Reads the threads and agent comments now and on a timer.
  pub(super) fn watch_threads(&mut self, cx: &mut Context<Self>) {
    self.threads = Threads::default();
    self.poll_threads(cx);
  }

  /// Reads the threads and agent comments now and on a timer, keeping
  /// what was read.
  pub(super) fn poll_threads(&mut self, cx: &mut Context<Self>) {
    if self.parent.workspace.is_none() {
      return;
    }
    self.threads.poll = Some(cx.spawn(async move |this, cx| {
      let mut tick = 0u32;
      loop {
        let github = tick.is_multiple_of(GITHUB_EVERY);
        if this
          .update(cx, |view, cx| view.read_threads(github, cx))
          .is_err()
        {
          return;
        }
        cx.background_executor().timer(AGENT_POLL).await;
        tick = tick.wrapping_add(1);
      }
    }));
  }

  pub(super) fn stop_threads(&mut self) {
    self.threads.poll = None;
  }

  fn read_threads(&mut self, github: bool, cx: &mut Context<Self>) {
    self.read_agent(cx);
    if !github {
      return;
    }
    let branch = self
      .parent
      .workspace
      .as_ref()
      .map(|w| w.branch_name.clone())
      .unwrap_or_default();
    // Each poll redraws only when it read something new.
    self.run_changed(
      move |host, parent| host.review_threads(&parent.repo_path, &branch),
      |view, result, _| match result {
        Ok(threads) if threads == view.threads.github => false,
        Ok(threads) => {
          view.set_github_threads(threads);
          true
        }
        Err(err) => {
          log::warn!("Failed to read review threads: {err}");
          false
        }
      },
      cx,
    );
  }

  /// Resolved threads start folded the first time they are seen.
  pub(super) fn set_github_threads(&mut self, threads: Vec<GhReviewThread>) {
    for t in &threads {
      if self.threads.seen.insert(t.id.clone()) && t.is_resolved {
        self.threads.collapsed.insert(t.id.clone());
      }
    }
    self.threads.github = threads;
    self.threads.revision += 1;
  }

  fn read_agent(&mut self, cx: &mut Context<Self>) {
    let Some(id) = self.parent.workspace_id() else {
      return;
    };
    self.run_changed(
      move |host, parent| host.agent_comments(&parent.repo_path, id),
      |view, result, _| match result {
        Ok(comments) if comments == view.threads.agent => false,
        Ok(comments) => {
          view.threads.agent = comments;
          view.threads.revision += 1;
          true
        }
        Err(err) => {
          log::warn!("Failed to read agent review comments: {err}");
          false
        }
      },
      cx,
    );
  }

  /// Anchors the threads and comments on the diff as it is now.
  pub(super) fn place_threads(&mut self) {
    // The rows' revision follows the hunks and the snapshot.
    let key = (self.diff.revision, self.threads.revision);
    if self.threads.placed_for == Some(key) {
      return;
    }
    self.threads.placed_for = Some(key);
    // Only the files a thread is on are looked at.
    let paths: HashSet<&str> = self
      .threads
      .github
      .iter()
      .map(|t| t.path.as_str())
      .collect();
    fn on<'a>(
      files: &'a HashMap<String, FileHunks>,
      paths: &HashSet<&str>,
    ) -> HashMap<String, &'a [JjDiffHunk]> {
      files
        .iter()
        .filter(|(p, _)| paths.contains(p.as_str()))
        .map(|(p, d)| (p.clone(), &d.hunks[..]))
        .collect()
    }
    let mut working = on(&self.hunks, &paths);
    // Files shown as their branch diff anchor on it alone.
    if let Some(s) = &self.snapshot {
      working.retain(|p, _| !s.shows_branch_diff(p));
    }
    let committed = self
      .snapshot
      .as_ref()
      .map(|s| on(&s.committed_hunks, &paths))
      .unwrap_or_default();
    self.threads.placed = place_threads(&self.threads.github, &[&working, &committed]);
    self.threads.agent_lines = agent_by_line(&self.threads.agent);
  }

  /// Resolve, Delete or Apply, then a fresh read.
  fn agent_action(&mut self, id: &str, action: AgentAction, cx: &mut Context<Self>) {
    if !self.threads.busy.insert(id.to_string()) {
      return;
    }
    let comment = id.to_string();
    self.run(
      move |host, parent| match action {
        AgentAction::Resolve => host.resolve_agent_comment(&parent.repo_path, &comment),
        AgentAction::Delete => host.delete_agent_comment(&parent.repo_path, &comment),
        AgentAction::Apply => host.apply_agent_suggestion(&parent.repo_path, &comment),
      },
      {
        let id = id.to_string();
        move |view, result, cx| {
          view.threads.busy.remove(&id);
          match result {
            Ok(()) => {
              if action == AgentAction::Apply {
                view.load(cx);
              }
            }
            Err(err) => view.toast(ToastTone::Error, "Review comment action failed", err),
          }
          view.read_agent(cx);
        }
      },
      cx,
    );
    cx.notify();
  }

  /// `onSendAgentReviewCommentToAgent`: React starts the agent.
  fn send_agent_comment(&self, comment: &AgentReviewComment, cx: &mut App) {
    self.field(
      "changes.agentComment",
      serde_json::to_value(comment).unwrap_or_default(),
      cx,
    );
  }

  fn toggle_thread(&mut self, id: &str, cx: &mut Context<Self>) {
    if !self.threads.collapsed.remove(id) {
      self.threads.collapsed.insert(id.to_string());
    }
    cx.notify();
  }

  /// A thread's quote seeds a comment on its line (`buildQuotedPendingComment`).
  fn quote(
    &mut self,
    comment: &GhReviewComment,
    path: &str,
    hunk: Option<(String, LineRef)>,
    line: u32,
    side: Side,
    cx: &mut Context<Self>,
  ) {
    let (hunk_id, at) = match hunk {
      Some((id, at)) => (id, Some(at)),
      None => (String::new(), None),
    };
    self.open_draft(
      Pending {
        file_path: path.into(),
        hunk_id,
        at,
        start_line: line,
        end_line: line,
        line_content: vec![comment.body.clone()],
        side,
        quote: Some(Quote {
          author: comment.author.login.clone(),
          avatar_url: comment.author.avatar_url.clone(),
          url: comment.url.clone(),
        }),
      },
      cx,
    );
  }

  // Drawing.

  /// What a diff line carries from others: agent comments, then threads.
  /// `ReviewActionBar`'s count of the open local agent comments.
  pub(super) fn agent_count(&self, p: &Palette) -> Option<Div> {
    let open = self
      .threads
      .agent
      .iter()
      .filter(|c| c.status != "resolved")
      .count();
    (open > 0).then(|| {
      ui::text(div(), TextSize::Xs)
        .debug_selector(|| "agent-review-comment-count".into())
        .text_color(Tone::GitMerged.color(p))
        .child(count_label(open, "local review comment"))
    })
  }

  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  pub(super) fn line_threads(
    &self,
    path: &str,
    hunks: &[JjDiffHunk],
    h: usize,
    l: usize,
    numbers: (Option<u32>, Option<u32>),
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Vec<AnyElement> {
    let hunk = &hunks[h];
    let (number, side) = number_and_side(numbers, l);
    let mut out = Vec::new();
    if let Some(comments) = self
      .threads
      .agent_lines
      .get(&(path.to_string(), number, side))
    {
      let mut list = div()
        .debug_selector(|| "agent-review-inline-list".into())
        .border_y_1()
        .border_color(violet(p, 0.3))
        .bg(violet(p, 0.03))
        .px_4()
        .py_2()
        .flex()
        .flex_col()
        .gap_2()
        .font_family(theme::SANS_FONT);
      for c in comments.iter().filter_map(|&i| self.threads.agent.get(i)) {
        list = list.child(self.agent_card(c, original_text(c, hunk), p, cx));
      }
      out.push(list.into_any_element());
    }
    let key = (path.to_string(), hunk.id.clone(), number, side);
    if let Some(threads) = self.threads.placed.by_line.get(&key) {
      let mut list = div()
        .border_y_1()
        .border_color(p.border.opacity(0.4))
        .px_4()
        .py_2()
        .flex()
        .flex_col()
        .gap_2();
      for t in threads.iter().filter_map(|&i| self.threads.github.get(i)) {
        let anchor = Some((hunk.id.clone(), (h, l)));
        list = list.child(self.thread_card(t, path, anchor, number, side, p, cx));
      }
      out.push(list.into_any_element());
    }
    out
  }

  /// Threads whose line is not in the diff, at the top of the file.
  pub(super) fn unplaced_threads(
    &self,
    path: &str,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    let threads = self.threads.placed.unplaced.get(path)?;
    let mut section = div()
      .debug_selector(|| "github-outdated-threads".into())
      .border_b_1()
      .border_color(sky(p, 0.4))
      .bg(sky(p, 0.05))
      .px_4()
      .py_3()
      .flex()
      .flex_col()
      .gap_3();
    for t in threads.iter().filter_map(|&i| self.threads.github.get(i)) {
      let line = t.line.and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
      section = section.child(self.thread_card(t, path, None, line, Side::New, p, cx));
    }
    let drafting = self
      .review
      .draft
      .as_ref()
      .is_some_and(|d| d.file_path == path && d.hunk_id.is_empty());
    if drafting {
      section = section.child(self.comment_input(p, cx));
    }
    Some(section)
  }

  /// `GithubCommentCard`.
  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  fn thread_card(
    &self,
    thread: &GhReviewThread,
    path: &str,
    anchor: Option<(String, LineRef)>,
    line: u32,
    side: Side,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let collapsed = self.threads.collapsed.contains(&thread.id);
    let first = thread.comments.first();
    let muted = p.muted_foreground;
    let id = thread.id.clone();
    let toggle_hover = sky(p, 0.1);
    let mut toggle = div()
      .id(SharedString::from(format!("github-thread-{}", thread.id)))
      .debug_selector(|| "github-thread-toggle".into())
      .w_full()
      .min_w_0()
      .flex()
      .items_center()
      .gap_2()
      .px_3()
      .py_1p5()
      .cursor_pointer()
      .hover(move |s| s.bg(toggle_hover))
      .on_click(cx.listener(move |view, _, _, cx| view.toggle_thread(&id, cx)))
      .child(ui::icon(
        if collapsed {
          theme::ICON_CHEVRON_RIGHT
        } else {
          theme::ICON_CHEVRON_DOWN
        },
        12.,
        muted,
      ));
    if let (true, Some(first)) = (collapsed, first) {
      toggle = toggle
        .child(avatar(&first.author.login, 20., p))
        .child(profile_link(&first.author.login, p, cx));
    }
    if thread.is_resolved {
      toggle = toggle.child(
        ui::text(div(), TextSize::Xs)
          .flex_none()
          .flex()
          .items_center()
          .gap_1()
          .px_1p5()
          .py_0p5()
          .rounded(radius::SM)
          .bg(p.muted)
          .text_color(muted)
          .font_weight(FontWeight::MEDIUM)
          .child(ui::icon(theme::ICON_CIRCLE_CHECK, 12., muted))
          .child("Resolved"),
      );
    }
    if let (true, Some(first)) = (collapsed, first) {
      toggle = toggle.child(
        ui::text(div(), TextSize::Xs)
          .debug_selector(|| "github-thread-collapsed-preview".into())
          .min_w_0()
          .truncate()
          .text_color(muted.opacity(0.7))
          .child(preview(&first.body)),
      );
    }
    if !collapsed {
      let n = thread.comments.len();
      toggle = toggle.child(
        ui::text(div(), TextSize::Xs)
          .text_color(muted)
          .child(format!("{n} comment{}", if n == 1 { "" } else { "s" })),
      );
    }
    let mut card = div()
      .rounded(radius::MD)
      .border_1()
      .border_color(sky(p, 0.3))
      .bg(sky(p, 0.05))
      .overflow_hidden()
      .font_family(theme::SANS_FONT)
      .child(toggle);
    if !collapsed {
      let mut body = div().px_3().pb_3().flex().flex_col().gap_2();
      for comment in &thread.comments {
        body = body.child(self.thread_comment(comment, path, anchor.clone(), line, side, p, cx));
      }
      card = card.child(body.children(self.thread_reply(thread, p, cx)));
    }
    card.into_any_element()
  }

  /// `GithubCommentBody`: author, date and text, with a Quote button.
  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  fn thread_comment(
    &self,
    comment: &GhReviewComment,
    path: &str,
    anchor: Option<(String, LineRef)>,
    line: u32,
    side: Side,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    use chrono::Datelike;
    let this_year = chrono::Local::now().year();
    let accent = p.accent;
    let fg = p.foreground;
    let quoted = comment.clone();
    let file = path.to_string();
    div()
      .debug_selector(|| "github-comment-card".into())
      .group("github-comment")
      .relative()
      .bg(p.background)
      .rounded(radius::MD)
      .p_2p5()
      .pr_9()
      .border_1()
      .border_color(p.border.opacity(0.6))
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .mb_1p5()
          .child(avatar(&comment.author.login, 32., p))
          .child(profile_link(&comment.author.login, p, cx))
          .child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child(comment_date(&comment.created_at, this_year)),
          )
          .child(ui::icon(theme::ICON_GITHUB, 14., Tone::Primary.text(p))),
      )
      .child(div().debug_selector(|| "github-comment-body".into()).child(
        super::super::code::markdown(
          format!("github-comment-{}", comment.id),
          &comment.body,
          TextSize::Code.metrics().0,
          self.dark,
        ),
      ))
      .child(reactions::bar(
        &self.tips,
        self.threads.reaction_picker.as_deref(),
        &comment.id,
        &comment.reactions,
        p,
        cx,
      ))
      .child(
        self.tip(
          format!("quote-{}", comment.id),
          "Quote this comment",
          div()
            .id(SharedString::from(format!("quote-{}", comment.id)))
            .debug_selector(|| "github-comment-quote".into())
            .absolute()
            .top_2()
            .right_2()
            .p_1()
            .rounded(radius::SM)
            .cursor_pointer()
            .opacity(0.)
            .group_hover("github-comment", |s| s.opacity(1.))
            .hover(move |s| s.bg(accent).text_color(fg))
            .child(ui::icon(theme::ICON_QUOTE, 14., p.muted_foreground))
            .on_click(cx.listener(move |view, _, _, cx| {
              view.quote(&quoted, &file, anchor.clone(), line, side, cx)
            })),
          cx,
        ),
      )
  }

  /// `AgentReviewCommentCard`.
  fn agent_card(
    &self,
    comment: &AgentReviewComment,
    original: Option<String>,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let busy = self.threads.busy.contains(&comment.id);
    let muted = p.muted_foreground;
    let range = if comment.end_line != comment.start_line {
      format!("{}-{}", comment.start_line, comment.end_line)
    } else {
      comment.start_line.to_string()
    };
    let mut card = div()
      .debug_selector(|| "agent-review-comment-card".into())
      .font_family(theme::MONO_FONT)
      .rounded(radius::MD)
      .border_1()
      .border_color(violet(p, 0.4))
      .bg(violet(p, 0.05))
      .p_3()
      .flex()
      .flex_col()
      .gap_2()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(ui::icon(theme::ICON_BOT, 14., Tone::GitMerged.color(p)))
          .child(
            ui::text(div(), TextSize::Xs)
              .font_weight(FontWeight::MEDIUM)
              .text_color(Tone::GitMerged.color(p))
              .child("Local"),
          )
          .child(
            ui::text(div(), TextSize::Xs)
              .text_color(muted)
              .child(format!("{}:{range}", comment.file_path)),
          ),
      )
      .child(
        div()
          .whitespace_normal()
          .child(comment.comment_text.clone()),
      );
    if let Some(suggested) = &comment.suggested_replacement {
      card = card.child(suggestion_box(original.as_deref(), suggested, p));
    }
    let id = comment.id.clone();
    let mut actions = div().flex().flex_wrap().items_center().gap_2();
    if comment.suggested_replacement.is_some() {
      let id = id.clone();
      actions = actions.child(
        Button::new(SharedString::from(format!("agent-apply-{id}")), p)
          .size(Size::Sm)
          .gap_1p5()
          .disabled(busy)
          .icon(theme::ICON_WAND, 12.)
          .label("Apply")
          .on_click(
            cx.listener(move |view, _, _, cx| view.agent_action(&id, AgentAction::Apply, cx)),
          ),
      );
    }
    let sent = comment.clone();
    actions = actions
      .child(
        Button::new(SharedString::from(format!("agent-send-{id}")), p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .gap_1p5()
          .disabled(busy)
          .icon(theme::ICON_TERMINAL, 12.)
          .label("Send to agent")
          .on_click(cx.listener(move |view, _, _, cx| view.send_agent_comment(&sent, cx))),
      )
      .child({
        let id = id.clone();
        Button::new(SharedString::from(format!("agent-resolve-{id}")), p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .gap_1p5()
          .disabled(busy)
          .icon(theme::ICON_CHECK, 12.)
          .label("Resolve")
          .on_click(
            cx.listener(move |view, _, _, cx| view.agent_action(&id, AgentAction::Resolve, cx)),
          )
      })
      .child(
        Button::new(SharedString::from(format!("agent-delete-{id}")), p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .gap_1p5()
          .text_color(muted)
          .disabled(busy)
          .icon(theme::ICON_TRASH, 12.)
          .label("Delete")
          .on_click(
            cx.listener(move |view, _, _, cx| view.agent_action(&id, AgentAction::Delete, cx)),
          ),
      );
    card.child(actions)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentAction {
  Resolve,
  Delete,
  Apply,
}

fn sky(p: &Palette, alpha: f32) -> Hsla {
  Tone::Primary.color(p).opacity(alpha)
}

fn violet(p: &Palette, alpha: f32) -> Hsla {
  Tone::GitMerged.color(p).opacity(alpha)
}

/// `AuthorAvatar`'s fallback: the login's initial in a circle.
fn avatar(login: &str, size: f32, p: &Palette) -> impl IntoElement {
  ui::text(div(), TextSize::Xs)
    .size(zpx(size))
    .flex_none()
    .rounded_full()
    .bg(p.muted)
    .flex()
    .items_center()
    .justify_center()
    .font_weight(FontWeight::MEDIUM)
    .text_color(p.muted_foreground)
    .child(
      login
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default(),
    )
}

/// `AuthorProfileLink`: "@login", opening the GitHub profile.
fn profile_link(login: &str, p: &Palette, cx: &mut Context<ChangesView>) -> impl IntoElement {
  let url = format!("https://github.com/{login}");
  ui::text(div(), TextSize::Xs)
    .id(SharedString::from(format!("profile-{login}")))
    .flex_none()
    .font_weight(FontWeight::MEDIUM)
    .text_color(p.foreground)
    .cursor_pointer()
    .hover(|s| s.underline())
    .child(format!("@{login}"))
    .on_mouse_down(MouseButton::Right, {
      let url = url.clone();
      cx.listener(move |view, e: &gpui::MouseDownEvent, _, cx| {
        cx.stop_propagation();
        view.copy_link(url.clone(), e.position, cx);
      })
    })
    .on_click(cx.listener(move |_, _, _, cx| {
      cx.stop_propagation();
      cx.open_url(&url);
    }))
}

/// "Suggested change", as a diff against the lines it replaces.
pub(in crate::gpui_shell::screen) fn suggestion_box(
  original: Option<&str>,
  suggested: &str,
  p: &Palette,
) -> impl IntoElement {
  let header = ui::text(div(), TextSize::Xs)
    .px_2()
    .py_1()
    .text_color(p.muted_foreground)
    .bg(p.muted.opacity(0.6))
    .border_b_1()
    .border_color(p.border.opacity(0.6))
    .child("Suggested change");
  let body = match suggestion_diff(original, suggested) {
    Some(rows) => {
      let mut el = ui::text(div(), TextSize::Xs)
        .debug_selector(|| "agent-review-suggestion".into())
        .font_family(theme::MONO_FONT);
      for row in rows {
        let (marker, bg, mark) = match row.kind {
          RowKind::Removed => ("-", Some(Tone::GitClosed.tint(p)), Tone::GitClosed),
          RowKind::Added => ("+", Some(Tone::GitOpen.tint(p)), Tone::GitOpen),
          RowKind::Unchanged => (" ", None, Tone::GitOpen),
        };
        let mut text = String::new();
        let mut highlights = Vec::new();
        for segment in &row.segments {
          let start = text.len();
          text.push_str(&segment.text);
          if segment.changed && row.kind != RowKind::Unchanged {
            highlights.push((
              start..text.len(),
              gpui::HighlightStyle {
                background_color: Some(mark.color(p).opacity(0.3)),
                strikethrough: (row.kind == RowKind::Removed).then(|| StrikethroughStyle {
                  thickness: px(1.),
                  color: None,
                }),
                ..Default::default()
              },
            ));
          }
        }
        el = el.child(
          div()
            .flex()
            .items_start()
            .px_2()
            .py_px()
            .when_some(bg, |this, bg| this.bg(bg))
            .child(
              div()
                .w_3()
                .flex_none()
                .text_color(p.muted_foreground)
                .child(marker),
            )
            .child(
              div()
                .flex_1()
                .min_w_0()
                .child(StyledText::new(text).with_highlights(highlights)),
            ),
        );
      }
      el.into_any_element()
    }
    None => ui::text(div(), TextSize::Xs)
      .debug_selector(|| "agent-review-suggestion".into())
      .font_family(theme::MONO_FONT)
      .px_2()
      .py_1p5()
      .bg(Tone::GitOpen.tint(p))
      .child(suggested.to_string())
      .into_any_element(),
  };
  div()
    .rounded(radius::SM)
    .border_1()
    .border_color(p.border.opacity(0.6))
    .overflow_hidden()
    .child(header)
    .child(body)
}

impl ChangesView {
  /// The GitHub review comment `id`.
  fn review_comment_mut(&mut self, id: &str) -> Option<&mut GhReviewComment> {
    self
      .threads
      .github
      .iter_mut()
      .flat_map(|t| t.comments.iter_mut())
      .find(|c| c.id == id)
  }
}

impl Reacts for ChangesView {
  fn reaction_picker(&mut self) -> &mut Option<String> {
    &mut self.threads.reaction_picker
  }

  fn react(&mut self, id: &str, content: &'static str, cx: &mut Context<Self>) {
    let Some(comment) = self.review_comment_mut(id) else {
      return;
    };
    let on = reactions::toggle(&mut comment.reactions, content);
    cx.notify();
    let id = id.to_string();
    self.run(
      {
        let id = id.clone();
        move |host, _| host.react(&id, content, on)
      },
      move |view, result, _| {
        if let Err(err) = result {
          if let Some(comment) = view.review_comment_mut(&id) {
            reactions::toggle(&mut comment.reactions, content);
          }
          view.toast(ToastTone::Error, "Failed to react", err);
        }
      },
      cx,
    );
  }
}
