---
sidebar_position: 2
description: Create and manage terminal sessions in Treq. Each session is a full PTY shell in a workspace directory, for dev servers, tests, or agents.
---

# Creating Terminal Sessions

_How to create and manage terminal sessions across workspaces._

[Terminal sessions](/docs/concepts/terminal-sessions) in Treq are full PTY shells bound to a [workspace's](/docs/concepts/workspaces) directory. Each workspace can have multiple sessions for running parallel processes like dev servers, tests, and builds. Sessions support copy/paste, clickable URLs, and your default shell.

## Creating Sessions

When you create a workspace and click "Open," Treq automatically creates a session with the working directory set to the workspace path. To create additional sessions for an existing workspace, right-click the workspace and select **New Session**, or click the **+** icon in the session tabs.

## Managing Sessions

Give sessions meaningful names by right-clicking the tab and selecting **Rename Session**. Names like "Dev Server," "Tests," or "Build" help you identify their purpose. Session tabs show status indicators: green for active, gray for backgrounded, yellow for unread output, and ✕ for exited processes.

Switch between sessions by clicking tabs, using `Cmd+1` through `Cmd+9` for quick access, or `Cmd+[` / `Cmd+]` for previous and next. The command palette (`Cmd+K`) also lets you jump to any session by name.

## Working with Multiple Sessions

Create separate sessions for long-running processes so you don't have to stop and restart them. A typical full-stack setup uses one session for the frontend dev server and another for the backend. A third runs the database container, and a fourth handles general commands and git operations.

The terminal supports all standard operations: `Cmd+C` to copy or interrupt, `Cmd+V` to paste, `Cmd+F` to search output, and `Cmd+K` or `clear` to clear the screen. URLs are clickable with Cmd+Click.

## Session Settings

Configure terminal appearance in Settings → Terminal.

| Setting | Options |
| --- | --- |
| Font family | Any monospace font, such as Fira Code or JetBrains Mono |
| Font size | 12-16px recommended |
| Default shell | `bash`, `zsh`, `fish`, or a custom path |
| Scrollback buffer | Number of lines retained per session |

For session-specific initialization, set environment variables with `export` or add logic to your shell's RC file.

## Closing Sessions

Close sessions by clicking the ✕ on the tab or right-clicking and selecting **Close Session**. If processes are running, Treq asks you to confirm. Close unused sessions periodically to keep the UI clean. Terminal buffers consume minimal resources, but clutter adds up.

## Troubleshooting

If a session won't open, verify the shell path in Settings → Terminal → Default Shell (`which zsh` or `which bash`). For garbled output, type `reset` or reopen the session. If commands fail, check your working directory with `pwd` and verify you're in the correct workspace.

