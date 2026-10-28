//! The Details popover: the workspace's branch, target and creation time,
//! and the prompt it started with (`ShowWorkspace`'s `PopoverContent`
//! `w-96`).

use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, ClipboardItem, Context, Div, FontWeight, SharedString,
};

use super::{data::StartingPrompt, Popover};
use crate::gpui_shell::{
  native::{NativeEvent, ToastTone},
  theme::{self, Palette},
  time,
  ui::{self, Button, Size, TextSize, Variant},
};

/// `w-96`.
pub const WIDTH: f32 = 384.;

/// The workspace the popover is for.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
  pub repo_path: String,
  pub workspace_id: i64,
  pub branch: String,
  /// The workspace's target branch, or the repository's default one.
  pub target: String,
  pub created_at: String,
}

pub struct State {
  pub(super) spec: Spec,
  /// `None` until read, and when no prompt was recorded.
  prompt: Option<StartingPrompt>,
}

impl State {
  pub fn new(spec: Spec) -> Self {
    Self { spec, prompt: None }
  }

  #[cfg(test)]
  pub fn prompt(&self) -> Option<&StartingPrompt> {
    self.prompt.as_ref()
  }
}

impl Popover {
  /// `useSWR(["workspace-starting-prompt", ...])`, fetched as the popover opens.
  pub(super) fn load_details(&mut self, cx: &mut Context<Self>) {
    let super::Content::Details(state) = &self.content else {
      return;
    };
    let (host, repo, id) = (
      self.host.clone(),
      state.spec.repo_path.clone(),
      state.spec.workspace_id,
    );
    self._load = Some(cx.spawn(async move |this, cx| {
      let prompt = cx
        .background_spawn(async move { host.starting_prompt(&repo, id) })
        .await;
      this
        .update(cx, |this, cx| {
          if let super::Content::Details(state) = &mut this.content {
            state.prompt = prompt;
            cx.notify();
          }
        })
        .ignore_closed();
    }));
  }

  /// `handleCopyStartingPrompt`.
  fn copy_prompt(&mut self, cx: &mut Context<Self>) {
    let super::Content::Details(state) = &self.content else {
      return;
    };
    let Some(prompt) = state.prompt.as_ref().filter(|p| !p.text.is_empty()) else {
      return;
    };
    cx.write_to_clipboard(ClipboardItem::new_string(prompt.text.clone()));
    self.host.notify(NativeEvent::Toast {
      title: "Prompt copied".into(),
      description: String::new(),
      tone: ToastTone::Success,
      action: None,
    });
  }

  /// "View full prompt": React's prompt history opens on it.
  fn view_full_prompt(&mut self, cx: &mut Context<Self>) {
    let super::Content::Details(state) = &self.content else {
      return;
    };
    let Some(id) = state.prompt.as_ref().map(|p| p.id) else {
      return;
    };
    self.close(cx);
    self.field("workspace.viewFullPrompt", id.into(), cx);
  }

  pub(super) fn render_details(
    &mut self,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> gpui::Stateful<Div> {
    let super::Content::Details(state) = &self.content else {
      return div().id("workspace-details-popover");
    };
    let p = *p;
    let spec = state.spec.clone();
    let prompt = state.prompt.clone();
    let row = |label: &'static str, value: String, mono: bool| {
      ui::text(div(), TextSize::Sm)
        .flex()
        .justify_between()
        .gap_2()
        .child(
          div()
            .flex_none()
            .text_color(p.muted_foreground)
            .child(label),
        )
        .child(
          div()
            .min_w_0()
            .truncate()
            .when(mono, |this| this.font_family(theme::MONO_FONT))
            .child(value),
        )
    };
    let details = div()
      .child(
        ui::text(div(), TextSize::Sm)
          .font_weight(FontWeight::SEMIBOLD)
          .child("Workspace details"),
      )
      .child(
        div()
          .mt_2()
          .flex()
          .flex_col()
          .gap_1()
          .child(row("Branch", spec.branch.clone(), true))
          .child(row("Target branch", spec.target.clone(), true))
          .child(row("Created", time::locale(&spec.created_at), false)),
      );

    let copy = prompt.is_some().then(|| {
      let copy = cx.listener(|this, _, _, cx| this.copy_prompt(cx));
      ui::copy_feedback::copyable("details-copy-feedback", move |copied| {
        ui::text(
          Button::new("details-copy", &p)
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .h(zpx(24.))
            .px_2()
            .gap_1()
            .copy_icon(copied, 12.)
            .label("Copy"),
          TextSize::Xs,
        )
        .on_click(copy)
        .into_any_element()
      })
    });
    let body = match &prompt {
      Some(prompt) => div()
        .child(
          ui::text(div(), TextSize::Sm)
            .id("details-prompt")
            .debug_selector(|| "details-prompt".into())
            .mt_1()
            .max_h(zpx(160.))
            .overflow_y_scroll()
            .text_color(p.muted_foreground)
            .child(SharedString::from(prompt.text.clone())),
        )
        .child(
          ui::text(div(), TextSize::Xs)
            .id("details-view-full")
            .debug_selector(|| "details-view-full".into())
            .mt_1()
            .cursor_pointer()
            .text_color(theme::palette(self.dark).primary)
            .hover(|s| s.underline())
            .child("View full prompt")
            .on_click(cx.listener(|this, _, _, cx| this.view_full_prompt(cx))),
        ),
      None => div().child(
        ui::text(div(), TextSize::Sm)
          .id("details-no-prompt")
          .debug_selector(|| "details-no-prompt".into())
          .mt_1()
          .text_color(p.muted_foreground)
          .child("No prompt recorded for this workspace yet."),
      ),
    };
    let starting = div()
      .child(
        div()
          .flex()
          .items_center()
          .justify_between()
          .child(
            ui::text(div(), TextSize::Sm)
              .font_weight(FontWeight::MEDIUM)
              .child("Starting prompt"),
          )
          .children(copy),
      )
      .child(body);

    self
      .panel("workspace-details-popover", WIDTH, 16., &p)
      .flex()
      .flex_col()
      .gap_3()
      .child(details)
      .child(starting)
  }
}
