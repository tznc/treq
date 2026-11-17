//! The app's named actions: one label, description and shortcut each,
//! read by the buttons, the command palette and the "?" shortcuts sheet so
//! the same action never goes by two names.

use crate::gpui_shell::keys::{PLATFORM, SECONDARY, SHIFT};

/// A named action: its label, a one-line description and its shortcut's
/// keys (empty for none).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Action {
  pub label: &'static str,
  pub description: Option<&'static str>,
  pub keys: &'static [&'static str],
}

pub const GO_TO_FILE: Action = Action {
  label: "Go to file",
  description: Some("Jump to a file in the repository"),
  keys: &[SECONDARY, "P"],
};

pub const NEW_WORKSPACE: Action = Action {
  label: "New workspace…",
  description: Some("Create a new workspace"),
  keys: &[SECONDARY, "N"],
};

pub const STACK_ON_THIS: Action = Action {
  label: "Stack on this…",
  description: Some("Create a workspace stacked on the one on screen"),
  keys: &[SECONDARY, SHIFT, "N"],
};

pub const COMMAND_PALETTE: Action = Action {
  label: "Command palette",
  description: None,
  keys: &[SECONDARY, "K"],
};

pub const NEW_AGENT_SESSION: Action = Action {
  label: "New agent session",
  description: Some("Start the default agent immediately"),
  keys: &[SECONDARY, "]"],
};

pub const NEW_AGENT_WITH_PROMPT: Action = Action {
  label: "New agent session with a prompt…",
  description: Some("Write a prompt and choose its workspace"),
  keys: &[SECONDARY, "I"],
};

pub const NEW_SHELL: Action = Action {
  label: "New shell",
  description: Some("Create a new shell session"),
  keys: &[SECONDARY, "\\"],
};

pub const TOGGLE_TERMINAL: Action = Action {
  label: "Show/hide terminal",
  description: Some("Show or hide the terminal pane"),
  keys: &[SECONDARY, "J"],
};

pub const MAXIMIZE_TERMINAL: Action = Action {
  label: "Maximize/restore terminal",
  description: Some("Let the terminal pane fill the window, or restore it"),
  keys: &[PLATFORM, "Ctrl", "J"],
};

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn names_each_action_once() {
    assert_eq!(GO_TO_FILE.label, "Go to file");
    assert_eq!(GO_TO_FILE.keys, &[SECONDARY, "P"]);
    assert_eq!(STACK_ON_THIS.label, "Stack on this…");
    assert_eq!(STACK_ON_THIS.keys, &[SECONDARY, SHIFT, "N"]);
    assert_eq!(NEW_WORKSPACE.label, "New workspace…");
    assert_eq!(NEW_AGENT_SESSION.label, "New agent session");
    assert_eq!(NEW_AGENT_SESSION.keys, &[SECONDARY, "]"]);
    assert_eq!(
      NEW_AGENT_WITH_PROMPT.label,
      "New agent session with a prompt…"
    );
    assert_eq!(NEW_AGENT_WITH_PROMPT.keys, &[SECONDARY, "I"]);
    assert_eq!(NEW_SHELL.label, "New shell");
    assert_eq!(TOGGLE_TERMINAL.label, "Show/hide terminal");
    assert_eq!(MAXIMIZE_TERMINAL.label, "Maximize/restore terminal");
    assert_eq!(MAXIMIZE_TERMINAL.keys, &[PLATFORM, "Ctrl", "J"]);
  }
}
