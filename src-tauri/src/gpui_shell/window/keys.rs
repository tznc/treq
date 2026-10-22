//! Which of the window's keyboard shortcuts a key press is, for a window the
//! model drives: Dashboard's (`shortcuts::app_shortcut`), the terminal
//! pane's (`useGpuiTerminalPane`) and the zoom keys (`AppStoreEffects`).
//!
//! The shell reads every key before an element takes it (an interceptor,
//! with the modifiers GDK reports held), so a text field never types a
//! shortcut.

use tauri_plugin_gpui::gpui::Modifiers;

use super::actions::Shortcut;
use crate::gpui_shell::shortcuts::AppShortcut;

/// Dashboard's shortcut, as the model's.
pub fn from_app(found: AppShortcut) -> Shortcut {
  match found {
    AppShortcut::Palette => Shortcut::Palette,
    AppShortcut::Key("i") => Shortcut::AgentPrompt,
    AppShortcut::Key("N") => Shortcut::Stack,
    AppShortcut::Key("p") => Shortcut::FilePicker,
    AppShortcut::Key("?") => Shortcut::ToggleShortcuts,
    AppShortcut::Key(_) => Shortcut::NewWorkspace,
  }
}

/// The shortcuts beyond Dashboard's. `in_terminal`: the terminal has focus
/// and handles its pane's keys itself.
pub fn window_shortcut(key: &str, m: Modifiers, in_terminal: bool) -> Option<Shortcut> {
  if m.alt {
    return None;
  }
  if key == "escape" {
    return (!m.secondary() && !m.shift).then_some(Shortcut::Escape);
  }
  if !m.secondary() {
    return None;
  }
  match key {
    "=" | "+" => return Some(Shortcut::ZoomIn),
    "-" | "_" => return Some(Shortcut::ZoomOut),
    _ if in_terminal || m.shift => return None,
    _ => {}
  }
  Some(match key {
    // `requireBothCmdAndCtrl`.
    "j" if m.platform && m.control => Shortcut::MaximizeTerminal,
    "j" => Shortcut::ToggleTerminal,
    "]" => Shortcut::NewAgent,
    "\\" => Shortcut::NewShell,
    _ => return None,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn secondary() -> Modifiers {
    Modifiers::secondary_key()
  }

  #[test]
  fn maps_dashboards_shortcuts() {
    assert_eq!(from_app(AppShortcut::Palette), Shortcut::Palette);
    assert_eq!(from_app(AppShortcut::Key("n")), Shortcut::NewWorkspace);
    assert_eq!(from_app(AppShortcut::Key("N")), Shortcut::Stack);
    assert_eq!(from_app(AppShortcut::Key("?")), Shortcut::ToggleShortcuts);
  }

  #[test]
  fn zooms_anywhere() {
    assert_eq!(
      window_shortcut("=", secondary(), true),
      Some(Shortcut::ZoomIn)
    );
    let shift = Modifiers {
      shift: true,
      ..secondary()
    };
    assert_eq!(window_shortcut("_", shift, false), Some(Shortcut::ZoomOut));
    assert_eq!(window_shortcut("=", Modifiers::none(), false), None);
  }

  #[test]
  fn leaves_the_terminal_its_pane_keys() {
    assert_eq!(
      window_shortcut("j", secondary(), false),
      Some(Shortcut::ToggleTerminal)
    );
    assert_eq!(window_shortcut("j", secondary(), true), None);
    assert_eq!(
      window_shortcut("]", secondary(), false),
      Some(Shortcut::NewAgent)
    );
    assert_eq!(
      window_shortcut("\\", secondary(), false),
      Some(Shortcut::NewShell)
    );
    let both = Modifiers {
      platform: true,
      control: true,
      ..Modifiers::none()
    };
    assert_eq!(
      window_shortcut("j", both, false),
      Some(Shortcut::MaximizeTerminal)
    );
  }

  #[test]
  fn escape_alone() {
    assert_eq!(
      window_shortcut("escape", Modifiers::none(), true),
      Some(Shortcut::Escape)
    );
    assert_eq!(window_shortcut("escape", secondary(), false), None);
  }
}
