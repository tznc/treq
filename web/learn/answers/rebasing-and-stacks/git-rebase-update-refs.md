---
sidebar_position: 2
description: Use git rebase --update-refs to rebase a whole stack of branches in one command, so every branch in the stack moves with its commits.
---

# Rebase a Whole Stack With --update-refs

_Run `git rebase --update-refs main` from the top branch. Git moves every branch in the stack, not only the one you are on._

## The Fix

From the top of the stack:

```bash
git switch part-3
git rebase --update-refs main
git push --force-with-lease origin part-1 part-2 part-3
```

To make it the default for every rebase:

```bash
git config --global rebase.updateRefs true
```

## Why It Happens

A plain rebase moves only the branch you are on. Lower branches in the stack still point at the old commits, so `part-1` and `part-2` look unchanged and their pull requests go stale. `--update-refs`, added in Git 2.38, moves any local branch that points at a commit in the rebased range.

It only rebases on request. When a teammate or an agent changes `part-1` later, you run it again.

## How Treq Handles It

Treq rebases dependent workspaces whenever a lower one changes, without a command. Conflicts go back to the agent that owns the workspace. See [Auto-Rebasing AI Workspaces](/learn/how-to/auto-rebase-ai-branches).

## Related

- [Rebase a stacked branch after a squash merge](./rebase-stack-after-squash-merge)
- [Merge vs Rebase](/learn/concepts/git/merge-vs-rebase)
