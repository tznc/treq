//! The workspace's commits.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{h_flex, tag::Tag, v_flex, ActiveTheme},
  *,
};
use treq_lib::{core::remote::TreqCommandRequest, jj::JjLogResult};

use super::{on_epoch, workspace::Target};
use crate::{
  remote,
  ui::{load, nav, widgets, Load, Location},
};

pub fn open(target: Target, cx: &mut App) {
  let location = Location {
    repo: Some(target.repo.clone()),
    workspace: Some(crate::store::SnapshotWorkspace {
      id: target.id.clone(),
      name: target.name.clone(),
    }),
  };
  nav::push("Commits", location, cx, move |_, cx| {
    cx.new(|cx| CommitsScreen::new(target, cx))
  });
}

pub struct CommitsScreen {
  target: Target,
  log: Load<JjLogResult>,
  scroll: ScrollHandle,
}

impl CommitsScreen {
  fn new(target: Target, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let mut this = Self {
      target,
      log: Load::Idle,
      scroll: ScrollHandle::new(),
    };
    this.reload(cx);
    this
  }

  fn reload(&mut self, cx: &mut Context<Self>) {
    self.log = Load::Loading;
    let request = TreqCommandRequest::ListCommits {
      repo: self.target.repo.clone(),
      workspace: Some(self.target.id.clone()),
    };
    load::run(
      cx,
      remote::read::<JjLogResult>(request),
      |this, result, _| this.log = Load::from_result(result),
    );
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
    let header = self.log.ready().map(|log| {
      widgets::muted(
        format!("{} → {}", log.workspace_branch, log.target_branch),
        cx,
      )
    });
    div()
      .id("commits")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(widgets::page().children(header).child(body))
  }
}
