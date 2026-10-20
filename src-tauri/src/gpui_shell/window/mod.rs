//! The window model: for a local repository, the per-window state React's
//! Dashboard held (the page, the selection, the open dialogs, theme, zoom
//! and sidebar width), the data it read through SWR, and the snapshots the
//! shell draws from them.
//!
//! - `model` is the state and `actions` what each action and shortcut does
//!   to it; `data` is what was read, `host` where it is read from.
//! - `sidebar` and `snapshots` build what the shell draws.
//! - `controller` runs it all for one window; `shell` connects it to the
//!   `Shell`, and `keys` is its keymap.

mod actions;
mod controller;
mod data;
mod host;
mod keys;
mod model;
mod shell;
mod sidebar;
mod snapshots;
mod tree;

pub(super) use actions::Shortcut;
pub(super) use controller::{Presentation, WindowController};
pub(super) use keys::{bind as bind_keys, on_shortcuts, KEY_CONTEXT};
pub(super) use model::DEFAULT_ZOOM;
pub(super) use shell::{
  drives, follow_appearance, forget, listen, menu_event, native_event, overlay_action, react_overlay,
  react_owns, routed, screen_action, set_react_owns, sidebar_action, start, window_focused,
};

#[cfg(test)]
mod tests;
