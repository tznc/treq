//! Dashboard's `AlertDialog` that confirms moving files dropped on another
//! workspace, and the move itself.

use crate::result_ext::ResultExt;
use std::sync::Arc;

use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  anchored, deferred, div, point, prelude::*, px, Context, MouseButton, Window,
};

use super::{
  workspace_data::WorkspaceHost,
  workspace_dialog_data::WorkspaceDialogHost,
  workspace_modal::{dialog_description, dialog_header, dialog_panel, press},
  ActionSink,
};
use crate::{
  core::WorkspaceMoveRequest,
  gpui_shell::{
    native::{NativeEvent, ToastTone},
    theme,
    ui::{Button, Variant},
  },
};

/// Dashboard's `pendingChangeMove`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeMoveRoute {
  pub repo_path: String,
  pub files: Vec<String>,
  pub source_branch: String,
  pub destination_branch: String,
  pub destination_label: String,
}

impl ChangeMoveRoute {
  pub fn title(&self) -> String {
    let n = self.files.len();
    format!(
      "Move {n} {} to {}?",
      if n == 1 { "file" } else { "files" },
      self.destination_label
    )
  }

  pub fn description(&self) -> String {
    let one = self.files.len() == 1;
    format!(
      "This moves the selected uncommitted {} into the target workspace and removes {} from the \
       current one.",
      if one { "change" } else { "changes" },
      if one { "it" } else { "them" }
    )
  }
}

pub struct ChangeMoveDialog {
  route: ChangeMoveRoute,
  host: Arc<dyn WorkspaceDialogHost>,
  sink: ActionSink,
  dark: bool,
  /// `changeMovePending`.
  pub(super) pending: bool,
  pub(super) dismissed: bool,
}

impl ChangeMoveDialog {
  pub fn new(
    route: ChangeMoveRoute,
    host: Arc<dyn WorkspaceDialogHost>,
    sink: ActionSink,
    dark: bool,
    _: &mut Context<Self>,
  ) -> Self {
    Self {
      route,
      host,
      sink,
      dark,
      pending: false,
      dismissed: false,
    }
  }

  pub fn set_route(&mut self, route: ChangeMoveRoute, dark: bool, cx: &mut Context<Self>) {
    if self.route != route || self.dark != dark {
      self.route = route;
      self.dark = dark;
      cx.notify();
    }
  }

  fn cancel(&mut self, cx: &mut Context<Self>) {
    if self.pending {
      return;
    }
    self.dismissed = true;
    press(&self.sink, "changeMove.cancel", cx);
    cx.notify();
  }

  /// `handleConfirmChangeMove`.
  fn confirm(&mut self, cx: &mut Context<Self>) {
    if self.pending {
      return;
    }
    self.pending = true;
    cx.notify();
    let (host, route) = (self.host.clone(), self.route.clone());
    cx.spawn(async move |this, cx| {
      let moved = cx
        .background_spawn({
          let host = host.clone();
          let route = route.clone();
          async move {
            host.move_changes(
              &route.repo_path,
              &route.source_branch,
              &route.destination_branch,
              WorkspaceMoveRequest {
                files: route.files.clone(),
                ..Default::default()
              },
            )
          }
        })
        .await;
      let (tone, title, description) = match &moved {
        Ok(()) => (
          ToastTone::Success,
          "Files moved",
          format!(
            "Moved {} file(s) to {}",
            route.files.len(),
            route.destination_label
          ),
        ),
        Err(err) => (ToastTone::Error, "Failed to move files", err.clone()),
      };
      WorkspaceHost::notify(
        &*host,
        NativeEvent::Toast {
          title: title.into(),
          description,
          tone,
          action: None,
        },
      );
      this
        .update(cx, |view, cx| {
          view.pending = false;
          if moved.is_ok() {
            view.dismissed = true;
            press(&view.sink, "changeMove.done", cx);
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }
}

impl Render for ChangeMoveDialog {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.dismissed {
      return div().into_any_element();
    }
    let p = theme::palette(self.dark);
    let (backdrop, panel) = dialog_panel("change-move-dialog", 512., window.viewport_size(), &p);
    let pending = self.pending;
    let panel = panel
      .child(dialog_header(
        self.route.title(),
        dialog_description(self.route.description(), &p),
      ))
      .child(
        div()
          .mt_4()
          .flex()
          .justify_end()
          .gap_2()
          .child(
            Button::new("change-move-cancel", &p)
              .variant(Variant::Ghost)
              .disabled(pending)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.cancel(cx))),
          )
          .child(
            Button::new("change-move-confirm", &p)
              .disabled(pending)
              .label("Move")
              .on_click(cx.listener(|view, _, _, cx| view.confirm(cx))),
          ),
      );
    deferred(
      anchored().position(point(px(0.), px(0.))).child(
        backdrop
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _, _, cx| view.cancel(cx)),
          )
          .child(
            panel
              .id("change-move-panel")
              .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
          ),
      ),
    )
    .with_priority(2)
    .into_any_element()
  }
}
