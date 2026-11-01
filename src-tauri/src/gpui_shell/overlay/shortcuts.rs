//! `KeyboardShortcutsModal`: the "?" reference of the app's shortcuts.

use tauri_plugin_gpui::gpui::{self, div, prelude::*, SharedString};

use crate::gpui_shell::{
  keys::{PLATFORM, SECONDARY, SHIFT},
  theme::Palette,
  ui::{self, TextSize},
};

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
      (&[SECONDARY, "N"], "New workspace"),
      (&[SECONDARY, SHIFT, "N"], "Create stacked workspace"),
      (&[SECONDARY, "K"], "Command palette"),
      (&[SECONDARY, "P"], "File search"),
      (&["?"], "Show keyboard shortcuts"),
      (&["Esc"], "Close dialog"),
    ],
  ),
  (
    "Terminal",
    &[
      (&[SECONDARY, "J"], "Toggle terminal pane"),
      (&[PLATFORM, "Ctrl", "J"], "Maximize/restore terminal"),
      (&[SECONDARY, "]"], "New agent terminal"),
      (&[SECONDARY, "\\"], "New shell terminal"),
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
            .child(div().flex().flex_none().items_center().gap_1().children(
              keys.iter().enumerate().map(|(i, key)| {
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
              }),
            ))
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
