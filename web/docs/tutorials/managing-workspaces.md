---
sidebar_position: 3
description: Create, work in, configure, and troubleshoot Treq workspaces, so you can work on several branches at once, each in its own directory.
---

# Managing Workspaces

_How to create, work in, configure, and troubleshoot Treq workspaces._

## Creating a Workspace

[Workspaces](/docs/concepts/workspaces) let you work on multiple branches simultaneously, each in its own directory. Click **New Workspace** in the dashboard (or `Cmd+N`) to open the creation dialog.

If you started work in the wrong place, you can [move files between workspaces](/docs/how-to/moving-files-between-workspaces) afterward.

Choose **Create new branch** and enter a name or intent like `add-user-profile`. Treq applies your branch naming pattern (default: `treq/{name}`) so this becomes `treq/add-user-profile`. Alternatively, select **From existing branch** to check out an existing local or remote branch.

Select a **base branch** (usually `main` or `develop`) that your new branch will start from. Optionally add a **plan title** like "Add user profile page with avatar upload" to help you remember what you're working on.

Click **Create Workspace**. Treq creates the directory in `.treq/workspaces/` and checks out the branch.

## Working in Your Workspace

Click **Open** on the workspace card to open a terminal session. The working directory is automatically set to your workspace path. Make changes, run commands, and use Git as you normally would. Treq detects changes automatically and updates the dashboard.

The workspace card shows status indicators: commits ahead/behind the base branch, uncommitted changes, and available actions (Open, View Changes, Merge, Delete).

## Branch Naming Patterns

Configure patterns in Settings → Repository → Branch Pattern, described in [Customizing Settings](/docs/how-to/customizing-settings). Examples: `treq/{name}` (default), `feature/{name}`, `dev/{user}/{name}`, or just `{name}` for no prefix. Available variables include `{name}` (your input), `{user}` (git username), and `{date}` (`YYYY-MM-DD`).

## Working with Remote Branches

To work on a teammate's branch, first fetch (`git fetch origin`), then create a workspace from the existing branch and select the remote branch (e.g., `origin/feature-branch`). Treq creates a local tracking branch.

## FAQ

<details>
<summary>Where are workspaces stored?</summary>

In `.treq/workspaces/{branch-name}/` relative to your repository.

</details>

<details>
<summary>Can I use my regular editor?</summary>

Yes. Open the workspace directory in VS Code, Cursor, or any editor.

</details>

<details>
<summary>Can I create multiple workspaces?</summary>

Yes, create as many as you need for different features or fixes.

</details>

<details>
<summary>How do I delete a workspace?</summary>

Click Delete on the workspace card, after committing or discarding changes.

</details>

## Troubleshooting

If you see "Branch already exists," choose a different name or select the existing branch. If a workspace doesn't appear, use Settings → Rebuild Workspaces Database.
