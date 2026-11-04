//! Project home: the window's repositories as a two-column grid of cards.
//! A card shows the repository's GitHub name (or folder), its checked-out
//! branch and its workspaces; a click opens that repository's home.

use gpui_kit::base::TestSupportExt as _;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{div, prelude::*, AnyElement, FontWeight, SharedString};

use super::{Ctx, ScreenAction};
use crate::gpui_shell::{
  theme,
  ui::{self, zpx, TextSize},
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  /// The saved project's name; `None` for an unsaved project.
  pub name: Option<String>,
  pub repos: Vec<RepoCard>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCard {
  pub path: String,
  /// GitHub's `owner/name`, or the folder name.
  pub label: String,
  pub home_ref: Option<String>,
  pub missing: bool,
  /// Branches, in stack order, with their depth.
  pub workspaces: Vec<(String, u32)>,
}

/// The press that opens a card's repository.
pub fn open_press(path: &str) -> ScreenAction {
  ScreenAction::Press {
    id: format!("projectHome.open:{path}"),
  }
}

fn card(repo: &RepoCard, i: usize, ctx: &Ctx) -> AnyElement {
  let p = ctx.palette;
  let count = repo.workspaces.len();
  div()
    .id(("project-home-card", i))
    .debug_selector({
      let id = format!("project-home-card-{i}");
      move || id
    })
    .role(tauri_plugin_gpui::gpui::Role::Button)
    .aria_label(SharedString::from(format!("Open {}", repo.label)))
    .test_support()
    .flex()
    .flex_col()
    .gap_2()
    .p_4()
    .min_w_0()
    .rounded_lg()
    .border_1()
    .border_color(p.border)
    .bg(p.card)
    .cursor_pointer()
    .hover(|s| s.border_color(p.primary))
    .when(repo.missing, |this| this.opacity(0.5))
    .child(
      div()
        .flex()
        .items_baseline()
        .gap_2()
        .min_w_0()
        .child(
          ui::text(div(), TextSize::Lg)
            .font_weight(FontWeight::SEMIBOLD)
            .truncate()
            .child(repo.label.to_uppercase()),
        )
        .children(repo.home_ref.clone().map(|branch| {
          ui::text(div(), TextSize::Xs)
            .font_family(theme::MONO_FONT)
            .text_color(p.muted_foreground)
            .child(branch)
        })),
    )
    .child(
      ui::text(div(), TextSize::Xs)
        .text_color(p.muted_foreground)
        .truncate()
        .child(if repo.missing {
          format!("{} (not on this machine)", repo.path)
        } else {
          repo.path.clone()
        }),
    )
    .child(
      div()
        .flex()
        .flex_col()
        .gap_1()
        .children(repo.workspaces.iter().take(6).map(|(branch, depth)| {
          div()
            .flex()
            .items_center()
            .gap_1p5()
            .pl(zpx(*depth as f32 * 12.))
            .child(ui::icon(theme::ICON_PR, 12., p.muted_foreground))
            .child(
              ui::text(div(), TextSize::Sm)
                .font_family(theme::MONO_FONT)
                .truncate()
                .child(branch.clone()),
            )
        }))
        .when(count > 6, |this| {
          this.child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child(format!("+{} more", count - 6)),
          )
        })
        .when(count == 0 && !repo.missing, |this| {
          this.child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child("No workspaces"),
          )
        }),
    )
    .on_click(ctx.send(open_press(&repo.path)))
    .into_any_element()
}

pub fn render(route: &Route, ctx: &Ctx) -> AnyElement {
  let p = ctx.palette;
  let title = route
    .name
    .clone()
    .unwrap_or_else(|| "Unsaved project".into());
  let workspaces: usize = route.repos.iter().map(|r| r.workspaces.len()).sum();
  let mut rows = Vec::new();
  let mut it = route.repos.iter().enumerate().map(|(i, r)| card(r, i, ctx));
  while let Some(first) = it.next() {
    let second = it.next();
    rows.push(
      div()
        .flex()
        .gap_3()
        .child(div().flex_1().min_w_0().child(first))
        .child(div().flex_1().min_w_0().children(second)),
    );
  }
  div()
    .id("project-home")
    .debug_selector(|| "project-home".into())
    .size_full()
    .overflow_y_scroll()
    .bg(p.background)
    .text_color(p.foreground)
    .flex()
    .flex_col()
    .gap_4()
    .p_6()
    .child(
      div()
        .flex()
        .items_baseline()
        .gap_3()
        .child(
          ui::text(div().id("project-home-heading"), TextSize::Xl)
            .role(tauri_plugin_gpui::gpui::Role::Heading)
            .aria_label(SharedString::from(title.clone()))
            .test_support()
            .font_weight(FontWeight::SEMIBOLD)
            .child(title),
        )
        .child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .child(format!(
              "{} repositories · {} workspaces",
              route.repos.len(),
              workspaces
            )),
        ),
    )
    .child(div().flex().flex_col().gap_3().children(rows))
    .into_any_element()
}
