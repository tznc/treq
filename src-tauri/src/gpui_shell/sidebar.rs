//! The workspace sidebar, drawn by GPUI.
//!
//! React still owns the sidebar's data: it builds a [`SidebarSnapshot`] from
//! the same queries its own sidebar uses and pushes it with the
//! `gpui_sidebar_update` command. GPUI only draws it, and sends clicks back
//! to the window's webview as `gpui-sidebar-action` events, which React
//! handles with the same code as clicks in its own sidebar.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, EventTarget};
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, px, svg, Animation, AnimationExt as _, Hsla, SharedString, Transformation,
};

use super::theme::{self, Palette};

/// Event React listens to on its own webview window.
pub const ACTION_EVENT: &str = "gpui-sidebar-action";

/// What React shows in its sidebar, flattened for drawing.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarSnapshot {
  pub repo_name: String,
  /// The home row's label: the repository's current branch.
  pub home_branch: Option<String>,
  pub home_selected: bool,
  pub settings_open: bool,
  /// The GitHub nav row; true while its page is open.
  pub github: bool,
  /// The Linear nav row, or `None` when the Linear preview is off.
  pub linear: Option<bool>,
  /// React's resolved theme, which can differ from the OS appearance.
  pub dark: bool,
  /// Logical pixels; React's persisted sidebar width.
  pub width: f32,
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
  pub status: Option<RowStatus>,
  pub pr: Option<PrState>,
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

/// A click in the GPUI sidebar, for React to carry out.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SidebarAction {
  SelectHome,
  SelectWorkspace { id: i64 },
  OpenSettings,
  OpenCommandPalette,
  OpenGithub,
  OpenLinear,
}

/// Sends `action` to the webview of window `label`. Tauri calls must not
/// run inside a GPUI update, so this runs in a task.
fn send(app: &AppHandle, label: &SharedString, action: SidebarAction) {
  let app = app.clone();
  let label = label.to_string();
  tauri::async_runtime::spawn(async move {
    if let Err(err) = app.emit_to(EventTarget::webview_window(&label), ACTION_EVENT, action) {
      log::warn!("failed to send GPUI sidebar action: {err}");
    }
  });
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

fn icon(path: &'static str, size: f32, color: Hsla) -> gpui::Svg {
  svg()
    .path(path)
    .size(px(size))
    .flex_none()
    .text_color(color)
}

/// Draws the sidebar for window `label` from `snapshot`.
pub fn render(
  snapshot: &SidebarSnapshot,
  label: &SharedString,
  app: &AppHandle,
) -> impl IntoElement {
  let palette = theme::palette(snapshot.dark);
  // Same label as the React sidebar, which shows it on every platform.
  let shortcut = "⌘ + K";

  let header = div()
    .flex()
    .items_center()
    .gap_2()
    .p_2()
    .child(
      div()
        .id("command-palette")
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
            .child(shortcut),
        )
        .on_click({
          let (app, label) = (app.clone(), label.clone());
          move |_, _, _| send(&app, &label, SidebarAction::OpenCommandPalette)
        }),
    )
    .child(
      div()
        .id("settings")
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(32.))
        .rounded_lg()
        .border_1()
        .border_color(palette.border)
        .when(snapshot.settings_open, |this| this.bg(palette.accent))
        .hover(|style| style.bg(palette.accent))
        .child(icon(theme::ICON_SETTINGS, 16., palette.foreground))
        .on_click({
          let (app, label) = (app.clone(), label.clone());
          move |_, _, _| send(&app, &label, SidebarAction::OpenSettings)
        }),
    );

  let home = nav_row(
    "home",
    theme::ICON_HOUSE,
    div().font_family(theme::MONO_FONT).child(
      snapshot
        .home_branch
        .clone()
        .unwrap_or_else(|| "home".into()),
    ),
    snapshot.home_selected,
    &palette,
    (app, label, SidebarAction::SelectHome),
  );
  let github = nav_row(
    "github",
    theme::ICON_GITHUB,
    div().child("Github"),
    snapshot.github,
    &palette,
    (app, label, SidebarAction::OpenGithub),
  );
  let linear = snapshot.linear.map(|active| {
    nav_row(
      "linear",
      theme::ICON_LINEAR,
      div().child("Linear"),
      active,
      &palette,
      (app, label, SidebarAction::OpenLinear),
    )
  });

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
  if snapshot.loading && snapshot.workspaces.is_empty() {
    list = list.child(
      div()
        .px_2()
        .py_1()
        .text_color(palette.muted_foreground)
        .child("Loading…"),
    );
  }
  for row in &snapshot.workspaces {
    list = list.child(render_row(row, &palette, label, app));
  }

  div()
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
    .child(header)
    .child(home)
    .child(github)
    .children(linear)
    .child(div().mx_2().mt_2().h_px().bg(palette.border))
    .child(
      div()
        .px_4()
        .pt_3()
        .pb_1()
        .text_size(px(11.))
        .text_color(palette.muted_foreground)
        .child("WORKSPACES"),
    )
    .child(list)
}

/// A row above the workspace list: home, GitHub, Linear.
fn nav_row(
  id: &'static str,
  icon_path: &'static str,
  text: gpui::Div,
  active: bool,
  palette: &Palette,
  (app, label, action): (&AppHandle, &SharedString, SidebarAction),
) -> impl IntoElement {
  let (app, label) = (app.clone(), label.clone());
  div()
    .id(id)
    .flex()
    .items_center()
    .gap(px(6.))
    .mx_2()
    .px_2()
    .h(px(32.))
    .rounded_md()
    .when(active, |this| this.bg(palette.accent))
    .when(!active, |this| this.hover(|style| style.bg(palette.accent)))
    .child(icon(icon_path, 12., palette.sidebar_foreground))
    .child(text.flex_1().min_w_0().truncate())
    .on_click(move |_, _, _| send(&app, &label, action.clone()))
}

fn render_row(
  row: &SidebarRow,
  palette: &Palette,
  label: &SharedString,
  app: &AppHandle,
) -> impl IntoElement {
  let id = row.id;
  let branch_color = match row.pr {
    Some(pr) => pr_color(pr, palette),
    None if row.selected => palette.primary,
    None => palette.muted_foreground,
  };
  let title_color = if row.selected {
    palette.primary
  } else {
    palette.muted_foreground
  };

  div()
    .id(("workspace", id as u64))
    .relative()
    .flex()
    .items_center()
    .gap(px(6.))
    .h(px(34.))
    .pl(px(row_indent(row.depth)))
    .pr_2()
    .rounded_md()
    .when(row.selected, |this| this.bg(palette.accent))
    .when(!row.selected, |this| {
      this.hover(|style| style.bg(palette.accent.opacity(0.6)))
    })
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
    // The React sidebar flips the branch icon vertically.
    .child(
      icon(theme::ICON_BRANCH, 12., branch_color)
        .with_transformation(Transformation::scale(gpui::size(1., -1.))),
    )
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
    .when_some(row.status.as_ref(), |this, status| {
      let color = status_color(status.color, palette);
      if status.spin {
        this.child(icon(theme::ICON_SPINNER, 14., color).with_animation(
          ("spin", id as u64),
          Animation::new(std::time::Duration::from_secs(1)).repeat(),
          |svg, delta| svg.with_transformation(Transformation::rotate(gpui::percentage(delta))),
        ))
      } else {
        this.child(div().flex_none().size(px(8.)).rounded_full().bg(color))
      }
    })
    .on_click({
      let (app, label) = (app.clone(), label.clone());
      move |_, _, _| send(&app, &label, SidebarAction::SelectWorkspace { id })
    })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_snapshot_from_react() {
    let json = r#"{
      "repoName": "treq",
      "homeBranch": "main",
      "homeSelected": false,
      "settingsOpen": false,
      "github": false,
      "linear": null,
      "dark": true,
      "width": 280,
      "loading": false,
      "workspaces": [
        {"id": 7, "title": "feat-a", "depth": 1, "selected": true,
         "status": {"color": "yellow", "spin": true}, "pr": "draft"},
        {"id": 8, "title": "feat-b", "depth": 2, "selected": false,
         "status": null, "pr": null}
      ]
    }"#;
    let snapshot: SidebarSnapshot = serde_json::from_str(json).expect("parse");
    assert_eq!(snapshot.repo_name, "treq");
    assert_eq!(snapshot.workspaces.len(), 2);
    assert_eq!(
      snapshot.workspaces[0].status,
      Some(RowStatus {
        color: StatusColor::Yellow,
        spin: true
      })
    );
    assert_eq!(snapshot.workspaces[0].pr, Some(PrState::Draft));
    assert_eq!(snapshot.workspaces[1].depth, 2);
    assert_eq!(snapshot.linear, None);
  }

  #[test]
  fn serializes_actions_for_react() {
    let select = serde_json::to_value(SidebarAction::SelectWorkspace { id: 3 }).expect("json");
    assert_eq!(
      select,
      serde_json::json!({"kind": "selectWorkspace", "id": 3})
    );
    let palette = serde_json::to_value(SidebarAction::OpenCommandPalette).expect("json");
    assert_eq!(palette, serde_json::json!({"kind": "openCommandPalette"}));
  }

  #[test]
  fn indents_rows_like_the_react_sidebar() {
    assert_eq!(row_indent(0), 10.);
    assert_eq!(row_indent(1), 16.);
    assert_eq!(row_indent(3), 28.);
    assert_eq!(guide_left(2), 8.);
    assert_eq!(guide_left(4), 20.);
  }
}
