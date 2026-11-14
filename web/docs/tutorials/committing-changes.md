---
sidebar_position: 4
description: Select files or hunks and commit them in Treq. There is no separate staging area, and you can discard what you do not want.
---

# Committing Changes

_How to select and commit changes using Treq's visual tools._

Treq has no separate staging area. You select the files or hunks you want in a [commit](/docs/concepts/commit-management), then commit them directly. Anything you do not want can be [discarded](/docs/how-to/discarding-changes). Treq's diff viewer makes this process visual, showing which changes are selected for the next commit and which aren't.

## Accessing the Diff Viewer

Open the diff viewer from the dashboard by clicking any [changed file](/docs/concepts/changes-and-reviews) in the Git Changes section, or from a workspace session by clicking the Changes tab. The top section shows your file tree with selection checkboxes. The bottom section displays the diff with Monaco Editor syntax highlighting.

## Creating Commits

Select the files you want to include, then write your commit message in the text area. Follow conventional commit format: `type(scope): description`. Common types include `feat` for features, `fix` for bugs, `docs` for documentation, and `refactor` for restructuring. Keep the first line under 72 characters.

Click **Commit** or press `Cmd+Enter` to create the commit. Treq validates that you have selected files and a non-empty message before proceeding.

## Discarding Changes

**Deselecting** a file removes it from the next commit without losing changes. **Discarding** restores the file to its last committed state. Right-click a file and choose Discard. The success toast includes **Undo**, which restores the working-copy snapshot Treq took before the discard.

