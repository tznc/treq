//! The window model: for a local repository, the per-window state React's
//! Dashboard held (the page, the selection, the open dialogs, theme, zoom
//! and sidebar width), the data it read through SWR, and the snapshots the
//! shell draws from them.

mod actions;
mod data;
mod model;
mod sidebar;
mod snapshots;
mod tree;
