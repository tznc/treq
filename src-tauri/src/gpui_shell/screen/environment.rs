//! `EnvironmentSelector`: where new terminals and agent sessions in a
//! repository run, this machine or its devcontainer. The task input, the
//! agent prompt dialog and the terminal pane each show one.

use crate::gpui_shell::ui::zpx;
use tauri_plugin_gpui::gpui::{div, prelude::*, Context, Div, Stateful};

use super::{code::menu_panel, code_data::Environment};
use crate::gpui_shell::{
  theme::{self, Palette},
  ui::{self, Button, Size, TextSize, Variant},
};

/// How big the pane's trigger draws (`h-5 px-1.5`, 12px icons).
#[derive(Clone, Copy)]
pub(crate) struct TriggerSize {
  pub height: f32,
  pub padding: f32,
  pub icon: f32,
}

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
          crate::gpui_shell::ui::Tone::Success.color(p)
        } else {
          p.muted_foreground.opacity(0.5)
        }))
      }
    })
}

/// The selector as a toolbar picker: it shows a value, so it reads as one.
pub(crate) fn picker(id: &'static str, env: &Environment, p: &Palette) -> ui::Picker {
  let (icon, label) = if env.devcontainer {
    (theme::ICON_BOX, "Devcontainer")
  } else {
    (theme::ICON_LAPTOP, "Local")
  };
  ui::Picker::new(id, p)
    .placement(ui::PickerPlacement::Toolbar)
    .icon(icon)
    .value(label)
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
    ui::MenuItem::new(id, title, p)
      .icon(icon)
      .description(detail.to_string())
      .checked(selected)
      .disabled(disabled)
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
        ui::MenuItem::new("environment-start", "Start devcontainer", p)
          .icon(theme::ICON_PLAY)
          .disabled(starting)
          .when(!starting, |this| {
            this.on_click(cx.listener(move |view, _, _, cx| start(view, cx)))
          }),
      );
  }
  panel
}
