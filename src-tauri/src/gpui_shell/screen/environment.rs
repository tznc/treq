//! `EnvironmentSelector`: where new terminals and agent sessions in a
//! repository run, this machine or its devcontainer. The task input, the
//! agent prompt dialog and the terminal pane each show one.

use crate::gpui_shell::ui::zpx;
use tauri_plugin_gpui::gpui::{div, prelude::*, Context, Div, FontWeight, Stateful};

use super::{
  code::{menu_item, menu_panel, tw, GREEN_500},
  code_data::Environment,
};
use crate::gpui_shell::{
  theme::{self, Palette},
  ui::{self, Button, Size, TextSize, Variant},
};

/// How big the trigger draws: the task input's (`h-7`, 14px icons) or the
/// terminal pane's (`h-5 px-1.5`, 12px icons).
#[derive(Clone, Copy)]
pub(crate) struct TriggerSize {
  pub height: f32,
  pub padding: f32,
  pub icon: f32,
}

pub(crate) const TASK_TRIGGER: TriggerSize = TriggerSize {
  height: 28.,
  padding: 8.,
  icon: 14.,
};

pub(crate) const PANE_TRIGGER: TriggerSize = TriggerSize {
  height: 20.,
  padding: 6.,
  icon: 12.,
};

/// The selector's button: the environment, and while it is the
/// devcontainer, whether it runs.
pub(crate) fn trigger(
  id: &'static str,
  env: &Environment,
  starting: bool,
  size: TriggerSize,
  p: &Palette,
) -> Button {
  let devcontainer = env.devcontainer;
  let (icon, label) = if devcontainer {
    (theme::ICON_BOX, "Devcontainer")
  } else {
    (theme::ICON_LAPTOP, "Local")
  };
  let running = env.status.as_ref().is_some_and(|s| s.running);
  Button::new(id, p)
    .aria_label(label)
    .variant(Variant::Ghost)
    .size(Size::Sm)
    .h(zpx(size.height))
    .px(zpx(size.padding))
    .gap_1p5()
    .child(ui::icon(icon, size.icon, p.foreground))
    .child(ui::text(div(), TextSize::Xs).child(label))
    .when(devcontainer, |b| {
      if starting {
        b.child(ui::spinner(
          "devcontainer-starting",
          12.,
          p.muted_foreground,
        ))
      } else {
        b.child(div().size(zpx(6.)).rounded_full().bg(if running {
          tw(GREEN_500)
        } else {
          p.muted_foreground.opacity(0.5)
        }))
      }
    })
}

/// The selector's menu (`DropdownMenuContent`, `w-64`), unpositioned:
/// `set` picks the devcontainer (`true`) or this machine, `start` starts
/// the stopped devcontainer.
pub(crate) fn menu<V: 'static>(
  id: &'static str,
  env: &Environment,
  starting: bool,
  p: &Palette,
  set: fn(&mut V, bool, &mut Context<V>),
  start: fn(&mut V, &mut Context<V>),
  cx: &mut Context<V>,
) -> Stateful<Div> {
  let status = env.status.as_ref();
  let usable = status.is_some_and(|s| s.configured && s.cli_available);
  let detail = match status {
    None => "Checking…",
    Some(s) if !s.configured => "No .devcontainer config in this repo",
    Some(s) if !s.cli_available => "Install the Dev Containers CLI",
    Some(_) if starting => "Starting…",
    Some(s) if s.running => "Running",
    Some(_) => "Stopped",
  };
  let item = |id: &'static str,
              icon: &'static str,
              title: &'static str,
              detail: &str,
              selected: bool,
              disabled: bool| {
    menu_item(id, p, disabled)
      .items_start()
      .child(
        div()
          .mt_0p5()
          .child(ui::icon(icon, 14., p.popover_foreground)),
      )
      .child(
        div()
          .flex_1()
          .min_w_0()
          .flex()
          .flex_col()
          .gap_0p5()
          .child(div().font_weight(FontWeight::MEDIUM).child(title))
          .child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child(detail.to_string()),
          ),
      )
      .when(selected, |this| {
        this.child(
          div()
            .mt_0p5()
            .child(ui::icon(theme::ICON_CHECK, 14., p.popover_foreground)),
        )
      })
  };
  let devcontainer = env.devcontainer;
  let mut panel = menu_panel(id, p)
    .w(zpx(256.))
    .child(
      ui::text(div(), TextSize::Xs)
        .px_2()
        .py_1p5()
        .text_color(p.muted_foreground)
        .child("New terminals and agents run in"),
    )
    .child(
      item(
        "environment-local",
        theme::ICON_LAPTOP,
        "Local",
        "This machine",
        !devcontainer,
        false,
      )
      .on_click(cx.listener(move |view, _, _, cx| set(view, false, cx))),
    )
    .child(
      item(
        "environment-devcontainer",
        theme::ICON_BOX,
        "Devcontainer",
        detail,
        devcontainer,
        !usable,
      )
      .when(usable, |this| {
        this.on_click(cx.listener(move |view, _, _, cx| set(view, true, cx)))
      }),
    );
  if devcontainer && usable && !status.is_some_and(|s| s.running) {
    panel = panel
      .child(div().h_px().my_1().mx_neg_1().bg(p.border))
      .child(
        menu_item("environment-start", p, starting)
          .child(ui::icon(theme::ICON_PLAY, 14., p.popover_foreground))
          .child("Start devcontainer")
          .when(!starting, |this| {
            this.on_click(cx.listener(move |view, _, _, cx| start(view, cx)))
          }),
      );
  }
  panel
}
