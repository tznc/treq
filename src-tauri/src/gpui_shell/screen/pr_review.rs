//! `PrReviewScreen` / `PrReviewPage`: a GitHub pull request reviewed like a
//! workspace's Review tab. The header names the PR and offers an AI review
//! and the "Finish your review" popover that submits the drafts to GitHub;
//! the changed files list on the left; the diff is the Changes tab's
//! ([`ChangesView`] in its pull request mode).
//!
//! Drawn from the backend directly ([`pr_review_data`]). React still owns
//! leaving the screen (`prReview.close`) and the agent terminal an AI
//! review starts in (`prReview.startAgentReview`).
//!
//! [`pr_review_data`]: super::pr_review_data

use std::{
  cell::RefCell,
  rc::Rc,
  sync::Arc,
  time::{Duration, Instant},
};

use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use serde::Deserialize;
use serde_json::json;
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, point, prelude::*, px, App, Bounds, Context, Entity, FontWeight,
  KeyDownEvent, MouseButton, Pixels, SharedString, Subscription, Window,
};

use super::{
  changes::{pr::PrTarget, ChangesView},
  changes_data::{review_prompt_for, split_path},
  pr_review_data::{
    can_submit, comments_to_review, files_label, finish_text, review_summary, Event, PrReviewHost,
    OWN_PR_HINT, TARGET,
  },
  ActionSink, ScreenAction,
};
use crate::{
  github::GhPullRequest,
  gpui_shell::{
    native::{NativeEvent, ToastTone},
    popup::{PopupSink, Side, TooltipContent},
    theme::{self, tw, Palette},
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, Size, TextSize, Variant},
  },
};

/// What React sends: the repository and the pull request's number.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  pub pr_number: u64,
}

pub struct PrReviewView {
  host: Arc<dyn PrReviewHost>,
  react: ActionSink,
  popups: PopupSink,
  route: Route,
  dark: bool,
  tips: Tooltips,
  /// The repository's GitHub `owner/repo`; the screen stays empty until read.
  full_name: Option<String>,
  pr: Option<GhPullRequest>,
  /// The login `gh` is signed in as.
  viewer: Option<String>,
  pub(super) changes: Option<Entity<ChangesView>>,
  active: Option<String>,
  starting: bool,
  pub(super) finish_open: bool,
  /// When a press outside shut the popover: the press on its own button
  /// that shut it must not open it again.
  finish_closed_at: Option<Instant>,
  summary: String,
  summary_input: Option<(Entity<TextareaState>, Subscription)>,
  pub(super) event: Event,
  submitting: bool,
  /// Where the Finish review button was drawn.
  anchor: Rc<RefCell<Option<Bounds<Pixels>>>>,
  _observe: Option<Subscription>,
}

impl HasTooltips for PrReviewView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tips
  }
}

impl PrReviewView {
  pub fn new(
    route: Route,
    host: Arc<dyn PrReviewHost>,
    react: ActionSink,
    dark: bool,
    popups: PopupSink,
    cx: &mut Context<Self>,
  ) -> Self {
    let mut this = Self {
      host,
      react,
      tips: Tooltips::new(popups.clone(), dark),
      popups,
      route,
      dark,
      full_name: None,
      pr: None,
      viewer: None,
      changes: None,
      active: None,
      starting: false,
      finish_open: false,
      finish_closed_at: None,
      summary: String::new(),
      summary_input: None,
      event: Event::default(),
      submitting: false,
      anchor: Rc::default(),
      _observe: None,
    };
    this.load(cx);
    this
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      self.tips.dark = dark;
      if let Some(changes) = &self.changes {
        changes.update(cx, |c, cx| c.set_dark(dark, cx));
      }
    }
    if self.route != route {
      self.route = route;
      self.full_name = None;
      self.pr = None;
      self.changes = None;
      self._observe = None;
      self.active = None;
      self.finish_open = false;
      self.summary.clear();
      self.summary_input = None;
      self.event = Event::default();
      self.load(cx);
    }
    cx.notify();
  }

  fn run<T: Send + 'static>(
    &self,
    work: impl FnOnce(&dyn PrReviewHost) -> T + Send + 'static,
    apply: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let host = self.host.clone();
    let route = self.route.clone();
    cx.spawn(async move |this, cx| {
      let value = cx
        .background_spawn(async move { work(host.as_ref()) })
        .await;
      let _ = this.update(cx, |view, cx| {
        if view.route == route {
          apply(view, value, cx);
          cx.notify();
        }
      });
    })
    .detach();
  }

  fn toast(&self, tone: ToastTone, title: &str, description: impl Into<String>) {
    super::workspace_data::WorkspaceHost::notify(
      self.host.as_ref(),
      NativeEvent::Toast {
        title: title.into(),
        description: description.into(),
        tone,
        action: None,
      },
    );
  }

  fn send(&self, action: ScreenAction, cx: &mut App) {
    let react = self.react.clone();
    cx.defer(move |cx| react(action, cx));
  }

  /// The remote, then the PR and the diff under it.
  fn load(&mut self, cx: &mut Context<Self>) {
    let repo = self.route.repo_path.clone();
    self.run(
      move |host| host.remote_full_name(&repo),
      |view, full_name, cx| {
        let Some(full_name) = full_name else {
          return;
        };
        view.full_name = Some(full_name.clone());
        let target = PrTarget {
          repo_path: view.route.repo_path.clone(),
          full_name: full_name.clone(),
          number: view.route.pr_number,
        };
        let (host, react, popups, dark) = (
          view.host.clone(),
          view.react.clone(),
          view.popups.clone(),
          view.dark,
        );
        let changes = cx.new(|cx| ChangesView::new_pr(target, host, react, popups, dark, cx));
        view._observe = Some(cx.observe(&changes, |_, _, cx| cx.notify()));
        view.changes = Some(changes);
        let number = view.route.pr_number;
        view.run(
          move |host| host.view_pr(&full_name, number),
          |view, result, _| match result {
            Ok(pr) => view.pr = Some(pr),
            Err(err) => log::warn!("Failed to read pull request: {err}"),
          },
          cx,
        );
        view.run(
          |host| host.current_user(),
          |view, result, _| view.viewer = result.ok(),
          cx,
        );
      },
      cx,
    );
  }

  /// The PR's files, in diff order.
  fn files(&self, cx: &App) -> Vec<super::changes_data::ChangedFile> {
    self
      .changes
      .as_ref()
      .and_then(|c| c.read(cx).snapshot.as_ref().map(|s| s.files.clone()))
      .unwrap_or_default()
  }

  /// The drafts a review posts, and how many stay behind as outdated.
  fn drafts(&self, cx: &App) -> (Vec<super::changes_review::LineComment>, usize) {
    self
      .changes
      .as_ref()
      .and_then(|c| {
        c.read(cx)
          .pr_mode()
          .map(|pr| (pr.postable(), pr.outdated()))
      })
      .unwrap_or_default()
  }

  fn own_pr(&self) -> bool {
    matches!((&self.viewer, &self.pr), (Some(v), Some(pr)) if *v == pr.author.login)
  }

  fn start_agent_review(&mut self, cx: &mut Context<Self>) {
    let Some(full_name) = self.full_name.clone() else {
      return;
    };
    self.starting = true;
    let number = self.route.pr_number;
    let repo = self.route.repo_path.clone();
    let title = self
      .pr
      .as_ref()
      .map(|p| p.title.clone())
      .unwrap_or_default();
    let files = self.files(cx).len();
    self.run(
      move |host| {
        let keys = ["review_prompt".to_string(), "review_agent".to_string()];
        let settings = host.repo_settings(&repo, &keys).unwrap_or_default();
        let custom = settings.get("review_prompt").and_then(|v| v.clone());
        let agent = settings
          .get("review_agent")
          .and_then(|v| v.clone())
          .filter(|a| !a.is_empty());
        let summary = review_summary(&full_name, number, &title, files);
        let prompt = review_prompt_for(custom.as_deref(), TARGET, &number.to_string(), &summary);
        (prompt, agent)
      },
      |view, (prompt, agent), cx| {
        view.starting = false;
        view.send(
          ScreenAction::Field {
            field: "prReview.startAgentReview".into(),
            value: json!({ "prompt": prompt, "agent": agent }),
          },
          cx,
        );
      },
      cx,
    );
    cx.notify();
  }

  /// `handleSubmitReview`: posts the drafts that still match the diff, then
  /// resolves them; outdated ones stay for the user.
  fn submit(&mut self, cx: &mut Context<Self>) {
    let Some(full_name) = self.full_name.clone() else {
      return;
    };
    let (postable, _) = self.drafts(cx);
    if self.submitting || !can_submit(self.event, &self.summary, postable.len()) {
      return;
    }
    let (inline, file_comments) = comments_to_review(&postable);
    let body = [self.summary.trim().to_string(), file_comments]
      .into_iter()
      .filter(|s| !s.is_empty())
      .collect::<Vec<_>>()
      .join("\n\n");
    let (number, event, repo) = (
      self.route.pr_number,
      self.event,
      self.route.repo_path.clone(),
    );
    let ids: Vec<String> = postable.iter().map(|c| c.id.clone()).collect();
    self.submitting = true;
    self.run(
      move |host| {
        host.submit_review(&full_name, number, event.value(), &body, &inline)?;
        for id in &ids {
          host.resolve_agent_comment(&repo, id)?;
        }
        Ok::<(), String>(())
      },
      move |view, result, cx| {
        view.submitting = false;
        match result {
          Ok(()) => {
            view.summary.clear();
            view.summary_input = None;
            view.event = Event::default();
            view.finish_open = false;
            if let Some(changes) = &view.changes {
              changes.update(cx, |c, cx| {
                c.read_pr_threads(cx);
                c.read_drafts(cx);
              });
            }
            view.toast(
              ToastTone::Success,
              "Review submitted",
              format!("#{number}: {}", event.label()),
            );
          }
          Err(err) => view.toast(ToastTone::Error, "Failed to submit review", err),
        }
      },
      cx,
    );
    cx.notify();
  }

  fn show_file(&mut self, path: &str, cx: &mut Context<Self>) {
    self.active = Some(path.to_string());
    if let Some(changes) = &self.changes {
      changes.update(cx, |c, cx| c.scroll_to_path(path, cx));
    }
    cx.notify();
  }

  fn ensure_summary_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if !self.finish_open {
      self.summary_input = None;
      return;
    }
    if self.summary_input.is_some() {
      return;
    }
    let state = cx.new(|cx| {
      TextareaState::new(window, cx)
        .rows(3)
        .placeholder("Leave a comment")
        .default_value(self.summary.clone())
    });
    let sub = cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
      if let InputEvent::Change = event {
        view.summary = state.read(cx).value().to_string();
        cx.notify();
      }
    });
    state.update(cx, |s, cx| s.focus(window, cx));
    self.summary_input = Some((state, sub));
  }
}

impl Render for PrReviewView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    self.ensure_summary_input(window, cx);
    let p = theme::palette(self.dark);
    let page = div()
      .debug_selector(|| "pr-review-page".into())
      .size_full()
      .bg(p.background)
      .text_color(p.foreground);
    let Some(changes) = self.changes.clone() else {
      return page;
    };
    let header = self.header(&p, cx);
    let nav = self.nav(&p, cx);
    let popover = self.finish_popover(&p, cx);
    page
      .flex()
      .flex_col()
      .min_h_0()
      .child(header)
      .child(
        div()
          .flex()
          .flex_1()
          .min_h_0()
          .overflow_hidden()
          .child(nav)
          .child(div().flex_1().flex().flex_col().min_w_0().child(changes)),
      )
      .children(popover)
  }
}

impl PrReviewView {
  fn header(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let number = self.route.pr_number;
    let files = self.files(cx);
    let (postable, _) = self.drafts(cx);
    let muted = p.muted_foreground;
    let title = self
      .pr
      .as_ref()
      .map_or_else(|| format!("Pull request #{number}"), |pr| pr.title.clone());
    let mono = |text: String| div().font_family(theme::MONO_FONT).child(text);
    let subtitle = self.pr.as_ref().map(|pr| {
      ui::text(div(), TextSize::Xs)
        .debug_selector(|| "pr-review-subtitle".into())
        .flex()
        .gap(px(4.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(muted)
        .child(format!("{} wants to merge", pr.author.login))
        .child(mono(pr.head_ref_name.clone()))
        .child("into")
        .child(mono(pr.base_ref_name.clone()))
        .child(format!("· {}", files_label(files.len())))
    });
    let open = self.pr.as_ref().map(|pr| {
      let (host, url) = (self.host.clone(), pr.url.clone());
      Button::new("pr-review-open-github", p)
        .variant(Variant::Ghost)
        .size(Size::Icon)
        .w(px(32.))
        .h(px(32.))
        .icon(theme::ICON_EXTERNAL_LINK, 16.)
        .on_click(move |_, _, _| host.open_url(&url))
    });
    let anchor = self.anchor.clone();
    let mut finish = String::from("Finish review");
    if !postable.is_empty() {
      finish.push_str(&format!(" ({})", postable.len()));
    }
    div()
      .flex()
      .flex_none()
      .items_center()
      .gap_3()
      .px_4()
      .py_2()
      .border_b_1()
      .border_color(p.border)
      .child(
        Button::new("pr-review-back", p)
          .variant(Variant::Ghost)
          .size(Size::Icon)
          .w(px(32.))
          .h(px(32.))
          .icon(theme::ICON_ARROW_LEFT, 16.)
          .on_click(cx.listener(|view, _, _, cx| {
            view.send(
              ScreenAction::Press {
                id: "prReview.close".into(),
              },
              cx,
            )
          })),
      )
      .child(ui::icon(theme::ICON_PR, 16., tw::color(tw::EMERALD_500)).flex_none())
      .child(
        div()
          .min_w_0()
          .flex_1()
          .child(
            ui::text(div(), TextSize::Sm)
              .debug_selector(|| "pr-review-title".into())
              .flex()
              .gap(px(4.))
              .overflow_hidden()
              .whitespace_nowrap()
              .font_weight(FontWeight::MEDIUM)
              .child(div().truncate().child(title))
              .child(
                div()
                  .flex_none()
                  .text_color(muted)
                  .child(format!("#{number}")),
              ),
          )
          .children(subtitle),
      )
      .children(open)
      .child(
        Button::new("pr-review-ai-review", p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .gap_2()
          .disabled(self.starting || files.is_empty())
          .icon(theme::ICON_BOT, 12.)
          .label("AI review")
          .on_click(cx.listener(|view, _, _, cx| view.start_agent_review(cx))),
      )
      .child(
        div()
          .relative()
          .child(
            Button::new("pr-review-finish", p)
              .size(Size::Sm)
              .gap_2()
              .icon(theme::ICON_SEND, 12.)
              .label(finish)
              .on_click(cx.listener(|view, _, _, cx| {
                let just_closed = view
                  .finish_closed_at
                  .take()
                  .is_some_and(|at| at.elapsed() < Duration::from_millis(300));
                view.finish_open = !view.finish_open && !just_closed;
                cx.notify();
              })),
          )
          .child(
            gpui::canvas(
              move |b, _, _| {
                *anchor.borrow_mut() = Some(b);
              },
              |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
          ),
      )
  }

  /// The changed files (`nav aria-label="Changed files"`).
  fn nav(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let mut nav = div()
      .id("pr-review-files")
      .debug_selector(|| "pr-review-files".into())
      .w_64()
      .flex_none()
      .border_r_1()
      .border_color(p.border)
      .overflow_y_scroll()
      .py_2();
    let accent = p.accent;
    for file in self.files(cx) {
      let (name, _) = split_path(&file.path);
      let path = file.path.clone();
      let active = self.active.as_deref() == Some(file.path.as_str());
      let row = ui::text(div(), TextSize::Xs)
        .id(SharedString::from(format!("pr-file-{}", file.path)))
        .debug_selector({
          let id = format!("pr-file-{}", file.path);
          move || id
        })
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .cursor_pointer()
        .hover(move |s| s.bg(accent))
        .when(active, |this| this.bg(accent))
        .child(ui::icon(theme::ICON_FILE_TEXT, 12., p.muted_foreground).flex_none())
        .child(div().truncate().flex_1().child(name))
        .child(
          div()
            .text_size(px(10.))
            .text_color(p.muted_foreground)
            .child(file.status.clone()),
        )
        .on_click(cx.listener(move |view, _, _, cx| view.show_file(&path, cx)));
      let tip = file.path.clone();
      nav = nav.child(self.tips.wrap(
        format!("pr-file-tip-{}", file.path),
        Side::Bottom,
        Some(TooltipContent::Text(tip.into())),
        row,
        cx,
      ));
    }
    nav
  }

  /// The "Finish your review" popover, below its button and right-aligned.
  fn finish_popover(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    if !self.finish_open {
      return None;
    }
    let anchor = (*self.anchor.borrow())?;
    let width = 384.;
    let at = point(anchor.right() - px(width), anchor.bottom() + px(4.));
    let (postable, outdated) = self.drafts(cx);
    let own = self.own_pr();
    let mut radios = div()
      .debug_selector(|| "pr-review-events".into())
      .flex()
      .flex_col()
      .gap_2();
    for event in Event::ALL {
      let checked = self.event == event;
      let disabled = own && event != Event::Comment;
      let dot = div()
        .mt_1()
        .flex_none()
        .size(px(13.))
        .rounded_full()
        .border_1()
        .border_color(if checked {
          p.primary
        } else {
          p.muted_foreground
        })
        .flex()
        .items_center()
        .justify_center()
        .when(checked, |this| {
          this.child(div().size(px(7.)).rounded_full().bg(p.primary))
        });
      radios = radios.child(
        ui::text(div(), TextSize::Sm)
          .id(SharedString::from(format!(
            "pr-review-event-{}",
            event.value()
          )))
          .debug_selector({
            let id = format!("pr-review-event-{}", event.value());
            move || id
          })
          .flex()
          .items_start()
          .gap_2()
          .when(!disabled, |this| {
            this
              .cursor_pointer()
              .on_click(cx.listener(move |view, _, _, cx| {
                view.event = event;
                cx.notify();
              }))
          })
          .child(dot.when(disabled, |this| this.opacity(0.5)))
          .child(
            div()
              .child(div().font_weight(FontWeight::MEDIUM).child(event.label()))
              .child(
                ui::text(div(), TextSize::Xs)
                  .text_color(p.muted_foreground)
                  .child(if disabled { OWN_PR_HINT } else { event.hint() }),
              ),
          ),
      );
    }
    let enabled = !self.submitting && can_submit(self.event, &self.summary, postable.len());
    let panel = div()
      .id("pr-review-popover")
      .debug_selector(|| "pr-review-popover".into())
      .w(px(width))
      .p_4()
      .rounded(px(8.))
      .border_1()
      .border_color(p.border)
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_md()
      .occlude()
      .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .on_mouse_down_out(cx.listener(|view, _, _, cx| {
        view.finish_open = false;
        view.finish_closed_at = Some(Instant::now());
        cx.notify();
      }))
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        if e.keystroke.key == "escape" {
          cx.stop_propagation();
          view.finish_open = false;
          cx.notify();
        }
      }))
      .flex()
      .flex_col()
      .gap_3()
      .child(
        div()
          .child(
            ui::text(div(), TextSize::Sm)
              .font_weight(FontWeight::MEDIUM)
              .mb_1()
              .child("Finish your review"),
          )
          .child(
            ui::text(div(), TextSize::Sm)
              .debug_selector(|| "pr-review-pending".into())
              .text_color(p.muted_foreground)
              .child(finish_text(postable.len(), outdated)),
          ),
      )
      .children(
        self
          .summary_input
          .as_ref()
          .map(|(state, _)| ui::text(Textarea::new(state), TextSize::Sm).min_h(px(80.))),
      )
      .child(radios)
      .child(
        div().flex().justify_end().child(
          Button::new("pr-review-submit", p)
            .size(Size::Sm)
            .disabled(!enabled)
            .label("Submit review")
            .on_click(cx.listener(|view, _, _, cx| view.submit(cx))),
        ),
      );
    Some(deferred(anchored().position(at).child(panel)).with_priority(1))
  }
}
