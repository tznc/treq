---
sidebar_position: 1
slug: /
description: "Technical overview of Treq: managed workspaces in your Git repository, review, and push through your Git remotes on macOS, Windows, and Linux."
---

# Treq

_Technical overview of Treq's workspace model and runtime behavior on macOS, Windows, and Linux._

Treq is a local review and workspace manager for Git repositories. You open a repo, create workspaces for parallel work, and push through your normal Git remotes. You never need to know [Jujutsu](https://jj-vcs.github.io/jj/latest/) to use it.

Under the hood, Treq puts Git and Jujutsu side by side in the same repo, which is how it can rebase workspace branches when their targets move. [Git worktrees](/learn/concepts/git/git-worktrees) cannot do that. Treq runs those operations for you, and the `jj` CLI is a power-user escape hatch rather than the normal path.

The app treats [workspaces](/docs/concepts/workspaces) as places for agents and people to work. It tracks branch state, file changes, terminal sessions, review state, and how stacks relate, so work can move through review without tying up your main checkout.

## Architecture

Treq has four main layers.

| Layer | Responsibility |
|---|---|
| Interface | Renders the dashboard, workspace detail view, diffs, review UI, terminal panes, and settings. |
| Core runtime | Creates workspaces, reads repository state, stages files, commits changes, rebases stacks, and moves changes between workspaces. |
| Repository-local storage | Stores workspace metadata, repository settings, review comments, terminal session state, and plan history under `.treq`. This state belongs to the selected repository. |
| Global app storage | Stores app-level preferences and cross-repository state in the application database. This state follows the local app install, not a specific repository. |

Git is the version control you actually work with. Remotes, authentication, and push or pull all stay on the Git side. Jujutsu, sitting beside it, supplies the revision model behind automatic workspace rebasing. The interface refreshes after you act, after a file watcher fires, after terminal activity, and when you ask it to. Slow repository reads are cached, and Treq clears that cache after anything that could change branch or file state.

## Workspace Behavior

A workspace is a separate working copy for one line of work. Treq creates it under `.treq/workspaces/<workspace-name>` and keeps metadata about it for display and coordination.

The source repository stays the authority for shared history. Workspaces are meant to be thrown away. Delete one only after you have committed, moved, or discarded the work you care about.

## Review Behavior

Treq reviews are local. Comments attach to files and line ranges in the workspace diff. The review screen keeps file navigation, the diff, inline comments, and commit history in one place, so a reviewer can read the whole change or one commit at a time.

Review comments do not sync to a remote review system by default. When a workspace has an open GitHub pull request, the Review tab can also show GitHub review threads inline. See [GitHub Integration](/docs/concepts/github-integration).

## Terminal Behavior

Each workspace can hold several [terminal sessions](/docs/concepts/terminal-sessions). A terminal starts in the workspace directory, runs as a real shell process, and keeps running when you switch tabs or workspaces.

Terminal state belongs to the workspace. Closing a session can kill the process behind it, and deleting a workspace closes its sessions, because the directory they lived in is gone.

## Learn More

- [Security and Privacy](/docs/security-and-privacy)
- [Workspaces](/docs/concepts/workspaces)
- [Using Treq with a Git Repository](/docs/tutorials/using-treq-with-git-repo)
- [Changes and Reviews](/docs/concepts/changes-and-reviews)
- [GitHub Integration](/docs/concepts/github-integration)
- [Under the Hood](/docs/under-the-hood)
- [Terminal Sessions](/docs/concepts/terminal-sessions)
- [CLI](/docs/reference/cli)
- [Contributing](/docs/reference/contributing)
