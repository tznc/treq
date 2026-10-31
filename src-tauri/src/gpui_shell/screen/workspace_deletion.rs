//! `WorkspaceDeletion`: the command palette's "Delete workspace" search. It
//! picks the workspace; React asks for confirmation and deletes it, as it
//! does from the workspace header.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use gpui_kit::component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  anchored, deferred, div, point, prelude::*, px, Context, Entity, FontWeight, MouseButton,
  SharedString, Subscription, Window,
};

use super::{workspace_modal::press, ActionSink, ScreenAction};
use crate::gpui_shell::{
  theme,
  ui::{self, TextSize},
};

/// A workspace as the dialog lists it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionWorkspace {
  pub id: i64,
  pub branch_name: String,
  pub workspace_path: String,
}

/// Dashboard's `showWorkspaceDeletion`, with what the palette passes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionRoute {
  pub repo_path: String,
  pub workspaces: Vec<DeletionWorkspace>,
  #[serde(default)]
  pub current_workspace: Option<DeletionWorkspace>,
}

/// One row: cmdk's item value, and the workspace it deletes.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
  pub value: String,
  pub workspace: DeletionWorkspace,
  /// The "Current workspace (default)" row.
  pub default: bool,
  /// `(current)` after the branch.
  pub current: bool,
}

/// Whether `query` matches `value` the way cmdk's filter keeps it: its
/// characters in order, ignoring case.
pub fn matches(value: &str, query: &str) -> bool {
  let mut chars = value.chars().flat_map(char::to_lowercase);
  query
    .trim()
    .chars()
    .flat_map(char::to_lowercase)
    .all(|q| chars.any(|c| c == q))
}

impl DeletionRoute {
  /// The home repository cannot be deleted.
  fn deletable(&self) -> impl Iterator<Item = &DeletionWorkspace> {
    self
      .workspaces
      .iter()
      .filter(|w| w.workspace_path != self.repo_path)
  }

  fn current(&self) -> Option<&DeletionWorkspace> {
    self
      .current_workspace
      .as_ref()
      .filter(|w| w.workspace_path != self.repo_path)
  }

  /// Every row, before the search filters them.
  pub fn rows(&self) -> Vec<Row> {
    let current_id = self.current_workspace.as_ref().map(|w| w.id);
    let mut rows: Vec<Row> = self
      .current()
      .map(|w| Row {
        value: format!("current-default-{}", w.branch_name),
        workspace: w.clone(),
        default: true,
        current: false,
      })
      .into_iter()
      .collect();
    rows.extend(self.deletable().map(|w| Row {
      value: format!("workspace-{}-{}", w.id, w.branch_name),
      workspace: w.clone(),
      default: false,
      current: Some(w.id) == current_id,
    }));
    rows
  }
}

pub struct WorkspaceDeletion {
  route: DeletionRoute,
  sink: ActionSink,
  dark: bool,
  input: Option<Entity<InputState>>,
  pub(super) query: String,
  /// The highlighted row's value (cmdk's `value`).
  pub(super) selected: String,
  pub(super) dismissed: bool,
  _subscription: Option<Subscription>,
}

impl WorkspaceDeletion {
  pub fn new(route: DeletionRoute, sink: ActionSink, dark: bool, _: &mut Context<Self>) -> Self {
    // The current workspace's own row starts highlighted.
    let selected = route
      .current_workspace
      .as_ref()
      .map(|w| format!("workspace-{}-{}", w.id, w.branch_name))
      .unwrap_or_default();
    Self {
      route,
      sink,
      dark,
      input: None,
      query: String::new(),
      selected,
      dismissed: false,
      _subscription: None,
    }
  }

  pub fn set_route(&mut self, route: DeletionRoute, dark: bool, cx: &mut Context<Self>) {
    if self.route != route || self.dark != dark {
      self.route = route;
      self.dark = dark;
      cx.notify();
    }
  }

  /// The rows the search keeps.
  pub(super) fn visible(&self) -> Vec<Row> {
    self
      .route
      .rows()
      .into_iter()
      .filter(|row| matches(&row.value, &self.query))
      .collect()
  }

  /// The highlighted row: the selected one if still shown, else the first.
  fn highlighted(&self, rows: &[Row]) -> Option<usize> {
    rows
      .iter()
      .position(|r| r.value == self.selected)
      .or((!rows.is_empty()).then_some(0))
  }

  fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
    let rows = self.visible();
    let Some(at) = self.highlighted(&rows) else {
      return;
    };
    // cmdk stops at the ends rather than wrapping.
    let next = (at as isize + delta).clamp(0, rows.len() as isize - 1) as usize;
    self.selected = rows[next].value.clone();
    cx.notify();
  }

  pub(super) fn dismiss(&mut self, cx: &mut Context<Self>) {
    self.dismissed = true;
    press(&self.sink, "workspaceDeletion.close", cx);
    cx.notify();
  }

  /// `handleSelect`: React confirms and deletes.
  fn choose(&mut self, value: &str, cx: &mut Context<Self>) {
    let Some(row) = self.visible().into_iter().find(|r| r.value == value) else {
      return;
    };
    (self.sink)(
      ScreenAction::Field {
        field: "workspaceDeletion.delete".into(),
        value: row.workspace.id.into(),
      },
      cx,
    );
    self.dismissed = true;
    cx.notify();
  }

  fn choose_highlighted(&mut self, cx: &mut Context<Self>) {
    let rows = self.visible();
    if let Some(at) = self.highlighted(&rows) {
      let value = rows[at].value.clone();
      self.choose(&value, cx);
    }
  }

  fn input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
    if let Some(input) = &self.input {
      return input.clone();
    }
    let input =
      cx.new(|cx| InputState::new(window, cx).placeholder("Search workspaces to delete…"));
    input.update(cx, |input, cx| input.focus(window, cx));
    self._subscription = Some(cx.subscribe(
      &input,
      |view, input, event: &InputEvent, cx| match event {
        InputEvent::Change => {
          view.query = input.read(cx).value().to_string();
          // cmdk highlights the first match as the search changes.
          view.selected = view
            .visible()
            .first()
            .map(|r| r.value.clone())
            .unwrap_or_default();
          cx.notify();
        }
        InputEvent::PressEnter { .. } => view.choose_highlighted(cx),
        _ => {}
      },
    ));
    self.input = Some(input.clone());
    input
  }
}

impl Render for WorkspaceDeletion {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.dismissed {
      return div().into_any_element();
    }
    let p = theme::palette(self.dark);
    let input = self.input(window, cx);
    let viewport = window.viewport_size();
    let rows = self.visible();
    let highlighted = self.highlighted(&rows);
    let no_deletable = self.route.deletable().next().is_none();
    let list = if rows.is_empty() {
      ui::text(div(), TextSize::Sm)
        .px_4()
        .py_8()
        .text_center()
        .text_color(p.muted_foreground)
        .child("No deletable workspaces found")
        .into_any_element()
    } else {
      div()
        .children(rows.iter().enumerate().map(|(i, row)| {
          let id = SharedString::from(format!("workspace-deletion-item-{i}"));
          let selector = id.to_string();
          let value = row.value.clone();
          div()
            .id(id)
            .debug_selector(move || selector)
            .px_3()
            .py_1p5()
            .mx_2()
            .rounded(radius::MD)
            .flex()
            .items_center()
            .gap_3()
            .cursor_pointer()
            .when(Some(i) == highlighted, |this| {
              this.bg(p.menu_accent.opacity(0.5))
            })
            .when(Some(i) != highlighted, |this| {
              this.hover(move |s| s.bg(p.hover()))
            })
            .child(ui::icon(theme::ICON_BRANCH, 16., p.muted_foreground))
            .child(
              div()
                .flex_1()
                .min_w_0()
                .child(
                  ui::text(div(), TextSize::Sm)
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .child(row.workspace.branch_name.clone())
                    .when(row.current, |this| {
                      this.child(
                        ui::text(div(), TextSize::Xs)
                          .ml_2()
                          .font_weight(FontWeight::NORMAL)
                          .text_color(p.muted_foreground)
                          .child("(current)"),
                      )
                    }),
                )
                .when(row.default, |this| {
                  this.child(
                    ui::text(div(), TextSize::Sm)
                      .truncate()
                      .text_color(p.muted_foreground)
                      .child("Current workspace (default)"),
                  )
                }),
            )
            .on_click(cx.listener(move |view, _, _, cx| view.choose(&value, cx)))
        }))
        .into_any_element()
    };
    let dialog = div()
      .id("workspace-deletion")
      .debug_selector(|| "workspace-deletion".into())
      .w(viewport.width * 0.4)
      .map(|el| ui::floating(el, &p))
      .bg(p.background)
      .text_color(p.foreground)
      .overflow_hidden()
      .occlude()
      .capture_action(cx.listener(|view, _: &MoveDown, _, cx| {
        view.step(1, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|view, _: &MoveUp, _, cx| {
        view.step(-1, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|view, _: &Escape, _, cx| {
        view.dismiss(cx);
        cx.stop_propagation();
      }))
      .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .px_3()
          .py_2()
          .border_b_1()
          .border_color(p.border)
          .bg(p.destructive.opacity(0.1))
          .child(ui::icon(theme::ICON_TRIANGLE_ALERT, 16., p.destructive))
          .child(
            ui::text(div(), TextSize::Sm)
              .font_weight(FontWeight::MEDIUM)
              .text_color(p.destructive)
              .child("Delete workspace"),
          ),
      )
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
          .id("workspace-deletion-list")
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
              .child("Delete"),
          )
          .child(
            div()
              .flex()
              .gap_1()
              .child(ui::Kbd::new("Esc", &p))
              .child("Cancel"),
          ),
      )
      .when(no_deletable, |this| {
        this.child(
          ui::text(div(), TextSize::Sm)
            .debug_selector(|| "workspace-deletion-home".into())
            .px_4()
            .py_3()
            .border_t_1()
            .border_color(p.border)
            .bg(p.muted.opacity(0.3))
            .flex()
            .items_center()
            .gap_2()
            .text_color(p.muted_foreground)
            .child(ui::icon(theme::ICON_HOUSE, 16., p.muted_foreground))
            .child("Home repository cannot be deleted"),
        )
      });
    deferred(
      anchored().position(point(px(0.), px(0.))).child(
        div()
          .id("workspace-deletion-backdrop")
          .w(viewport.width)
          .h(viewport.height)
          .bg(p.scrim())
          .flex()
          .flex_col()
          .items_center()
          // Pinned from the top, so the dialog stays put as the filter
          // shortens its list.
          .pt(viewport.height * 0.2)
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _, _, cx| view.dismiss(cx)),
          )
          .child(dialog),
      ),
    )
    .with_priority(2)
    .into_any_element()
  }
}
