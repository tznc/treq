//! `KeyboardShortcutsModal`: the "?" reference of the app's shortcuts.

use tauri_plugin_gpui::gpui::{self, div, prelude::*, SharedString};

use crate::gpui_shell::{
  actions::{
    Action, COMMAND_PALETTE, GO_TO_FILE, MAXIMIZE_TERMINAL, NEW_AGENT_SESSION,
    NEW_AGENT_WITH_PROMPT, NEW_SHELL, NEW_WORKSPACE, STACK_ON_THIS, TOGGLE_TERMINAL,
  },
  keys::SECONDARY,
  theme::Palette,
  ui::{self, TextSize},
};

/// An action's row on the sheet.
const fn row(action: Action) -> Shortcut {
  (action.keys, action.label)
}

/// A shortcut's keys and what it does.
type Shortcut = (&'static [&'static str], &'static str);

/// The terminal's copy and paste: Cmd+C/V on macOS, Ctrl+Shift+C/V elsewhere
/// (Ctrl+C interrupts).
const COPY: &[&str] = if cfg!(target_os = "macos") {
  &["⌘", "C"]
} else {
  &["Ctrl", "Shift", "C"]
};
const PASTE: &[&str] = if cfg!(target_os = "macos") {
  &["⌘", "V"]
} else {
  &["Ctrl", "Shift", "V"]
};

/// The shortcut sheet: each section's title and its shortcuts.
pub const SECTIONS: &[(&str, &[Shortcut])] = &[
  (
    "Global",
    &[
      row(NEW_WORKSPACE),
      row(STACK_ON_THIS),
      row(COMMAND_PALETTE),
      row(GO_TO_FILE),
      row(NEW_AGENT_WITH_PROMPT),
      (&["?"], "Show keyboard shortcuts"),
      (&["Esc"], "Close dialog"),
    ],
  ),
  (
    "Terminal",
    &[
      row(TOGGLE_TERMINAL),
      row(MAXIMIZE_TERMINAL),
      row(NEW_AGENT_SESSION),
      row(NEW_SHELL),
      (&[SECONDARY, "W"], "Close selected terminal"),
      (COPY, "Copy selection (or interrupt)"),
      (PASTE, "Paste"),
    ],
  ),
  (
    "Files & Diff",
    &[
      (&[SECONDARY, "F"], "Search in file or diff"),
      (&[SECONDARY, "A"], "Select all changed files"),
    ],
  ),
];

/// A shortcut's keys, "Ctrl + K" style: each key a `Kbd`, joined by "+".
/// The palette's rows show them the same way.
pub fn keys(p: &Palette, keys: &[&str]) -> gpui::Div {
  div()
    .flex()
    .flex_none()
    .items_center()
    .gap_1()
    .children(keys.iter().enumerate().map(|(i, key)| {
      div()
        .flex()
        .items_center()
        .gap_1()
        .when(i > 0, |this| {
          this.child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child("+"),
          )
        })
        .child(ui::Kbd::from_label(key.to_string(), p))
    }))
}

/// A `<section>`: its uppercase heading, then one row per shortcut.
fn section(p: &Palette, title: &str, shortcuts: &[Shortcut]) -> gpui::Div {
  div()
    .flex_1()
    .min_w_0()
    .flex()
    .flex_col()
    .child(
      ui::text(div(), TextSize::Xs)
        .mb_3()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(p.muted_foreground)
        .child(title.to_uppercase()),
    )
    .child(
      div()
        .flex()
        .flex_col()
        .gap_2()
        .children(shortcuts.iter().map(|(keys, action)| {
          ui::text(div(), TextSize::Sm)
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
              div()
                .text_color(p.foreground)
                .child(SharedString::from(action.to_string())),
            )
            .child(self::keys(p, keys))
        })),
    )
}

/// The sheet: a dialog with no action pair, so an X closes it, over the
/// `sm:grid-cols-2` grid of sections, which scrolls past 85% of the window.
pub fn render(
  p: &Palette,
  viewport: gpui::Size<gpui::Pixels>,
  close: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
) -> ui::Dialog {
  let mut grid = div().flex().flex_col().gap_6();
  for pair in SECTIONS.chunks(2) {
    let mut row = div().flex().gap_6();
    for (title, shortcuts) in pair {
      row = row.child(section(p, title, shortcuts));
    }
    if pair.len() == 1 {
      row = row.child(div().flex_1());
    }
    grid = grid.child(row);
  }
  ui::Dialog::new("keyboard-shortcuts-modal", p)
    .width(672.)
    .title("Keyboard shortcuts")
    .on_close(close)
    .child(
      div()
        .id("keyboard-shortcuts-grid")
        .max_h(viewport.height * 0.75)
        .overflow_y_scroll()
        .child(grid),
    )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::gpui_shell::actions;

  fn rows() -> impl Iterator<Item = &'static Shortcut> {
    SECTIONS.iter().flat_map(|(_, rows)| rows.iter())
  }

  #[test]
  fn the_sheet_uses_the_action_names() {
    for action in [
      actions::GO_TO_FILE,
      actions::STACK_ON_THIS,
      actions::NEW_WORKSPACE,
      actions::NEW_AGENT_SESSION,
      actions::NEW_AGENT_WITH_PROMPT,
      actions::NEW_SHELL,
      actions::TOGGLE_TERMINAL,
      actions::MAXIMIZE_TERMINAL,
    ] {
      let row = rows()
        .find(|(_, label)| *label == action.label)
        .unwrap_or_else(|| panic!("{} is not on the sheet", action.label));
      assert_eq!(row.0, action.keys, "{}", action.label);
    }
    for old in [
      "File search",
      "Create stacked workspace",
      "New shell terminal",
      "New agent terminal",
      "Toggle terminal pane",
    ] {
      assert!(rows().all(|(_, label)| *label != old), "{old}");
    }
  }
}
