---
name: treq
description: >-
  Work inside a Treq agent terminal: use the treq CLI to create and inspect
  workspaces, send files or text to the user in the Treq UI, notify the user
  with treq notify, and create commits with treq commit. Use this skill for
  every Treq agent session, whenever you need another workspace, should show
  the user a file or preview, finish a task or need the user's input, need to
  record a commit, or are finishing inplace conflict resolution.
---

# Treq

You are running inside Treq. The session system prompt names your working
directory and the home repository. Stay inside that filesystem scope for
direct file reads and writes. Use the `treq` CLI when you need to create or
manage other workspaces. Those commands may write under `.treq/workspaces/`
outside the current working directory.

Run `treq --help` if a flag is unclear. Prefer `treq` over raw `git` or `jj`
for workspace, commit, send, notify, and resolve operations.

## Workspaces

A workspace is a separate working copy of the same repository. Files live
under `.treq/workspaces/` in the home repo. Uncommitted work in one workspace
does not appear in another.

```bash
treq st
treq st <workspace_name>
treq diff
treq diff <workspace_name>
treq add <branch_name> [-d <description>] [-l <title>] [-s <source_branch>] [-p <sparse_path>]... [-k <symlink_path>]...
treq set <workspace_name> [-d <description>] [-l <title>] [-t <target_branch>]
treq mv <source> <destination> -f <file> [-f <file> ...]
treq mv <source> <destination> -c <commit> [-c <commit> ...]
```

`treq st` from a workspace directory shows that workspace only. You get the
stacked parent and children, the uncommitted change count, whether files are
conflicted, and the commit count. It omits the repository default branch. If
files are conflicted, it tells you to run `treq diff`. `treq st` from the home
repository with no name lists every workspace.

Use `.` as a workspace name when a command needs the home repository. Create a
new workspace when the task should stay isolated from the current working copy.
Do not edit sibling workspace directories by hand.

Workspaces you create must be nested under the branch they are based on. From
the home repository, `treq add <branch_name>` targets the repository's default
branch. When stacking more work on an existing workspace, pass its branch to
`treq add` with `-s`. After creation, run `treq st` from the home repository and
confirm the new workspace appears below its target. If it does not, correct the
metadata with `treq set <workspace_name> -t <target_branch>` before continuing.

## Supporting repositories

When the system prompt lists supporting repositories, you may read them but
must not edit them directly. Delegate changes to an agent scoped to a
workspace in that repository:

```bash
treq add -r <repo> <branch_name> [-d <description>]
treq agent -r <repo> <branch_name> "<prompt>"
treq st -r <repo> [<workspace_name>]
treq diff -r <repo> <workspace_name>
```

- `<repo>` is the supporting repository's path or directory name.
- `-r` works on `add`, `set`, `st`, `diff`, `agent`, and `commit`. `mv` takes
  `--repo` because `-r` is its range flag.
- `treq agent` needs a workspace branch. It cannot start an agent in a home
  repository.
- `treq agent` returns once the agent is started. Poll `treq st -r` to follow
  its progress.

## Send files to the user

Use `treq send` when the user should see a file, screenshot, or text preview
in the Treq UI. Do not ask them to open the path themselves when send can
show it.

```bash
treq send <path>
treq send -
echo "notes" | treq send
```

- Pass an existing file path, or omit the path / use `-` to read stdin.
- Images (`png`, `jpg`, `jpeg`, `gif`, `webp`, `bmp`, `svg`) show as
  thumbnails in the terminal that ran the command.
- Everything else opens as selectable text.
- Piped stdin is staged under `.treq/send/` (gitignored).
- Treq must already have this repository open.

## Notify the user

The user often runs several agents and switches to other work. When you finish
a task or need their input, run `treq notify` with a one-line summary:

```bash
treq notify "Tests pass. Ready for review"
treq notify "Need input: keep the v1 endpoint or remove it?"
```

- Treq shows an OS notification titled with the workspace name.
- Treq skips it while a Treq window is focused, or when the user turned it
  off in Settings. The command still succeeds.
- Run it once per finished task or open question, not for progress updates.
- Ask the full question in the terminal too. The notification only brings the
  user back to this session.

## Commits

When the task needs a commit, use Treq. Do not use `git commit` or `jj commit`
for this flow.

```bash
treq commit <workspace_name> -m <message> [--push]
```

- `workspace_name` is the workspace branch name (the current workspace if you
  are already inside one).
- `-m` is required. Keep the message to a single clear subject, at most 500
  characters.
- `--push` pushes that workspace to the remote after a successful commit.
- This records the pending working-copy changes in that workspace. It does
  not merge the workspace into its target.

Create a commit when the user asked for one, when the work is a complete
reviewable slice, or when leaving uncommitted changes would block a rebase or
sync. Do not create empty or placeholder commits.

## Conflict resolution

Run `treq st` first. If it reports conflicts, run `treq diff`. That command
lists conflicted files, conflict hunks, and conflicted commits with change
ids. Pick one of the two flows below. Do not mix them on the same conflict.

### New-commit resolution

Use this when the workspace working copy holds the conflict and you should
record a new commit on the branch.

1. Run `treq st` and `treq diff` in the workspace.
2. Edit each conflicted file in the workspace working copy.
3. Remove conflict markers and keep the intended result.
4. Run `treq st` until it reports no conflicts.
5. Run `treq commit <workspace_name> -m "<message>"` with a message that
   describes the resolution.

Do not use `git commit` or `jj commit` for this step.

### In-place commit resolution

Use this when a rebase left one or more existing commits conflicted. Treq
rewrites those commits in place. It does not dirty the product workspace
working copy. Resolve directories live under `.treq/resolve/<workspace-slug>/`.
The Commits tab **Resolve conflicts…** action creates them.

1. Confirm `treq diff` lists conflicted commits and their change ids.
2. Work in `.treq/resolve/<workspace-slug>/<change-id>/` once that directory
   exists.
3. Edit the conflict markers in those files.
4. Or run `treq resolve <change-id> [1|2|base|both]`.
5. Or pipe JSON replacements:
   `echo '{"path/to/file": "replacement\n"}' | treq resolve <change-id>`.
6. Repeat for every change-id directory.

Your work is complete when no change-id directories remain under the resolve
root. `treq resolve` always takes a change id.
