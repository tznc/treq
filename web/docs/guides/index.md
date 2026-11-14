---
sidebar_position: 1
description: "Guides to using Treq with coding agents: isolated workspaces, automatic rebasing, review before merge, and GitHub pull requests."
---

# Guides

Treq is an AI code review manager for developers who use coding agents. It gives each task an isolated workspace, keeps those workspaces rebased as the underlying code changes, and lets you review agent output before it reaches your main branch.

Use Treq to inspect diffs like a GitHub PR, leave inline comments, and send those comments back to an agent for revision. Feedback stays tied to the specific code so agents can act on it in the background. With a GitHub `origin` remote and GitHub access set up, you can also create and open real pull requests, read CI, and load GitHub review threads on the Review tab. See [GitHub Integration](/docs/concepts/github-integration) and [Connecting GitHub](/docs/how-to/connecting-github).

Workspaces are isolated copies of your codebase. Multiple agents can run in parallel without stepping on each other. Treq automatically rebases dependent workspaces, can hand conflict resolution to AI, and supports stacked workspaces for breaking large features into smaller reviewable branches.

Start with [Installation](/docs/getting-started/installation) to select a repository and create your first workspace. Follow the [Tutorials](/docs/tutorials) for step-by-step Treq workflows. For conceptual background on AI-assisted development and Git workflows, see the [Learn](/learn) section.
