# Mobile

## Status

Active.

This document defines the durable product contract for Treq on Android and iOS. It replaces the previous phase-by-phase implementation journal.

## Summary

Treq mobile is the same Tauri application as desktop, compiled for Android and iOS with the shared Rust core and React frontend.

Mobile is primarily a remote-control and review surface for repositories reachable through Treq Remote Development. It should let a user inspect work, review changes, interact with coding agents, perform deliberate mutations, and attach to remote terminals without maintaining a second mobile-specific backend or repository model.

See [Remote Development](./remote-development.md) for the remote repository, trust, mutation, terminal, and managed-compute contracts.

## Direction: GPUI shell

The mobile shell is moving from the React frontend to a native [GPUI](https://www.gpui.rs) interface in [`src-tauri/src/mobile/`](../src-tauri/src/mobile/README.md). It is experimental, and it is the current path forward. It runs inside a Tauri Android app and links the shared Rust core (`treq_lib`) directly, so it still shares repository, remote transport, command, authentication, and domain logic with desktop. Where this document says "React frontend", read it as the shared application shell, which for mobile is now GPUI.

## Goals

- Ship Android and iOS from the existing Tauri codebase.
- Share repository, remote transport, command, authentication, and domain logic with desktop.
- Use a touch-first mobile shell rather than forcing the desktop multi-pane layout onto a small screen.
- Review remote workspaces, changes, diffs, file context, commits, and conflicts.
- Start, inspect, interact with, reattach to, and stop remote coding agents.
- Support a deliberately limited set of confirmed remote mutations.
- Provide an interactive terminal for remote workspaces.
- Recover coherently from mobile app suspension, connectivity changes, and process restarts.

## Non-goals

- A separate React Native or native mobile product.
- A second mobile-specific SSH/repository protocol.
- Owning managed-VM provisioning logic inside the mobile UI layer.
- General filesystem mounting or arbitrary SSH-client functionality.
- Server-side custody of user SSH private keys.
- Unbounded background SSH execution when the operating system suspends the app.
- Treating mobile device state as the source of truth for remote repository or agent state.

## Architecture

```text
Treq shared application
├── Desktop shell
└── Mobile shell
      ↓
Shared Tauri commands / Rust core
      ↓
Remote Development transport
├── Treq-managed VM
└── User-managed SSH
      ↓
Repositories / workspaces / agents / persistent terminal sessions
```

Mobile-specific UI should adapt presentation and interaction, not redefine the underlying domain objects.

## Mobile product behavior

### Navigation and review

The mobile shell should optimize for one primary task at a time.

Users must be able to navigate remote repositories/workspaces, inspect changed files, read diffs and context, inspect commit/conflict state, and move between review items without depending on desktop-only multi-window or drag-and-drop interactions.

### Mutations

Mutations must be explicit and intentionally constrained for mobile.

Destructive or high-impact operations require confirmation where appropriate. Mutations use the same remote command and verification semantics as desktop; mobile must not introduce a weaker retry model.

### Agents

Users can inspect existing remote agent sessions and start or stop agents in a selected workspace.

If an agent continues running remotely while the mobile app is suspended or disconnected, reopening the app should restore the session from remote state rather than assuming the previous socket is still alive.

### Terminal

Mobile provides an interactive terminal bound to a selected remote workspace.

The terminal supports bidirectional I/O and resize. Where the remote side supports persistent sessions, the user can detach and later reattach to the same session after navigation, app suspension, or reconnection.

### App lifecycle

Mobile operating systems may suspend networking or terminate the application.

Remote repository, agent, and terminal identities therefore cannot depend only on in-memory frontend state. Reopening the app should re-query authoritative remote/control-plane state and restore what can safely be restored.

### Security and key custody

User private SSH keys remain in platform-appropriate protected local storage and are never uploaded to Treq's control plane.

Host-key verification and managed credential expiry/revocation follow the Remote Development trust model.

The mobile app requests only the platform/Tauri capabilities it actually needs.

## Acceptance criteria

1. Android and iOS builds use the shared Tauri/Rust/React application rather than a separate mobile codebase.
2. An authenticated user can open the mobile shell and select a remote repository/workspace available through Remote Development.
3. The user can inspect changed files, diffs, file context, commits, and conflict state from the touch-first review UI.
4. The user can perform the supported, explicitly exposed remote mutations with the same structured-command and reconnect safety semantics as desktop.
5. The user can start, inspect, interact with, and stop a remote coding agent in the selected workspace.
6. The user can open an interactive remote terminal in the selected workspace and exchange terminal I/O.
7. When the app is suspended, disconnected, or restarted, it restores repository/agent/session state from authoritative remote identifiers rather than assuming stale in-memory state is valid.
8. Unknown/changed host trust or expired/revoked authorization blocks remote interaction until resolved.
9. User SSH private keys remain device-local and are not stored in Treq's server-side systems.

## Future considerations

- Mobile-native notifications for completed agent work or requested review.
- Deeper platform integrations such as share sheets, files, and shortcuts where they fit the review workflow.
- Richer background behavior where iOS/Android lifecycle rules permit it.
- UX for choosing among multiple managed VMs if Remote Development later adds multi-VM support.

Implementation milestones, reverted approaches, individual bug fixes, and test-run status belong in Git/PR history and test documentation rather than this PRD.
