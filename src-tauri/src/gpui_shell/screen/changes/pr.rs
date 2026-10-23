//! The Changes tab's diff reviewing a GitHub pull request instead of a
//! workspace (`PrReviewPage`'s `DiffContentArea`, read-only): the diff is
//! the PR's, the comments are review drafts kept in the local database
//! (`github_pr`), and the review threads take replies.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{sync::Arc, time::Duration};

use gpui::{AnyElement, Task};

use super::*;
use crate::{
  core::generated::GeneratedFiles,
  github::GhReviewThread,
  gpui_shell::screen::{
    changes_review::{LineComment, Pending},
    pr_review_data::{self, Draft, NewDraft, PrReviewHost},
  },
  local_db::AgentReviewComment,
};

/// How often the drafts are read again (`refreshMs: 3_000`).
const DRAFT_POLL: Duration = Duration::from_secs(3);
/// The diff is read every this many draft polls (a minute), to pick up pushes.
const DIFF_EVERY: u32 = 20;
/// The review threads are read every this many draft polls (two minutes).
const THREADS_EVERY: u32 = 40;

/// The pull request under review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrTarget {
  pub repo_path: String,
  pub full_name: String,
  pub number: u64,
}

/// A thread's reply box (`ThreadReply`).
#[derive(Default)]
struct Reply {
  open: bool,
  text: String,
  input: Option<(Entity<TextareaState>, Subscription)>,
  sending: bool,
  error: Option<String>,
}

pub(in crate::gpui_shell::screen) struct PrMode {
  host: Arc<dyn PrReviewHost>,
  pub target: PrTarget,
  /// Why the diff could not be read.
  pub error: Option<String>,
  /// The diff was read once (`rawDiff !== undefined`).
  pub read: bool,
  rows: Vec<AgentReviewComment>,
  /// The drafts as the diff shows them.
  pub drafts: Vec<Draft>,
  /// Drafts whose line hash was written.
  hashed: HashSet<String>,
  replies: HashMap<String, Reply>,
  poll: Option<Task<()>>,
}

impl PrMode {
  pub fn new(host: Arc<dyn PrReviewHost>, target: PrTarget) -> Self {
    Self {
      host,
      target,
      error: None,
      read: false,
      rows: Vec::new(),
      drafts: Vec::new(),
      hashed: HashSet::new(),
      replies: HashMap::new(),
      poll: None,
    }
  }

  /// The drafts a review posts: all but the outdated.
  pub fn postable(&self) -> Vec<LineComment> {
    self
      .drafts
      .iter()
      .filter(|d| !d.outdated)
      .map(|d| d.comment.clone())
      .collect()
  }

  pub fn outdated(&self) -> usize {
    self.drafts.iter().filter(|d| d.outdated).count()
  }
}

impl ChangesView {
  /// The tab reviewing `target`'s pull request.
  pub fn new_pr(
    target: PrTarget,
    host: Arc<dyn PrReviewHost>,
    react: ActionSink,
    popups: PopupSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let parent = Parent {
      repo_path: target.repo_path.clone(),
      workspace: None,
      conflicted_files: Vec::new(),
      default_branch: String::new(),
      revision: 0,
      remote: false,
    };
    let changes: Host = host.clone();
    let mut this = Self::blank(parent, Options::default(), changes, react, popups, dark, cx);
    this.pr = Some(PrMode::new(host, target));
    this.watch_pr(cx);
    this
  }

  pub(super) fn read_only(&self) -> bool {
    self.pr.is_some()
  }

  pub(in crate::gpui_shell::screen) fn pr_mode(&self) -> Option<&PrMode> {
    self.pr.as_ref()
  }

  /// Runs `work` against the PR off the main thread, then `apply`s it.
  fn run_pr<T: Send + 'static>(
    &self,
    work: impl FnOnce(&dyn PrReviewHost, &PrTarget) -> T + Send + 'static,
    apply: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let Some(pr) = &self.pr else {
      return;
    };
    let (host, target) = (pr.host.clone(), pr.target.clone());
    cx.spawn(async move |this, cx| {
      let value = cx
        .background_spawn({
          let target = target.clone();
          async move { work(host.as_ref(), &target) }
        })
        .await;
      this
        .update(cx, |view, cx| {
          if view.pr.as_ref().is_some_and(|p| p.target == target) {
            apply(view, value, cx);
            cx.notify();
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  /// Reads the diff, the drafts and the threads now and on a timer.
  fn watch_pr(&mut self, cx: &mut Context<Self>) {
    let task = cx.spawn(async move |this, cx| {
      let mut tick = 0u32;
      loop {
        let ok = this
          .update(cx, |view, cx| {
            if tick.is_multiple_of(DIFF_EVERY) {
              view.read_pr_diff(cx);
            }
            if tick.is_multiple_of(THREADS_EVERY) {
              view.read_pr_threads(cx);
            }
            view.read_drafts(cx);
          })
          .is_ok();
        if !ok {
          return;
        }
        cx.background_executor().timer(DRAFT_POLL).await;
        tick = tick.wrapping_add(1);
      }
    });
    if let Some(pr) = self.pr.as_mut() {
      pr.poll = Some(task);
    }
  }

  pub(super) fn read_pr_diff(&mut self, cx: &mut Context<Self>) {
    self.run_pr(
      |host, t| host.pr_diff(&t.full_name, t.number),
      |view, result, cx| match result {
        Ok(diff) => {
          let parsed = pr_review_data::parse_unified_diff(&diff);
          view.hunks = parsed
            .hunks
            .into_iter()
            .map(|(path, hunks)| {
              (
                path,
                FileHunks {
                  hunks: hunks.into(),
                  ..FileHunks::default()
                },
              )
            })
            .collect();
          let listed: HashSet<String> = parsed.files.iter().map(|f| f.path.clone()).collect();
          view.viewed.retain(|p, _| listed.contains(p));
          view.snapshot = Some(
            Snapshot {
              files: parsed.files,
              ..Snapshot::default()
            }
            .with_generated(&GeneratedFiles::default()),
          );
          if let Some(pr) = view.pr.as_mut() {
            pr.error = None;
            pr.read = true;
          }
          view.place_drafts(cx);
        }
        Err(err) => {
          if let Some(pr) = view.pr.as_mut() {
            pr.error = Some(err);
          }
        }
      },
      cx,
    );
  }

  pub(in crate::gpui_shell::screen) fn read_pr_threads(&mut self, cx: &mut Context<Self>) {
    self.run_pr(
      |host, t| host.pr_threads(&t.full_name, t.number),
      |view, result, _| match result {
        Ok(threads) => view.set_github_threads(threads),
        Err(err) => log::warn!("Failed to read review threads: {err}"),
      },
      cx,
    );
  }

  pub(in crate::gpui_shell::screen) fn read_drafts(&mut self, cx: &mut Context<Self>) {
    self.run_pr(
      |host, t| host.drafts(&t.repo_path, t.number),
      |view, result, cx| match result {
        Ok(rows) => {
          if let Some(pr) = view.pr.as_mut() {
            pr.rows = rows;
          }
          view.place_drafts(cx);
        }
        Err(err) => log::warn!("Failed to read review drafts: {err}"),
      },
      cx,
    );
  }

  /// The drafts on the diff as it is now, shown as the tab's comments. The
  /// agent writes drafts without a line hash, so they get one on first sight.
  fn place_drafts(&mut self, cx: &mut Context<Self>) {
    let hunks: HashMap<String, Vec<JjDiffHunk>> = self
      .hunks
      .iter()
      .map(|(p, d)| (p.clone(), d.hunks.to_vec()))
      .collect();
    let Some(pr) = self.pr.as_mut() else {
      return;
    };
    pr.drafts = if pr.read {
      pr_review_data::drafts(&pr.rows, &hunks)
    } else {
      Vec::new()
    };
    self.review.comments = pr.drafts.iter().map(|d| d.comment.clone()).collect();
    let unhashed: Vec<(String, String)> = pr
      .drafts
      .iter()
      .filter(|d| d.row.line_hash.is_none() && !pr.hashed.contains(&d.row.id))
      .filter_map(|d| Some((d.row.id.clone(), d.line_hash.clone()?)))
      .collect();
    if unhashed.is_empty() {
      return;
    }
    pr.hashed.extend(unhashed.iter().map(|(id, _)| id.clone()));
    self.run_pr(
      move |host, t| {
        for (id, hash) in &unhashed {
          if let Err(err) = host.update_draft(&t.repo_path, id, None, Some(hash)) {
            log::warn!("Failed to hash review draft: {err}");
          }
        }
      },
      |view, (), cx| view.read_drafts(cx),
      cx,
    );
  }

  fn draft_failed(&self, err: String) {
    self.toast(ToastTone::Error, "Failed to save draft comment", err);
  }

  /// `addComment`: the draft is kept with the hash of its lines.
  #[allow(
    clippy::expect_used,
    reason = "the probe is a constant JSON shape of the comment type"
  )]
  pub(super) fn submit_pr_draft(&mut self, draft: Pending, text: String, cx: &mut Context<Self>) {
    let hunks: HashMap<String, Vec<JjDiffHunk>> = self
      .hunks
      .iter()
      .map(|(p, d)| (p.clone(), d.hunks.to_vec()))
      .collect();
    let probe: AgentReviewComment = serde_json::from_value(json!({
      "id": "",
      "repo_path": self.parent.repo_path,
      "target_type": pr_review_data::TARGET,
      "target_id": "",
      "file_path": draft.file_path,
      "hunk_id": null,
      "start_line": draft.start_line,
      "end_line": draft.end_line,
      "side": pr_review_data::side_name(draft.side),
      "comment_text": "",
      "suggested_replacement": null,
      "status": "open",
      "source": "local-human",
      "created_at": "",
      "resolved_at": null,
    }))
    .expect("draft probe");
    let line_hash = pr_review_data::draft_to_comment(&probe, &hunks).line_hash;
    let new = NewDraft {
      file_path: draft.file_path.clone(),
      start_line: i64::from(draft.start_line),
      end_line: i64::from(draft.end_line),
      side: draft.side,
      text,
      line_hash,
    };
    self.run_pr(
      move |host, t| host.create_draft(&t.repo_path, t.number, &new),
      |view, result, cx| match result {
        Ok(()) => {
          view.review.draft = None;
          view.review.draft_input = None;
          view.review.draft_text.clear();
          view.read_drafts(cx);
        }
        Err(err) => view.draft_failed(err),
      },
      cx,
    );
  }

  pub(super) fn save_pr_edit(&mut self, id: String, text: String, cx: &mut Context<Self>) {
    self.run_pr(
      move |host, t| host.update_draft(&t.repo_path, &id, Some(&text), None),
      |view, result, cx| match result {
        Ok(()) => {
          view.review.editing = None;
          view.review.edit_input = None;
          view.read_drafts(cx);
        }
        Err(err) => view.draft_failed(err),
      },
      cx,
    );
  }

  pub(super) fn delete_pr_draft(&mut self, id: String, cx: &mut Context<Self>) {
    self.run_pr(
      move |host, t| host.delete_agent_comment(&t.repo_path, &id),
      |view, result, cx| match result {
        Ok(()) => view.read_drafts(cx),
        Err(err) => view.draft_failed(err),
      },
      cx,
    );
  }

  // Thread replies.

  fn reply(&mut self, thread: &str) -> Option<&mut Reply> {
    Some(
      self
        .pr
        .as_mut()?
        .replies
        .entry(thread.to_string())
        .or_default(),
    )
  }

  fn open_reply(&mut self, thread: &str, open: bool, cx: &mut Context<Self>) {
    if let Some(reply) = self.reply(thread) {
      reply.open = open;
      if !open {
        reply.input = None;
      }
    }
    cx.notify();
  }

  fn send_reply(&mut self, thread: &str, cx: &mut Context<Self>) {
    let Some(reply) = self.reply(thread) else {
      return;
    };
    let body = reply.text.trim().to_string();
    if body.is_empty() || reply.sending {
      return;
    }
    reply.sending = true;
    reply.error = None;
    let id = thread.to_string();
    self.run_pr(
      move |host, t| {
        host
          .reply_thread(&id, &body)
          .and_then(|()| host.pr_threads(&t.full_name, t.number))
      },
      {
        let id = thread.to_string();
        move |view, result, _| {
          let threads = match result {
            Ok(threads) => {
              if let Some(reply) = view.reply(&id) {
                *reply = Reply::default();
              }
              threads
            }
            Err(err) => {
              if let Some(reply) = view.reply(&id) {
                reply.sending = false;
                reply.error = Some(err);
              }
              return;
            }
          };
          view.set_github_threads(threads);
        }
      },
      cx,
    );
    cx.notify();
  }

  /// Builds the open reply boxes, so render can draw them.
  pub(super) fn ensure_reply_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let Some(pr) = self.pr.as_ref() else {
      return;
    };
    let missing: Vec<(String, String)> = pr
      .replies
      .iter()
      .filter(|(_, r)| r.open && r.input.is_none())
      .map(|(id, r)| (id.clone(), r.text.clone()))
      .collect();
    for (id, text) in missing {
      let state = cx.new(|cx| {
        TextareaState::new(window, cx)
          .rows(3)
          .placeholder("Reply on GitHub…")
          .default_value(text)
      });
      let thread = id.clone();
      let sub = cx.subscribe(&state, move |view, state, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
          let value = state.read(cx).value().to_string();
          if let Some(reply) = view.reply(&thread) {
            reply.text = value;
          }
          cx.notify();
        }
      });
      state.update(cx, |s, cx| s.focus(window, cx));
      if let Some(reply) = self.reply(&id) {
        reply.input = Some((state, sub));
      }
    }
  }

  /// `ThreadReply`: "Reply", or the box that posts one.
  pub(super) fn thread_reply(
    &self,
    thread: &GhReviewThread,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<AnyElement> {
    let pr = self.pr.as_ref()?;
    let reply = pr.replies.get(&thread.id);
    let muted = p.muted_foreground;
    let fg = p.foreground;
    let id = thread.id.clone();
    if !reply.is_some_and(|r| r.open) {
      return Some(
        ui::text(div(), TextSize::Xs)
          .id(SharedString::from(format!("reply-{id}")))
          .debug_selector(|| "github-thread-reply".into())
          .font_family(theme::SANS_FONT)
          .text_color(muted)
          .cursor_pointer()
          .hover(move |s| s.text_color(fg))
          .child("Reply")
          .on_click(cx.listener(move |view, _, _, cx| view.open_reply(&id, true, cx)))
          .into_any_element(),
      );
    }
    let reply = reply?;
    let disabled = reply.sending || reply.text.trim().is_empty();
    let (send, close, cancel) = (id.clone(), id.clone(), id.clone());
    Some(
      div()
        .debug_selector(|| "github-thread-reply-box".into())
        .flex()
        .flex_col()
        .gap_1p5()
        .font_family(theme::SANS_FONT)
        .capture_key_down(cx.listener(move |view, e: &KeyDownEvent, _, cx| {
          let k = &e.keystroke;
          if ui::is_escape(k) {
            cx.stop_propagation();
            view.open_reply(&close, false, cx);
          } else if k.key == "enter"
            && (crate::gpui_shell::keys::held(k.modifiers).platform
              || crate::gpui_shell::keys::held(k.modifiers).control)
          {
            cx.stop_propagation();
            view.send_reply(&send, cx);
          }
        }))
        .children(
          reply
            .input
            .as_ref()
            .map(|(state, _)| ui::text(Textarea::new(state), TextSize::Xs).min_h(zpx(60.))),
        )
        .children(reply.error.clone().map(|err| {
          ui::text(div(), TextSize::Xs)
            .text_color(p.destructive)
            .child(err)
        }))
        .child(
          div()
            .flex()
            .justify_end()
            .gap_2()
            .child(
              ui::text(div(), TextSize::Xs)
                .id(SharedString::from(format!("reply-cancel-{id}")))
                .text_color(muted)
                .cursor_pointer()
                .hover(move |s| s.text_color(fg))
                .child("Cancel")
                .on_click(cx.listener(move |view, _, _, cx| view.open_reply(&cancel, false, cx))),
            )
            .child({
              ui::text(div(), TextSize::Xs)
                .id(SharedString::from(format!("reply-send-{id}")))
                .debug_selector(|| "github-thread-reply-send".into())
                .rounded(radius::SM)
                .bg(p.primary)
                .text_color(p.primary_foreground)
                .px_2()
                .py_1()
                .when(disabled, |this| this.opacity(0.5))
                .when(!disabled, |this| {
                  this
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _, _, cx| view.send_reply(&id, cx)))
                })
                .child("Send reply")
            }),
        )
        .into_any_element(),
    )
  }
}
