//! `Onboarding`: the window with no repository open.

use serde::Deserialize;
use tauri_plugin_gpui::gpui::{div, prelude::*, px, FontWeight};

use super::{
  checklist::{self, Checklist},
  Ctx, ScreenAction,
};
use crate::gpui_shell::{
  theme,
  ui::{self, Button, TextSize, Variant},
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Onboarding {
  /// The second button's label ("Open via SSH" or "Open cloud workspace"),
  /// or `None` without it.
  pub remote_label: Option<String>,
  pub checklist: Checklist,
}

pub fn render(screen: &Onboarding, ctx: &Ctx) -> impl IntoElement {
  let p = ctx.palette;
  let header = div()
    .flex()
    .flex_col()
    .items_center()
    .gap(px(6.))
    .p_6()
    .child(
      div()
        .mb_2()
        .child(ui::icon(theme::ICON_FOLDER_OPEN, 40., p.muted_foreground)),
    )
    .child(
      ui::text(div(), TextSize::Lg)
        .font_weight(FontWeight::SEMIBOLD)
        .child("Open a Repository"),
    )
    .child(
      ui::text(div(), TextSize::Sm)
        .text_center()
        .text_color(p.muted_foreground)
        .child("Select a folder containing a Git repository to get started."),
    );

  let actions = div()
    .flex()
    .flex_col()
    .items_center()
    .gap_3()
    .px_6()
    .pb_6()
    .child(
      Button::new("onboarding-open-repo", &p)
        .w_full()
        .label("Open Repository")
        .on_click(ctx.send(ScreenAction::OpenRepo)),
    )
    .when_some(screen.remote_label.clone(), |this, label| {
      this.child(
        Button::new("onboarding-open-remote", &p)
          .variant(Variant::Outline)
          .w_full()
          // `mr-2` on the icon, on top of the button's own gap.
          .child(
            div()
              .flex()
              .mr_2()
              .child(ui::icon(theme::ICON_SERVER, 16., p.foreground)),
          )
          .label(label)
          .on_click(ctx.send(ScreenAction::OpenRemote)),
      )
    });

  div()
    .size_full()
    .flex()
    .items_center()
    .justify_center()
    .child(
      ui::card(&p).w(px(384.)).child(header).child(actions).child(
        div()
          .border_t_1()
          .border_color(p.border.opacity(0.5))
          .px_6()
          .pt_4()
          .pb_6()
          .child(checklist::render(&screen.checklist, ctx)),
      ),
    )
}
