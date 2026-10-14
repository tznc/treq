---
sidebar_position: 10
---

# GPUI sidebar

_The desktop workspace sidebar, drawn natively with GPUI._

Treq's window is a web view with one exception: the workspace sidebar. The desktop app builds the window with GPUI, draws the sidebar as native UI on the left, and runs the rest of the app in a web view beside it. There is no setting for this.

## What the sidebar shows

- A home row for each repository, showing its current branch. Supporting repositories get their own section.
- The workspace list, with stacked workspaces indented under their parents.
- A status dot per workspace: red for conflicts, yellow for uncommitted changes, and a spinner while an agent in that workspace is streaming output.
- PR state per workspace: draft, open, merged or closed. It appears only when the repository has a GitHub remote.
- The selected workspace, or the home row, highlighted. Nothing is highlighted while you are on Settings, GitHub or Linear.
- The command palette, settings, GitHub and Linear buttons.

## What you can do in it

- Click a row to open that workspace. Shift-click selects a range and Cmd/Ctrl-click toggles one.
- Right-click a row for its menu, for example rename or archive a workspace, or switch the home branch.
- Drag a workspace onto another to restack it, or onto the home row to move it home.
- Drag the sidebar edge to resize it. Double-click the edge to reset the width.
- Collapse a repository section. Treq remembers this per main repository.

Uncommitted files are not dragged onto the sidebar. To move them, use **Move to workspace…** in the file menu of the Review tab.

## Platform notes

- Linux sessions on Wayland run through XWayland.
- macOS and Windows are untested.
