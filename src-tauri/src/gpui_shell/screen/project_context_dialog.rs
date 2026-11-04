//! The Project instructions dialog: the prompt every agent in the project's
//! windows starts with, and the files, folders or URLs they may read
//! (`core::projects::set_context`).

use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContextRoute {
  pub repo_path: String,
  pub name: String,
  pub prompt: String,
  pub context_paths: Vec<String>,
}

/// One context path per line, trimmed, blank lines dropped.
pub(super) fn parse_paths(text: &str) -> Vec<String> {
  text
    .lines()
    .map(str::trim)
    .filter(|l| !l.is_empty())
    .map(str::to_string)
    .collect()
}

pub struct ProjectContextDialog {
  route: ProjectContextRoute,
  host: Arc<dyn WorkspaceDialogHost>,
  sink: ActionSink,
  dark: bool,
  focus: FocusHandle,
  fields: Option<(Entity<TextareaState>, Entity<TextareaState>)>,
  pub(super) prompt: String,
  pub(super) paths: String,
  pub(super) pending: bool,
  pub(super) error: String,
  pub(super) dismissed: bool,
  _subscriptions: Vec<Subscription>,
}

impl ProjectContextDialog {
  pub fn new(
    route: ProjectContextRoute,
    host: Arc<dyn WorkspaceDialogHost>,
    sink: ActionSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    Self {
      prompt: route.prompt.clone(),
      paths: route.context_paths.join("\n"),
      route,
      host,
      sink,
      dark,
      focus: cx.focus_handle(),
      fields: None,
      pending: false,
      error: String::new(),
      dismissed: false,
      _subscriptions: Vec::new(),
    }
  }

  pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      cx.notify();
    }
  }

  fn close(&mut self, cx: &mut Context<Self>) {
    self.dismissed = true;
    press(&self.sink, "projectContext.close", cx);
    cx.notify();
  }

  fn submit(&mut self, cx: &mut Context<Self>) {
    if self.pending {
      return;
    }
    self.pending = true;
    cx.notify();
    let (host, prompt, paths) = (
      self.host.clone(),
      self.prompt.clone(),
      parse_paths(&self.paths),
    );
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn({
          let host = host.clone();
          async move { host.set_project_context(&prompt, &paths) }
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
                  title: "Saved project instructions".into(),
                  description: "Agents started from now on get them.".into(),
                  tone: ToastTone::Success,
                  action: None,
                },
              );
              view.close(cx);
            }
            Err(err) => view.error = err,
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn fields(
    &mut self,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> (Entity<TextareaState>, Entity<TextareaState>) {
    if let Some(fields) = &self.fields {
      return fields.clone();
    }
    let area =
      |value: &str, placeholder: &str, rows: usize, window: &mut Window, cx: &mut Context<Self>| {
        let (value, placeholder) = (value.to_string(), placeholder.to_string());
        cx.new(|cx| {
          TextareaState::new(window, cx)
            .auto_grow(rows, 12)
            .placeholder(placeholder)
            .default_value(value)
        })
      };
    let prompt = area(
      &self.prompt,
      "e.g., Use pnpm. Keep the API and web client in sync.",
      4,
      window,
      cx,
    );
    let paths = area(
      &self.paths,
      "One per line: a file, a folder or a URL",
      3,
      window,
      cx,
    );
    prompt.update(cx, |input, cx| input.focus(window, cx));
    self._subscriptions.push(
      cx.subscribe(&prompt, |view, input, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
          view.prompt = input.read(cx).value().to_string();
          cx.notify();
        }
      }),
    );
    self
      ._subscriptions
      .push(cx.subscribe(&paths, |view, input, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
          view.paths = input.read(cx).value().to_string();
          cx.notify();
        }
      }));
    self.fields = Some((prompt.clone(), paths.clone()));
    (prompt, paths)
  }
}

impl Render for ProjectContextDialog {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.dismissed {
      return div().into_any_element();
    }
    let p = theme::palette(self.dark);
    let (prompt, paths) = self.fields(window, cx);
    let pending = self.pending;
    let label = |text: &'static str| {
      ui::text(div(), TextSize::Sm)
        .font_weight(FontWeight::MEDIUM)
        .child(text)
    };
    ui::Dialog::new("project-context-dialog", &p)
      .overlay(window.viewport_size())
      .width(560.)
      .title(format!("{} instructions", self.route.name))
      .description("Every agent started in this project's windows gets these instructions and may read these paths.")
      .focus(&self.focus)
      .on_close(ui::handler(cx, |view, _, cx| view.close(cx)))
      .child(
        div()
          .flex()
          .flex_col()
          .gap_3()
          .py_4()
          .child(label("Instructions"))
          .child(
            div()
              .debug_selector(|| "project-context-prompt".into())
              .min_h(zpx(90.))
              .child(ui::text(Textarea::new(&prompt), TextSize::Sm)),
          )
          .child(label("Context files and links"))
          .child(
            div()
              .debug_selector(|| "project-context-paths".into())
              .min_h(zpx(70.))
              .child(ui::text(Textarea::new(&paths), TextSize::Sm)),
          )
          .when(!self.error.is_empty(), |this| {
            this.child(
              ui::text(div(), TextSize::Sm)
                .text_color(p.destructive)
                .child(self.error.clone()),
            )
          }),
      )
      .footer([
        Button::new("project-context-cancel", &p)
          .variant(Variant::Outline)
          .disabled(pending)
          .label("Cancel")
          .on_click(cx.listener(|view, _, _, cx| view.close(cx)))
          .into_any_element(),
        Button::new("project-context-save", &p)
          .disabled(pending)
          .label(if pending { "Saving…" } else { "Save" })
          .on_click(cx.listener(|view, _, _, cx| view.submit(cx)))
          .into_any_element(),
      ])
      .into_any_element()
  }
}

#[cfg(test)]
mod tests {
  use super::parse_paths;

  #[test]
  fn parses_one_path_per_line() {
    assert_eq!(
      parse_paths(" /docs/a.md\n\n https://x.dev \n"),
      ["/docs/a.md", "https://x.dev"]
    );
  }
}
