# Multi-Repository

## Status

Active.

## Summary

A Treq window opens one **main repository**. The user can add **supporting repositories** to it. Each supporting repository appears in the sidebar as its own home-repository row with its own group of workspaces, as if a second repository were open in the same window.

A workspace always belongs to exactly one repository. Supporting repositories are developed independently: their branches and workspaces are not mirrored or kept in sync with the main repository.

Agents started from the main repository know where the supporting repositories are, can read them, and change them by starting scoped Treq agents inside supporting-repository workspaces.

## Goals

- Add and remove supporting repositories from an open main repository.
- Browse, create, and work in supporting-repository workspaces from the same window.
- Let a main-repository agent read supporting repositories and delegate changes to agents scoped to supporting-repository workspaces.
- Keep every existing single-repository surface (workspace view, changes, commits, review, terminal) unchanged: each still operates on one repository.

## Non-goals

- Mirrored or linked workspaces across repositories.
- Atomic cross-repository commits, pushes, or pull requests.
- Nesting: a supporting repository cannot have supporting repositories of its own.
- Remote (managed VM or SSH) repositories as supporting repositories.
- Integrations (Linear, GitHub issues) and the review agent for supporting repositories.

## Domain

### Supporting-repository list

- The list is stored in the main repository's `.treq/local.db`. Paths are machine-specific and are not committed.
- A supporting repository must be a Git or jj repository. Adding it initializes Treq in it (its own `.treq/local.db`) the same way opening it would.
- The link is one-way. Opening a supporting repository on its own shows a normal single-repository window.
- Adding is rejected when the path is the main repository, is already listed, is inside the main repository, or contains the main repository.
- A listed repository whose path no longer exists is shown as missing, with Locate and Remove actions.
- Removing a supporting repository removes it from the list only. Its workspaces stay on disk.

### Workspaces and sessions

- Workspaces and agent sessions of a supporting repository are stored in that repository's own `.treq/local.db`.
- Workspace creation in a supporting repository follows that repository's `.treq/config.yaml` (setup script, copied files, branch-name pattern, target branch).

## Product behavior

### Adding a repository

- File menu: **Add Repository…** (desktop menus on every platform).
- Sidebar: **Add Repository…** in the main home-repository row's context menu.
- The folder picker result is validated as above; errors surface as toasts.
- Repository trust for setup scripts and checks is requested per repository the first time it is needed, as for any opened repository.

### Sidebar

Order, top to bottom:

1. Main home-repository row.
2. One home-repository row per supporting repository.
3. Integration items.
4. Workspaces, grouped by repository: one labelled group for the main repository, then one per supporting repository.

- When supporting repositories exist, every home-repository row and workspace group shows the repository directory name. When two repositories share a directory name, the parent directory is included (`org/app`).
- Each repository group can be collapsed. The collapsed state is remembered per main repository.
- Only the repository on screen shows row actions (start agent, shell, stack, archive, drag-to-move). Clicking a row in another repository puts that repository on screen. Stack selection, multi-select, and drag-to-move therefore never cross repositories.
- A supporting home-repository row has a context menu with **Repository Settings** and **Remove Repository**.

### Active repository

The window keeps two repository paths:

- the **main repository**, which identifies the window (`?repo=`);
- the **active repository**, the repository of the current sidebar selection.

Pages that show a home repository or workspace operate on the active repository. The new-workspace dialog targets the active repository.

Status and polling (workspace status, PR status, merge queue) run for every repository in the window, not only the active one.

### Agents

**Sessions started from the main repository or its workspaces**

- Working directory: the main repository or the workspace.
- The system prompt lists every supporting repository path.
- Supporting home repositories are readable but not writable. This is enforced for Claude through its settings file; other agents receive the paths and the rule in the prompt only.
- Changes to a supporting repository are delegated with the Treq CLI:

  ```bash
  treq add -r <repo> <branch>
  treq agent -r <repo> <branch> "<prompt>"
  treq st -r <repo> [<workspace>]
  ```

**Sessions started from a supporting repository or its workspaces** behave like single-repository sessions. Their system prompt does not mention the main repository.

### Treq CLI `-r`

- `-r <repo>` targets a supporting repository of the repository the command runs in. It accepts an absolute or relative path, the repository directory name, or a trailing path such as `org/app`.
- It is accepted by `add`, `agent`, `st`, `diff`, `set`, and `commit`. `mv` takes `--repo` only, because `-r` is its range flag.
- `-r` only resolves repositories that are in the current main repository's supporting list. Any other value is an error.
- `treq agent` never starts an agent in a home repository copy; it requires a workspace branch.
- `treq agent` returns after dispatch. Callers check progress with `treq st -r`.

### Window routing

Treq CLI requests (`agent`, `send`, `commit`) find the app window by repository path. A window registers its main repository and its supporting repositories. When a repository is the main repository of one window and a supporting repository of another, the window where it is the main repository handles the request.

## Acceptance criteria

- A user can add a local repository from the File menu or the main home row's context menu, and it appears as a home-repository row below the main home row and above the integrations, with its own workspace group.
- Selecting a supporting home row or one of its workspaces shows the same pages as a single-repository window, for that repository.
- Creating a workspace while a supporting repository is active creates it in that repository and lists it under that repository's group.
- A main-repository Claude session can read a supporting repository's files and cannot write them.
- `treq add -r <name> <branch>` followed by `treq agent -r <name> <branch> "<prompt>"` from a main-repository session starts an agent in the supporting-repository workspace, shown in the main repository's window.
- `treq agent -r <name> .` is rejected.
- Removing a supporting repository removes its sidebar group and leaves its files and workspaces untouched.
- Invalid additions (main repository, duplicate, nested, containing, non-repository) are rejected with a message.
