//! Keyboard shortcut hints.

use gpui::{div, prelude::*, App, Keystroke, SharedString, Window};

use crate::{text, theme::Palette, zpx, TextSize};

/// A shortcut, such as `⌘K` on macOS and `Ctrl K` elsewhere: the
/// platform's glyphs, never " + " between keys.
#[derive(IntoElement)]
pub struct Kbd {
  label: SharedString,
  palette: Palette,
}

impl Kbd {
  /// `keystroke` as GPUI binds it, such as `secondary-k` or `cmd-shift-p`.
  /// One that doesn't parse shows as written.
  pub fn new(keystroke: &str, palette: &Palette) -> Self {
    Self {
      label: Self::label(keystroke).into(),
      palette: *palette,
    }
  }

  /// `keystroke` as this platform writes it, for a shortcut shown as text
  /// rather than a key cap, such as in a menu.
  pub fn label(keystroke: &str) -> String {
    Keystroke::parse(keystroke)
      .map(|key| Self::format(&key, cfg!(target_os = "macos")))
      .unwrap_or_else(|_| keystroke.to_string())
  }

  /// `key` as macOS (`mac`) or Windows and Linux write it.
  pub fn format(key: &Keystroke, mac: bool) -> String {
    let m = &key.modifiers;
    let modifiers = if mac {
      [
        (m.control, "⌃"),
        (m.alt, "⌥"),
        (m.shift, "⇧"),
        (m.platform, "⌘"),
      ]
    } else {
      [
        (m.control, "Ctrl"),
        (m.alt, "Alt"),
        (m.shift, "Shift"),
        (m.platform, "Super"),
      ]
    };
    let key_name = match (key.key.as_str(), mac) {
      ("enter", true) => "↩".to_string(),
      ("enter", false) => "Enter".to_string(),
      ("escape", _) => "Esc".to_string(),
      ("backspace", true) => "⌫".to_string(),
      ("backspace", false) => "Backspace".to_string(),
      ("tab", true) => "⇥".to_string(),
      ("tab", false) => "Tab".to_string(),
      ("space", _) => "Space".to_string(),
      ("up", _) => "↑".to_string(),
      ("down", _) => "↓".to_string(),
      ("left", _) => "←".to_string(),
      ("right", _) => "→".to_string(),
      (k, _) => {
        let mut chars = k.chars();
        match chars.next() {
          Some(first) => first.to_uppercase().chain(chars).collect(),
          None => String::new(),
        }
      }
    };
    let parts = modifiers
      .into_iter()
      .filter(|(on, _)| *on)
      .map(|(_, name)| name.to_string())
      .chain([key_name]);
    parts.collect::<Vec<_>>().join(if mac { "" } else { " " })
  }
}

impl RenderOnce for Kbd {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = &self.palette;
    text(div(), TextSize::Xs)
      .flex()
      .flex_none()
      .items_center()
      .justify_center()
      .h(zpx(20.))
      .min_w(zpx(20.))
      .px_1()
      .rounded(zpx(crate::theme::RADIUS_SM))
      .bg(p.muted)
      .text_color(p.muted_foreground)
      .font_weight(gpui::FontWeight::MEDIUM)
      .whitespace_nowrap()
      .child(self.label)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn format(keystroke: &str, mac: bool) -> String {
    Kbd::format(&Keystroke::parse(keystroke).unwrap(), mac)
  }

  #[test]
  fn formats_with_each_platforms_glyphs() {
    assert_eq!(format("cmd-k", true), "⌘K");
    assert_eq!(format("cmd-shift-p", true), "⇧⌘P");
    assert_eq!(format("ctrl-k", false), "Ctrl K");
    assert_eq!(format("ctrl-shift-enter", false), "Ctrl Shift Enter");
    assert_eq!(format("escape", true), "Esc");
    assert_eq!(format("f12", false), "F12");
  }

  #[test]
  fn test_kbd_builder() {
    let p = crate::theme::palette(false);
    let expected = if cfg!(target_os = "macos") {
      "⌃K"
    } else {
      "Ctrl K"
    };
    assert_eq!(Kbd::new("ctrl-k", &p).label.as_ref(), expected);
  }
}
