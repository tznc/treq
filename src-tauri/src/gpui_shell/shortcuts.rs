//! Dashboard's keyboard shortcuts pressed while GPUI has focus. The
//! webview's own keys reach React directly; GPUI's go back to it as the key
//! React listens for (`useKeyboardShortcut`), so both answer the same way.

/// What a key press GPUI saw asks of the app.
#[derive(Debug, Clone, PartialEq)]
pub enum AppShortcut {
  /// Cmd+K: the command palette.
  Palette,
  /// Another of Dashboard's shortcuts, as the key React's `KeyboardEvent`
  /// carries: "i", "n", "p", "N" (Cmd+Shift+N) or "?".
  Key(&'static str),
}

/// Which of Dashboard's shortcuts a key press is. `secondary` is Cmd on
/// macOS and Ctrl elsewhere. `in_input` and `in_terminal` say where focus is:
/// as in React, "?" is typed into a text field or terminal, and Cmd+I goes
/// to the terminal.
pub fn app_shortcut(
  key: &str,
  key_char: Option<&str>,
  secondary: bool,
  shift: bool,
  alt: bool,
  in_input: bool,
  in_terminal: bool,
) -> Option<AppShortcut> {
  if alt {
    return None;
  }
  if !secondary {
    let typed = in_input || in_terminal;
    return (key_char == Some("?") && !typed).then_some(AppShortcut::Key("?"));
  }
  Some(match (key, shift) {
    ("k", false) => AppShortcut::Palette,
    ("i", false) if !in_terminal => AppShortcut::Key("i"),
    ("n", false) => AppShortcut::Key("n"),
    ("n", true) => AppShortcut::Key("N"),
    ("p", false) => AppShortcut::Key("p"),
    _ => return None,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn cmd(key: &str, shift: bool) -> Option<AppShortcut> {
    app_shortcut(key, None, true, shift, false, false, false)
  }

  #[test]
  fn cmd_shortcuts_reach_the_app() {
    assert_eq!(cmd("k", false), Some(AppShortcut::Palette));
    assert_eq!(cmd("i", false), Some(AppShortcut::Key("i")));
    assert_eq!(cmd("n", false), Some(AppShortcut::Key("n")));
    assert_eq!(cmd("n", true), Some(AppShortcut::Key("N")));
    assert_eq!(cmd("p", false), Some(AppShortcut::Key("p")));
    assert_eq!(cmd("c", false), None);
    assert_eq!(cmd("k", true), None);
    assert_eq!(app_shortcut("k", None, true, false, true, false, false), None);
  }

  #[test]
  fn cmd_shortcuts_work_in_a_text_field() {
    let in_input = |key, shift| app_shortcut(key, None, true, shift, false, true, false);
    assert_eq!(in_input("i", false), Some(AppShortcut::Key("i")));
    assert_eq!(in_input("n", true), Some(AppShortcut::Key("N")));
  }

  #[test]
  fn the_terminal_keeps_cmd_i_but_not_the_others() {
    let term = |key, shift| app_shortcut(key, None, true, shift, false, false, true);
    assert_eq!(term("i", false), None);
    assert_eq!(term("k", false), Some(AppShortcut::Palette));
    assert_eq!(term("n", true), Some(AppShortcut::Key("N")));
    assert_eq!(term("p", false), Some(AppShortcut::Key("p")));
  }

  #[test]
  fn question_mark_opens_the_reference_outside_text() {
    let q = |input, terminal| app_shortcut("/", Some("?"), false, true, false, input, terminal);
    assert_eq!(q(false, false), Some(AppShortcut::Key("?")));
    assert_eq!(q(true, false), None);
    assert_eq!(q(false, true), None);
    assert_eq!(app_shortcut("/", Some("/"), false, false, false, false, false), None);
    assert_eq!(app_shortcut("i", Some("i"), false, false, false, false, false), None);
  }
}
