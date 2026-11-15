---
sidebar_position: 2
description: Each git worktree has its own untracked files, so node_modules is not shared. Install per worktree, use a shared package store, or symlink it.
---

# Share node_modules Across Git Worktrees

_Worktrees share Git history, not untracked files. Install per worktree, use a package manager with a shared store, or symlink when dependencies match._

## The Fix

Pick one:

- **Install in each worktree.** Safest, and correct when branches change dependencies.
- **Use a shared store.** pnpm links packages from one store on disk, so a second install is fast and uses little space.
- **Symlink the folder.** Fastest, but only safe while every worktree uses the same lockfile:

```bash
ln -s ../repo/node_modules ../repo-feature/node_modules
```

## Why It Happens

A worktree checks out tracked files only. `node_modules`, `.env` files and build output are untracked, so a new worktree starts without them. Agents then fail on missing packages or install a full copy per worktree, which multiplies disk use.

## How Treq Handles It

`treq add` takes a `--symlink` option that links a path from the home repository into the new workspace, such as `--symlink node_modules`. See the [CLI reference](/docs/reference/cli).

## Related

- [Stop two coding agents from overwriting each other](./agents-overwriting-each-other)
- [Git Worktrees vs Clones for Agents](/learn/how-to/git-worktrees-vs-clones-for-agents)
