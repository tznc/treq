---
sidebar_position: 5
---

# Remote SSH Workspaces

:::note[Work in progress]

Remote SSH is a Feature Preview. Turn **Remote SSH** on in Settings → Feature Preview. Dev builds start with the flag on. Release builds follow the shipped default until you change the switch.

:::

Remote SSH covers machines you run yourself. For a machine that Treq creates and maintains for Pro and Team accounts, see [Cloud Workspaces](/docs/how-to/cloud-workspaces), which does not need this preview.

Treq can connect to a remote development node through your existing SSH configuration so you can prepare a repository directory, clone a repository when needed, validate the remote Treq installation, inspect repository identity through the remote CLI, and run remote terminal or agent sessions from the client UI.

Remote SSH mode follows a **remote-control** model: the desktop UI stays on your local machine, while the remote node remains the source of truth for repository files, `.treq` metadata, Jujutsu and Git state, workspaces, commits, conflicts, terminals, and coding-agent processes.

## Prerequisites

Treq does **not** install software on remote nodes. Before opening a remote workspace, install and configure these tools yourself on the remote host:

- Treq CLI or remote helper available on `PATH`
- `jj` for workspace and commit operations
- `git` for cloning repositories and interacting with Git remotes
- Any coding agents you want to run remotely, such as Claude Code, Codex, or Cursor tooling

On the client, configure SSH in `~/.ssh/config`. Host aliases, identity files, and `ProxyJump` entries should live there. If `ssh my-host` works in a terminal, Treq can use the same host alias.

## Open a remote repository

1. Choose **File → Open via SSH...** or select **Open via SSH** from the onboarding screen.
2. Enter an SSH host alias from your SSH config.
3. Enter the remote repository directory.
4. If the directory does not contain a repository, enter a Git URL so Treq can clone it into that directory.
5. Treq checks the remote environment, records the host in your recent-host list, and asks the remote Treq CLI to inspect the repository with JSON output.

Treq combines recent hosts with aliases discovered from SSH configuration and offers them as suggestions. After a successful inspection, it stores the remote repository descriptor locally and restores the remote repository screen when the app is reopened without a local repository URL.

## How the client and remote node interact

The MVP uses a layered model:

```text
Local Treq UI
  ↓ Tauri API
SSH transport
  ↓
treq <command> --format=json
  ↓
Remote Treq core
  ↓
Remote JJ, Git, files and workspaces
```

The SSH layer is intentionally generic. It establishes the connection, runs typed Treq CLI commands in non-interactive mode, separates stdout from stderr, and parses JSON. Repository behavior stays in the remote Treq CLI and core Rust functions.

## Current remote capabilities

### Host and repository setup

The current implementation can list SSH host aliases, show remote prerequisite results, probe a remote directory for repository markers, clone a repository on the remote host, and inspect the remote repository through `treq repo inspect --format=json`. The connection screen supports explicit refresh and close actions.

### Repository inspection

`treq repo inspect --repo <path> --format=json` validates the remote path, resolves repository type, and returns repository identity and initial branch/commit information. This replaces identity-only remote opening: the client should treat the CLI inspection result as the source of truth for the opened remote repository.

### Terminal and agent sessions

When you start a shell or coding-agent session from the connected remote repository screen, Treq opens a local PTY whose command is an SSH invocation. The remote shell changes into the selected repository or workspace directory before launching the shell or agent. Any files created or edited by the terminal or agent are written directly on the remote filesystem.

### Persistent terminals on the host

Remote terminals and agents keep running when the connection drops. When you reconnect, Treq reattaches and shows the current screen plus the last 250 lines of output.

By default the Treq terminal daemon holds these sessions. The `treq` CLI starts it on the host the first time it is needed, and it stops itself a minute after the last session ends. The daemon listens on a socket in `~/.treq/pty/` that only your user can open, and opens no network port. It runs on Linux and macOS hosts.

The host's tools popover on the start screen shows what holds its terminals right now, for example **Treq daemon running (pid 4242, treq 0.3.0): 2 terminals**. To ask the host yourself, run `treq pty-remote status` there:

```text
$ treq pty-remote status
Treq daemon: running (pid 4242, treq 0.3.0), 2 sessions
tmux: installed, 0 Treq sessions
```

Add `--format json` for the same report as JSON.

To keep sessions in tmux instead, open the host's tools popover on the start screen and click **Use tmux…**. The host then needs tmux installed. Switching closes the terminals and agents that are running on the host.

To close every Treq terminal and agent on a host, click **Reset terminals…** in the same popover. With the daemon, this also stops the daemon.

## Read-only review CLI

The remote CLI exposes typed JSON commands that return the same core DTOs used locally:

- `treq workspace list --repo <path> --format=json`
- `treq workspace inspect --repo <path> --workspace <id> --format=json`
- `treq changes list --repo <path> --workspace <id> --format=json`
- `treq changes diff --repo <path> --workspace <id> --path <file> --format=json`
- `treq file read --repo <path> --workspace <id> --path <file> --revision working-copy --format=json`
- `treq commits list --repo <path> --workspace <id> --format=json`
- `treq conflicts list --repo <path> --workspace <id> --format=json`

Connecting these responses to the full workspace, diff, file, commit, and conflict component tree remains follow-up UI work. Remote terminal and agent sessions operate on the remote filesystem today. The connected screen intentionally presents inspection details and an SSH-backed terminal rather than passing the remote path into local repository APIs.

## Future mutation flow

Workspace creation, renaming, deletion, rebasing, patch application, commits, conflict resolution, pushes, and bookmark operations should use the same CLI JSON mechanism. The local app should not duplicate Jujutsu or Git logic for remote repositories and should not pass remote paths into local filesystem APIs.

## Proxy jumps and tunnels

Use normal SSH config for proxy jumps and tunnels. For example:

```ssh-config
Host devbox
  HostName devbox.internal
  User you
  ProxyJump bastion
  LocalForward 8080 127.0.0.1:8080
```

Treq invokes SSH through the configured host alias and does not need to know the proxy details. If the remote workflow needs forwarded ports for a dev server, database, or agent service, define those forwards in SSH config before opening the remote repository.

## Operational notes

- Use stable SSH host aliases instead of raw hostnames so proxy jumps and forwarded ports stay centralized in SSH config.
- Prefer absolute remote paths for team documentation and scripts. `~` may depend on the remote login shell.
- Keep remote Treq, `jj`, `git`, and coding-agent versions aligned with the client workflow to avoid command output mismatches.
- Treat remote repositories as independent from local checkouts. A local repo and `devbox:/same/path/name` have separate state and processes.

## Troubleshooting

- **Connection fails**: verify `ssh <host-alias>` works outside Treq.
- **Missing tool**: install the missing tool on the remote node and ensure it is on `PATH` for login shells.
- **Clone fails**: confirm the remote node has network access and credentials for the Git URL.
- **Repository inspect fails**: run `ssh <host-alias> treq repo inspect --repo <path> --format=json` and confirm stdout is valid JSON.
- **Terminal starts in the wrong directory**: use an absolute remote path and verify that the remote shell can `cd` into it.
- **Review data is unavailable**: ensure the relevant read-only review CLI command has been implemented before expecting the local UI to render that remote data.
