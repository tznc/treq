---
sidebar_position: 1
description: "Fix fatal: 'branch' is already used by worktree (or is already checked out at) when adding a git worktree or switching branches."
---

# Fix "fatal: branch is already used by worktree"

_Git allows each branch in only one worktree at a time. Create a new branch for the second worktree, or free the first one._

## The Fix

Give the new worktree its own branch:

```bash
git worktree add -b feature-copy ../repo-feature-copy feature
```

Or find the worktree that holds the branch and switch it away or remove it:

```bash
git worktree list
git -C ../repo-feature switch main
git worktree add ../repo-feature-2 feature
```

## Why It Happens

Git 2.43 prints `fatal: 'feature' is already used by worktree at '/path/to/repo-feature'`. Older versions say `is already checked out at`. Git blocks a second checkout of the same branch because a commit in one worktree would move the branch under the other one's files.

## Overriding the Check

`git worktree add -f ../path feature` and `git switch --ignore-other-worktrees feature` both skip the check. Use them only when one of the two worktrees is read-only, such as a build or test copy. Two worktrees committing to one branch overwrite each other's work.

## How Treq Handles It

Each [Treq workspace](/docs/concepts/workspaces) gets its own branch when you create it, so two agents never share one. A workspace opened from an existing branch or pull request takes that branch, and Treq names the rest for you.

## Related

- [Remove a locked git worktree](./remove-locked-worktree)
- [Git Worktrees vs Clones](/learn/concepts/git/git-worktrees-vs-clones)
