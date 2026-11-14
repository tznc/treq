---
sidebar_position: 3
description: Worktree or fresh clone to review a pull request? Use a detached git worktree on the PR head. It is faster than a clone and leaves your branch alone.
---

# Worktree or Clone to Review a Pull Request?

_Use a detached worktree. It checks out the pull request next to your current work in seconds, and one command removes it when you finish._

## The Fix

Fetch the pull request head and add a detached worktree for it:

```bash
git fetch origin pull/123/head
git worktree add --detach ../review-123 FETCH_HEAD
cd ../review-123
```

Git 2.43 prints:

```text
From github.com:org/repo
 * branch            refs/pull/123/head -> FETCH_HEAD
Preparing worktree (detached HEAD e9e2909)
HEAD is now at e9e2909 feat
```

`refs/pull/<number>/head` is the ref GitHub keeps for each pull request. If the branch is in the same repository, `git worktree add --detach ../review-123 origin/<branch>` works too.

Run the tests and read the code in `../review-123`. When you finish, go back to your main checkout and remove the worktree:

```bash
git worktree remove ../review-123
```

## Why a Worktree Beats a Fresh Clone Here

A review only reads a commit and runs it. It does not need its own remotes, config, or hooks, which are the reasons to [pick a clone](/learn/concepts/git/git-worktrees-vs-clones). A worktree reuses the object database you already have, so Git copies no history. A fresh clone downloads the whole repository again.

Check out the commit, not a local branch. `git worktree add ../review-123 feature` fails with `fatal: 'feature' is already used by worktree` when another worktree holds `feature`. A detached `HEAD` holds no branch, so it never collides with your own checkout. It also stops you from committing to the author's branch by mistake.

Your current branch stays as it was. Uncommitted changes, open editors, and running dev servers in the main checkout are untouched, which `git stash` and `git switch` cannot promise.

## When a Clone Is the Better Choice

Use a clone when the review needs a different remote, such as a fork you do not want in your main repository's config. Use a container or VM when the pull request comes from someone you do not trust. A worktree and a clone both run code as your user, with your credentials.

## How Treq Handles It

Treq can open a [workspace](/docs/concepts/workspaces) from an existing pull request. The workspace takes the pull request's branch in its own working copy, so your other workspaces keep their state.

## Related

- [Fix "fatal: branch is already used by worktree"](./branch-already-used-by-worktree)
- [Should Git worktrees go inside the repo or next to it?](./nested-or-sibling-worktrees)
- [Git Worktree vs Clone](/learn/concepts/git/git-worktrees-vs-clones)
