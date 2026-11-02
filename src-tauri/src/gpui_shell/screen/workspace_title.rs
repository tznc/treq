//! The workspace header's title: the workspace's name in place of its
//! branch, edited in place with the pencil. On a workspace that follows its
//! pull request, Save writes the title to the PR too, unless "Save as local
//! draft" keeps it here until the next push.

use std::sync::Arc;

use gpui_kit::component::input::{Enter, Escape, InputEvent, Textarea, TextareaState};
use tauri_plugin_gpui::gpui::{
  deferred, div, prelude::*, Context, Entity, FontWeight, SharedString, Subscription, Window,
};

use super::code::menu_panel;
use super::code_data::CodeHost;
use crate::gpui_shell::{
  native::{NativeEvent, ToastTone},
  theme::{self, Palette},
  ui::{self, zpx, Button, DismissExt as _, Size, TextSize, Variant},
};

/// One workspace's title, and its editor while open.
pub struct TitleEditor {
  host: Arc<dyn CodeHost>,
  repo_path: String,
  workspace_id: i64,
  title: String,
  dark: bool,
  draft: Option<Entity<TextareaState>>,
  saving: bool,
  menu_open: bool,
  /// The workspace follows a pull request, read when the editor opens.
  follows_pr: bool,
  _subscriptions: Vec<Subscription>,
}

impl TitleEditor {
  pub fn new(
    host: Arc<dyn CodeHost>,
    repo_path: String,
    workspace_id: i64,
    title: String,
    dark: bool,
  ) -> Self {
    Self {
      host,
      repo_path,
      workspace_id,
      title,
      dark,
      draft: None,
      saving: false,
      menu_open: false,
      follows_pr: false,
      _subscriptions: Vec::new(),
    }
  }

  pub fn workspace(&self) -> (&str, i64) {
    (&self.repo_path, self.workspace_id)
  }

  /// Shows `title` as read anew; an open editor keeps what is typed.
  pub fn set_title(&mut self, title: String, dark: bool, cx: &mut Context<Self>) {
    if self.title != title || self.dark != dark {
      self.title = title;
      self.dark = dark;
      cx.notify();
    }
  }

  fn edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let text = self.title.clone();
    let draft = cx.new(|cx| {
      TextareaState::new(window, cx)
        .auto_grow(1, 1)
        .placeholder("Workspace title")
        .default_value(text)
    });
    draft.update(cx, |d, cx| d.focus(window, cx));
    // Save is disabled while the title is blank.
    self._subscriptions = vec![cx.subscribe(&draft, |_, _, event: &InputEvent, cx| {
      if matches!(event, InputEvent::Change) {
        cx.notify();
      }
    })];
    self.draft = Some(draft);
    let (host, repo, id) = (self.host.clone(), self.repo_path.clone(), self.workspace_id);
    cx.spawn(async move |this, cx| {
      let follows = cx
        .background_spawn(async move { host.pr_sync(&repo, id).is_some() })
        .await;
      this
        .update(cx, |view, cx| {
          view.follows_pr = follows;
          cx.notify();
        })
        .ok();
    })
    .detach();
    cx.notify();
  }

  fn cancel(&mut self, cx: &mut Context<Self>) {
    self.draft = None;
    self.menu_open = false;
    cx.notify();
  }

  /// Saves the title being edited; to the pull request too unless `local`.
  fn save(&mut self, local: bool, cx: &mut Context<Self>) {
    let Some(editor) = &self.draft else {
      return;
    };
    let text = editor.read(cx).value().trim().to_string();
    if text.is_empty() || self.saving {
      return;
    }
    let to_pr = !local && self.follows_pr;
    self.saving = true;
    self.menu_open = false;
    let (host, repo, id) = (self.host.clone(), self.repo_path.clone(), self.workspace_id);
    cx.notify();
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn({
          let (host, repo, text) = (host.clone(), repo.clone(), text.clone());
          async move {
            host.set_title(&repo, id, &text)?;
            Ok::<_, String>(to_pr.then(|| host.push_to_pr(&repo, id).err()).flatten())
          }
        })
        .await;
      this
        .update(cx, |view, cx| {
          view.saving = false;
          match result {
            Ok(pr_error) => {
              view.title = text;
              view.draft = None;
              view.tell(NativeEvent::WorkspaceChanged {
                repo_path: view.repo_path.clone(),
              });
              if let Some(err) = pr_error {
                view.toast(
                  ToastTone::Error,
                  "Saved as a local draft",
                  format!("The pull request wasn't updated: {err}"),
                );
              }
            }
            Err(err) => view.toast(ToastTone::Error, "Failed to save title", err),
          }
          cx.notify();
        })
        .ok();
    })
    .detach();
  }

  fn tell(&self, event: NativeEvent) {
    super::workspace_data::WorkspaceHost::notify(self.host.as_ref(), event);
  }

  fn toast(&self, tone: ToastTone, title: &str, description: impl Into<String>) {
    self.tell(NativeEvent::Toast {
      title: title.into(),
      description: description.into(),
      tone,
      action: None,
    });
  }

  fn save_button(&self, disabled: bool, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let id = "workspace-title-save";
    if !self.follows_pr {
      return Button::new(id, p)
        .size(Size::Sm)
        .disabled(disabled)
        .label("Save")
        .on_click(cx.listener(|view, _, _, cx| view.save(false, cx)))
        .into_any_element();
    }
    let open = self.menu_open && !disabled;
    let panel = menu_panel("workspace-title-save-panel", p)
      .top(zpx(32.))
      .right_0()
      .w(zpx(240.))
      .child(
        ui::MenuItem::new(
          SharedString::from(format!("{id}-draft")),
          "Save as local draft",
          p,
        )
        .description("Keep it here; it goes to the pull request on your next push")
        .on_click(cx.listener(|view, _, _, cx| view.save(true, cx))),
      )
      .dismiss_with("workspace-title-menu", cx, |view, cx| {
        let open = std::mem::take(&mut view.menu_open);
        if open {
          cx.notify();
        }
        open
      });
    div()
      .relative()
      .child(
        ui::SplitButton::new(id, "Save", p)
          .menu_label("More save options")
          .disabled(disabled)
          .open(open)
          .on_click(cx.listener(|view, _, _, cx| view.save(false, cx)))
          .on_menu(cx.listener(|view, _, _, cx| {
            view.menu_open = !view.menu_open;
            cx.notify();
          })),
      )
      .when(open, |this| this.child(deferred(panel).with_priority(1)))
      .into_any_element()
  }
}

impl Render for TitleEditor {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let p = theme::palette(self.dark);
    let heading = |el: gpui::Div| {
      ui::text(el, TextSize::Sm)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.foreground)
    };
    let panel = div()
      .debug_selector(|| "workspace-title".into())
      .flex()
      .items_center()
      .gap_1()
      .min_w_0();
    let Some(draft) = self.draft.clone() else {
      return panel
        .child(
          heading(div())
            .min_w_0()
            .max_w(zpx(360.))
            .truncate()
            .debug_selector(|| "workspace-title-text".into())
            .child(self.title.clone()),
        )
        .child(
          Button::new("workspace-title-edit", &p)
            .aria_label("Edit workspace title")
            .variant(Variant::Ghost)
            .size(Size::Icon)
            .flex_none()
            .w(zpx(20.))
            .h(zpx(20.))
            .child(ui::icon(theme::ICON_PENCIL, 14., p.muted_foreground))
            .on_click(cx.listener(|view, _, window, cx| view.edit(window, cx))),
        )
        .into_any_element();
    };
    let empty = draft.read(cx).value().trim().is_empty();
    let saving = self.saving;
    panel
      .id("workspace-title-editor")
      .capture_action(cx.listener(|view, _: &Enter, _, cx| {
        view.save(false, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|view, _: &Escape, _, cx| {
        view.cancel(cx);
        cx.stop_propagation();
      }))
      .child(
        heading(div())
          .w(zpx(320.))
          .debug_selector(|| "workspace-title-input".into())
          .child(Textarea::new(&draft)),
      )
      .child(
        Button::new("workspace-title-cancel", &p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .disabled(saving)
          .label("Cancel")
          .on_click(cx.listener(|view, _, _, cx| view.cancel(cx))),
      )
      .child(self.save_button(saving || empty, &p, cx))
      .into_any_element()
  }
}

use tauri_plugin_gpui::gpui;
