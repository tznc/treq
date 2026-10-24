//! The workspace's commits, for its Commits tab.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{h_flex, tag::Tag, v_flex, ActiveTheme},
  *,
};
use treq_lib::{core::remote::TreqCommandRequest, jj::JjLogResult};

use super::workspace::Target;
use crate::{
  remote,
  ui::{load, widgets, Load},
};

pub struct CommitsScreen {
  target: Target,
  log: Load<JjLogResult>,
  scroll: ScrollHandle,
}

impl CommitsScreen {
  pub fn new(target: Target, cx: &mut Context<Self>) -> Self {
    // Its workspace screen reloads it (on a new epoch too), so it doesn't
    // watch the epoch itself. The log is usually prefetched (see
    // [`prefetch`]), or shown from the last visit.
    let log = remote::last(&request(&target)).map_or(Load::Idle, Load::Ready);
    let mut this = Self {
      target,
      log,
      scroll: ScrollHandle::new(),
    };
    this.fetch(true, cx);
    this
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
  }

  /// `join` as in [`remote::read_cached`]. Keeps the commits on screen
  /// while they're read again.
  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    if self.log.ready().is_none() {
      self.log = Load::Loading;
    }
    load::run(
      cx,
      remote::read_cached::<JjLogResult>(request(&self.target), join),
      |this, result, _| this.log = Load::from_result(result),
    );
  }
}

fn request(target: &Target) -> TreqCommandRequest {
  TreqCommandRequest::ListCommits {
    repo: target.repo.clone(),
    workspace: Some(target.id.clone()),
  }
}

/// Starts reading `target`'s commits before its tab is first shown.
pub fn prefetch(target: &Target) {
  if let Ok(endpoint) = crate::backend::get().endpoint() {
    remote::prefetch_on::<JjLogResult>(endpoint, request(target));
  }
}

impl Render for CommitsScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let body = match &self.log {
      Load::Idle | Load::Loading => {
        widgets::card(cx).child(widgets::centered_message("Loading commits…", cx))
      }
      Load::Failed(error) => widgets::card(cx)
        .p_4()
        .child(widgets::error_text(error.clone(), cx)),
      Load::Ready(log) if log.commits.is_empty() => {
        widgets::card(cx).child(widgets::centered_message("No commits.", cx))
      }
      Load::Ready(log) => widgets::card(cx).children(log.commits.iter().map(|commit| {
        let description = commit.description.lines().next().unwrap_or("").to_string();
        v_flex()
          .px_4()
          .py_3()
          .gap_1()
          .border_b_1()
          .border_color(cx.theme().border)
          .when(commit.on_target_only, |c| c.opacity(0.6))
          .child(
            h_flex()
              .gap_2()
              .items_center()
              .child(widgets::mono(commit.short_id.clone(), cx).text_color(cx.theme().primary))
              .when(commit.is_working_copy, |c| c.child(Tag::info().child("@")))
              .when(commit.has_conflicts, |c| {
                c.child(Tag::danger().child("conflict"))
              })
              .children(
                commit
                  .bookmarks
                  .iter()
                  .map(|b| Tag::secondary().child(b.clone())),
              ),
          )
          .child(div().child(if description.is_empty() {
            "(no description)".to_string()
          } else {
            description
          }))
          .child(widgets::muted(
            format!(
              "{} · {} · +{} −{}",
              commit.author_name, commit.timestamp, commit.insertions, commit.deletions
            ),
            cx,
          ))
      })),
    };
    div()
      .id("commits")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(widgets::page().child(body))
  }
}
