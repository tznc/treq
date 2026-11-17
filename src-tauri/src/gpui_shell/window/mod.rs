//! The window model: for a local repository, the per-window state React's
//! Dashboard held (the page, the selection, the open dialogs, theme, zoom
//! and sidebar width), the data it read through SWR, and the snapshots the
//! shell draws from them.
//!
//! - `model` is the state and `actions` what each action and shortcut does
//!   to it; `data` is what was read, `host` where it is read from.
//! - `sidebar` and `snapshots` build what the shell draws.
//! - `remote` is a window's remote side (its dialogs, the SSH endpoint and
//!   the remote repository) and `remote_actions` what the remote actions
//!   do to the model.
//! - `controller` runs it all for one window; `shell` connects it to the
//!   `Shell`, and `keys` is its keymap.

mod actions;
mod controller;
mod data;
mod host;
mod keys;
mod model;
pub(crate) mod remote;
mod remote_actions;
mod shell;
mod sidebar;
mod snapshots;
mod tree;

pub(super) use actions::Shortcut;
pub(super) use controller::{Presentation, WindowController};
pub(crate) use host::note_just_initialized;
pub(super) use host::WindowHost;
pub(super) use keys::{from_app, window_shortcut};
pub(super) use shell::{
  answer, cutoff_cleared, follow_appearance, forget, instance_state, listen, managed_endpoint,
  menu_event, native_event, overlay_action, routed, screen_action, shortcut, sidebar_action,
  skill_install_link, start, window_focused, workspace, workspace_route,
};

#[cfg(test)]
mod tests;
