---
sidebar_position: 4
description: Move uncommitted changes from one Treq workspace to another when work started in the wrong place or belongs on another branch.
---

# Moving Files Between Workspaces

_How to transfer uncommitted changes from one workspace to another._

Use this when you started work in the wrong workspace, want to split changes into multiple branches, or realize changes belong elsewhere.

## Using Treq's Move Feature

In the source workspace's Changes tab, select changed files (use `Cmd/Ctrl+Click` for multiple or `Shift+Click` for ranges). Right-click and choose **Move to Workspace**, or click **Move**. Pick the destination workspace and confirm.

Only uncommitted changes move. [Committed changes](/docs/concepts/commit-management) belong on a different path, such as [cherry-pick](/learn/concepts/git/cherry-pick-vs-rebase) or moving a commit from the Commits tab.

## Recovery

If you moved the wrong files, discard them in the destination and use **Undo** on that discard toast if you still need the destination copy. The source workspace already lost those working-copy edits when the move succeeded.

If the move created a commit in the destination, delete that commit from the Commits tab. The success toast includes **Undo**, which restores the abandoned commit while it is still the latest repository operation.

Do not use `git stash` or the [git reflog](/learn/concepts/git/git-reflog) for this. Treq workspaces share a colocated Jujutsu repo. Undo is a Treq toast action on the operation Treq just recorded.

## Commit and Move

To keep a named copy, commit in the source, then use **Move commit** on the Commits tab to send that commit to another workspace.
