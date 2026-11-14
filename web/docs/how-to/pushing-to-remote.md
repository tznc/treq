---
sidebar_position: 2
description: "Push commits from Treq to a remote repository: set upstream tracking, push from the dashboard, handle rejections, and force push."
---

# Pushing to Remote

_How to push commits to remote repositories._

## Basic Push

For new branches not yet on remote, use `-u` to set up tracking:

```bash
git push -u origin treq/your-branch
```

After the first push, subsequent pushes only need `git push`.

## In Treq Dashboard

Click the push icon (↑) on the workspace card, or right-click and select **Push**. Status indicators show commits ahead (↑2) and behind (↓3) relative to the remote.

On a GitHub remote, the workspace header **Create PR** control pushes the branch when needed, then opens a pull request. The Review tab Commit dropdown also offers **Commit and push** and **Commit and create PR**. See [Creating and Viewing Pull Requests](/docs/how-to/creating-and-viewing-pull-requests).

## Auto-Push After Commit

Repository Settings includes **Auto-push to remote**. When enabled, Treq pushes after every commit in that repository. Leave it off if you prefer to push only when you ask, or when Create PR pushes for you.

## Handling Rejections

If remote has commits you don't have ("Updates were rejected"), pull first:

```bash
git pull origin treq/your-branch
# Or with rebase:
git pull --rebase origin treq/your-branch
```

Then push again.

For authentication issues, configure your credential helper for HTTPS (`git config --global credential.helper store`) or ensure your SSH key is added (`ssh-add ~/.ssh/id_rsa`).

## Force Push

Only force push when you [rebased](/learn/concepts/git/merge-vs-rebase) your own branch or are fixing mistakes in unpushed [commits](/docs/concepts/commit-management). Never force push shared or main branches.

```bash
git push --force-with-lease origin treq/your-branch
```

Use [`--force-with-lease`](/learn/concepts/git/force-with-lease) instead of `--force`. It fails if someone else pushed, preventing accidental overwrites.

## Best Practices

Push often to keep local and remote in sync. Pull before pushing to avoid conflicts. Never force push main or shared branches.

## Next Steps

- [Creating and Viewing Pull Requests](/docs/how-to/creating-and-viewing-pull-requests)
- [Connecting GitHub](/docs/how-to/connecting-github)
- [Customizing Settings](/docs/how-to/customizing-settings)
- [Commit Management](/docs/concepts/commit-management)

