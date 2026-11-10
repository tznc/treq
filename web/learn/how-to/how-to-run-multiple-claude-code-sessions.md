---
sidebar_position: 10
description: Run multiple Claude Code sessions in parallel with separate git worktrees or Treq workspaces so agents do not share a checkout.
---

# How to Run Multiple Claude Code Sessions

_Start several Claude Code agents at once without sharing one working directory._

## Goal

Run two or more Claude Code sessions on the same repository, each on its own branch and working copy, then review and merge their results independently.

## Prerequisites

- A Git repository with a clean or committed `main`
- Claude Code installed and available as `claude` on your `PATH`
- Enough disk for one dependency install per worktree

## CLI-first setup with git worktrees

Fetch current `main`, then create one worktree per task:

```bash
git fetch origin

git worktree add -b agent/payments-retry ../repo-payments-retry origin/main
git worktree add -b agent/search-filters ../repo-search-filters origin/main

git worktree list
```

Install dependencies in each worktree if the project needs them:

```bash
npm install --prefix ../repo-payments-retry
npm install --prefix ../repo-search-filters
```

Start Claude Code inside each directory, in separate terminal tabs:

```bash
cd ../repo-payments-retry
claude

# other tab
cd ../repo-search-filters
claude
```

Give each session one task, the files it may change, and the tests it must run. Do not point two sessions at the same directory.

When a session finishes:

```bash
cd ../repo-payments-retry
git status
git diff
git push -u origin agent/payments-retry
```

Open a pull request, review it, and only then start another large task for the same reviewer.

## Setup with Treq workspaces

In Treq, create one workspace per task from the dashboard. Each workspace gets a directory under `.treq/workspaces/` and its own bookmark. Start a Claude Code session from the workspace so the process inherits that working directory.

See [Agent Sessions](/docs/concepts/agent-sessions) and [Workspaces](/docs/concepts/workspaces).

## Mix agents in one project

Treq is not tied to one agent. Claude Code, Codex and Cursor Agent all run in the same kind of workspace, so you can pick the agent per task instead of per project. The repository setting only sets the default.

Match each task to the model you trust most for that kind of work. For example, give exploratory or unclear tasks to an agent with a plan mode, such as Claude Code or Cursor Agent, and give well-scoped edits to whichever agent is fastest for you. Your own results are the best guide, since model strengths shift with every release.

Agents can share one stack. The database layer can come from one agent and the UI layer from another. Treq still rebases the upper workspace when the lower one changes, and your review comments go back to the agent that owns each workspace.

```bash
treq add db-schema
treq add orders-ui --source-branch db-schema
treq agent start db-schema "Add the orders table and migration" --agent claude
treq agent start orders-ui "Build the orders page on the new API" --agent codex
```

## Common failure cases

**Both sessions share the home checkout.** Symptoms include interleaved edits, unexpected `git status`, and agents fighting over the same file. Fix by moving each session into its own worktree before continuing.

**Same branch checked out twice.** Git blocks a second worktree on the same local branch. Use unique branch names per session.

**Stale base after the other PR merges.** Rebase the remaining worktree:

```bash
git -C ../repo-search-filters fetch origin
git -C ../repo-search-filters rebase origin/main
```

**Orphan worktree after a killed terminal.** Remove or prune it:

```bash
git worktree remove ../repo-payments-retry
# or, if the directory is already gone:
git worktree prune
```

**Review queue overflow.** Cap concurrent sessions to what you can review the same day. Unused finished branches go stale and create conflicts later.

## Cleanup

```bash
git worktree remove ../repo-payments-retry
git worktree remove ../repo-search-filters
git branch -d agent/payments-retry agent/search-filters
```

## Related

- [How to Run Multiple Codex Agents in Parallel](/learn/how-to/how-to-run-multiple-codex-agents)
- [How to Run Multiple Cursor Agents in Parallel](/learn/how-to/how-to-run-multiple-cursor-agents)
- [Git Worktrees for AI Coding Agents](/learn/git-worktrees-for-ai)
- [Parallel Coding Agents Without Branch Chaos](/learn/parallel-coding-agents)
- [Git Worktrees vs Clones for Agents](/learn/how-to/git-worktrees-vs-clones-for-agents)
- [Setting Up Claude Code](/learn/tutorials/setting-up-claude-code)
