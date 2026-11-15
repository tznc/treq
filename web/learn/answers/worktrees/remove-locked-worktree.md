---
sidebar_position: 2
description: "Fix fatal: cannot remove a locked working tree. Unlock it or force removal with git worktree remove -f -f, and prune worktrees whose directory is gone."
---

# Remove a Locked Git Worktree

_A locked worktree needs `git worktree unlock` first, or `-f` twice. A worktree whose folder is already gone needs `git worktree prune`._

## The Fix

Unlock it, then remove it:

```bash
git worktree unlock ../repo-feature
git worktree remove ../repo-feature
```

Or force removal in one step. A single `-f` is not enough for a locked worktree:

```bash
git worktree remove -f -f ../repo-feature
```

If you deleted the folder yourself and Git still lists it, clean up the stale entry:

```bash
git worktree prune
```

## Why It Happens

A locked worktree is protected from `remove` and `prune`. People and tools lock a worktree when it lives on a removable drive or a process is using it. Git refuses with `fatal: cannot remove a locked working tree, lock reason: ...` and suggests `remove -f -f`. Check the lock reason before you force it, because the tool that locked it might still be running.

## How Treq Handles It

When you delete a workspace, Treq removes the workspace, its directory and its local metadata together, so you do not clean up worktree entries by hand. See [Managing Workspaces](/docs/tutorials/managing-workspaces).

## Related

- [Fix "branch is already used by worktree"](./branch-already-used-by-worktree)
- [Git Worktrees](/learn/concepts/git/git-worktrees)
