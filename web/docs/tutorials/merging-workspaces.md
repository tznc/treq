---
sidebar_position: 6
description: Merge a Treq workspace back into your main branch once its changes are committed, reviewed, and ready to land.
---

# Merging Workspaces

_How to bring completed workspace changes back into your main branch._

Merge a workspace after its changes are [committed](/docs/concepts/commit-management), [reviewed](/docs/concepts/changes-and-reviews), and ready to land. Treq keeps each [workspace](/docs/concepts/workspaces) on its own branch, so merging is the point where that isolated work becomes part of your main repository history.

## Before You Merge

Open the workspace and confirm there are no unexpected uncommitted changes. Commit anything that should be included, then run the relevant tests from the workspace terminal.

If the branch is shared with a remote, [push it](/docs/how-to/pushing-to-remote) before review or merge so collaborators can see the same commits.

## Merging From Treq

From the dashboard, select the workspace and choose **Merge**. Treq shows the target branch, the commits that will be merged, and any warnings about uncommitted changes or branch divergence.

Review the summary carefully, then confirm the merge. After the merge completes, Treq updates the dashboard so the workspace no longer shows commits ahead of the target branch.

## Handling Conflicts

Treq only opens the merge preview once the workspace has no conflicts against the target branch. If conflicts exist, resolve them in the workspace first: open the affected files, resolve the conflict markers, and commit the resolution. Git's [zdiff3](/learn/concepts/git/zdiff3) conflict style can make those markers easier to read before you edit. For agent-driven stacks, see [How to Fix Merge Conflicts Created by Coding Agents](/learn/how-to/merge-conflicts-with-coding-agents).

For large conflicts, pause and make sure the target branch is up to date before continuing. Pull the latest main branch, resolve conflicts in the workspace, and retry once tests pass. The merge preview becomes available as soon as the workspace is conflict-free.

## After Merging

Treq deletes the workspace automatically once the merge completes. Push the target branch if the merge should be shared with a remote.

