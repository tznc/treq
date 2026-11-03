//! `RenameWorkspaceDialog`: renames a workspace's branch, checking each
//! typed name with a dry run first.

use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{sync::Arc, time::Duration};

use gpui_kit::component::input::{InputEvent, InputState};
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  div, prelude::*, Context, Entity, FocusHandle, FontWeight, Subscription, Task, Window,
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

/// `useDebounce(branchName, 500)`.
const DEBOUNCE: Duration = Duration::from_millis(500);

/// Dashboard's `renameTarget`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRoute {
  pub repo_path: String,
  pub workspace_id: i64,
  /// The workspace's branch now.
  pub branch_name: String,
}

/// A dry run's answer for `name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
  pub name: String,
  pub success: bool,
  pub message: String,
}

pub struct RenameDialog {
  route: RenameRoute,
  host: Arc<dyn WorkspaceDialogHost>,
  sink: ActionSink,
  dark: bool,
  focus: FocusHandle,
  input: Option<Entity<InputState>>,
  pub(super) name: String,
  /// Whether a dry run is in flight.
  pub(super) running: bool,
  pub(super) checked: Option<Checked>,
  pub(super) pending: bool,
  pub(super) error: String,
  pub(super) dismissed: bool,
  _check: Option<Task<()>>,
  _subscription: Option<Subscription>,
}

impl RenameDialog {
  pub fn new(
    route: RenameRoute,
    host: Arc<dyn WorkspaceDialogHost>,
    sink: ActionSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    Self {
      name: route.branch_name.clone(),
      route,
      host,
      sink,
      dark,
      focus: cx.focus_handle(),
      input: None,
      running: false,
      checked: None,
      pending: false,
      error: String::new(),
      dismissed: false,
      _check: None,
      _subscription: None,
    }
  }

  pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      cx.notify();
    }
  }

  fn trimmed(&self) -> &str {
    self.name.trim()
  }

  /// The dry run for the name typed now; one for an earlier name says
  /// nothing about it.
  pub(super) fn validation(&self) -> Option<&Checked> {
    self.checked.as_ref().filter(|c| c.name == self.trimmed())
  }

  pub(super) fn is_checking(&self) -> bool {
    let name = self.trimmed();
    self.running
      || (!name.is_empty() && name != self.route.branch_name && self.validation().is_none())
  }

  pub(super) fn can_submit(&self) -> bool {
    let name = self.trimmed();
    !name.is_empty()
      && name != self.route.branch_name
      && !self.is_checking()
      && self.validation().is_some_and(|c| c.success)
      && !self.pending
  }

  fn set_name(&mut self, name: String, cx: &mut Context<Self>) {
    self.name = name;
    let name = self.trimmed().to_string();
    self.running = false;
    cx.notify();
    if name.is_empty() || name == self.route.branch_name {
      self._check = None;
      return;
    }
    let (host, route) = (self.host.clone(), self.route.clone());
    self._check = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(DEBOUNCE).await;
      this
        .update(cx, |view, cx| {
          view.running = true;
          cx.notify();
        })
        .ignore_closed();
      let checked = cx
        .background_spawn({
          let name = name.clone();
          async move { host.rename_workspace(&route.repo_path, route.workspace_id, &name, true) }
        })
        .await;
      let checked = match checked {
        Ok(result) => Checked {
          name,
          success: result.success,
          message: result.message,
        },
        Err(message) => Checked {
          name,
          success: false,
          message,
        },
      };
      this
        .update(cx, |view, cx| {
          view.running = false;
          view.checked = Some(checked);
          cx.notify();
        })
        .ignore_closed();
    }));
  }

  fn close(&mut self, cx: &mut Context<Self>) {
    self.dismissed = true;
    press(&self.sink, "renameWorkspace.close", cx);
    cx.notify();
  }

  fn toast(&self, tone: ToastTone, title: &str, description: String) {
    WorkspaceHost::notify(
      &*self.host,
      NativeEvent::Toast {
        title: title.into(),
        description,
        tone,
        action: None,
      },
    );
  }

  /// The form's action.
  fn submit(&mut self, cx: &mut Context<Self>) {
    if !self.can_submit() {
      return;
    }
    let next = self.trimmed().to_string();
    self.pending = true;
    cx.notify();
    let (host, route) = (self.host.clone(), self.route.clone());
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn({
          let next = next.clone();
          let route = route.clone();
          async move { host.rename_workspace(&route.repo_path, route.workspace_id, &next, false) }
        })
        .await;
      this
        .update(cx, |view, cx| {
          view.pending = false;
          match result {
            Ok(result) if !result.success => view.error = result.message,
            Ok(_) => {
              view.error.clear();
              view.toast(
                ToastTone::Success,
                &format!("Renamed to {next}"),
                String::new(),
              );
              WorkspaceHost::notify(
                &*view.host,
                NativeEvent::WorkspaceChanged {
                  repo_path: route.repo_path.clone(),
                },
              );
              view.close(cx);
            }
            Err(err) => {
              view.toast(ToastTone::Error, "Failed to rename workspace", err.clone());
              view.error = err;
            }
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
        .placeholder("e.g., feat/new-name")
        .default_value(self.name.clone())
    });
    // `autoFocus`.
    input.update(cx, |input, cx| input.focus(window, cx));
    self._subscription = Some(cx.subscribe(
      &input,
      |view, input, event: &InputEvent, cx| match event {
        InputEvent::Change => {
          let value = input.read(cx).value().to_string();
          view.set_name(value, cx);
        }
        // Enter submits the form, unless its Rename button is disabled.
        InputEvent::PressEnter { .. } => view.submit(cx),
        _ => {}
      },
    ));
    self.input = Some(input.clone());
    input
  }
}

impl Render for RenameDialog {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.dismissed {
      return div().into_any_element();
    }
    let p = theme::palette(self.dark);
    let input = self.input(window, cx);
    let checking = self.is_checking();
    let validation = self.validation().cloned();
    let pending = self.pending;
    let status = if checking {
      Some(ui::spinner("rename-checking", 16., p.muted_foreground).into_any_element())
    } else {
      validation.as_ref().map(|v| {
        if v.success {
          ui::icon(theme::ICON_CHECK, 16., p.success).into_any_element()
        } else {
          ui::icon(theme::ICON_CIRCLE_ALERT, 16., p.destructive).into_any_element()
        }
      })
    };
    let invalid = validation.filter(|v| !checking && !v.success);
    let can_submit = self.can_submit();
    ui::Dialog::new("rename-workspace-dialog", &p)
      .overlay(window.viewport_size())
      .width(500.)
      .title("Rename workspace")
      .description("Rename the branch for this workspace")
      .focus(&self.focus)
      .on_close(ui::handler(cx, |view, _, cx| view.close(cx)))
      .on_primary(ui::handler(cx, |view, _, cx| view.submit(cx)))
      .primary_disabled(!can_submit)
      .child(
        div()
          .flex()
          .flex_col()
          .gap_4()
          .py_4()
          .child(
            div()
              .flex()
              .flex_col()
              .gap_2()
              .child(
                ui::text(div(), TextSize::Sm)
                  .font_weight(FontWeight::MEDIUM)
                  .child("Branch name"),
              )
              .child(
                div()
                  .id("rename-branch-name")
                  .debug_selector(|| "rename-branch-name".into())
                  .w_full()
                  .child(
                    Styled::h(ui::input(&input).disabled(pending), zpx(40.))
                      .when_some(status, |this, icon| this.suffix(div().pr_1().child(icon))),
                  ),
              )
              .when_some(invalid, |this, v| {
                this.child(
                  ui::text(div(), TextSize::Sm)
                    .debug_selector(|| "rename-invalid".into())
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(p.destructive)
                    .child(ui::icon(theme::ICON_CIRCLE_ALERT, 12., p.destructive))
                    .child(v.message),
                )
              }),
          )
          .when(!self.error.is_empty(), |this| {
            this.child(
              ui::text(div(), TextSize::Sm)
                .debug_selector(|| "rename-error".into())
                .text_color(p.destructive)
                .child(self.error.clone()),
            )
          }),
      )
      .footer([
        Button::new("rename-cancel", &p)
          .variant(Variant::Outline)
          .disabled(pending)
          .label("Cancel")
          .on_click(cx.listener(|view, _, _, cx| view.close(cx)))
          .into_any_element(),
        Button::new("rename-submit", &p)
          .disabled(!can_submit)
          .label(if pending { "Renaming…" } else { "Rename" })
          .on_click(cx.listener(|view, _, _, cx| view.submit(cx)))
          .into_any_element(),
      ])
      .into_any_element()
  }
}
