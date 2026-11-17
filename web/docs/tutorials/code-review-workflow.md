---
sidebar_position: 5
description: "Review code in a Treq workspace: read the diff, add inline comments, load GitHub review threads, and send feedback to a coding agent."
---

# Code Review Workflow

_How to review code using Treq's built-in review tools._

Treq's review interface lets you examine [changes](/docs/concepts/changes-and-reviews) in a dedicated [workspace](/docs/concepts/workspaces) without disrupting your current work. You can add inline comments, read GitHub review threads on an open PR, test changes locally, and export review summaries.

## Starting a Review

Click the **Review** button on any workspace in the dashboard. To review someone else's branch, fetch remote changes (`git fetch origin`), create a workspace from their branch via "New Workspace" → "From existing branch," then open the review interface.

If the branch already has a [GitHub pull request](/docs/concepts/github-integration), the Review tab also shows GitHub review threads on matching hunks. Local comments you add still stay on your machine until you copy or send them.

## The Review Interface

The interface has four areas. The **file tree** on the left shows all changed files with status indicators (M for modified, A for added, D for deleted) and checkmarks for files you've reviewed. The **diff viewer** in the center displays side-by-side or unified diffs with syntax highlighting. Click any line to add a comment. The **review panel** on the right collects all your comments and provides export options. The **commit history** at the bottom lets you review individual commits or the cumulative diff.

Navigate files with arrow keys, press Enter to open, and Space to mark as reviewed. Toggle between side-by-side and unified diff modes in the toolbar.

## Adding Comments

Click a line number in the diff viewer to add an inline comment.

## Reviewing via the File Browser

Open the file tree on the left and click a changed file to load its diff. Click a line number to leave a comment describing the change you want. Repeat across as many files as you need to review.

Once you're done, send your comments to the **Agent Terminal**. The agent picks them up as instructions and works through them in the workspace's terminal session.

## Reviewing via the Commit Browser

Open the commit history at the bottom of the review interface and click a commit to load its individual diff. Review the commit the same way you'd review a file: click a line number and leave a comment.

This is useful when an agent produced a sequence of commits and you want to review each step on its own, not the cumulative diff. Once you're done, send your comments to the **Agent Terminal** the same way as in the file browser.

## Completing the Review

When you've reviewed all files, choose your verdict. Click **Approve** if everything looks good, or **Request Changes** if issues need fixing. Add an overall summary comment explaining your decision.

The review panel shows your progress: total comments, files reviewed, and diff statistics. Use **Copy Summary** to generate formatted markdown you can paste into GitHub, GitLab, or other platforms.

When the workspace is ready for a remote review, use **Create PR** in the workspace header or **Commit and create PR** from the Review tab Commit control. See [Creating and Viewing Pull Requests](/docs/how-to/creating-and-viewing-pull-requests).

## Testing Locally

Since the workspace contains the actual code, you can run it during review. Open a terminal session, install dependencies if needed, run the application, and execute tests. Finding bugs locally is more valuable than catching them after merge.

## Tips

Review commit-by-commit to understand the development progression, or view the cumulative diff for small changes. Establish team guidelines for response times and approval requirements. Be constructive in feedback: suggest solutions, not just problems.

## Keyboard Shortcuts

`J`/`K` for next/previous file, `C` to add comment, `R` to mark reviewed, `A` to approve, `X` to request changes.

## Next Steps

- [Changes and Reviews](/docs/concepts/changes-and-reviews)
- [Creating and Viewing Pull Requests](/docs/how-to/creating-and-viewing-pull-requests)
- [GitHub Integration](/docs/concepts/github-integration)
- [Committing Changes](/docs/tutorials/committing-changes)
