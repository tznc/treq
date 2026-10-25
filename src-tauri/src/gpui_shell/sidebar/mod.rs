//! The workspace sidebar, drawn by GPUI.
//!
//! The window model builds its [`SidebarSnapshot`] and carries out every
//! click the sidebar sends back as a [`SidebarAction`].

use crate::gpui_shell::ui::{self, zpx, A11yDisabledExt as _, TextSize, Tone};
use crate::result_ext::ResultExt;
use std::{
  cell::{Cell, RefCell},
  rc::Rc,
  time::Duration,
};

use gpui_kit::base::TestSupportExt as _;
use serde::{Deserialize, Serialize};
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, px, svg, Animation, AnimationExt as _, AnyElement, App, ClickEvent,
  Context, Entity, Hsla, ListAlignment, ListState, MouseButton, MouseDownEvent, Pixels, Point,
  Role, SharedString, StyleRefinement, Transformation, Window,
};

use super::{
  popup::{PopupRequest, PopupSink, Side, TooltipContent},
  screen::prefetch,
  theme::{self, Palette},
  title_bar,
};

#[cfg(test)]
mod tests;

/// How long the pointer rests on an element before its tooltip shows, as in
/// the React app's `TooltipProvider`.
const TOOLTIP_DELAY: Duration = Duration::from_millis(200);

/// The sidebar's width range, as React clamps it (`sidebarWidthStore`).
const MIN_WIDTH: f32 = 200.;
const MAX_WIDTH: f32 = 480.;

/// What the sidebar shows, flattened for drawing.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarSnapshot {
  /// The command palette button's label.
  pub repo_name: String,
  pub settings_open: bool,
  /// The GitHub nav row; true while its page is open.
  pub github: bool,
  /// The Linear nav row, or `None` when the Linear preview is off.
  pub linear: Option<bool>,
  /// React's resolved theme, which can differ from the OS appearance.
  pub dark: bool,
  /// Logical pixels; the persisted sidebar width.
  pub width: f32,
  /// Why "Start agent" is unavailable here, or `None` when it works.
  #[serde(default)]
  pub agent_unavailable: Option<String>,
  /// Why "Open shell" is unavailable here, or `None` when it works.
  #[serde(default)]
  pub shell_unavailable: Option<String>,
  /// The "Archive N workspaces" button's count, or `None` to hide it.
  #[serde(default)]
  pub bulk_archive: Option<u32>,
  /// The window's repositories: the main one first.
  pub repos: Vec<RepoSection>,
  /// The opacity of an open webview modal's backdrop, continued over the
  /// sidebar; `None` with no modal open.
  #[serde(default)]
  pub modal: Option<f32>,
}

/// One repository: its home row and its workspaces.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSection {
  pub path: String,
  /// The home row's label: the checked-out branch, or a short commit hash
  /// for a detached HEAD. `None` until it loads.
  pub home_ref: Option<String>,
  pub home_selected: bool,
  /// The repository on screen; only it gets workspace actions.
  pub active: bool,
  /// The workspace list has not loaded yet.
  pub loading: bool,
  pub workspaces: Vec<SidebarRow>,
  /// The repository's name in a window with several; `None` with one.
  #[serde(default)]
  pub label: Option<String>,
  /// A supporting repository whose folder is gone.
  #[serde(default)]
  pub missing: bool,
  /// Its workspace group is folded away.
  #[serde(default)]
  pub collapsed: bool,
}

/// One workspace row, in stack order.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarRow {
  pub id: i64,
  /// The workspace's branch.
  pub title: String,
  /// The workspace's own title, drawn in place of the branch; `None` when
  /// it has none.
  #[serde(default)]
  pub name: Option<String>,
  /// 0 for a stack root, +1 per level below it.
  pub depth: u32,
  pub selected: bool,
  #[serde(default)]
  pub conflicted: bool,
  pub status: Option<RowStatus>,
  pub pr: Option<PrState>,
  /// The workspace's merge queue entry, when the merge queue is on.
  #[serde(default)]
  pub queue: Option<String>,
  /// Being archived: dimmed, with a spinner, and not clickable.
  #[serde(default)]
  pub archiving: bool,
  /// Archived and collapsing out of the list.
  #[serde(default)]
  pub exiting: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RowStatus {
  pub color: StatusColor,
  /// An agent in the workspace is streaming output.
  pub spin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusColor {
  Red,
  Yellow,
  Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrState {
  Draft,
  Open,
  Merged,
  Closed,
}

impl PrState {
  pub(super) fn label(self) -> &'static str {
    match self {
      PrState::Draft => "Draft PR",
      PrState::Open => "Open PR",
      PrState::Merged => "PR merged",
      PrState::Closed => "PR closed",
    }
  }
}

/// A click in the GPUI sidebar, for React to carry out. A workspace `id` of
/// `None` means the active repository's home.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SidebarAction {
  #[serde(rename_all = "camelCase")]
  SelectHome {
    repo_path: String,
  },
  /// A single click on a workspace row. `shift` extends a range from the
  /// last clicked row; `toggle` (Cmd or Ctrl) adds or removes the row.
  SelectWorkspace {
    id: i64,
    shift: bool,
    toggle: bool,
  },
  /// A double click on a workspace row: select its whole stack, or with
  /// Shift only the row and its descendants.
  SelectStack {
    id: i64,
    descendants: bool,
  },
  /// A click on the empty list background.
  ClearSelection,
  OpenSettings,
  OpenCommandPalette,
  OpenGithub,
  OpenLinear,
  /// The Open button beside WORKSPACES: opens workspaces from branches,
  /// links and pull requests.
  OpenWorkspaces,
  StartAgent {
    id: Option<i64>,
  },
  OpenShell {
    id: Option<i64>,
  },
  /// "Stack a workspace" on top of a workspace or the home branch.
  Stack {
    id: Option<i64>,
  },
  BulkArchive,
  /// Right-click on a repository's home row: React pops its native menu.
  #[serde(rename_all = "camelCase")]
  HomeMenu {
    repo_path: String,
  },
  /// Right-click on a workspace row.
  WorkspaceMenu {
    id: i64,
  },
  /// A click on the modal backdrop drawn over the sidebar.
  DismissModal,
  /// A workspace row dropped onto another row, to stack it there, or onto
  /// the list (`onto: None`), to move it to the home branch.
  MoveWorkspace {
    id: i64,
    onto: Option<i64>,
  },
  /// The resize handle was let go at `width`.
  ResizeSidebar {
    width: f32,
  },
  /// A double click on the resize handle.
  ResetSidebarWidth,
  /// A click on a workspace row of a repository that is not on screen.
  #[serde(rename_all = "camelCase")]
  OpenRepoWorkspace {
    repo_path: String,
    id: i64,
  },
  /// A right-click on a workspace row of a repository that is not on screen.
  #[serde(rename_all = "camelCase")]
  RepoWorkspaceMenu {
    repo_path: String,
    id: i64,
  },
  /// A click on a repository's workspace group label.
  #[serde(rename_all = "camelCase")]
  ToggleRepo {
    repo_path: String,
  },
  /// The pointer rests on a workspace row of the active repository: read
  /// ahead what selecting it would show.
  PrefetchWorkspace {
    id: i64,
  },
}

/// A workspace row being dragged.
#[derive(Clone)]
struct DraggedWorkspace {
  id: i64,
  title: SharedString,
  /// The title is a branch, drawn in mono as its row draws it.
  mono: bool,
  palette: Palette,
}

impl Render for DraggedWorkspace {
  fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
    ui::text(div(), TextSize::Sm)
      .px_2()
      .py_1()
      .rounded_md()
      .border_1()
      .border_color(self.palette.border)
      .bg(self.palette.sidebar)
      .text_color(self.palette.sidebar_foreground)
      .when(self.mono, |this| this.font_family(theme::MONO_FONT))
      .shadow_md()
      .child(self.title.clone())
  }
}

/// The resize handle being dragged.
#[derive(Clone, Copy)]
struct ResizeDrag;

impl Render for ResizeDrag {
  fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
    gpui::Empty
  }
}

/// Where the sidebar's actions go: the window model in the app, a list in
/// tests.
pub type ActionSink = Rc<dyn Fn(SidebarAction, &mut App)>;

/// The sidebar view of one window.
pub struct Sidebar {
  snapshot: SidebarSnapshot,
  sink: ActionSink,
  popups: PopupSink,
  /// The element whose tooltip popup is open.
  tooltip_sent: Rc<RefCell<Option<SharedString>>>,
  /// Where the last right-click was, for placing its menu.
  menu_anchor: Rc<Cell<Point<Pixels>>>,
  /// The element the pointer rests on, and whether its tooltip shows yet.
  tooltip: Option<(SharedString, bool)>,
  /// The row under the pointer. A button inside it takes the tooltip, and
  /// the row's own tooltip comes back when the pointer leaves the button.
  hovered_row: Option<SharedString>,
  /// The width while the resize handle is dragged, ahead of React.
  drag_width: Option<f32>,
  /// The workspace row the pointer rests on, to read ahead.
  prefetch: prefetch::Intent<i64>,
  /// The workspace list's entries, and the virtual list that draws only
  /// those on screen: a hover or a spinning status dot redraws a few rows,
  /// not every workspace.
  items: Vec<Item>,
  list: ListState,
}

/// One entry of the workspace list, in display order, by repository index.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Item {
  /// The repository's group label, in a window with several.
  Label(usize),
  /// A placeholder row while the repository's list loads.
  Skeleton(usize, usize),
  /// Workspace row `.1` of the repository.
  Row(usize, usize),
  /// "Archive N workspaces", under the active repository's rows.
  BulkArchive(usize),
}

impl Item {
  fn repo(self) -> usize {
    match self {
      Item::Label(repo)
      | Item::Skeleton(repo, _)
      | Item::Row(repo, _)
      | Item::BulkArchive(repo) => repo,
    }
  }
}

/// What the workspace list shows of `snapshot`.
fn items(snapshot: &SidebarSnapshot) -> Vec<Item> {
  let mut items = Vec::new();
  for (index, repo) in snapshot.repos.iter().enumerate() {
    if repo.label.is_some() {
      items.push(Item::Label(index));
    }
    if repo.collapsed {
      continue;
    }
    if repo.loading && repo.workspaces.is_empty() {
      items.extend((0..6).map(|i| Item::Skeleton(index, i)));
    }
    items.extend((0..repo.workspaces.len()).map(|row| Item::Row(index, row)));
    if repo.active && snapshot.bulk_archive.is_some() {
      items.push(Item::BulkArchive(index));
    }
  }
  items
}

impl Sidebar {
  pub fn new(snapshot: SidebarSnapshot, sink: ActionSink, popups: PopupSink) -> Self {
    let items = items(&snapshot);
    Self {
      list: ListState::new(items.len(), ListAlignment::Top, px(200.)),
      items,
      snapshot,
      sink,
      popups,
      tooltip_sent: Rc::default(),
      menu_anchor: Rc::default(),
      tooltip: None,
      hovered_row: None,
      drag_width: None,
      prefetch: prefetch::Intent::default(),
    }
  }

  /// The width drawn: the dragged width while resizing, else React's.
  pub fn width(&self) -> f32 {
    self.drag_width.unwrap_or(self.snapshot.width)
  }

  /// Ends a resize: keeps the new width until React sends it back, and
  /// tells React.
  fn finish_resize(&mut self, cx: &mut Context<Self>) {
    let Some(width) = self.drag_width.take() else {
      return;
    };
    self.snapshot.width = width;
    (self.sink)(SidebarAction::ResizeSidebar { width }, cx);
    cx.notify();
  }

  pub fn snapshot(&self) -> &SidebarSnapshot {
    &self.snapshot
  }

  /// Where the last right-click was, in the window's coordinates.
  pub fn menu_anchor(&self) -> Point<Pixels> {
    self.menu_anchor.get()
  }

  pub fn set_snapshot(&mut self, snapshot: SidebarSnapshot, cx: &mut Context<Self>) {
    if self.snapshot != snapshot {
      if snapshot.modal.is_some() {
        self.tooltip = None;
      }
      let items = items(&snapshot);
      if items != self.items {
        // Rows are one height, so the same index keeps the scroll in place.
        let top = self.list.logical_scroll_top();
        self.list.splice(0..self.items.len(), items.len());
        self.list.scroll_to(top);
        self.items = items;
      }
      self.snapshot = snapshot;
      cx.notify();
    }
  }

  /// The sidebar as the shell lays it out, cached: a frame another view
  /// asks for (a terminal's output, a hover in the screen) reuses the last
  /// drawing instead of building every row again. The sidebar notifies
  /// itself for everything it draws.
  pub fn element(this: &Entity<Self>, cx: &App) -> impl IntoElement {
    let width = this.read(cx).width();
    this.clone().cached(
      StyleRefinement::default()
        .flex_none()
        .w(zpx(width))
        .h_full(),
    )
  }

  /// A click handler that sends `action` and stops the click there, so a
  /// button inside a row does not also select the row.
  fn on_action(
    &self,
    action: SidebarAction,
  ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let sink = self.sink.clone();
    move |_, _, cx| {
      cx.stop_propagation();
      sink(action.clone(), cx);
    }
  }

  /// A workspace row's click handler, which reads the modifiers and click
  /// count into the action.
  fn on_row_click(&self, id: i64) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let sink = self.sink.clone();
    move |event, _, cx| {
      cx.stop_propagation();
      let modifiers = event.modifiers();
      let action = if event.click_count() >= 2 {
        SidebarAction::SelectStack {
          id,
          descendants: modifiers.shift,
        }
      } else {
        SidebarAction::SelectWorkspace {
          id,
          shift: modifiers.shift,
          toggle: modifiers.control || modifiers.platform,
        }
      };
      sink(action, cx);
    }
  }

  /// A drop handler that moves the dragged workspace onto `onto`.
  fn on_workspace_drop(
    &self,
    onto: Option<i64>,
  ) -> impl Fn(&DraggedWorkspace, &mut Window, &mut App) + 'static {
    let sink = self.sink.clone();
    move |dragged, _, cx| {
      if onto != Some(dragged.id) {
        sink(
          SidebarAction::MoveWorkspace {
            id: dragged.id,
            onto,
          },
          cx,
        );
      }
    }
  }

  /// A right-click handler that hides any tooltip, since the menu opens
  /// over it, notes where the menu goes, and sends `action`.
  fn on_menu(
    &self,
    action: SidebarAction,
    cx: &mut Context<Self>,
  ) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    let sidebar = cx.entity().downgrade();
    let sink = self.sink.clone();
    let anchor = self.menu_anchor.clone();
    move |event, _, cx| {
      cx.stop_propagation();
      anchor.set(event.position);
      sidebar
        .update(cx, |this, cx| {
          if this.tooltip.take().is_some() {
            cx.notify();
          }
        })
        .ignore_closed();
      sink(action.clone(), cx);
    }
  }

  /// Starts or stops the tooltip timer for element `key`.
  fn hover_tooltip(&mut self, key: SharedString, hovered: bool, cx: &mut Context<Self>) {
    if !hovered {
      if self.tooltip.as_ref().is_some_and(|(k, _)| *k == key) {
        self.tooltip = None;
        cx.notify();
        if let Some(row) = self.hovered_row.clone().filter(|row| *row != key) {
          self.start_tooltip(row, cx);
        }
      }
      return;
    }
    self.start_tooltip(key, cx);
  }

  /// Hovering a row; a button inside it keeps the tooltip if it has it.
  fn hover_row_tooltip(&mut self, key: SharedString, hovered: bool, cx: &mut Context<Self>) {
    if hovered {
      if self.hovered_row.as_ref() != Some(&key) {
        self.hovered_row = Some(key.clone());
        // The hovered workspace row reserves room for its actions.
        cx.notify();
      }
      if self.tooltip.is_some() {
        return;
      }
    } else if self.hovered_row.as_ref() == Some(&key) {
      self.hovered_row = None;
      cx.notify();
    }
    self.hover_tooltip(key, hovered, cx);
  }

  /// The pointer entered or left workspace `id`'s row: once it rests there,
  /// what selecting the row would show is read ahead.
  fn hover_workspace(&mut self, id: i64, hovered: bool, cx: &mut Context<Self>) {
    self.prefetch.hover(id, hovered, cx, move |this, cx| {
      (this.sink)(SidebarAction::PrefetchWorkspace { id }, cx)
    });
  }

  fn start_tooltip(&mut self, key: SharedString, cx: &mut Context<Self>) {
    self.tooltip = Some((key.clone(), false));
    cx.spawn(async move |this, cx| {
      cx.background_executor().timer(TOOLTIP_DELAY).await;
      this
        .update(cx, |this, cx| {
          if let Some((k, shown)) = this.tooltip.as_mut() {
            if *k == key && !*shown {
              *shown = true;
              cx.notify();
            }
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  fn tooltip_shown(&self, key: &str) -> bool {
    matches!(&self.tooltip, Some((k, true)) if k.as_ref() == key)
  }

  /// A layer over the element whose tooltip shows, which opens the tooltip
  /// popup beside the element once its bounds are known.
  /// Reports the bounds of its parent, `border` px wide borders included,
  /// as the anchor of the tooltip `key`.
  fn tooltip_anchor(
    &self,
    key: SharedString,
    side: Side,
    content: TooltipContent,
    border: f32,
  ) -> impl IntoElement {
    let sent = self.tooltip_sent.clone();
    let popups = self.popups.clone();
    let dark = self.snapshot.dark;
    gpui::canvas(
      move |anchor, _, cx| {
        if sent.borrow().as_ref() != Some(&key) {
          *sent.borrow_mut() = Some(key);
          popups(
            PopupRequest::ShowTooltip {
              anchor,
              side,
              content,
              dark,
            },
            cx,
          );
        }
      },
      |_, _, _, _| {},
    )
    .absolute()
    .top(zpx(-border))
    .left(zpx(-border))
    .right(zpx(-border))
    .bottom(zpx(-border))
  }

  /// Closes the tooltip popup once no tooltip shows.
  fn sync_tooltip_popup(&self, cx: &mut App) {
    let shown = matches!(self.tooltip, Some((_, true)));
    if !shown && self.tooltip_sent.borrow_mut().take().is_some() {
      (self.popups)(PopupRequest::HideTooltip, cx);
    }
  }
}

/// Indentation of a row's content, matching the React sidebar's
/// `16 + (depth - 1) * 6` px.
fn row_indent(depth: u32) -> f32 {
  16. + (depth as f32 - 1.) * 6.
}

/// Where a nested row's stack guide line sits, matching the React sidebar.
fn guide_left(depth: u32) -> f32 {
  8. + depth.saturating_sub(2) as f32 * 6.
}

fn status_color(color: StatusColor, palette: &Palette) -> Hsla {
  match color {
    StatusColor::Red => palette.destructive,
    StatusColor::Yellow => palette.warning,
    StatusColor::Neutral => palette.muted_foreground,
  }
}

pub(super) fn pr_color(pr: PrState, palette: &Palette) -> Hsla {
  match pr {
    PrState::Draft => palette.muted_foreground,
    PrState::Open => palette.pr_open,
    PrState::Merged => palette.pr_merged,
    PrState::Closed => palette.pr_closed,
  }
}

/// A merge queue entry's dot: color, whether it pulses, and its label, as in
/// the React sidebar.
fn queue_dot(status: &str, palette: &Palette) -> (Hsla, bool, String) {
  match status {
    "queued" => (Tone::Warning.color(palette), false, "In merge queue".into()),
    "testing" => (
      Tone::Primary.color(palette),
      true,
      "CI running in merge queue".into(),
    ),
    "merging" => (
      Tone::Success.color(palette),
      true,
      "Passed CI, merging now".into(),
    ),
    "merged" => (
      Tone::GitMerged.color(palette),
      false,
      "Merged via queue".into(),
    ),
    "failed" => (
      Tone::Danger.color(palette),
      false,
      "Failed in merge queue".into(),
    ),
    other => (palette.muted_foreground, false, other.into()),
  }
}

/// Three 20px row buttons with 2px gaps.
const ROW_ACTIONS_WIDTH: f32 = 3. * 20. + 2. * 2.;

fn icon(path: &'static str, size: f32, color: Hsla) -> gpui::Svg {
  svg()
    .path(path)
    .size(zpx(size))
    .flex_none()
    .text_color(color)
}

impl Render for Sidebar {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.sync_tooltip_popup(cx);
    let snapshot = &self.snapshot;
    let palette = theme::palette(snapshot.dark);
    let width = self.width();

    let header = div()
      .flex()
      .items_center()
      .gap_2()
      .p_2()
      .child(
        div()
          .id("command-palette")
          .debug_selector(|| "command-palette".into())
          .role(Role::Button)
          .aria_label("Command palette")
          .test_support()
          .flex()
          .flex_1()
          .min_w_0()
          .items_center()
          .gap_2()
          .px_3()
          .h(zpx(32.))
          .rounded_lg()
          .border_1()
          .border_color(palette.border)
          .bg(palette.muted.opacity(0.5))
          .cursor_pointer()
          .hover(|style| style.bg(palette.muted))
          .text_color(palette.muted_foreground)
          .child(icon(theme::ICON_SEARCH, 16., palette.muted_foreground))
          .child(
            div()
              .flex_1()
              .min_w_0()
              .truncate()
              .child(snapshot.repo_name.clone()),
          )
          .child(ui::Kbd::new("secondary-k", &palette))
          .on_click(self.on_action(SidebarAction::OpenCommandPalette)),
      )
      .child(self.header_button(
        "settings",
        theme::ICON_SETTINGS,
        "Settings",
        None,
        snapshot.settings_open,
        SidebarAction::OpenSettings,
        &palette,
        cx,
      ));

    let mut sidebar = ui::text(div(), TextSize::Sm)
      .id("sidebar")
      .debug_selector(|| "sidebar".into())
      .relative()
      .flex()
      .flex_col()
      .flex_none()
      .w(zpx(width))
      .h_full()
      .bg(palette.sidebar)
      .text_color(palette.sidebar_foreground)
      // Room for the right edge line, drawn as a child below so the modal
      // backdrop covers it (GPUI paints a border above the children).
      .pr_px()
      // The traffic lights sit here; the edge line runs past them to the
      // window's top edge.
      .when(title_bar::SHOWN, |this| {
        this.child(title_bar::drag_area("sidebar-title-bar"))
      })
      .child(header);

    for repo in &snapshot.repos {
      sidebar = sidebar.child(self.home_row(repo, &palette, cx));
    }
    sidebar = sidebar
      .child(self.nav_row(
        "github",
        theme::ICON_GITHUB,
        "Github",
        snapshot.github,
        SidebarAction::OpenGithub,
        &palette,
      ))
      .children(snapshot.linear.map(|active| {
        self.nav_row(
          "linear",
          theme::ICON_LINEAR,
          "Linear",
          active,
          SidebarAction::OpenLinear,
          &palette,
        )
      }));

    let grouped = snapshot.repos.iter().any(|repo| repo.label.is_some());
    let any_rows = self.items.iter().any(|item| matches!(item, Item::Row(..)));
    let list = div()
      .id("workspaces")
      .debug_selector(|| "workspaces".into())
      .flex_1()
      .min_h_0()
      // A click on the empty list clears the selection; it is not a control.
      .cursor_default()
      .on_click(self.on_action(SidebarAction::ClearSelection))
      .on_drop(self.on_workspace_drop(None))
      .child(
        gpui::list(
          self.list.clone(),
          cx.processor(|this, ix, _, cx| this.item(ix, cx)),
        )
        .size_full(),
      );

    sidebar
      .when(
        grouped || any_rows || snapshot.repos.iter().any(|r| r.loading),
        |this| this.child(div().mx_2().mt_2().h_px().bg(palette.border)),
      )
      .child(
        div()
          .flex()
          .items_center()
          .justify_between()
          .pl_4()
          .pr_2()
          .pt_3()
          .pb_1()
          .child(
            ui::text(div(), TextSize::Xs)
              .font_weight(gpui::FontWeight::MEDIUM)
              .text_color(palette.sidebar_foreground.opacity(0.7))
              .child("WORKSPACES"),
          )
          .when(snapshot.repos.iter().any(|r| r.active), |this| {
            this.child(self.open_workspaces_button(&palette))
          }),
      )
      .child(list)
      .child(
        div()
          .absolute()
          .top_0()
          .bottom_0()
          .right_0()
          .w(zpx(1.))
          .bg(palette.border),
      )
      .child(self.resize_handle(&palette, cx))
      .on_drag_move(
        cx.listener(|this, event: &gpui::DragMoveEvent<ResizeDrag>, _, cx| {
          let width = f32::from(event.event.position.x)
            .round()
            .clamp(MIN_WIDTH, MAX_WIDTH);
          if this.drag_width != Some(width) {
            this.drag_width = Some(width);
            cx.notify();
          }
        }),
      )
      .on_mouse_up(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| this.finish_resize(cx)),
      )
      .on_mouse_up_out(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| this.finish_resize(cx)),
      )
      .when_some(snapshot.modal, |this, opacity| {
        this.child(
          modal_backdrop(&palette, opacity).on_click(self.on_action(SidebarAction::DismissModal)),
        )
      })
  }
}

/// The webview modal's backdrop, continued over the sidebar in the same
/// translucent black. It takes every click and hover.
fn modal_backdrop(palette: &Palette, opacity: f32) -> gpui::Stateful<gpui::Div> {
  div()
    .id("modal-backdrop")
    .debug_selector(|| "modal-backdrop".into())
    .absolute()
    .inset_0()
    .bg(palette.scrim().alpha(opacity.clamp(0., 1.)))
    .occlude()
}

impl Sidebar {
  /// The WORKSPACES heading's Open button.
  fn open_workspaces_button(&self, palette: &Palette) -> impl IntoElement {
    ui::text(div(), TextSize::Xs)
      .id("open-workspaces")
      .debug_selector(|| "open-workspaces".into())
      .role(Role::Button)
      .aria_label("New workspaces")
      .test_support()
      .flex()
      .flex_none()
      .items_center()
      .gap_1()
      .px_1p5()
      .h(zpx(22.))
      .rounded_md()
      .text_color(palette.sidebar_foreground.opacity(0.7))
      .cursor_pointer()
      .hover(|style| style.bg(palette.accent))
      .child(icon(
        theme::ICON_BRANCH_PLUS,
        12.,
        palette.sidebar_foreground.opacity(0.7),
      ))
      .child("New")
      .on_click(self.on_action(SidebarAction::OpenWorkspaces))
  }

  /// A square header button with a tooltip below it.
  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  fn header_button(
    &self,
    id: &'static str,
    icon_path: &'static str,
    tooltip: &'static str,
    disabled_reason: Option<&str>,
    active: bool,
    action: SidebarAction,
    palette: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let key = SharedString::from(id);
    let disabled = disabled_reason.is_some();
    let tip = disabled_reason.unwrap_or(tooltip).to_string();
    div()
      .relative()
      .flex_none()
      .child(
        ui::IconButton::new(id, icon_path, tooltip, palette)
          .outline()
          .selected(active)
          .disabled(disabled)
          .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            this.hover_tooltip(key.clone(), *hovered, cx)
          }))
          .when(!disabled, |this| this.on_click(self.on_action(action))),
      )
      .when(self.tooltip_shown(id), |this| {
        this.child(self.tooltip_anchor(
          id.into(),
          Side::Bottom,
          TooltipContent::Text(tip.into()),
          0.,
        ))
      })
  }

  /// A small action button inside a row, with a tooltip.
  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  fn row_button(
    &self,
    id: SharedString,
    icon_path: &'static str,
    tooltip: &'static str,
    disabled_reason: Option<&str>,
    action: SidebarAction,
    palette: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let disabled = disabled_reason.is_some();
    let tip = disabled_reason.unwrap_or(tooltip).to_string();
    let key = id.clone();
    let selector = id.to_string();
    div()
      .id(id.clone())
      .debug_selector(move || selector)
      .role(Role::Button)
      .aria_label(tooltip)
      .a11y_disabled(disabled)
      .test_support()
      .relative()
      .flex()
      .flex_none()
      .items_center()
      .justify_center()
      .size(zpx(20.))
      .rounded_sm()
      .when(!disabled, |this| {
        this
          .cursor_pointer()
          .hover(|style| style.bg(palette.accent))
      })
      .when(disabled, |this| this.opacity(0.4))
      .child(icon(icon_path, 16., palette.foreground))
      .on_hover(
        cx.listener(move |this, hovered: &bool, _, cx| {
          this.hover_tooltip(key.clone(), *hovered, cx)
        }),
      )
      .when(self.tooltip_shown(&id), |this| {
        this.child(self.tooltip_anchor(
          id.clone(),
          Side::Bottom,
          TooltipContent::Text(tip.into()),
          0.,
        ))
      })
      // Swallow clicks on a disabled button too, so the row is not selected.
      .on_click(if disabled {
        Box::new(|_: &ClickEvent, _: &mut Window, cx: &mut App| cx.stop_propagation())
          as Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>
      } else {
        Box::new(self.on_action(action))
      })
  }

  /// The agent, shell and stack buttons of a home or workspace row, laid out
  /// [`ROW_ACTIONS_WIDTH`] wide.
  fn row_actions(
    &self,
    prefix: &str,
    id: Option<i64>,
    palette: &Palette,
    cx: &mut Context<Self>,
  ) -> gpui::Div {
    let snapshot = &self.snapshot;
    div()
      .flex()
      .flex_none()
      .items_center()
      .gap_0p5()
      .child(self.row_button(
        format!("{prefix}-start-agent").into(),
        theme::ICON_BOT,
        "Start agent",
        snapshot.agent_unavailable.as_deref(),
        SidebarAction::StartAgent { id },
        palette,
        cx,
      ))
      .child(self.row_button(
        format!("{prefix}-open-shell").into(),
        theme::ICON_TERMINAL,
        "Open shell",
        snapshot.shell_unavailable.as_deref(),
        SidebarAction::OpenShell { id },
        palette,
        cx,
      ))
      .child(self.row_button(
        format!("{prefix}-stack").into(),
        theme::ICON_LAYERS,
        "Stack workspace",
        None,
        SidebarAction::Stack { id },
        palette,
        cx,
      ))
  }

  /// A repository's home row: its checked-out branch, with the agent, shell
  /// and stack buttons always showing for the active repository.
  fn home_row(
    &self,
    repo: &RepoSection,
    palette: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let label = repo.home_ref.clone().unwrap_or_else(|| "…".into());
    let key = SharedString::from(format!("home:{}", repo.path));
    let title: gpui::AnyElement = match &repo.label {
      // Several repositories: the name, then the checked-out branch.
      Some(name) => div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_baseline()
        .gap_1p5()
        .child(
          div()
            .flex_none()
            .truncate()
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(name.clone()),
        )
        .when_some(
          repo.home_ref.clone().filter(|_| !repo.missing),
          |this, home_ref| {
            this.child(
              div()
                .min_w_0()
                .truncate()
                .font_family(theme::MONO_FONT)
                .text_color(palette.muted_foreground)
                .child(home_ref),
            )
          },
        )
        .into_any_element(),
      None => div()
        .flex_1()
        .min_w_0()
        .truncate()
        .font_family(theme::MONO_FONT)
        .child(label.clone())
        .into_any_element(),
    };
    let shown = self.tooltip_shown(&key);
    let tip = TooltipContent::Branch {
      name: None,
      title: label.clone().into(),
      conflicted: false,
      pr: None,
    };
    div()
      .id(key.clone())
      .debug_selector({
        let selector = match &repo.label {
          Some(name) if !repo.active => format!("home-{name}"),
          _ => "home".to_string(),
        };
        move || selector
      })
      .role(Role::Button)
      .aria_label(label)
      .aria_selected(repo.home_selected)
      .test_support()
      .relative()
      .flex()
      .items_center()
      .gap_1p5()
      .mx_2()
      .px_2()
      .h(zpx(32.))
      .rounded_md()
      .when(repo.home_selected, |this| this.bg(palette.selected()))
      .cursor_pointer()
      .when(!repo.home_selected, |this| {
        this.hover(|style| style.bg(palette.accent))
      })
      .when(repo.missing, |this| this.opacity(0.5))
      .child(icon(theme::ICON_HOUSE, 16., palette.sidebar_foreground))
      .child(title)
      .when(repo.missing, |this| {
        this.child(
          ui::text(div(), TextSize::Xs)
            .flex_none()
            .text_color(palette.muted_foreground)
            .child("missing"),
        )
      })
      .when(repo.active, |this| {
        this.child(self.row_actions("home", None, palette, cx))
      })
      .on_hover(cx.listener({
        let key = key.clone();
        move |this, hovered: &bool, _, cx| this.hover_row_tooltip(key.clone(), *hovered, cx)
      }))
      .when(shown, |this| {
        this.child(self.tooltip_anchor(key.clone(), Side::Right, tip, 0.))
      })
      .on_click(self.on_action(SidebarAction::SelectHome {
        repo_path: repo.path.clone(),
      }))
      .on_mouse_down(
        MouseButton::Right,
        self.on_menu(
          SidebarAction::HomeMenu {
            repo_path: repo.path.clone(),
          },
          cx,
        ),
      )
  }

  /// A row above the workspace list: GitHub or Linear.
  fn nav_row(
    &self,
    id: &'static str,
    icon_path: &'static str,
    text: &'static str,
    active: bool,
    action: SidebarAction,
    palette: &Palette,
  ) -> impl IntoElement {
    div()
      .id(id)
      .debug_selector(move || id.into())
      .role(Role::Button)
      .aria_label(text)
      .aria_selected(active)
      .test_support()
      .flex()
      .items_center()
      .gap_1p5()
      .mx_2()
      .px_2()
      .h(zpx(32.))
      .rounded_md()
      .cursor_pointer()
      .when(active, |this| this.bg(palette.selected()))
      .when(!active, |this| this.hover(|style| style.bg(palette.accent)))
      .child(icon(icon_path, 16., palette.sidebar_foreground))
      .child(div().flex_1().min_w_0().truncate().child(text))
      .on_click(self.on_action(action))
  }

  /// The strip on the sidebar's right edge that drags its width, or resets
  /// it on double click.
  fn resize_handle(&self, palette: &Palette, _cx: &mut Context<Self>) -> impl IntoElement {
    let dragging = self.drag_width.is_some();
    let primary = palette.primary;
    let sink = self.sink.clone();
    div()
      .id("resize-handle")
      .debug_selector(|| "resize-handle".into())
      .absolute()
      .top_0()
      .bottom_0()
      // Inside the sidebar, so the screen keeps every click right of it.
      .right_0()
      .w(zpx(4.))
      .cursor_ew_resize()
      .when(dragging, |this| this.bg(primary))
      .hover(move |style| style.bg(primary.opacity(0.5)))
      .on_drag(ResizeDrag, |drag, _, _, cx| cx.new(|_| *drag))
      .on_click(move |event: &ClickEvent, _, cx| {
        cx.stop_propagation();
        if event.click_count() >= 2 {
          sink(SidebarAction::ResetSidebarWidth, cx);
        }
      })
  }

  /// Entry `ix` of the workspace list, spaced as the list and its
  /// repository sections were as flex columns.
  fn item(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
    let Some(&item) = self.items.get(ix) else {
      return div().into_any_element();
    };
    let index = item.repo();
    let Some(repo) = self.snapshot.repos.get(index) else {
      return div().into_any_element();
    };
    let palette = theme::palette(self.snapshot.dark);
    let first_of_repo = ix == 0 || self.items[ix - 1].repo() != index;
    let gap = match item {
      _ if ix == 0 => 0.,
      _ if first_of_repo => 2.,
      _ => 4.,
    };
    let content = match item {
      Item::Label(_) => {
        let name = repo.label.as_deref().unwrap_or_default();
        self.group_label(repo, name, &palette).into_any_element()
      }
      Item::Skeleton(_, i) => skeleton_row(i, &palette).into_any_element(),
      Item::Row(_, row) => match repo.workspaces.get(row) {
        Some(row) => self
          .workspace_row(row, repo, index, &palette, cx)
          .into_any_element(),
        None => div().into_any_element(),
      },
      Item::BulkArchive(_) => {
        bulk_archive_button(self.snapshot.bulk_archive.unwrap_or(0), &palette)
          .on_click(self.on_action(SidebarAction::BulkArchive))
          .test_support()
          .into_any_element()
      }
    };
    // Each repository's rows sit in their own element, so workspace ids,
    // which are per repository, do not collide.
    div()
      .id(("repo-section", index))
      .px_2()
      .pt(zpx(gap))
      .when(ix + 1 == self.items.len(), |this| this.pb_2())
      .child(content)
      .into_any_element()
  }

  /// A repository's workspace group label in a window with several; a
  /// click folds the group.
  fn group_label(&self, repo: &RepoSection, name: &str, palette: &Palette) -> impl IntoElement {
    let chevron = if repo.collapsed {
      theme::ICON_CHEVRON_RIGHT
    } else {
      theme::ICON_CHEVRON_DOWN
    };
    let selector = format!("group-{name}");
    ui::text(div(), TextSize::Xs)
      .id("group-label")
      .debug_selector(move || selector)
      .flex()
      .items_center()
      .gap_1()
      .px_2()
      .h(zpx(32.))
      .rounded_md()
      .text_color(palette.sidebar_foreground.opacity(0.7))
      .cursor_pointer()
      .hover(|style| style.bg(palette.accent))
      .child(icon(chevron, 16., palette.sidebar_foreground.opacity(0.7)))
      .child(div().min_w_0().truncate().child(name.to_uppercase()))
      .on_click(self.on_action(SidebarAction::ToggleRepo {
        repo_path: repo.path.clone(),
      }))
  }

  fn workspace_row(
    &self,
    row: &SidebarRow,
    repo: &RepoSection,
    repo_index: usize,
    palette: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let id = row.id;
    let repo_active = repo.active;
    // Ids are per repository; rows of other repositories get their own keys.
    let prefix = if repo_active {
      String::new()
    } else {
      format!("repo{repo_index}-")
    };
    let group = SharedString::from(format!("{prefix}workspace-row-{id}"));
    let key = SharedString::from(format!("{prefix}workspace:{id}"));
    let selector = format!("{prefix}workspace-{id}");
    let branch_color = match row.pr {
      Some(pr) => pr_color(pr, palette),
      None => palette.muted_foreground,
    };
    let title_color = if row.selected {
      palette.sidebar_foreground
    } else {
      palette.muted_foreground
    };

    let label = row.name.as_ref().unwrap_or(&row.title).clone();
    let tip = TooltipContent::Branch {
      name: row.name.clone().map(Into::into),
      title: row.title.clone().into(),
      conflicted: row.conflicted,
      pr: row.pr,
    };

    let leading: gpui::AnyElement = if row.archiving {
      ui::spinner(("archiving", id as u64), 12., palette.muted_foreground).into_any_element()
    } else {
      // The React sidebar flips the branch icon vertically.
      icon(theme::ICON_BRANCH, 12., branch_color)
        .with_transformation(Transformation::scale(gpui::size(1., -1.)))
        .into_any_element()
    };

    // The status dot and queue dot give way to the action buttons on hover.
    let indicators = div()
      .flex()
      .flex_none()
      .items_center()
      .gap_1()
      .when(repo_active, |this| {
        this.group_hover(group.clone(), |style| style.invisible())
      })
      // A conflict is one danger dot: the red status dot already says it.
      .when(
        row.conflicted
          && !row
            .status
            .as_ref()
            .is_some_and(|s| s.color == StatusColor::Red),
        |this| {
          this.child(
            div()
              .debug_selector(move || format!("conflict-{id}"))
              .child(ui::Dot::new(Tone::Danger, palette)),
          )
        },
      )
      .when_some(row.queue.as_deref(), |this, status| {
        let (color, pulse, _) = queue_dot(status, palette);
        let dot = div()
          .id(("queue", id as u64))
          .debug_selector(move || format!("queue-{id}"))
          .flex_none()
          .size(zpx(8.))
          .rounded_full()
          .bg(color);
        if pulse {
          this.child(
            dot.with_animation(
              ("queue-pulse", id as u64),
              Animation::new(Duration::from_secs(2))
                .repeat()
                .with_max_fps(ui::PULSE_FPS),
              |dot, delta| dot.opacity(0.5 + 0.5 * (delta * std::f32::consts::TAU).cos().abs()),
            ),
          )
        } else {
          this.child(dot)
        }
      })
      .when_some(row.status.as_ref(), |this, status| {
        let color = status_color(status.color, palette);
        if status.spin {
          this.child(ui::spinner(("spin", id as u64), 14., color))
        } else {
          this.child(
            div()
              .debug_selector(move || format!("status-{id}"))
              .flex_none()
              .size(zpx(8.))
              .rounded_full()
              .bg(color),
          )
        }
      });

    let interactive = repo_active && !row.archiving;
    let row_el = div()
      .id(("workspace", id as u64))
      .debug_selector(move || selector)
      .role(Role::Button)
      .aria_label(label.clone())
      .aria_selected(row.selected)
      .test_support()
      .group(group.clone())
      .relative()
      .flex()
      .items_center()
      .gap_1p5()
      .h(zpx(32.))
      .pl(zpx(row_indent(row.depth)))
      .pr_2()
      .rounded_md()
      .when(!row.archiving, |this| this.cursor_pointer())
      .when(row.selected, |this| this.bg(palette.selected()))
      .when(!row.selected && !row.archiving, |this| {
        this.hover(|style| style.bg(palette.hover()))
      })
      .when(row.archiving, |this| this.opacity(0.5))
      .when(row.depth > 0, |this| {
        // An elbow into the row, not a bar that reads as a selection marker.
        this.child(
          div()
            .absolute()
            .top_0()
            .left(zpx(guide_left(row.depth)))
            .h(zpx(16.))
            .w(zpx(6.))
            .border_l_1()
            .border_b_1()
            .border_color(palette.border)
            .rounded_bl(ui::radius::SM),
        )
      })
      .child(leading)
      .child(
        div()
          .debug_selector({
            let selector = format!("{prefix}workspace-{id}-title");
            move || selector
          })
          .flex_1()
          .min_w_0()
          .truncate()
          // A branch is code; a title is prose.
          .when(row.name.is_none(), |this| {
            this.font_family(theme::MONO_FONT)
          })
          .text_color(title_color)
          .child(label.clone()),
      )
      .child(
        div()
          .relative()
          .flex()
          .flex_none()
          .items_center()
          .justify_end()
          .child(indicators)
          .when(interactive, |this| {
            // Hovering reserves the buttons' width, so the title truncates
            // before them instead of running under them. Hover styles do not
            // reach layout, so this follows the hovered row instead.
            this
              .when(self.hovered_row.as_ref() == Some(&key), |this| {
                this.min_w(zpx(ROW_ACTIONS_WIDTH))
              })
              .child(
                self
                  .row_actions(&format!("workspace-{id}"), Some(id), palette, cx)
                  .absolute()
                  .right_0()
                  .invisible()
                  .group_hover(group.clone(), |style| style.visible()),
              )
          }),
      )
      .on_hover(cx.listener({
        let key = key.clone();
        let prefetch = !row.archiving && !row.selected && repo_active;
        move |this, hovered: &bool, _, cx| {
          this.hover_row_tooltip(key.clone(), *hovered, cx);
          if prefetch {
            this.hover_workspace(id, *hovered, cx);
          }
        }
      }))
      .when(self.tooltip_shown(&key), |this| {
        this.child(self.tooltip_anchor(key.clone(), Side::Right, tip, 0.))
      })
      .when(!row.archiving && repo_active, |this| {
        this.on_click(self.on_row_click(id))
      })
      .when(!repo_active, |this| {
        this.on_click(self.on_action(SidebarAction::OpenRepoWorkspace {
          repo_path: repo.path.clone(),
          id,
        }))
      })
      .when(interactive, |this| {
        let primary = palette.primary;
        this
          .on_drag(
            DraggedWorkspace {
              id,
              title: label.clone().into(),
              mono: row.name.is_none(),
              palette: *palette,
            },
            |dragged, _, _, cx| cx.new(|_| dragged.clone()),
          )
          .drag_over::<DraggedWorkspace>(move |style, dragged, _, _| {
            if dragged.id == id {
              style
            } else {
              style.bg(primary.opacity(0.2))
            }
          })
          .on_drop(self.on_workspace_drop(Some(id)))
      })
      // An archiving row swallows the click, so it does not clear the selection.
      .when(row.archiving, |this| {
        this.on_click(|_, _, cx| cx.stop_propagation())
      })
      .on_mouse_down(
        MouseButton::Right,
        self.on_menu(
          if repo_active {
            SidebarAction::WorkspaceMenu { id }
          } else {
            SidebarAction::RepoWorkspaceMenu {
              repo_path: repo.path.clone(),
              id,
            }
          },
          cx,
        ),
      );

    if row.exiting {
      // Collapse out of the list, as the React sidebar does before the
      // archived workspace is dropped from the data.
      row_el
        .with_animation(
          ("exiting", id as u64),
          Animation::new(Duration::from_millis(200)),
          |row, delta| row.h(zpx(32. * (1. - delta))).opacity(1. - delta),
        )
        .into_any_element()
    } else {
      row_el.into_any_element()
    }
  }
}

/// A placeholder row while the workspace list loads.
fn skeleton_row(index: usize, palette: &Palette) -> impl IntoElement {
  // Varying widths, like the React sidebar's skeleton.
  let width = [70., 55., 80., 60., 75., 50.][index % 6];
  div()
    .debug_selector(move || format!("skeleton-{index}"))
    .flex()
    .items_center()
    .gap_2()
    .h(zpx(32.))
    .px_2()
    .child(div().size(zpx(16.)).rounded_md().bg(palette.muted))
    .child(
      div()
        .h(zpx(16.))
        .w(gpui::relative(width / 100.))
        .rounded_md()
        .bg(palette.muted),
    )
}

/// "Archive N workspaces", shown while workspaces are multi-selected.
fn bulk_archive_button(count: u32, palette: &Palette) -> gpui::Stateful<gpui::Div> {
  let label = if count == 1 {
    "Archive 1 workspace".to_string()
  } else {
    format!("Archive {count} workspaces")
  };
  div()
    .id("bulk-archive")
    .debug_selector(|| "bulk-archive".into())
    .role(Role::Button)
    .aria_label(label.clone())
    .flex()
    .items_center()
    .justify_center()
    .gap_2()
    .mt_2()
    .h(zpx(32.))
    .rounded_md()
    .bg(palette.destructive)
    .text_color(palette.destructive_foreground)
    .cursor_pointer()
    .hover(|style| style.opacity(0.9))
    .child(icon(
      theme::ICON_ARCHIVE,
      14.,
      palette.destructive_foreground,
    ))
    .child(label)
}
