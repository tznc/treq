//! Modifiers for key events.

/// `m` with the modifiers the keyboard holds. TAO on Linux reports only the
/// last modifier pressed, so Ctrl+Shift+N reached GPUI as Shift+N and
/// Cmd+Ctrl+J as Ctrl+J. Every key handler reads its modifiers through here.
pub fn held(mut m: tauri_plugin_gpui::gpui::Modifiers) -> tauri_plugin_gpui::gpui::Modifiers {
  // GDK panics before GTK is up, as in headless tests.
  #[cfg(target_os = "linux")]
  if gtk::is_initialized_main_thread() {
    let keymap = gtk::gdk::Display::default().and_then(|d| gtk::gdk::Keymap::for_display(&d));
    let Some(keymap) = keymap else {
      return m;
    };
    use gtk::gdk::ModifierType;
    let held = ModifierType::from_bits_truncate(keymap.modifier_state());
    m.control |= held.contains(ModifierType::CONTROL_MASK);
    m.shift |= held.contains(ModifierType::SHIFT_MASK);
    m.alt |= held.contains(ModifierType::MOD1_MASK);
    m.platform |= held.contains(ModifierType::SUPER_MASK) || held.contains(ModifierType::META_MASK);
  }
  m
}

/// The secondary modifier's name in a shortcut hint: ⌘ on macOS, Ctrl
/// elsewhere (`Modifiers::secondary`).
pub const SECONDARY: &str = if cfg!(target_os = "macos") {
  "⌘"
} else {
  "Ctrl"
};

/// The platform modifier's name: ⌘ on macOS, Super elsewhere.
pub const PLATFORM: &str = if cfg!(target_os = "macos") {
  "⌘"
} else {
  "Super"
};

/// Shift's name: ⇧ on macOS, Shift elsewhere.
pub const SHIFT: &str = if cfg!(target_os = "macos") {
  "⇧"
} else {
  "Shift"
};

/// A shortcut hint written with ⌘ for the secondary modifier, for this
/// platform: "⌘ + J" reads "Ctrl + J" on Linux and Windows.
pub fn hint(keys: &str) -> String {
  keys.replace('⌘', SECONDARY)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn hint_names_the_secondary_modifier() {
    assert_eq!(hint("⌘ + J"), format!("{SECONDARY} + J"));
    if !cfg!(target_os = "macos") {
      assert_eq!(hint("⌘+Enter"), "Ctrl+Enter");
    }
  }
}
