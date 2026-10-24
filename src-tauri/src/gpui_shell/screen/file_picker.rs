//! `FilePicker`: the "Jump to File" dialog, a search over the workspace's
//! file index. It covers the whole window, sidebar included, as React's
//! `Command.Dialog` does.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use std::{sync::Arc, time::Duration};

use gpui_kit::{
  base::TestSupportExt as _,
  component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp},
};
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, point, prelude::*, px, Context, Entity, EventEmitter,
  SharedString, Subscription, Task, Window,
};

use super::code_data::CodeHost;
use crate::gpui_shell::{
  theme,
  ui::{self, TextSize},
};

/// `useDebounce(query, 150)`.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Which files the picker searches.
#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
  pub repo_path: String,
  pub workspace_id: Option<i64>,
  /// The checkout to index (workspace or repository root).
  pub workspace_path: String,
}

pub enum FilePickerEvent {
  /// A file's path relative to the checkout.
  Selected(String),
  Dismissed,
}

pub struct FilePicker {
  host: Arc<dyn CodeHost>,
  scope: Scope,
  dark: bool,
  input: Option<Entity<InputState>>,
  pub(super) query: String,
  pub(super) results: Vec<String>,
  pub(super) selected: usize,
  _search: Option<Task<()>>,
  _subscription: Option<Subscription>,
}

impl EventEmitter<FilePickerEvent> for FilePicker {}

impl FilePicker {
  pub fn new(host: Arc<dyn CodeHost>, scope: Scope, dark: bool, cx: &mut Context<Self>) -> Self {
    let mut this = Self {
      host,
      scope,
      dark,
      input: None,
      query: String::new(),
      results: Vec::new(),
      selected: 0,
      _search: None,
      _subscription: None,
    };
    this.search(String::new(), cx);
    this
  }

  /// Searches for `query` once typing pauses (an empty one lists nothing,
  /// at once: the picker opening, or the query cleared).
  pub(super) fn search(&mut self, query: String, cx: &mut Context<Self>) {
    self.query = query.clone();
    let (host, scope) = (self.host.clone(), self.scope.clone());
    let debounce = !query.trim().is_empty();
    self._search = Some(cx.spawn(async move |this, cx| {
      if debounce {
        cx.background_executor().timer(DEBOUNCE).await;
      }
      let results = cx
        .background_spawn(async move {
          host.search_files(
            &scope.repo_path,
            scope.workspace_id,
            &scope.workspace_path,
            &query,
            50,
          )
        })
        .await
        .unwrap_or_default();
      let _ = this.update(cx, |picker, cx| {
        picker.results = results;
        picker.selected = 0;
        cx.notify();
      });
    }));
  }

  pub(super) fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
    if let Some(path) = self.results.get(index) {
      cx.emit(FilePickerEvent::Selected(path.clone()));
    }
  }

  pub(super) fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
    let len = self.results.len() as isize;
    if len > 0 {
      self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
      cx.notify();
    }
  }

  fn input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
    if let Some(input) = &self.input {
      return input.clone();
    }
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search files..."));
    input.update(cx, |input, cx| input.focus(window, cx));
    self._subscription =
      Some(cx.subscribe(
        &input,
        |picker, input, event: &InputEvent, cx| match event {
          InputEvent::Change => {
            let query = input.read(cx).value().to_string();
            picker.search(query, cx);
          }
          InputEvent::PressEnter { .. } => picker.choose(picker.selected, cx),
          _ => {}
        },
      ));
    self.input = Some(input.clone());
    input
  }
}

impl Render for FilePicker {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let p = theme::palette(self.dark);
    let input = self.input(window, cx);
    let viewport = window.viewport_size();
    let width = viewport.width * 0.4;
    let selected = self.selected;
    let list = if self.results.is_empty() {
      ui::text(div(), TextSize::Sm)
        .px_4()
        .py_8()
        .text_center()
        .text_color(p.muted_foreground)
        .child(if self.query.trim().is_empty() {
          "Type to search files..."
        } else {
          "No files found"
        })
        .into_any_element()
    } else {
      div()
        .children(self.results.iter().enumerate().map(|(i, path)| {
          let id = SharedString::from(format!("file-picker-item-{i}"));
          let selector = id.to_string();
          div()
            .id(id)
            .debug_selector(move || selector)
            .role(gpui::Role::ListBoxOption)
            .aria_label(path.clone())
            .aria_selected(i == selected)
            .px_3()
            .py_1p5()
            .mx_2()
            .rounded(radius::MD)
            .flex()
            .items_center()
            .gap_3()
            .cursor_pointer()
            .when(i == selected, |this| this.bg(p.accent.opacity(0.5)))
            .when(i != selected, |this| {
              this.hover(move |s| s.bg(p.accent.opacity(0.3)))
            })
            .child(ui::icon(theme::ICON_FILE_TEXT, 16., p.muted_foreground))
            .child(
              ui::text(div(), TextSize::Sm)
                .font_family(theme::MONO_FONT)
                .min_w_0()
                .truncate()
                .child(path.clone()),
            )
            .on_click(cx.listener(move |picker, _, _, cx| picker.choose(i, cx)))
            .test_support()
        }))
        .into_any_element()
    };
    let dialog = div()
      .id("file-picker")
      .debug_selector(|| "file-picker".into())
      .w(width)
      .rounded(radius::LG)
      .border_1()
      .border_color(p.border)
      .bg(p.background)
      .text_color(p.foreground)
      .shadow_2xl()
      .overflow_hidden()
      .occlude()
      .capture_action(cx.listener(|picker, _: &MoveDown, _, cx| {
        picker.step(1, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|picker, _: &MoveUp, _, cx| {
        picker.step(-1, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|_, _: &Escape, _, cx| {
        cx.emit(FilePickerEvent::Dismissed);
        cx.stop_propagation();
      }))
      .child(
        ui::text(div(), TextSize::Sm)
          .flex()
          .items_center()
          .h(zpx(48.))
          .px_3()
          .border_b_1()
          .border_color(p.border)
          .child(ui::input(&input).appearance(false).bordered(false).flex_1()),
      )
      .child(
        div()
          .id("file-picker-list")
          .max_h(zpx(300.))
          .overflow_y_scroll()
          .py_2()
          .child(list),
      )
      .child(
        ui::text(div(), TextSize::Sm)
          .border_t_1()
          .border_color(p.border)
          .px_3()
          .py_2()
          .flex()
          .items_center()
          .gap_3()
          .text_color(p.muted_foreground)
          .child(
            div()
              .flex()
              .gap_1()
              .child(ui::Kbd::new("↑↓", &p))
              .child("Navigate"),
          )
          .child(
            div()
              .flex()
              .gap_1()
              .child(ui::Kbd::new("↵", &p))
              .child("Open"),
          )
          .child(
            div()
              .flex()
              .gap_1()
              .child(ui::Kbd::new("Esc", &p))
              .child("Close"),
          ),
      );
    deferred(
      anchored().position(point(px(0.), px(0.))).child(
        div()
          .id("file-picker-backdrop")
          .w(viewport.width)
          .h(viewport.height)
          .bg(p.scrim())
          .flex()
          .items_center()
          .justify_center()
          .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|_, _, _, cx| cx.emit(FilePickerEvent::Dismissed)),
          )
          .child(dialog),
      ),
    )
    .with_priority(2)
  }
}
