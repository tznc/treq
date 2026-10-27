//! `PromptHistoryModal`: every prompt sent in the repository, newest first,
//! and the selected one in full. It reads a page of them, and the next as the
//! list scrolls near the end.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use serde::Deserialize;
use serde_json::json;
use std::{ops::Range, rc::Rc};
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, uniform_list, ClipboardItem, Context, FontWeight, SharedString, Window,
};

use super::{message, Env};
use crate::{
  gpui_shell::{
    native::ToastTone,
    theme::{self, Palette},
    time,
    ui::{self, Button, Size, TextSize, Variant},
  },
  local_db::PromptHistoryEntry,
};

/// What React sends: the repository, and the entry "View full prompt" opens.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  #[serde(default)]
  pub initial_selected_id: Option<i64>,
}

pub struct PromptHistoryView {
  env: Env,
  route: Route,
  dark: bool,
  /// `None` until the first read returns.
  pub(in crate::gpui_shell::screen) entries: Option<Rc<Vec<PromptHistoryEntry>>>,
  /// Every prompt is in `entries`.
  all_read: bool,
  /// A page is being read.
  reading: bool,
  pub(in crate::gpui_shell::screen) selected: Option<i64>,
  /// Takes the keyboard on opening, so Escape reaches the dialog.
  focus: gpui::FocusHandle,
  focused: bool,
}

/// Prompts read at a time, and how near the end of those listed the list
/// scrolls before the next page is read.
const PAGE: usize = 100;
const READ_AHEAD: usize = 30;

/// `workspaceLabelFor`.
pub fn workspace_label(entry: &PromptHistoryEntry) -> String {
  match &entry.workspace_label {
    Some(label) if !label.is_empty() => label.clone(),
    _ if entry.workspace_id.is_none() => "Home repo".into(),
    _ => "Deleted workspace".into(),
  }
}

/// A prompt's opening, for its row: the row truncates it anyway, and
/// shaping a long prompt whole for one line is wasted.
fn preview(text: &str) -> String {
  text.chars().take(200).collect()
}

/// `formatPromptsForCopy`.
pub fn format_for_copy(entries: &[PromptHistoryEntry]) -> String {
  entries
    .iter()
    .map(|entry| {
      format!(
        "[{}] {}\n{}",
        workspace_label(entry),
        time::locale(&entry.created_at),
        entry.prompt_text
      )
    })
    .collect::<Vec<_>>()
    .join("\n\n---\n\n")
}

impl PromptHistoryView {
  pub fn new(env: Env, route: Route, dark: bool, cx: &mut Context<Self>) -> Self {
    let mut this = Self {
      env,
      selected: route.initial_selected_id,
      route,
      dark,
      entries: None,
      all_read: false,
      reading: false,
      focus: cx.focus_handle(),
      focused: false,
    };
    this.load(cx);
    this
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.route == route && self.dark == dark {
      return;
    }
    let moved = self.route.repo_path != route.repo_path;
    self.route = route;
    self.dark = dark;
    if moved {
      self.entries = None;
      self.all_read = false;
      self.reading = false;
      self.selected = None;
      self.load(cx);
    }
    cx.notify();
  }

  /// Reads the first page; every prompt when the one asked for is older.
  fn load(&mut self, cx: &mut Context<Self>) {
    let (host, repo) = (self.env.host.clone(), self.route.repo_path.clone());
    let key = repo.clone();
    let wanted = self.route.initial_selected_id;
    self.reading = true;
    cx.spawn(async move |this, cx| {
      let read = cx
        .background_spawn(async move {
          let mut entries = host.prompt_history_page(&repo, 0, PAGE)?;
          let mut all = entries.len() < PAGE;
          if !all && wanted.is_some_and(|id| entries.iter().all(|e| e.id != id)) {
            entries = host.prompt_history(&repo)?;
            all = true;
          }
          Ok::<_, String>((entries, all))
        })
        .await;
      let (entries, all) = read.unwrap_or_else(|err| {
        log::warn!("Failed to load prompt history: {err}");
        (Vec::new(), true)
      });
      this
        .update(cx, |view, cx| {
          if view.route.repo_path != key {
            return;
          }
          // The most recent entry, unless one was already asked for or picked.
          if view.selected.is_none() {
            view.selected = entries.first().map(|e| e.id);
          }
          view.entries = Some(Rc::new(entries));
          view.all_read = all;
          view.reading = false;
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  /// Reads the next page once the list shows rows near the end of those read.
  fn read_ahead(&mut self, shown: usize, cx: &mut Context<Self>) {
    let Some(entries) = self.entries.as_ref() else {
      return;
    };
    if self.all_read || self.reading || shown + READ_AHEAD < entries.len() {
      return;
    }
    self.reading = true;
    let (host, repo, offset) = (
      self.env.host.clone(),
      self.route.repo_path.clone(),
      entries.len(),
    );
    cx.spawn(async move |this, cx| {
      let key = repo.clone();
      let page = cx
        .background_spawn(async move { host.prompt_history_page(&repo, offset, PAGE) })
        .await;
      this
        .update(cx, |view, cx| {
          if view.route.repo_path != key {
            return;
          }
          view.reading = false;
          match page {
            Ok(page) => {
              view.all_read = page.len() < PAGE;
              if let Some(entries) = view.entries.as_mut() {
                Rc::make_mut(entries).extend(page);
              }
            }
            Err(err) => {
              log::warn!("Failed to load prompt history: {err}");
              view.all_read = true;
            }
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  /// Copies every prompt, reading the ones not read yet first.
  fn copy_all(&mut self, cx: &mut Context<Self>) {
    if self.all_read {
      let all = self.entries.clone().unwrap_or_default();
      self.copy(format_for_copy(&all), "All prompts copied", cx);
      return;
    }
    let (host, repo) = (self.env.host.clone(), self.route.repo_path.clone());
    cx.spawn(async move |this, cx| {
      let all = cx
        .background_spawn(async move { host.prompt_history(&repo) })
        .await;
      this
        .update(cx, |view, cx| match all {
          Ok(all) => {
            view.copy(format_for_copy(&all), "All prompts copied", cx);
            view.entries = Some(Rc::new(all));
            view.all_read = true;
            cx.notify();
          }
          Err(err) => log::warn!("Failed to load prompt history: {err}"),
        })
        .ignore_closed();
    })
    .detach();
  }

  /// `handleCopy`.
  fn copy(&self, text: String, description: &str, cx: &mut Context<Self>) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    self.env.toast(ToastTone::Success, description, "");
  }

  fn close(&self, cx: &mut Context<Self>) {
    self.env.press("promptHistory.close", cx);
  }

  fn run(&self, entry: &PromptHistoryEntry, cx: &mut Context<Self>) {
    self.env.field(
      "promptHistory.run",
      json!({ "prompt": entry.prompt_text, "workspaceId": entry.workspace_id }),
      cx,
    );
  }

  /// The prompts, a row each, built only while on screen.
  fn list(&self, entries: &[PromptHistoryEntry], p: &Palette, cx: &mut Context<Self>) -> gpui::Div {
    div()
      .debug_selector(|| "prompt-history-list".into())
      .w(zpx(256.))
      .flex_none()
      .h_full()
      .pr_2()
      .border_r_1()
      .border_color(p.border)
      .child(
        uniform_list(
          "prompt-history-scroll",
          entries.len(),
          cx.processor(|view, range: Range<usize>, _, cx| view.rows(range, cx)),
        )
        .size_full(),
      )
  }

  fn rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
    self.read_ahead(range.end, cx);
    let p = theme::palette(self.dark);
    let muted = p.muted_foreground;
    let Some(entries) = self.entries.clone() else {
      return Vec::new();
    };
    range
      .filter_map(|i| entries.get(i).map(|entry| (i, entry)))
      .map(|(i, entry)| {
        let id = entry.id;
        let selected = self.selected == Some(id);
        let selector = format!("prompt-history-item-{i}");
        // `gap-1` between rows, as padding: a uniform list's rows touch.
        div()
          .pb_1()
          .child(
            ui::text(
              ui::Row::new(SharedString::from(selector), &p)
                .density(ui::RowDensity::TwoLine)
                .label(workspace_label(entry))
                .selected(selected),
              TextSize::Xs,
            )
            .w_full()
            .py_2()
            .rounded(radius::MD)
            .items_start()
            .child(
              div()
                .font_family(theme::MONO_FONT)
                .font_weight(FontWeight::MEDIUM)
                .truncate()
                .child(workspace_label(entry)),
            )
            .child(
              div()
                .text_color(muted)
                .truncate()
                .child(time::locale(&entry.created_at)),
            )
            .child(
              div()
                .mt_1()
                .text_color(muted)
                .truncate()
                .child(preview(&entry.prompt_text)),
            )
            .on_click(cx.listener(move |view, _, _, cx| {
              view.selected = Some(id);
              cx.notify();
            })),
          )
          .into_any_element()
      })
      .collect()
  }

  fn detail(
    &self,
    entries: &[PromptHistoryEntry],
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> gpui::Div {
    let pane = div()
      .debug_selector(|| "prompt-history-detail".into())
      .flex_1()
      .min_w_0()
      .h_full();
    let Some(entry) = entries.iter().find(|e| Some(e.id) == self.selected) else {
      return pane.child(message(
        "prompt-history-select",
        "Select a prompt to view its full text.",
        p,
      ));
    };
    let text = entry.prompt_text.clone();
    let header = ui::text(div(), TextSize::Xs)
      .flex()
      .items_center()
      .justify_between()
      .gap_2()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .min_w_0()
          .text_color(p.muted_foreground)
          .child(
            div()
              .font_family(theme::MONO_FONT)
              .font_weight(FontWeight::MEDIUM)
              .text_color(p.foreground)
              .truncate()
              .child(workspace_label(entry)),
          )
          .child(time::locale(&entry.created_at))
          .children(entry.agent.clone().filter(|a| !a.is_empty()).map(|agent| {
            div()
              .rounded_full()
              .bg(p.muted)
              .px_2()
              .py_0p5()
              .child(agent)
          })),
      )
      .child(ui::copy_feedback::copyable(
        "prompt-history-copy-feedback",
        {
          let p = *p;
          let copy =
            cx.listener(move |view, _, _, cx| view.copy(text.clone(), "Prompt copied", cx));
          move |copied| {
            Button::new("prompt-history-copy", &p)
              .aria_label("Copy")
              .variant(Variant::Ghost)
              .size(Size::Sm)
              .h(zpx(24.))
              .px_2()
              .gap_1()
              .copy_icon(copied, 12.)
              .child(ui::text(div(), TextSize::Xs).child("Copy"))
              .on_click(copy)
              .into_any_element()
          }
        },
      ));
    let entry = entry.clone();
    pane.child(
      div()
        .id("prompt-history-detail-scroll")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .child(header)
        .child(
          ui::text(div(), TextSize::Sm)
            .debug_selector(|| "prompt-history-detail-text".into())
            .child(entry.prompt_text.clone()),
        )
        .child(
          div().child(
            Button::new("prompt-history-run", p)
              .size(Size::Sm)
              .child(ui::icon(theme::ICON_PLAY, 14., p.primary_foreground).mr_1p5())
              .label("Run prompt...")
              .on_click(cx.listener(move |view, _, _, cx| view.run(&entry, cx))),
          ),
        ),
    )
  }
}

impl Render for PromptHistoryView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let p = theme::palette(self.dark);
    if !self.focused {
      self.focused = true;
      window.focus(&self.focus, cx);
    }
    let viewport = window.viewport_size();
    let entries = self.entries.clone();
    let copy_all = entries.as_ref().filter(|all| !all.is_empty()).map(|_| {
      let copy = cx.listener(|view, _, _, cx| view.copy_all(cx));
      ui::copy_feedback::copyable("prompt-history-copy-all-feedback", move |copied| {
        Button::new("prompt-history-copy-all", &p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .gap_1p5()
          .copy_icon(copied, 16.)
          .label("Copy all")
          .on_click(copy)
          .into_any_element()
      })
    });
    let header = div()
      .flex_none()
      .flex()
      .items_center()
      .justify_between()
      .gap_2()
      .child(
        ui::text(div(), TextSize::Lg)
          .font_weight(FontWeight::SEMIBOLD)
          .flex()
          .items_center()
          .gap_2()
          .child(ui::icon(theme::ICON_HISTORY, 16., p.popover_foreground))
          .child("Prompt History"),
      )
      .children(copy_all);
    let body = match &entries {
      None => ui::delayed(
        "prompt-history-loading",
        message("prompt-history-loading", "Loading prompt history...", &p),
      )
      .into_any_element(),
      Some(entries) if entries.is_empty() => {
        message("prompt-history-empty", "No prompts sent yet.", &p).into_any_element()
      }
      Some(entries) => div()
        .flex_1()
        .min_h_0()
        .flex()
        .gap_4()
        .overflow_hidden()
        .child(self.list(entries, &p, cx))
        .child(self.detail(entries, &p, cx))
        .into_any_element(),
    };
    ui::Dialog::new("prompt-history-modal", &p)
      .overlay(viewport)
      .relative_size(0.65, Some(0.75))
      .title("Prompt History")
      .header(header)
      .focus(&self.focus)
      .on_close(ui::handler(cx, |view, _, cx| view.close(cx)))
      .child(body)
  }
}
