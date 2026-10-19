//! `KeyboardShortcutsModal`: the "?" reference of the app's shortcuts.

use tauri_plugin_gpui::gpui::{self, div, prelude::*, px, SharedString};

use crate::gpui_shell::{
  theme::Palette,
  ui::{self, TextSize},
};

/// A shortcut's keys and what it does.
type Shortcut = (&'static [&'static str], &'static str);

/// `KEYBOARD_SHORTCUT_SECTIONS` in src/lib/keyboard-shortcuts.ts.
pub const SECTIONS: &[(&str, &[Shortcut])] = &[
  (
    "Global",
    &[
      (&["⌘", "N"], "New workspace"),
      (&["⌘", "⇧", "N"], "Create stacked workspace"),
      (&["⌘", "K"], "Command Palette"),
      (&["⌘", "P"], "File Search"),
      (&["?"], "Show keyboard shortcuts"),
      (&["Esc"], "Close dialog"),
    ],
  ),
  (
    "Terminal",
    &[
      (&["⌘", "J"], "Toggle terminal pane"),
      (&["⌘", "Ctrl", "J"], "Maximize/restore terminal"),
      (&["⌘", "]"], "New agent terminal"),
      (&["⌘", "\\"], "New shell terminal"),
      (&["⌘", "W"], "Close selected terminal"),
      (&["⌘", "C"], "Copy selection (or interrupt)"),
      (&["⌘", "V"], "Paste"),
    ],
  ),
  (
    "Files & Diff",
    &[
      (&["⌘", "F"], "Search in file or diff"),
      (&["⌘", "A"], "Select all changed files"),
    ],
  ),
];

/// `ui/kbd.tsx`'s `Kbd`: `h-5 min-w-5 rounded-sm px-1 text-xs font-medium
/// bg-muted text-muted-foreground`.
fn kbd(p: &Palette, key: &str) -> gpui::Div {
  ui::text(div(), TextSize::Xs)
    .flex()
    .flex_none()
    .items_center()
    .justify_center()
    .h(px(20.))
    .min_w(px(20.))
    .px_1()
    .rounded(px(6.))
    .bg(p.muted)
    .text_color(p.muted_foreground)
    .font_weight(gpui::FontWeight::MEDIUM)
    .child(SharedString::from(key.to_string()))
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
                  .child(kbd(p, key))
              }),
            ))
        })),
    )
}

/// `DialogContent className="max-h-[85vh] max-w-2xl w-full overflow-y-auto"`
/// with its header and the `sm:grid-cols-2` grid of sections.
pub fn render(p: &Palette, viewport: gpui::Size<gpui::Pixels>) -> gpui::Stateful<gpui::Div> {
  let mut grid = div().mt_4().flex().flex_col().gap_6();
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
  div()
    .id("keyboard-shortcuts-modal")
    .debug_selector(|| "keyboard-shortcuts-modal".into())
    .w_full()
    .max_w(px(672.))
    .max_h(viewport.height * 0.85)
    .overflow_y_scroll()
    .p_6()
    .rounded(px(16.))
    .border_1()
    .border_color(p.border.opacity(0.5))
    .bg(p.popover)
    .text_color(p.popover_foreground)
    .shadow_2xl()
    .occlude()
    .child(
      ui::text(div(), TextSize::Lg)
        .line_height(px(TextSize::Lg.metrics().0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child("Keyboard shortcuts"),
    )
    .child(grid)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn matches_the_react_reference() {
    let source = include_str!("../../../../src/lib/keyboard-shortcuts.ts");
    let actions = SECTIONS.iter().flat_map(|(_, s)| s.iter()).count();
    assert_eq!(source.matches("action: \"").count(), actions);
    for (title, shortcuts) in SECTIONS {
      assert!(source.contains(&format!("title: \"{title}\"")), "{title}");
      for (keys, action) in *shortcuts {
        let keys: Vec<String> = keys
          .iter()
          .map(|k| format!("\"{}\"", k.replace('\\', "\\\\")))
          .collect();
        let entry = format!("{{ keys: [{}], action: \"{action}\" }}", keys.join(", "));
        assert!(source.contains(&entry), "{entry}");
      }
    }
  }
}
