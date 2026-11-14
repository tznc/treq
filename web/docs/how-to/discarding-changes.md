---
sidebar_position: 3
description: Discard uncommitted edits in Treq and bring them back with Undo, which restores the working-copy snapshot taken before the discard.
---

# Discarding Changes

_How to drop uncommitted edits, and how to bring them back._

**Deselecting** a file removes it from the next [commit](/docs/concepts/commit-management) without losing the edits. **Discarding** restores the file to its last committed state in the working copy.

Treq records the working-copy commit before a discard. The success toast includes **Undo**, which restores that snapshot. Click it before you make another repository change.

## Discarding Files

In the Changes tab, right-click a changed file and choose **Discard file**, or select the file and use the discard control. To drop every pending file, use **Discard all changes** and confirm.

The file on disk returns to the parent of the working-copy commit. Untracked files that were only in that working copy disappear with it.

## Partial Discards

Treq does not discard individual hunks. Commit the lines you want to keep, discard the rest, then keep editing.

## Recovery

Use **Undo** on the discard toast. That restores the working-copy snapshot Treq took immediately before the discard.

If the toast is gone, later repository operations have moved on, or you discarded from outside Treq, Treq cannot reconstruct that snapshot. Editor local history can still help for a single file.

Do not use `git checkout` or the [git reflog](/learn/concepts/git/git-reflog) to undo a Treq discard. The working copy is a Jujutsu commit. Treq's undo talks to that operation. The reflog recovers moved committed tips in Git, not uncommitted Treq working-copy edits.

Before a large discard, [commit](/docs/concepts/commit-management) to a throwaway workspace if you want a named backup instead of a short-lived toast.

## Best Practices

Read the diff before you discard. Discard one file when you can. Commit often so a discard is a small loss even if you miss the toast.
