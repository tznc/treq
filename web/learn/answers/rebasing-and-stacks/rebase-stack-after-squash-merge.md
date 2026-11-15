---
sidebar_position: 1
description: After the bottom branch of a stack is squash merged, rebase the next branch with git rebase --onto so its parent's commits are not replayed.
---

# Rebase a Stacked Branch After a Squash Merge

_Use `git rebase --onto main <old-parent> <branch>` so only the branch's own commits move onto the squashed result._

## The Fix

Say `part-2` was built on `part-1`, and `part-1` was squash merged into `main`:

```bash
git fetch origin
git rebase --onto origin/main part-1 part-2
```

This takes the commits on `part-2` that are not on `part-1` and replays them on `origin/main`. Then force-push `part-2` with `--force-with-lease`.

## Why It Happens

A squash merge writes one new commit to `main`. The original `part-1` commits are not in `main`'s history, so a plain `git rebase main` tries to replay them again. Git skips a commit whose change already exists, but `part-1`'s commits rarely match the squashed result exactly once review fixes are in. The usual outcome is a set of conflicts against code that is already on `main`.

## How Treq Handles It

When a workspace's pull request merges on GitHub, Treq moves the workspace above it onto the merged workspace's target. The next layer is rebased onto `main` with only its own commits, without a manual `--onto`. See [Stacks and Rebasing](/docs/concepts/workspaces).

## Related

- [Rebase a whole stack with --update-refs](./git-rebase-update-refs)
- [Force Push with Lease](/learn/concepts/git/force-with-lease)
- [What are Stacked PRs?](/learn/concepts/git/stacked-prs)
