---
sidebar_position: 1
---

# Using Treq with a Git Repository

_How to set up Treq with your Git repository and understand the relationship between the repo and Treq workspaces._

Treq works with Git repositories. You pick a repository, create workspaces for parallel work, and keep using Git remotes to fetch and push. You never need to know [Jujutsu](https://jj-vcs.github.io/jj/latest/) to use Treq.

When you open a repository, Treq adds a `.treq/` folder for workspaces and local metadata, and sets up colocated `.jj/` state beside it. Each folder carries its own `.gitignore` that ignores it, so Treq never edits your repository's `.gitignore`.

Colocation is the [under-the-hood](/docs/under-the-hood) part. It lets Treq rebase [workspace](/docs/concepts/workspaces) branches when a target moves, which [Git worktrees](/learn/concepts/git/git-worktrees) cannot do. Treq runs those Git and Jujutsu commands for you, and the `jj` CLI is a power-user escape hatch rather than the normal path.

## Repository Structure

```text
your-project/
├── .git/                    # Your Git repository (native VCS)
├── .jj/                     # Colocated Jujutsu state (managed by Treq, git-ignored)
├── .treq/                   # Treq's data (git-ignored)
│   ├── workspaces/           # All workspaces stored here
│   │   ├── treq-feature-1/
│   │   └── treq-bugfix-2/
│   └── plans/               # Saved implementation plans
├── src/                     # Your source code
└── ...
```

Your repository stays at the root. Every workspace sits in `.treq/workspaces/`, each on a different branch but sharing the same history.

## Repository vs Workspaces

The **repository** is your original directory. Use it for quick fixes, for merging workspaces back, and for pulling remote updates. It sits on the left of the dashboard.

**Workspaces** are separate directories tied to the same repository, each with its own branch and its own working tree. Make one for a feature, a bug fix, a PR review, or to try two approaches side by side. They show up as cards on the right of the dashboard.

## Why Treq Uses Jujutsu

Git is still what you authenticate to remotes with, and it is still what the repository is. Jujutsu sits beside it in colocated mode so Treq can update dependent workspace branches on its own when a target moves.

That automatic rebase is the whole reason Jujutsu is there. Stay in Treq to create workspaces, commit, update a stack, merge, and sync. Drop to Git in the terminal when you want the commands you already know. Save `jj` for deliberate debugging.

## Git Configuration

Make sure your repository has a remote set up, so push, pull, and commit tracking work:

```bash
git remote -v
# If missing:
git remote add origin https://github.com/user/repo.git
```

Treq uses whatever Git authentication your system already has. Set up a credential helper for HTTPS or add SSH keys the way you normally would.

## Large Repositories

For a repository over 1GB, consider sparse checkout so a workspace only pulls the files it needs, shallow clones with `--depth 1`, or Git LFS for large binaries. Put build output and dependencies such as `dist/`, `node_modules/`, and `venv/` in `.gitignore` to speed up file watching and keep workspaces small. When creating a workspace, open Advanced and add those heavy directories under **Symlink from home repo** (suggestions come from `.gitignore`) so the workspace links to the home copy instead of duplicating them.

## Switching Repositories

Treq holds one repository at a time. To switch, click the folder icon and pick another. Each repository keeps its own workspaces, sessions, and settings in its `.treq/` folder.

## Maintenance

If workspaces are not showing up right, use Settings → Repository → **Rebuild Workspaces Database** to rescan `.treq/workspaces/`. Delete the ones you are done with, since each holds a full copy of your files.
