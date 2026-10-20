//! The window's keyboard shortcuts as a GPUI keymap: Dashboard's
//! `useKeyboardShortcut`s, the terminal pane's and the zoom keys, for the
//! keys GPUI receives (the webview's still reach React directly).
//!
//! Bindings match before an element's own key handlers, so the terminal's
//! keys (`Terminal`) and plain keys typed into a field (`Input`) are left
//! out where React's `useKeyboardShortcut` left them out.

use gpui_kit::KeyBinding;
use tauri_plugin_gpui::gpui::{self, prelude::*, App, Div};

use super::actions::Shortcut;
use crate::gpui_shell::Shell;

/// The shell's root key context.
pub const KEY_CONTEXT: &str = "TreqWindow";
/// Keys the terminal handles itself while it has focus.
const NOT_TERMINAL: &str = "TreqWindow && !Terminal";
/// Plain keys, which type into a field or the terminal instead.
const PLAIN: &str = "TreqWindow && !Input && !Terminal";

gpui_kit::actions!(
  treq_window,
  [
    NewWorkspace,
    Stack,
    Palette,
    AgentPrompt,
    FilePicker,
    ToggleShortcuts,
    Escape,
    ToggleTerminal,
    MaximizeTerminal,
    NewAgent,
    NewShell,
    ZoomIn,
    ZoomOut,
  ]
);

/// Binds the shortcuts; once, at launch.
pub fn bind(cx: &mut App) {
  cx.bind_keys([
    KeyBinding::new("secondary-n", NewWorkspace, Some(NOT_TERMINAL)),
    KeyBinding::new("secondary-shift-n", Stack, Some(NOT_TERMINAL)),
    KeyBinding::new("secondary-k", Palette, Some(NOT_TERMINAL)),
    KeyBinding::new("secondary-i", AgentPrompt, Some(KEY_CONTEXT)),
    KeyBinding::new("secondary-p", FilePicker, Some(NOT_TERMINAL)),
    KeyBinding::new("?", ToggleShortcuts, Some(PLAIN)),
    KeyBinding::new("shift-/", ToggleShortcuts, Some(PLAIN)),
    KeyBinding::new("escape", Escape, Some(PLAIN)),
    KeyBinding::new("secondary-j", ToggleTerminal, Some(NOT_TERMINAL)),
    KeyBinding::new("cmd-ctrl-j", MaximizeTerminal, Some(NOT_TERMINAL)),
    KeyBinding::new("secondary-]", NewAgent, Some(NOT_TERMINAL)),
    KeyBinding::new("secondary-\\", NewShell, Some(NOT_TERMINAL)),
    KeyBinding::new("secondary-=", ZoomIn, Some(KEY_CONTEXT)),
    KeyBinding::new("secondary-+", ZoomIn, Some(KEY_CONTEXT)),
    KeyBinding::new("secondary-shift-=", ZoomIn, Some(KEY_CONTEXT)),
    KeyBinding::new("secondary--", ZoomOut, Some(KEY_CONTEXT)),
    KeyBinding::new("secondary-_", ZoomOut, Some(KEY_CONTEXT)),
    KeyBinding::new("secondary-shift--", ZoomOut, Some(KEY_CONTEXT)),
  ]);
}

/// The shell root's handlers for the shortcuts.
pub fn on_shortcuts(root: Div, cx: &mut gpui::Context<Shell>) -> Div {
  macro_rules! on {
    ($root:expr, $($action:ident => $shortcut:ident),* $(,)?) => {
      $root$(.on_action(cx.listener(|shell, _: &$action, _, cx| {
        shell.shortcut(Shortcut::$shortcut, cx)
      })))*
    };
  }
  let root = on!(root,
    NewWorkspace => NewWorkspace,
    Stack => Stack,
    Palette => Palette,
    AgentPrompt => AgentPrompt,
    FilePicker => FilePicker,
    ToggleShortcuts => ToggleShortcuts,
    ToggleTerminal => ToggleTerminal,
    MaximizeTerminal => MaximizeTerminal,
    NewAgent => NewAgent,
    NewShell => NewShell,
    ZoomIn => ZoomIn,
    ZoomOut => ZoomOut,
  );
  // An open dialog's own Escape handling still runs.
  root.on_action(cx.listener(|shell, _: &Escape, _, cx| {
    shell.shortcut(Shortcut::Escape, cx);
    cx.propagate();
  }))
}

#[cfg(test)]
mod tests {
  use gpui::{div, FocusHandle, TestAppContext, Window};

  use super::*;

  /// The shell root's context around a focused child: the terminal's, or
  /// a plain one.
  struct Root {
    focus: FocusHandle,
    terminal: bool,
    seen: Vec<&'static str>,
  }

  impl Render for Root {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
      let child = div()
        .size_full()
        .track_focus(&self.focus)
        .when(self.terminal, |d| d.key_context("Terminal"));
      div()
        .size_full()
        .key_context(KEY_CONTEXT)
        .on_action(cx.listener(|v, _: &FilePicker, _, _| v.seen.push("filePicker")))
        .on_action(cx.listener(|v, _: &ToggleShortcuts, _, _| v.seen.push("shortcuts")))
        .on_action(cx.listener(|v, _: &AgentPrompt, _, _| v.seen.push("agentPrompt")))
        .on_action(cx.listener(|v, _: &ZoomIn, _, _| v.seen.push("zoomIn")))
        .on_action(cx.listener(|v, _: &ToggleTerminal, _, _| v.seen.push("terminal")))
        .child(child)
    }
  }

  fn pressed(cx: &mut TestAppContext, terminal: bool, keys: &str) -> Vec<&'static str> {
    cx.update(bind);
    let (root, cx) = cx.add_window_view(|_, cx| Root {
      focus: cx.focus_handle(),
      terminal,
      seen: Vec::new(),
    });
    cx.update(|window, cx| {
      let focus = root.read(cx).focus.clone();
      window.focus(&focus, cx);
    });
    cx.simulate_keystrokes(keys);
    cx.read(|cx| root.read(cx).seen.clone())
  }

  #[gpui::test]
  fn binds_the_window_shortcuts(cx: &mut TestAppContext) {
    assert_eq!(
      pressed(cx, false, "secondary-p ? secondary-= secondary-j secondary-i"),
      ["filePicker", "shortcuts", "zoomIn", "terminal", "agentPrompt"]
    );
  }

  #[gpui::test]
  fn leaves_the_terminal_its_own_keys(cx: &mut TestAppContext) {
    assert_eq!(
      pressed(cx, true, "secondary-p ? secondary-j secondary-i"),
      ["agentPrompt"]
    );
  }
}
