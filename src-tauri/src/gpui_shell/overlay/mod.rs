//! The window-wide dialogs, drawn over the whole window, sidebar included:
//! the command palette, the keyboard shortcuts sheet, the workspace picker
//! and the branch switcher.
//!
//! The window model keeps which one is open and pushes it; GPUI shows it in
//! a modal window over the shell window (see `modal`). The dialogs read and
//! switch branches through the backend themselves. What the model decides
//! (a palette command, the picked workspace, closing) goes back to it as a
//! screen action, and the model pushes what shows next.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{rc::Rc, sync::Arc};

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::input::{
  InputEvent, InputState, MoveEnd, MoveHome, MoveToEnd, MoveToStart,
};
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, list, prelude::*, px, Context, Entity, FocusHandle, KeyDownEvent, ListOffset,
  ListSizingBehavior, ScrollHandle, SharedString, Subscription, Task, Window,
};

use super::{
  screen::{rows::Rows, ActionSink, ScreenAction},
  theme::{self, Palette},
  ui::{self, TextSize},
};
use crate::jj::JjBranch;

pub mod cmdk;
mod data;
mod palette;
mod palette_run;
mod shortcuts;
#[cfg(test)]
mod tests;

pub use data::OverlayHost;
use data::{PaletteChecks, PrLink, WorkspaceRow};

/// Which dialog shows, and the theme to draw it in.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySnapshot {
  pub dark: bool,
  #[serde(flatten)]
  pub view: OverlayView,
}

/// One dialog, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OverlayView {
  CommandPalette(palette::Route),
  KeyboardShortcuts {},
  #[serde(rename_all = "camelCase")]
  WorkspacePicker {
    repo_path: String,
  },
  #[serde(rename_all = "camelCase")]
  BranchSwitcher {
    repo_path: String,
  },
}

impl OverlayView {
  /// Whether `other` is the same dialog for the same repository, so it keeps
  /// what was typed and selected.
  pub fn same_dialog(&self, other: &OverlayView) -> bool {
    match (self, other) {
      (OverlayView::CommandPalette(_), OverlayView::CommandPalette(_))
      | (OverlayView::KeyboardShortcuts {}, OverlayView::KeyboardShortcuts {}) => true,
      (
        OverlayView::WorkspacePicker { repo_path: a },
        OverlayView::WorkspacePicker { repo_path: b },
      )
      | (
        OverlayView::BranchSwitcher { repo_path: a },
        OverlayView::BranchSwitcher { repo_path: b },
      ) => a == b,
      _ => false,
    }
  }
}

/// What the open dialog lists.
pub(super) enum Body {
  Shortcuts,
  Palette {
    commands: Vec<palette::Command>,
    checks: Option<PaletteChecks>,
    pr: Option<PrLink>,
  },
  /// `None` until read.
  Workspaces(Option<Vec<WorkspaceRow>>),
  Branches {
    /// Grouped current, local, remote; `None` while loading.
    branches: Option<Vec<(JjBranch, usize)>>,
    error: Option<String>,
    switching: bool,
  },
}

/// `BranchSwitcher`'s groups.
const BRANCH_GROUPS: [&str; 3] = ["Current Branch", "Local Branches", "Remote Branches"];

/// Up to this many rows the list draws them all in a scrolled column, so
/// every row (the palette's commands, a short branch list) can be found and
/// clicked; beyond it only the rows on screen are drawn.
const ALL_ROWS_UP_TO: usize = 60;

/// What the list shows for the body and search, which hovering and
/// selecting leave as they are.
struct Listing {
  entries: Vec<cmdk::Entry>,
  /// The entries shown for the search, in order.
  order: Vec<usize>,
}

/// One row of the virtual list: an entry, or the heading of a group.
#[derive(Clone, Copy, PartialEq)]
enum Item {
  Heading(usize),
  Entry(usize),
}

/// The open dialog: its search, selection and what it read.
pub struct Overlay {
  view: OverlayView,
  dark: bool,
  host: Arc<dyn OverlayHost>,
  sink: ActionSink,
  focus: FocusHandle,
  focused: bool,
  input: Option<Entity<InputState>>,
  _input: Option<Subscription>,
  pub(super) search: String,
  /// The selected entry (cmdk's `value`), as an index into [`Self::entries`].
  pub(super) selected: Option<usize>,
  pub(super) body: Body,
  /// The list's rows, of which only those on screen are drawn once there
  /// are more than [`ALL_ROWS_UP_TO`].
  items: Rows<Item>,
  /// Scrolls the column that holds every row of a short list.
  scroll: ScrollHandle,
  /// Set when the rows no longer stand for the same entries, so none keeps
  /// the height it measured.
  items_changed: bool,
  /// Where each entry's row (and, for the first of a group, its heading) sit
  /// among the list's items, from the last render.
  rows: Vec<(usize, usize)>,
  /// [`Self::listing`], until the body or the search changes.
  listing: Option<Rc<Listing>>,
  _load: Option<Task<()>>,
}

impl Overlay {
  pub fn new(
    snapshot: OverlaySnapshot,
    host: Arc<dyn OverlayHost>,
    sink: ActionSink,
    cx: &mut Context<Self>,
  ) -> Self {
    let body = match &snapshot.view {
      OverlayView::KeyboardShortcuts {} => Body::Shortcuts,
      OverlayView::CommandPalette(_) => Body::Palette {
        commands: Vec::new(),
        checks: None,
        pr: None,
      },
      OverlayView::WorkspacePicker { .. } => Body::Workspaces(None),
      OverlayView::BranchSwitcher { .. } => Body::Branches {
        branches: None,
        error: None,
        switching: false,
      },
    };
    let mut this = Self {
      view: snapshot.view,
      dark: snapshot.dark,
      host,
      sink,
      focus: cx.focus_handle(),
      focused: false,
      input: None,
      _input: None,
      search: String::new(),
      selected: None,
      body,
      items: Rows::default(),
      scroll: ScrollHandle::new(),
      items_changed: false,
      rows: Vec::new(),
      listing: None,
      _load: None,
    };
    this.load(cx);
    this
  }

  pub fn view(&self) -> &OverlayView {
    &self.view
  }

  /// Takes React's next snapshot of the same dialog.
  pub fn set_snapshot(&mut self, snapshot: OverlaySnapshot, cx: &mut Context<Self>) {
    let reload = self.view != snapshot.view;
    self.view = snapshot.view;
    self.dark = snapshot.dark;
    if reload {
      self.load(cx);
    }
    cx.notify();
  }

  fn send(&self, action: ScreenAction, cx: &mut gpui::App) {
    (self.sink)(action, cx);
  }

  fn press(&self, id: &str, cx: &mut gpui::App) {
    self.send(ScreenAction::Press { id: id.into() }, cx);
  }

  /// Asks React to close the dialog (`onOpenChange(false)`).
  pub(super) fn close(&self, cx: &mut gpui::App) {
    self.press("overlay.close", cx);
  }

  /// Reads what the dialog lists, off the main thread.
  fn load(&mut self, cx: &mut Context<Self>) {
    let host = self.host.clone();
    match self.view.clone() {
      OverlayView::KeyboardShortcuts {} => {}
      OverlayView::CommandPalette(route) => {
        self.rebuild_commands();
        let Some(workspace) = route.workspace.clone() else {
          return;
        };
        self._load = Some(cx.spawn(async move |this, cx| {
          let (checks, pr) = cx
            .background_spawn(async move {
              (
                host.checks(&workspace.repo_path, workspace.id),
                host.pr(&route.repo_path, &workspace.branch),
              )
            })
            .await;
          this
            .update(cx, |this, cx| {
              if let Body::Palette {
                checks: c, pr: p, ..
              } = &mut this.body
              {
                *c = Some(checks);
                *p = pr;
              }
              this.rebuild_commands();
              cx.notify();
            })
            .ignore_closed();
        }));
      }
      OverlayView::WorkspacePicker { repo_path } => {
        self._load = Some(cx.spawn(async move |this, cx| {
          let rows = cx
            .background_spawn(async move { host.workspaces(&repo_path) })
            .await
            .unwrap_or_else(|err| {
              log::warn!("workspace picker: {err}");
              Vec::new()
            });
          this
            .update(cx, |this, cx| {
              this.body = Body::Workspaces(Some(rows));
              this.changed();
              this.select_first();
              cx.notify();
            })
            .ignore_closed();
        }));
      }
      OverlayView::BranchSwitcher { repo_path } => {
        self.body = Body::Branches {
          branches: None,
          error: None,
          switching: false,
        };
        self.changed();
        self._load = Some(cx.spawn(async move |this, cx| {
          let read = cx
            .background_spawn(async move { host.branches(&repo_path) })
            .await;
          this
            .update(cx, |this, cx| {
              this.body = match read {
                Ok(list) => Body::Branches {
                  branches: Some(group_branches(list)),
                  error: None,
                  switching: false,
                },
                Err(err) => Body::Branches {
                  branches: None,
                  error: Some(err),
                  switching: false,
                },
              };
              this.changed();
              this.select_first();
              cx.notify();
            })
            .ignore_closed();
        }));
      }
    }
  }

  /// The palette's commands from the route and what was read, keeping the
  /// selected command selected.
  fn rebuild_commands(&mut self) {
    let OverlayView::CommandPalette(route) = &self.view else {
      return;
    };
    let selected = self.selected_id();
    if let Body::Palette {
      commands,
      checks,
      pr,
    } = &mut self.body
    {
      *commands = palette::commands(route, checks.as_ref(), pr.as_ref());
      self.selected = selected.and_then(|id| commands.iter().position(|c| c.id == id));
    }
    self.changed();
    if self.selected.is_none() {
      self.select_first();
    }
  }

  fn selected_id(&self) -> Option<String> {
    match &self.body {
      Body::Palette { commands, .. } => self
        .selected
        .and_then(|i| commands.get(i))
        .map(|c| c.id.clone()),
      _ => None,
    }
  }

  /// The `Command.Item`s rendered now.
  pub(super) fn entries(&self) -> Vec<cmdk::Entry> {
    match &self.body {
      Body::Shortcuts => Vec::new(),
      Body::Palette { commands, .. } => commands
        .iter()
        .map(|c| cmdk::Entry {
          value: c.label.clone(),
          group: None,
          disabled: false,
        })
        .collect(),
      Body::Workspaces(rows) => rows
        .iter()
        .flatten()
        .map(|w| cmdk::Entry {
          value: w.branch_name.clone(),
          group: None,
          disabled: false,
        })
        .collect(),
      Body::Branches {
        branches: Some(list),
        error: None,
        switching,
      } => list
        .iter()
        .map(|(b, group)| cmdk::Entry {
          value: b.name.clone(),
          group: Some(*group),
          disabled: *switching,
        })
        .collect(),
      Body::Branches { .. } => Vec::new(),
    }
  }

  /// The entries shown for the search, in order.
  #[cfg(test)]
  pub(super) fn order(&self) -> Vec<usize> {
    cmdk::visible(&self.entries(), &self.search)
  }

  /// What the body or the search changed: the entries and their order are
  /// made again, and the rows measured again.
  fn changed(&mut self) {
    self.listing = None;
    self.items_changed = true;
  }

  /// The entries and their order for the search, made once until they change
  /// rather than for every frame a hover draws.
  fn listing(&mut self) -> Rc<Listing> {
    if let Some(listing) = &self.listing {
      return listing.clone();
    }
    let entries = self.entries();
    let order = cmdk::visible(&entries, &self.search);
    let listing = Rc::new(Listing { entries, order });
    self.listing = Some(listing.clone());
    listing
  }

  fn select_first(&mut self) {
    let listing = self.listing();
    self.selected = cmdk::first(&listing.entries, &listing.order);
  }

  pub(super) fn set_search(&mut self, search: String, cx: &mut Context<Self>) {
    if self.search != search {
      self.search = search;
      self.changed();
      self.select_first();
      self.items.list.scroll_to(ListOffset {
        item_ix: 0,
        offset_in_item: px(0.),
      });
      self.scroll.set_offset(gpui::point(px(0.), px(0.)));
      cx.notify();
    }
  }

  /// Moves the selection as cmdk does for a key, and scrolls it into view.
  pub(super) fn step(&mut self, step: cmdk::Move, cx: &mut Context<Self>) {
    let listing = self.listing();
    let next = cmdk::moved(&listing.entries, &listing.order, self.selected, step);
    if next != self.selected {
      self.selected = next;
      if let Some(&(_, item)) = next.and_then(|n| self.rows.iter().find(|(e, _)| *e == n)) {
        if self.items.len() <= ALL_ROWS_UP_TO {
          self.scroll.scroll_to_item(item);
        } else {
          self.items.list.scroll_to_reveal_item(item);
        }
      }
      cx.notify();
    }
  }

  /// Runs entry `index`, as cmdk's `onSelect`.
  pub(super) fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
    let field = |field: &str, value: serde_json::Value| ScreenAction::Field {
      field: field.into(),
      value,
    };
    match &self.body {
      Body::Shortcuts => {}
      Body::Palette { commands, pr, .. } => {
        let Some(command) = commands.get(index) else {
          return;
        };
        let url = pr.as_ref().map(|pr| pr.url.clone());
        match command.id.as_str() {
          palette::OPEN_PR_IN_BROWSER => {
            if let Some(url) = url {
              self.host.open_url(&url);
            }
            self.close(cx);
          }
          palette::OPEN_PR_HERE => {
            self.send(
              field("palette.navigate", url.unwrap_or_default().into()),
              cx,
            );
          }
          id => {
            let id = id.to_string();
            self.run_palette(&id, cx);
          }
        }
      }
      Body::Workspaces(rows) => {
        if let Some(row) = rows.iter().flatten().nth(index) {
          self.send(field("workspacePicker.select", row.id.into()), cx);
        }
      }
      Body::Branches {
        branches: Some(list),
        switching: false,
        ..
      } => {
        let Some((branch, _)) = list.get(index) else {
          return;
        };
        if branch.is_current {
          self.close(cx);
          return;
        }
        let name = branch.name.clone();
        self.switch_branch(name, cx);
      }
      Body::Branches { .. } => {}
    }
  }

  /// `handleSelectBranch` for a branch other than the current one.
  fn switch_branch(&mut self, name: String, cx: &mut Context<Self>) {
    let OverlayView::BranchSwitcher { repo_path } = self.view.clone() else {
      return;
    };
    self.set_switching(true, None, cx);
    let host = self.host.clone();
    self._load = Some(cx.spawn(async move |this, cx| {
      let switched = {
        let name = name.clone();
        cx.background_spawn(async move { host.switch_branch(&repo_path, &name) })
          .await
      };
      this
        .update(cx, |this, cx| match switched {
          Ok(()) => {
            this.set_switching(false, None, cx);
            this.send(
              ScreenAction::Field {
                field: "branchSwitcher.changed".into(),
                value: name.into(),
              },
              cx,
            );
          }
          Err(err) => this.set_switching(false, Some(err), cx),
        })
        .ignore_closed();
    }));
  }

  fn set_switching(&mut self, on: bool, failed: Option<String>, cx: &mut Context<Self>) {
    if let Body::Branches {
      switching, error, ..
    } = &mut self.body
    {
      *switching = on;
      *error = failed;
    }
    self.changed();
    if let Some(input) = &self.input {
      input.update(cx, |input, cx| input.set_disabled(on, cx));
    }
    cx.notify();
  }

  fn placeholder(&self) -> &'static str {
    match self.view {
      OverlayView::CommandPalette(_) => "Type a command or search...",
      OverlayView::WorkspacePicker { .. } => "Search workspaces...",
      OverlayView::BranchSwitcher { .. } => "Search branches...",
      OverlayView::KeyboardShortcuts {} => "",
    }
  }

  fn input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
    if let Some(input) = &self.input {
      return input.clone();
    }
    let placeholder = self.placeholder();
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
    input.update(cx, |input, cx| input.focus(window, cx));
    self._input = Some(cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
      if let InputEvent::Change = event {
        let search = input.read(cx).value().to_string();
        this.set_search(search, cx);
      }
    }));
    self.input = Some(input.clone());
    input
  }

  /// cmdk's root `onKeyDown`, and Escape closing the dialog.
  fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
    let key = event.keystroke.key.as_str();
    let m = crate::gpui_shell::keys::held(event.keystroke.modifiers);
    if ui::is_escape(&event.keystroke) {
      self.close(cx);
      cx.stop_propagation();
      return;
    }
    if matches!(self.body, Body::Shortcuts) {
      // "?" toggles the reference, as Dashboard's shortcut does.
      if event.keystroke.key_char.as_deref() == Some("?") || key == "?" {
        self.close(cx);
        cx.stop_propagation();
      }
      return;
    }
    let step = match key {
      "down" if m.platform => Some(cmdk::Move::Last),
      "up" if m.platform => Some(cmdk::Move::First),
      "down" if m.alt => Some(cmdk::Move::Group(1)),
      "up" if m.alt => Some(cmdk::Move::Group(-1)),
      "down" => Some(cmdk::Move::Step(1)),
      "up" => Some(cmdk::Move::Step(-1)),
      "n" | "j" if m.control => Some(cmdk::Move::Step(1)),
      "p" | "k" if m.control => Some(cmdk::Move::Step(-1)),
      "home" => Some(cmdk::Move::First),
      "end" => Some(cmdk::Move::Last),
      _ => None,
    };
    if let Some(step) = step {
      self.step(step, cx);
      cx.stop_propagation();
    } else if key == "enter" {
      if let Some(selected) = self.selected {
        self.choose(selected, cx);
      }
      cx.stop_propagation();
    }
  }
}

/// `groupedBranches`: current, then local, then remote (`@` in the name).
fn group_branches(list: Vec<JjBranch>) -> Vec<(JjBranch, usize)> {
  let group = |b: &JjBranch| {
    if b.is_current {
      0
    } else if b.name.contains('@') {
      2
    } else {
      1
    }
  };
  let mut grouped: Vec<(JjBranch, usize)> = list
    .into_iter()
    .map(|b| {
      let g = group(&b);
      (b, g)
    })
    .collect();
  grouped.sort_by_key(|(_, g)| *g);
  grouped
}

/// The footer of keyboard hints, `CmdkFooter` and its copies.
fn footer(p: &Palette, hints: &[(&str, &str)], right: Option<&'static str>) -> gpui::Div {
  ui::text(div(), TextSize::Sm)
    .border_t_1()
    .border_color(p.border)
    .px_3()
    .py_2()
    .flex()
    .items_center()
    .justify_between()
    .text_color(p.muted_foreground)
    .child(
      div()
        .flex()
        .items_center()
        .gap_3()
        .children(hints.iter().map(|(key, label)| {
          div()
            .flex()
            .items_center()
            .gap_1()
            .child(ui::Kbd::from_label(key.to_string(), p))
            .child(SharedString::from(label.to_string()))
        })),
    )
    .children(right)
}

/// A centred `px-4 py-8 text-sm text-muted-foreground` message.
fn message(p: &Palette, text: &'static str) -> gpui::Div {
  ui::text(div(), TextSize::Sm)
    .px_4()
    .py_8()
    .text_center()
    .text_color(p.muted_foreground)
    .child(text)
}

impl Overlay {
  /// One row's content and its look, by dialog.
  fn row(&self, p: &Palette, index: usize, selected: bool, cx: &mut Context<Self>) -> ui::Row {
    let id = SharedString::from(format!("overlay-item-{index}"));
    // cmdk's `[cmdk-item]` is an option named by its label.
    let description = match &self.body {
      Body::Palette { commands, .. } => commands[index].description.clone(),
      _ => None,
    };
    let name: Option<SharedString> = match &self.body {
      Body::Palette { commands, .. } => Some(commands[index].label.clone().into()),
      Body::Workspaces(Some(rows)) => Some(rows[index].branch_name.clone().into()),
      Body::Branches {
        branches: Some(list),
        ..
      } => Some(list[index].0.name.clone().into()),
      _ => None,
    };
    let base = ui::Row::new(id, p)
      .role(gpui::Role::ListBoxOption)
      .selected(selected)
      .when_some(name, |this, name| this.label(name))
      .when_some(description, |this, d| this.aria_description(d))
      .gap_3()
      .px_3()
      .on_mouse_move(cx.listener(move |this, _, _, cx| {
        // Disabled items take no pointer events.
        let disabled = matches!(
          this.body,
          Body::Branches {
            switching: true,
            ..
          }
        );
        if !disabled && this.selected != Some(index) {
          this.selected = Some(index);
          cx.notify();
        }
      }))
      .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)));
    let two_lines = |label: &str, description: Option<&str>, medium: bool| {
      div()
        .flex_1()
        .min_w_0()
        .child(
          ui::text(div(), TextSize::Sm)
            .truncate()
            .when(medium, |this| this.font_weight(gpui::FontWeight::MEDIUM))
            .child(SharedString::from(label.to_string())),
        )
        .children(description.map(|d| {
          ui::text(div(), TextSize::Sm)
            .truncate()
            .text_color(p.muted_foreground)
            .child(SharedString::from(d.to_string()))
        }))
    };
    match &self.body {
      Body::Palette { commands, .. } => {
        let command = &commands[index];
        base
          .py_1p5()
          .mx_2()
          .rounded(radius::MD)
          .leading(ui::icon(command.icon, 16., p.muted_foreground))
          .child(two_lines(
            &command.label,
            command.description.as_deref(),
            true,
          ))
      }
      Body::Workspaces(rows) => {
        let row = &rows.as_deref().unwrap_or_default()[index];
        let description = workspace_description(row);
        base
          .py_2()
          .mx_2()
          .rounded(radius::MD)
          .leading(ui::icon(theme::ICON_BRANCH, 16., p.muted_foreground))
          .child(two_lines(&row.branch_name, Some(&description), true))
      }
      Body::Branches {
        branches: Some(list),
        switching,
        ..
      } => {
        let (branch, group) = &list[index];
        let (icon, color, tag) = match group {
          0 => (theme::ICON_CHECK, p.success, Some("current")),
          1 => (theme::ICON_BRANCH, p.muted_foreground, None),
          _ => (theme::ICON_ARROW_RIGHT, p.muted_foreground, Some("remote")),
        };
        base
          .py_2()
          .rounded_none()
          .interactive(!*switching)
          .when(*switching, |this| this.opacity(0.5))
          .leading(ui::icon(icon, 16., color))
          .child(two_lines(&branch.name, None, *group == 0))
          .children(tag.map(|tag| {
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child(tag)
          }))
      }
      _ => base,
    }
  }

  /// The `Command.List`'s messages, which sit above its rows.
  fn messages(&mut self, p: &Palette) -> Vec<gpui::AnyElement> {
    let listing = self.listing();
    let mut children: Vec<gpui::AnyElement> = Vec::new();
    let empty = |p: &Palette, text| message(p, text).into_any_element();
    if let Body::Branches {
      branches, error, ..
    } = &self.body
    {
      if branches.is_none() && error.is_none() {
        children.push(empty(p, "Loading branches..."));
      }
      if let Some(error) = error {
        children.push(
          ui::text(div(), TextSize::Sm)
            .px_4()
            .py_3()
            .text_center()
            .text_color(p.destructive)
            .border_b_1()
            .border_color(p.border)
            .child(SharedString::from(format!("Error: {error}")))
            .into_any_element(),
        );
      }
    }
    let loaded = !matches!(self.body, Body::Workspaces(None));
    if listing.order.is_empty() && loaded {
      children.push(empty(
        p,
        match self.body {
          Body::Workspaces(_) => "No workspaces found",
          Body::Branches { .. } => "No branches found",
          _ => "No results found",
        },
      ));
    }
    children
  }

  /// The `Command.List`'s rows and group headings: all of them for a short
  /// list, or a virtual list that draws only those on screen.
  fn list(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<gpui::List> {
    let listing = self.listing();
    let mut items = Vec::new();
    self.rows.clear();
    let mut group = None;
    for &index in &listing.order {
      let entry = &listing.entries[index];
      if entry.group != group {
        group = entry.group;
        if let Some(g) = group {
          self.rows.push((index, items.len()));
          items.push(Item::Heading(g));
          items.push(Item::Entry(index));
          continue;
        }
      }
      self.rows.push((index, items.len()));
      items.push(Item::Entry(index));
    }
    let changed = std::mem::take(&mut self.items_changed);
    self.items.sync(window, items, |a, b| !changed && a == b);
    (self.items.len() > ALL_ROWS_UP_TO).then(|| {
      list(
        self.items.list.clone(),
        cx.processor(|this, ix, _, cx| this.item(ix, cx)),
      )
      .with_sizing_behavior(ListSizingBehavior::Infer)
    })
  }

  /// The list's row `ix`.
  fn item(&mut self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
    let p = theme::palette(self.dark);
    match self.items.get(ix).copied() {
      Some(Item::Heading(g)) => ui::text(div(), TextSize::Xs)
        .px_3()
        .py_2()
        .mt_2()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(p.muted_foreground)
        .child(BRANCH_GROUPS[g].to_uppercase())
        .into_any_element(),
      Some(Item::Entry(index)) => {
        let selected = self.selected == Some(index);
        self.row(&p, index, selected, cx).into_any_element()
      }
      None => div().into_any_element(),
    }
  }
}

/// `getWorkspaceDescription`; React passes no change counts.
fn workspace_description(row: &WorkspaceRow) -> String {
  let agents = row.agents;
  format!(
    "{agents} agent{}, 0 shells, 0 changes",
    if agents == 1 { "" } else { "s" }
  )
}

impl Render for Overlay {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let p = theme::palette(self.dark);
    let viewport = window.viewport_size();
    let shortcuts = matches!(self.body, Body::Shortcuts);
    if shortcuts && !self.focused {
      self.focused = true;
      self.focus.focus(window, cx);
    }
    let dialog = if shortcuts {
      let this = cx.weak_entity();
      shortcuts::render(&p, viewport, move |_, cx| {
        this.update(cx, |this, cx| this.close(cx)).ok();
      })
      .into_any_element()
    } else {
      let input = self.input(window, cx);
      let palette = matches!(self.body, Body::Palette { .. });
      let switching = matches!(
        self.body,
        Body::Branches {
          switching: true,
          ..
        }
      );
      let messages = self.messages(&p);
      let list = self.list(window, cx);
      let rows: Vec<gpui::AnyElement> = if list.is_none() {
        (0..self.items.len()).map(|ix| self.item(ix, cx)).collect()
      } else {
        Vec::new()
      };
      let (hints, right): (&[(&str, &str)], _) = match self.body {
        Body::Branches { .. } => (
          &[("↑↓", "Navigate"), ("↵", "Switch"), ("Esc", "Close")],
          switching.then_some("Switching..."),
        ),
        _ => (
          &[("↑↓", "Navigate"), ("↵", "Select"), ("Esc", "Close")],
          None,
        ),
      };
      // `[cmdk-root]` is `bg-background`, square, behind the rounded box.
      div()
        .bg(p.background)
        .occlude()
        .child(
          div()
            .id("overlay-dialog")
            .debug_selector(|| "overlay-dialog".into())
            .role(gpui::Role::Dialog)
            .test_support()
            .w(viewport.width * 0.4)
            .map(|el| ui::floating(el, &p))
            .bg(if palette { p.background } else { p.popover })
            .text_color(if palette {
              p.foreground
            } else {
              p.popover_foreground
            })
            .overflow_hidden()
            .child(
              ui::text(div(), TextSize::Sm)
                .flex()
                .items_center()
                .h(zpx(48.))
                .px_3()
                .border_b_1()
                .border_color(p.border)
                .when(!palette, |this| {
                  this.child(ui::icon(theme::ICON_BRANCH, 16., p.muted_foreground).mr_2())
                })
                .child(
                  // `[cmdk-input]` has no padding of its own: the row's.
                  ui::input(&input)
                    .appearance(false)
                    .bordered(false)
                    .px_0()
                    .flex_1(),
                ),
            )
            .child(
              div()
                .id("overlay-list")
                .max_h(zpx(if palette { 300. } else { 400. }))
                .flex()
                .flex_col()
                .py_2()
                .children(messages)
                .map(|el| match list {
                  Some(list) => el.child(list.flex_1().min_h_0()),
                  None => el.child(
                    div()
                      .id("overlay-rows")
                      .flex()
                      .flex_col()
                      .min_h_0()
                      .overflow_y_scroll()
                      .track_scroll(&self.scroll)
                      .children(rows),
                  ),
                }),
            )
            .child(footer(&p, hints, right)),
        )
        .into_any_element()
    };
    div()
      .id("overlay-backdrop")
      .debug_selector(|| "overlay-backdrop".into())
      .track_focus(&self.focus)
      .size_full()
      .flex()
      .items_center()
      .justify_center()
      // The shortcuts sheet is a `ui::Dialog`, which draws its own scrim.
      .when(!shortcuts, |this| this.bg(p.scrim()))
      .font_family(theme::SANS_FONT)
      .text_color(p.foreground)
      .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
        this.key_down(event, cx);
      }))
      // Home and End reach the search field as its own actions.
      .capture_action(cx.listener(|this, _: &MoveHome, _, cx| {
        this.step(cmdk::Move::First, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|this, _: &MoveEnd, _, cx| {
        this.step(cmdk::Move::Last, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|this, _: &MoveToStart, _, cx| {
        this.step(cmdk::Move::First, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|this, _: &MoveToEnd, _, cx| {
        this.step(cmdk::Move::Last, cx);
        cx.stop_propagation();
      }))
      .on_mouse_down(
        gpui::MouseButton::Left,
        cx.listener(|this, _, _, cx| this.close(cx)),
      )
      .child(dialog)
  }
}
