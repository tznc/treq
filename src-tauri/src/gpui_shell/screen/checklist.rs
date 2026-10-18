//! `PrerequisiteChecklist`: which of Git, the agent CLIs and the GitHub CLI
//! are installed, with an install link for each missing one.

use serde::Deserialize;
use tauri_plugin_gpui::gpui::{div, prelude::*, px, FontWeight, SharedString};

use super::{Ctx, ScreenAction};
use crate::gpui_shell::{
  theme,
  ui::{self, Button, Size, TextSize, Variant},
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checklist {
  pub title: String,
  /// A check is running: "Check again" is disabled and its icon spins.
  pub validating: bool,
  pub groups: Vec<Group>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
  pub label: String,
  /// Drawn in the destructive color: no agent CLI is installed.
  #[serde(default)]
  pub error: bool,
  pub tools: Vec<Tool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
  pub binary: String,
  pub label: String,
  #[serde(default)]
  pub note: Option<String>,
  /// Whether this tool is required; a missing required tool gets a red X.
  #[serde(default)]
  pub required: bool,
  /// `None` while the check is still running.
  pub installed: Option<bool>,
  pub install_url: String,
}

pub fn render(list: &Checklist, ctx: &Ctx) -> impl IntoElement {
  let p = ctx.palette;
  let header = div()
    .mb_1()
    .flex()
    .items_center()
    .justify_between()
    .child(ui::text(div().font_weight(FontWeight::MEDIUM), TextSize::Sm).child(list.title.clone()))
    .child({
      let refresh = ui::icon(theme::ICON_REFRESH, 12., p.foreground);
      ui::text(
        Button::new("prerequisites-recheck", &p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .disabled(list.validating)
          .mr(px(-8.))
          .h(px(24.))
          .gap_1()
          .px_2(),
        TextSize::Xs,
      )
      .child(if list.validating {
        ui::spin("prerequisites-spin", refresh).into_any_element()
      } else {
        refresh.into_any_element()
      })
      .child("Check again")
      .on_click(ctx.send(ScreenAction::RecheckPrerequisites))
    });

  div()
    .w_full()
    .child(header)
    .children(list.groups.iter().map(|group| {
      div()
        .mt_2()
        .child(
          ui::text(div(), TextSize::Xs)
            .text_color(if group.error {
              p.destructive
            } else {
              p.muted_foreground
            })
            .child(group.label.clone()),
        )
        .children(group.tools.iter().map(|tool| tool_row(tool, ctx)))
    }))
}

fn tool_row(tool: &Tool, ctx: &Ctx) -> impl IntoElement {
  let p = ctx.palette;
  let status = match tool.installed {
    Some(true) => ui::icon(theme::ICON_CIRCLE_CHECK, 16., p.success),
    Some(false) if tool.required => ui::icon(theme::ICON_CIRCLE_X, 16., p.destructive),
    _ => ui::icon(theme::ICON_CIRCLE, 16., p.muted_foreground),
  };
  let muted = |label: &'static str| {
    ui::text(div(), TextSize::Xs)
      .text_color(p.muted_foreground)
      .child(label)
      .into_any_element()
  };
  let trailing = match tool.installed {
    None => muted("Checking..."),
    Some(true) => muted("Installed"),
    Some(false) => {
      let id = SharedString::from(format!("install-{}", tool.binary));
      ui::text(
        Button::new(id.clone(), &p)
          .variant(Variant::Link)
          .size(Size::Sm)
          .h_auto()
          .gap_1()
          .p_0(),
        TextSize::Xs,
      )
      .label("Install")
      .icon(theme::ICON_EXTERNAL_LINK, 12.)
      .on_click(ctx.send(ScreenAction::OpenUrl {
        url: tool.install_url.clone(),
      }))
      .into_any_element()
    }
  };
  div()
    .flex()
    .items_center()
    .gap_2()
    .py_1()
    .child(status)
    .child(
      div()
        .min_w_0()
        .flex_1()
        .child(ui::text(div(), TextSize::Sm).child(tool.label.clone()))
        .when_some(tool.note.clone(), |this, note| {
          this.child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child(note),
          )
        }),
    )
    .child(trailing)
}
