---
sidebar_position: 10
---

# GPUI sidebar

_An experimental sidebar that Treq draws natively with GPUI while the rest of the window stays web._

Treq's window is a web view. With the GPUI sidebar on, Treq builds the window with GPUI, shows the workspace sidebar as native UI on the left, and runs the rest of the app in a web view beside it. It is off by default and experimental.

## Turn it on

1. Open **Settings > Feature Preview**.
2. Switch on **GPUI sidebar**.
3. Restart Treq.

The switch changes how the window is built, so nothing happens until you restart.

## What the GPUI sidebar does

- A home row for the repository, showing its current branch.
- The workspace list, with stacked workspaces indented under their parents.
- A status dot per workspace: red for conflicts, yellow for uncommitted changes, and a spinner while an agent in that workspace is streaming output.
- PR state per workspace: draft, open, merged or closed. It appears only when the repository has a GitHub remote.
- The selected workspace, or the home row, highlighted. Nothing is highlighted while you are on Settings, GitHub or Linear.
- The command palette button and the settings button at the top.

Clicking a row opens that workspace, the same as in the regular sidebar.

## What still needs the regular sidebar

The GPUI sidebar does not have these yet:

- Context menus on rows.
- Drag to restack workspaces.
- Multi-select and stack selection.
- Resizing the sidebar.
- Multiple repositories in one window.
- Remote repositories.

For a window with several repositories or a remote repository, Treq falls back to the regular sidebar on its own. You do not need to turn the preview off.

## Platform notes

- Linux needs X11. On Wayland, Treq uses the regular sidebar.
- macOS and Windows are untested.

## Turn it off

Switch off **GPUI sidebar** in **Settings > Feature Preview** and restart Treq. The regular sidebar comes back.
