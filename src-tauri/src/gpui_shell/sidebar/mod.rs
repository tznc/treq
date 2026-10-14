//! The workspace sidebar, drawn by GPUI.
//!
//! React still owns the sidebar's data: it builds a [`SidebarSnapshot`] from
//! the same queries the rest of the app uses and pushes it with the
//! `gpui_sidebar_update` command. GPUI draws it, and sends every click back
//! to the window's webview as a `gpui-sidebar-action` event, which React
//! carries out with the app's own handlers.

use std::{rc::Rc, time::Duration};

use serde::{Deserialize, Serialize};
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, px, rgb, svg, Animation, AnimationExt as _, App, ClickEvent, Context,
  Hsla, MouseButton, MouseDownEvent, SharedString, Transformation, Window,
};

use super::theme::{self, Palette};

#[cfg(test)]
mod tests;

/// Event React listens to on its own webview window.
pub const ACTION_EVENT: &str = "gpui-sidebar-action";

/// How long the pointer rests on an element before its tooltip shows, as in
/// the React app's `TooltipProvider`.
const TOOLTIP_DELAY: Duration = Duration::from_millis(200);

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
}

/// One workspace row, in stack order.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarRow {
  pub id: i64,
  pub title: String,
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
  fn label(self) -> &'static str {
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
  SelectWorkspace {
    id: i64,
  },
  OpenSettings,
  OpenCommandPalette,
  OpenGithub,
  OpenLinear,
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
}

/// Where the sidebar's actions go: the window's webview in the app, a list
/// in tests.
pub type ActionSink = Rc<dyn Fn(SidebarAction, &mut App)>;

/// The sidebar view of one window.
pub struct Sidebar {
  snapshot: SidebarSnapshot,
  sink: ActionSink,
  /// The element the pointer rests on, and whether its tooltip shows yet.
  tooltip: Option<(SharedString, bool)>,
  /// The row under the pointer. A button inside it takes the tooltip, and
  /// the row's own tooltip comes back when the pointer leaves the button.
  hovered_row: Option<SharedString>,
}

impl Sidebar {
  pub fn new(snapshot: SidebarSnapshot, sink: ActionSink) -> Self {
    Self {
      snapshot,
      sink,
      tooltip: None,
      hovered_row: None,
    }
  }

  pub fn snapshot(&self) -> &SidebarSnapshot {
    &self.snapshot
  }

  pub fn set_snapshot(&mut self, snapshot: SidebarSnapshot, cx: &mut Context<Self>) {
    if self.snapshot != snapshot {
      if snapshot.modal.is_some() {
        self.tooltip = None;
      }
      self.snapshot = snapshot;
      cx.notify();
    }
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

  /// A right-click handler that hides any tooltip, since the native menu
  /// opens over it, and sends `action`.
  fn on_menu(
    &self,
    action: SidebarAction,
    cx: &mut Context<Self>,
  ) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    let sidebar = cx.entity().downgrade();
    let sink = self.sink.clone();
    move |_, _, cx| {
      cx.stop_propagation();
      let _ = sidebar.update(cx, |this, cx| {
        if this.tooltip.take().is_some() {
          cx.notify();
        }
      });
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
      self.hovered_row = Some(key.clone());
      if self.tooltip.is_some() {
        return;
      }
    } else if self.hovered_row.as_ref() == Some(&key) {
      self.hovered_row = None;
    }
    self.hover_tooltip(key, hovered, cx);
  }

  fn start_tooltip(&mut self, key: SharedString, cx: &mut Context<Self>) {
    self.tooltip = Some((key.clone(), false));
    cx.spawn(async move |this, cx| {
      cx.background_executor().timer(TOOLTIP_DELAY).await;
      let _ = this.update(cx, |this, cx| {
        if let Some((k, shown)) = this.tooltip.as_mut() {
          if *k == key && !*shown {
            *shown = true;
            cx.notify();
          }
        }
      });
    })
    .detach();
  }

  fn tooltip_shown(&self, key: &str) -> bool {
    matches!(&self.tooltip, Some((k, true)) if k.as_ref() == key)
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

fn pr_color(pr: PrState, palette: &Palette) -> Hsla {
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
    "queued" => (rgb(0xfacc15).into(), false, "In merge queue".into()),
    "testing" => (
      rgb(0x60a5fa).into(),
      true,
      "CI running in merge queue".into(),
    ),
    "merging" => (rgb(0x4ade80).into(), true, "Passed CI, merging now".into()),
    "merged" => (rgb(0x16a34a).into(), false, "Merged via queue".into()),
    "failed" => (rgb(0xef4444).into(), false, "Failed in merge queue".into()),
    other => (palette.muted_foreground, false, other.into()),
  }
}

fn icon(path: &'static str, size: f32, color: Hsla) -> gpui::Svg {
  svg()
    .path(path)
    .size(px(size))
    .flex_none()
    .text_color(color)
}

fn spinner(id: impl Into<gpui::ElementId>, size: f32, color: Hsla) -> impl IntoElement {
  icon(theme::ICON_SPINNER, size, color).with_animation(
    id,
    Animation::new(Duration::from_secs(1)).repeat(),
    |svg, delta| svg.with_transformation(Transformation::rotate(gpui::percentage(delta))),
  )
}

/// A tooltip bubble below its element, kept inside the sidebar: the
/// webview covers anything drawn past the sidebar's right edge.
fn tooltip_bubble(
  palette: &Palette,
  max_width: f32,
  content: gpui::AnyElement,
) -> impl IntoElement {
  bubble(palette, max_width, content, false)
}

/// A one-line tooltip under a small button, right-aligned to it so it stays
/// clear of the sidebar's right edge.
fn button_tooltip_bubble(
  palette: &Palette,
  max_width: f32,
  content: gpui::AnyElement,
) -> impl IntoElement {
  bubble(palette, max_width, content, true)
}

fn bubble(
  palette: &Palette,
  max_width: f32,
  content: gpui::AnyElement,
  button: bool,
) -> impl IntoElement {
  gpui::deferred(
    div()
      .absolute()
      .top_full()
      .when(button, |this| this.right_0().whitespace_nowrap())
      .when(!button, |this| this.left_0())
      .mt_1()
      .max_w(px(max_width))
      .px_2()
      .py_1()
      .rounded_md()
      .border_1()
      .border_color(palette.border)
      .bg(palette.background)
      .text_color(palette.foreground)
      .text_size(px(12.))
      .shadow_md()
      .child(content),
  )
  .with_priority(1)
}

impl Render for Sidebar {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let snapshot = &self.snapshot;
    let palette = theme::palette(snapshot.dark);
    let tooltip_width = (snapshot.width - 24.).max(80.);

    let header = div()
      .flex()
      .items_center()
      .gap_2()
      .p_2()
      .child(
        div()
          .id("command-palette")
          .debug_selector(|| "command-palette".into())
          .flex()
          .flex_1()
          .min_w_0()
          .items_center()
          .gap_2()
          .px_3()
          .h(px(32.))
          .rounded_lg()
          .border_1()
          .border_color(palette.border)
          .bg(palette.muted.opacity(0.5))
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
          .child(
            div()
              .flex_none()
              .px_1()
              .rounded_sm()
              .text_size(px(11.))
              .bg(palette.muted)
              // Same label as the React app shows on every platform.
              .child("⌘ + K"),
          )
          .on_click(self.on_action(SidebarAction::OpenCommandPalette)),
      )
      .child(self.icon_button(
        "settings",
        theme::ICON_SETTINGS,
        "Settings",
        None,
        snapshot.settings_open,
        SidebarAction::OpenSettings,
        &palette,
        tooltip_width,
        cx,
      ));

    let mut sidebar = div()
      .id("sidebar")
      .debug_selector(|| "sidebar".into())
      .relative()
      .flex()
      .flex_col()
      .flex_none()
      .w(px(snapshot.width))
      .h_full()
      .bg(palette.sidebar)
      .text_color(palette.sidebar_foreground)
      .text_size(px(13.))
      .border_r_1()
      .border_color(palette.border)
      .child(header);

    for repo in &snapshot.repos {
      sidebar = sidebar.child(self.home_row(repo, &palette, tooltip_width, cx));
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

    let mut list = div()
      .id("workspaces")
      .flex()
      .flex_col()
      .flex_1()
      .min_h_0()
      .overflow_y_scroll()
      .px_2()
      .pb_2()
      .gap(px(2.));
    let mut any_rows = false;
    for repo in &snapshot.repos {
      if repo.loading && repo.workspaces.is_empty() {
        list = list.children((0..6).map(|i| skeleton_row(i, &palette)));
      }
      for row in &repo.workspaces {
        any_rows = true;
        list = list.child(self.workspace_row(row, repo.active, &palette, tooltip_width, cx));
      }
      if repo.active {
        if let Some(count) = snapshot.bulk_archive {
          list = list.child(
            bulk_archive_button(count, &palette)
              .on_click(self.on_action(SidebarAction::BulkArchive)),
          );
        }
      }
    }

    sidebar
      .when(
        any_rows || snapshot.repos.iter().any(|r| r.loading),
        |this| this.child(div().mx_2().mt_2().h_px().bg(palette.border)),
      )
      .child(
        div()
          .px_4()
          .pt_3()
          .pb_1()
          .text_size(px(11.))
          .text_color(palette.muted_foreground)
          .child("Workspaces"),
      )
      .child(list)
      .when_some(snapshot.modal, |this, opacity| {
        this.child(modal_backdrop(opacity).on_click(self.on_action(SidebarAction::DismissModal)))
      })
  }
}

/// The webview modal's backdrop, continued over the sidebar in the same
/// translucent black. It takes every click and hover.
fn modal_backdrop(opacity: f32) -> gpui::Stateful<gpui::Div> {
  div()
    .id("modal-backdrop")
    .debug_selector(|| "modal-backdrop".into())
    .absolute()
    .inset_0()
    // `inset_0` stops inside the sidebar's right border; cover it too, or it
    // shows as a light line between the two backdrops.
    .right(px(-1.))
    .bg(gpui::black().opacity(opacity.clamp(0., 1.)))
    .occlude()
}

impl Sidebar {
  /// A square header button with a tooltip below it.
  #[allow(clippy::too_many_arguments)]
  fn icon_button(
    &self,
    id: &'static str,
    icon_path: &'static str,
    tooltip: &'static str,
    disabled_reason: Option<&str>,
    active: bool,
    action: SidebarAction,
    palette: &Palette,
    tooltip_width: f32,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let key = SharedString::from(id);
    let disabled = disabled_reason.is_some();
    let tip = disabled_reason.unwrap_or(tooltip).to_string();
    div()
      .id(id)
      .debug_selector(move || id.into())
      .relative()
      .flex()
      .flex_none()
      .items_center()
      .justify_center()
      .size(px(32.))
      .rounded_lg()
      .border_1()
      .border_color(palette.border)
      .when(active, |this| this.bg(palette.primary.opacity(0.2)))
      .when(!disabled, |this| {
        this.hover(|style| style.bg(palette.accent))
      })
      .when(disabled, |this| this.opacity(0.5))
      .child(icon(
        icon_path,
        16.,
        if active {
          palette.primary
        } else {
          palette.foreground
        },
      ))
      .on_hover(
        cx.listener(move |this, hovered: &bool, _, cx| {
          this.hover_tooltip(key.clone(), *hovered, cx)
        }),
      )
      .when(self.tooltip_shown(id), |this| {
        this.child(button_tooltip_bubble(
          palette,
          tooltip_width,
          tip.into_any_element(),
        ))
      })
      .when(!disabled, |this| this.on_click(self.on_action(action)))
  }

  /// A small action button inside a row, with a tooltip.
  #[allow(clippy::too_many_arguments)]
  fn row_button(
    &self,
    id: SharedString,
    icon_path: &'static str,
    tooltip: &'static str,
    disabled_reason: Option<&str>,
    action: SidebarAction,
    palette: &Palette,
    tooltip_width: f32,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let disabled = disabled_reason.is_some();
    let tip = disabled_reason.unwrap_or(tooltip).to_string();
    let key = id.clone();
    let selector = id.to_string();
    div()
      .id(id.clone())
      .debug_selector(move || selector)
      .relative()
      .flex()
      .flex_none()
      .items_center()
      .justify_center()
      .size(px(20.))
      .rounded_sm()
      .when(!disabled, |this| {
        this.hover(|style| style.bg(palette.accent))
      })
      .when(disabled, |this| this.opacity(0.4))
      .child(icon(icon_path, 12., palette.muted_foreground))
      .on_hover(
        cx.listener(move |this, hovered: &bool, _, cx| {
          this.hover_tooltip(key.clone(), *hovered, cx)
        }),
      )
      .when(self.tooltip_shown(&id), |this| {
        this.child(button_tooltip_bubble(
          palette,
          tooltip_width,
          tip.into_any_element(),
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

  /// The agent, shell and stack buttons of a home or workspace row.
  fn row_actions(
    &self,
    prefix: &str,
    id: Option<i64>,
    palette: &Palette,
    tooltip_width: f32,
    cx: &mut Context<Self>,
  ) -> gpui::Div {
    let snapshot = &self.snapshot;
    div()
      .flex()
      .flex_none()
      .items_center()
      .gap(px(2.))
      .child(self.row_button(
        format!("{prefix}-start-agent").into(),
        theme::ICON_BOT,
        "Start agent",
        snapshot.agent_unavailable.as_deref(),
        SidebarAction::StartAgent { id },
        palette,
        tooltip_width,
        cx,
      ))
      .child(self.row_button(
        format!("{prefix}-open-shell").into(),
        theme::ICON_TERMINAL,
        "Open shell",
        snapshot.shell_unavailable.as_deref(),
        SidebarAction::OpenShell { id },
        palette,
        tooltip_width,
        cx,
      ))
      .child(self.row_button(
        format!("{prefix}-stack").into(),
        theme::ICON_LAYERS,
        "Stack workspace",
        None,
        SidebarAction::Stack { id },
        palette,
        tooltip_width,
        cx,
      ))
  }

  /// A repository's home row: its checked-out branch, with the agent, shell
  /// and stack buttons always showing for the active repository.
  fn home_row(
    &self,
    repo: &RepoSection,
    palette: &Palette,
    tooltip_width: f32,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let label = repo.home_ref.clone().unwrap_or_else(|| "…".into());
    let key = SharedString::from(format!("home:{}", repo.path));
    let shown = self.tooltip_shown(&key);
    let tip = div()
      .flex()
      .items_center()
      .gap_1()
      .font_family(theme::MONO_FONT)
      .child(icon(theme::ICON_BRANCH, 12., palette.foreground))
      .child(label.clone())
      .into_any_element();
    div()
      .id(key.clone())
      .debug_selector(|| "home".into())
      .relative()
      .flex()
      .items_center()
      .gap(px(6.))
      .mx_2()
      .px_2()
      .h(px(32.))
      .rounded_md()
      .when(repo.home_selected, |this| {
        this.bg(palette.primary.opacity(0.2))
      })
      .when(!repo.home_selected, |this| {
        this.hover(|style| style.bg(palette.accent))
      })
      .child(icon(theme::ICON_HOUSE, 12., palette.sidebar_foreground))
      .child(
        div()
          .flex_1()
          .min_w_0()
          .truncate()
          .font_family(theme::MONO_FONT)
          .child(label),
      )
      .when(repo.active, |this| {
        this.child(self.row_actions("home", None, palette, tooltip_width, cx))
      })
      .on_hover(cx.listener({
        let key = key.clone();
        move |this, hovered: &bool, _, cx| this.hover_row_tooltip(key.clone(), *hovered, cx)
      }))
      .when(shown, |this| {
        this.child(tooltip_bubble(palette, tooltip_width, tip))
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
      .flex()
      .items_center()
      .gap(px(6.))
      .mx_2()
      .px_2()
      .h(px(32.))
      .rounded_md()
      .when(active, |this| this.bg(palette.primary.opacity(0.2)))
      .when(!active, |this| this.hover(|style| style.bg(palette.accent)))
      .child(icon(icon_path, 12., palette.sidebar_foreground))
      .child(div().flex_1().min_w_0().truncate().child(text))
      .on_click(self.on_action(action))
  }

  fn workspace_row(
    &self,
    row: &SidebarRow,
    repo_active: bool,
    palette: &Palette,
    tooltip_width: f32,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let id = row.id;
    let group = SharedString::from(format!("workspace-row-{id}"));
    let key = SharedString::from(format!("workspace:{id}"));
    let branch_color = match row.pr {
      Some(pr) => pr_color(pr, palette),
      None if row.selected => palette.primary,
      None => palette.muted_foreground,
    };
    let title_color = if row.conflicted {
      palette.destructive
    } else if row.selected {
      palette.primary
    } else {
      palette.muted_foreground
    };

    let tip = div()
      .flex()
      .flex_col()
      .gap_1()
      .font_family(theme::MONO_FONT)
      .child(
        div()
          .flex()
          .items_center()
          .gap_1()
          .child(icon(theme::ICON_BRANCH, 12., palette.foreground))
          .child(row.title.clone()),
      )
      .when(row.conflicted, |this| {
        this.child(
          div()
            .text_color(palette.destructive)
            .child("Conflicts detected"),
        )
      })
      .when_some(row.pr, |this, pr| {
        this.child(div().text_color(pr_color(pr, palette)).child(pr.label()))
      })
      .into_any_element();

    let leading: gpui::AnyElement = if row.archiving {
      spinner(("archiving", id as u64), 12., palette.muted_foreground).into_any_element()
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
      .when_some(row.queue.as_deref(), |this, status| {
        let (color, pulse, _) = queue_dot(status, palette);
        let dot = div()
          .id(("queue", id as u64))
          .debug_selector(move || format!("queue-{id}"))
          .flex_none()
          .size(px(8.))
          .rounded_full()
          .bg(color);
        if pulse {
          this.child(dot.with_animation(
            ("queue-pulse", id as u64),
            Animation::new(Duration::from_secs(2)).repeat(),
            |dot, delta| dot.opacity(0.5 + 0.5 * (delta * std::f32::consts::TAU).cos().abs()),
          ))
        } else {
          this.child(dot)
        }
      })
      .when_some(row.status.as_ref(), |this, status| {
        let color = status_color(status.color, palette);
        if status.spin {
          this.child(spinner(("spin", id as u64), 14., color))
        } else {
          this.child(
            div()
              .debug_selector(move || format!("status-{id}"))
              .flex_none()
              .size(px(8.))
              .rounded_full()
              .bg(color),
          )
        }
      });

    let interactive = repo_active && !row.archiving;
    let row_el = div()
      .id(("workspace", id as u64))
      .debug_selector(move || format!("workspace-{id}"))
      .group(group.clone())
      .relative()
      .flex()
      .items_center()
      .gap(px(6.))
      .h(px(32.))
      .pl(px(row_indent(row.depth)))
      .pr_2()
      .rounded_md()
      .when(row.selected, |this| this.bg(palette.primary.opacity(0.2)))
      .when(!row.selected && !row.archiving, |this| {
        this.hover(|style| style.bg(palette.muted.opacity(0.5)))
      })
      .when(row.archiving, |this| this.opacity(0.5))
      .when(row.depth > 0, |this| {
        this.child(
          div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(guide_left(row.depth)))
            .w(px(2.))
            .bg(palette.stack_guide),
        )
      })
      .child(leading)
      .child(
        div()
          .flex_1()
          .min_w_0()
          .truncate()
          .font_family(theme::MONO_FONT)
          .text_color(title_color)
          .when(row.selected, |this| {
            this.font_weight(gpui::FontWeight::MEDIUM)
          })
          .child(row.title.clone()),
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
            this.child(
              self
                .row_actions(
                  &format!("workspace-{id}"),
                  Some(id),
                  palette,
                  tooltip_width,
                  cx,
                )
                .absolute()
                .right_0()
                .invisible()
                .group_hover(group.clone(), |style| style.visible()),
            )
          }),
      )
      .on_hover(cx.listener({
        let key = key.clone();
        move |this, hovered: &bool, _, cx| this.hover_row_tooltip(key.clone(), *hovered, cx)
      }))
      .when(self.tooltip_shown(&key), |this| {
        this.child(tooltip_bubble(palette, tooltip_width, tip))
      })
      .when(!row.archiving, |this| {
        this.on_click(self.on_action(SidebarAction::SelectWorkspace { id }))
      })
      .on_mouse_down(
        MouseButton::Right,
        self.on_menu(SidebarAction::WorkspaceMenu { id }, cx),
      );

    if row.exiting {
      // Collapse out of the list, as the React sidebar does before the
      // archived workspace is dropped from the data.
      row_el
        .with_animation(
          ("exiting", id as u64),
          Animation::new(Duration::from_millis(200)),
          |row, delta| row.h(px(32. * (1. - delta))).opacity(1. - delta),
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
    .h(px(32.))
    .px_2()
    .child(div().size(px(16.)).rounded_md().bg(palette.muted))
    .child(
      div()
        .h(px(16.))
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
    .flex()
    .items_center()
    .justify_center()
    .gap_2()
    .mt_2()
    .h(px(32.))
    .rounded_md()
    .bg(palette.destructive)
    .text_color(gpui::white())
    .hover(|style| style.opacity(0.9))
    .child(icon(theme::ICON_ARCHIVE, 14., gpui::white()))
    .child(label)
}
