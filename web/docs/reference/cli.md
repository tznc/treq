---
sidebar_position: 6
---

# CLI

_Reference for Treq's command-line interface._

The `treq` command lets you create and inspect [workspaces](/docs/concepts/workspaces) from a terminal. Run commands from inside a Git or Jujutsu repository, a Git worktree, or a Treq workspace, so Treq can detect the repository context.

## Exit status

- `0`: the command succeeded, or printed `--help` / `--version`.
- `1`: the command ran and failed. The error is on stderr (or in the JSON body with `--format json`).
- `2`: the invocation was malformed, for example an unknown command or a missing required argument.

The CLI does not need a display, so it works over SSH and in headless agent sandboxes.

## Supporting repositories

A window can link other local repositories to the repository it opened. Add one with **File > Add Repository…** or **Add Repository…** in the home repository row's context menu. Each linked repository gets its own home row and workspace group in the sidebar.

Workspace commands act on the repository that contains the current directory. Pass `-r <repo>` to act on a linked repository instead. `<repo>` is its path, its directory name, or a trailing path such as `org/app`. `add`, `set`, `st`, `diff`, `agent`, and `commit` accept `-r`. `mv` accepts `--repo`, because `-r` is its range flag. A repository that is not linked to the current one is an error.

```bash
treq add -r api feat/new-endpoint
treq agent -r api feat/new-endpoint "Add the endpoint"
treq st -r api feat/new-endpoint
```

## Commands

### `treq add`

Create a new workspace.

```bash
treq add <branch_name> [-d <description>] [-l <title>] [-s <source_branch>] [-p <sparse_path>]... [-k <symlink_path>]...
```

- `branch_name`: branch name for the workspace.
- `-d, --description`: optional workspace description.
- `-l, --title`: optional workspace title.
- `-s, --source-branch`: branch to stack the new workspace on.
- `-p, --sparse`: sparse checkout path prefix, repeatable. Only matching paths are materialized.
- `-k, --symlink`: path to symlink from the home repo into the new workspace, repeatable (e.g. `node_modules`).

Example:

```bash
treq add feat/deps -k node_modules -k target
```

### `treq set`

Update workspace metadata.

```bash
treq set <workspace_name> [-d <description>] [-l <title>] [-t <target_branch>]
```

- `workspace_name`: workspace branch name.
- `-d, --description`: set the workspace description.
- `-l, --title`: set the workspace title.
- `-t, --target-branch`: set the target branch.

### `treq st`

Show workspace status.

```bash
treq st [workspace_name]
```

From a workspace directory, or with `workspace_name`, Treq prints that workspace only: its stacked parent and children, uncommitted change count, conflicted-file count, and commit count. It omits the repository default branch. If files are conflicted, it tells you to run `treq diff`.

From the home repository with no name, it lists every workspace. GitHub pull request information is included when GitHub integration is available.

### `treq diff`

Show conflicted files and conflict hunks for a workspace.

```bash
treq diff [workspace_name]
```

Run this from a workspace directory, or pass `workspace_name`. The output lists conflicted files, conflicted commits with change ids, and the conflict hunks. See [Commit Management](/docs/concepts/commit-management).

### `treq mv`

Move selected changes from one workspace to another.

```bash
treq mv <source> <destination> -f <file> [-f <file> ...]
treq mv <source> <destination> -c <commit> [-c <commit> ...]
```

- `source`: source workspace branch name.
- `destination`: destination workspace branch name.
- `-f`: file path to move.
- `-c`: commit ID to move.

### `treq agent`

Start an agent session in a workspace.

```bash
treq agent <branch> <prompt> [-r <repo>] [-m <edit|plan>]
```

- `branch`: workspace branch name. Agents always start in a workspace, so `.` (the home repository) is rejected.
- `prompt`: prompt to send to the agent.
- `-r, --repo`: start the agent in a workspace of a [supporting repository](#supporting-repositories).
- `-m, --mode`: [permission mode](/docs/concepts/agent-sessions). Use `edit` or `plan`.

### `treq commit`

Create a commit from the pending changes in a workspace.

```bash
treq commit <workspace_name> -m <message> [--push]
```

- `workspace_name`: workspace branch name.
- `-m, --message`: commit message (required).
- `--push`: push the workspace to the remote after a successful commit.

This records working-copy changes in that workspace. It does not merge the workspace into its target. See [Commit Management](/docs/concepts/commit-management).

### `treq resolve`

Finish inplace conflict resolution for a conflicted commit that already has a resolve directory under `.treq/resolve/<workspace-slug>/`.

```bash
treq resolve <commit_id> [sides...]
echo '{"path/to/file": "replacement\n"}' | treq resolve <commit_id>
```

- `commit_id`: change id or commit id of the conflicted revision. Required.
- `sides`: optional conflict sides to take. Use `1`, `2`, `base`, or `both`.
- Piped stdin: JSON object of path to full file content replacements. Treq reads stdin only if it delivers data or closes within one second, so a tool that leaves stdin open does not hang the command.

When the change is clean, Treq rewrites that commit in place and deletes its resolve directory. See [Resolve commit conflicts inplace](/docs/concepts/commit-management#resolve-commit-conflicts-inplace).

### `treq send`

Send a file or stdin content to the open Treq window for preview. Images show as square thumbnails in the terminal that ran the command. Click a thumbnail to open a modal. Text opens a read-only, selectable preview.

```bash
treq send <path>
treq send -
echo "notes" | treq send
```

- `path`: existing file on disk. Omit or use `-` to read stdin.
- Image types: `png`, `jpg`, `jpeg`, `gif`, `webp`, `bmp`, `svg`.
- Everything else is treated as text.
- Piped stdin is staged under `.treq/send/` in the repo (already gitignored).
- With no path, Treq reads stdin only if it delivers data or closes within one second. Use `-` to wait for stdin however long it takes.
- When run inside a Treq terminal, previews attach to that pane via `TREQ_PTY_SESSION_ID`.

Treq must already have this repository open.

- `--browser`: open the given localhost URL or local HTML file directly in the in-app browser, instead of a preview thumbnail.

```bash
treq send --browser http://localhost:3000
treq send --browser ./dist/index.html
```

Only `http://localhost`, `http://127.0.0.1`, and `file://` URLs are allowed, matching the in-app browser's own scope. A filesystem path to an existing HTML file is resolved to a `file://` URL automatically. This switches the workspace to the Changes tab's Browser view and navigates there.

### `treq notify`

Show an OS notification for the current workspace. Agents run it when they finish a task or need input, so you can work elsewhere and come back when an agent needs you.

```bash
treq notify <message>
```

- `message`: one-line notification body. Treq joins multiple lines into one and cuts the text to 200 characters.
- `--agent-exited`: report that the agent process in this terminal exited. The body reads `Agent finished in <workspace>`. Agent sessions started by Treq run this when the agent exits on its own, so you never pass it yourself.

The notification title is the workspace branch name. In the home repository it is the repository directory name. Treq shows the notification only while no Treq window is focused. Turn notifications off with **Notify when an agent finishes** in **Settings › Application**. A skipped notification still exits with `0` and prints the reason.

The [bundled Treq skill](/docs/concepts/agent-sessions#bundled-treq-skill) tells agents to run `treq notify` with a one-line summary. Treq must already have this repository open.
