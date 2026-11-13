//! The Save project dialog: names the window's project and saves it
//! (`core::projects`). Closing a window on an unsaved project of several
//! repositories opens it with "Don't save", so the window can still close.

use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, InputState};
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  div, prelude::*, Context, Entity, FocusHandle, FontWeight, Subscription, Window,
};

use super::{
  workspace_data::WorkspaceHost, workspace_dialog_data::WorkspaceDialogHost,
  workspace_modal::press, ActionSink,
};
use crate::gpui_shell::{
  native::{NativeEvent, ToastTone},
  theme,
  ui::{self, Button, TextSize, Variant},
};

/// What the window model sends for the dialog (`snapshots::workspace_modal`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProjectRoute {
  pub repo_path: String,
  /// The name to start with: the saved name, or one made from the
  /// repository folders.
  pub name: String,
  /// The project is saved already, so this renames it.
  pub saved: bool,
  /// The window is closing and waits for this dialog.
  pub closing: bool,
  pub repos: Vec<String>,
}

pub struct SaveProjectDialog {
  route: SaveProjectRoute,
  host: Arc<dyn WorkspaceDialogHost>,
  sink: ActionSink,
  dark: bool,
  focus: FocusHandle,
  input: Option<Entity<InputState>>,
  pub(super) name: String,
  pub(super) pending: bool,
  pub(super) error: String,
  pub(super) dismissed: bool,
  _subscription: Option<Subscription>,
}

impl SaveProjectDialog {
  pub fn new(
    route: SaveProjectRoute,
    host: Arc<dyn WorkspaceDialogHost>,
    sink: ActionSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    Self {
      name: route.name.clone(),
      route,
      host,
      sink,
      dark,
      focus: cx.focus_handle(),
      input: None,
      pending: false,
      error: String::new(),
      dismissed: false,
      _subscription: None,
    }
  }

  pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      cx.notify();
    }
  }

  pub(super) fn can_submit(&self) -> bool {
    !self.name.trim().is_empty() && !self.pending
  }

  fn close(&mut self, cx: &mut Context<Self>) {
    self.dismissed = true;
    press(&self.sink, "saveProject.close", cx);
    cx.notify();
  }

  /// "Don't save": the window closes and the project stays unsaved.
  fn discard(&mut self, cx: &mut Context<Self>) {
    self.close(cx);
    self.host.close_window();
  }

  fn submit(&mut self, cx: &mut Context<Self>) {
    if !self.can_submit() {
      return;
    }
    let name = self.name.trim().to_string();
    self.pending = true;
    cx.notify();
    let host = self.host.clone();
    let closing = self.route.closing;
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn({
          let (host, name) = (host.clone(), name.clone());
          async move { host.save_project(&name) }
        })
        .await;
      this
        .update(cx, |view, cx| {
          view.pending = false;
          match result {
            Ok(()) => {
              WorkspaceHost::notify(
                &*view.host,
                NativeEvent::Toast {
                  title: format!("Saved project {name}"),
                  description: String::new(),
                  tone: ToastTone::Success,
                  action: None,
                },
              );
              view.dismissed = true;
              press(&view.sink, "saveProject.saved", cx);
              if closing {
                view.host.close_window();
              }
            }
            Err(err) => view.error = err,
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
    if let Some(input) = &self.input {
      return input.clone();
    }
    let input = cx.new(|cx| {
      InputState::new(window, cx)
        .placeholder("e.g., Checkout revamp")
        .default_value(self.name.clone())
    });
    input.update(cx, |input, cx| input.focus(window, cx));
    self._subscription = Some(cx.subscribe(
      &input,
      |view, input, event: &InputEvent, cx| match event {
        InputEvent::Change => {
          view.name = input.read(cx).value().to_string();
          view.error.clear();
          cx.notify();
        }
        InputEvent::PressEnter { .. } => view.submit(cx),
        _ => {}
      },
    ));
    self.input = Some(input.clone());
    input
  }
}

/// The folder name of a repository path.
fn folder(path: &str) -> &str {
  path
    .trim_end_matches(['/', '\\'])
    .rsplit(['/', '\\'])
    .next()
    .unwrap_or(path)
}

impl Render for SaveProjectDialog {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.dismissed {
      return div().into_any_element();
    }
    let p = theme::palette(self.dark);
    let input = self.input(window, cx);
    let pending = self.pending;
    let can_submit = self.can_submit();
    let (title, description) = match (self.route.saved, self.route.closing) {
      (true, _) => ("Rename project", "Projects open in their own window."),
      (false, true) => (
        "Save this project before closing?",
        "Name it to reopen these repositories together from the project menu or the welcome page.",
      ),
      (false, false) => (
        "Save project",
        "Name it to reopen these repositories together from the project menu or the welcome page.",
      ),
    };
    let repos = self
      .route
      .repos
      .iter()
      .map(|r| folder(r).to_string())
      .collect::<Vec<_>>()
      .join(", ");
    let mut footer = Vec::new();
    if self.route.closing {
      footer.push(
        Button::new("save-project-discard", &p)
          .variant(Variant::Outline)
          .disabled(pending)
          .label("Don't save")
          .on_click(cx.listener(|view, _, _, cx| view.discard(cx)))
          .into_any_element(),
      );
    }
    footer.push(
      Button::new("save-project-submit", &p)
        .disabled(!can_submit)
        .label(match (pending, self.route.saved) {
          (true, _) => "Saving…",
          (false, true) => "Rename",
          (false, false) => "Save project",
        })
        .on_click(cx.listener(|view, _, _, cx| view.submit(cx)))
        .into_any_element(),
    );
    ui::Dialog::new("save-project-dialog", &p)
      .overlay(window.viewport_size())
      .width(480.)
      .title(title)
      .description(description)
      .focus(&self.focus)
      .on_close(ui::handler(cx, |view, _, cx| view.close(cx)))
      .on_primary(ui::handler(cx, |view, _, cx| view.submit(cx)))
      .primary_disabled(!can_submit)
      .child(
        div()
          .flex()
          .flex_col()
          .gap_3()
          .py_4()
          .child(
            ui::text(div(), TextSize::Sm)
              .font_weight(FontWeight::MEDIUM)
              .child("Project name"),
          )
          .child(
            div()
              .id("save-project-name")
              .debug_selector(|| "save-project-name".into())
              .w_full()
              .child(Styled::h(ui::input(&input).disabled(pending), zpx(40.))),
          )
          .child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child(format!("Repositories: {repos}")),
          )
          .when(!self.error.is_empty(), |this| {
            this.child(
              ui::text(div(), TextSize::Sm)
                .debug_selector(|| "save-project-error".into())
                .text_color(p.destructive)
                .child(self.error.clone()),
            )
          }),
      )
      .footer(footer)
      .into_any_element()
  }
}

#[cfg(test)]
mod tests {
  use super::folder;

  #[test]
  fn folder_names_the_last_path_part() {
    assert_eq!(folder("/r/web/"), "web");
    assert_eq!(folder("C:\\code\\api"), "api");
  }
}
