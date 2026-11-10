---
sidebar_position: 11
description: Run multiple OpenAI Codex CLI agents in parallel with separate git worktrees or Treq workspaces so agents do not share a checkout.
---

# How to Run Multiple Codex Agents in Parallel

_Start several Codex CLI agents at once without sharing one working directory._

## Goal

Run two or more Codex CLI sessions on the same repository, each on its own branch and working copy, then review and merge their results independently.

## Prerequisites

- A Git repository with a clean or committed `main`
- The Codex CLI installed and available as `codex` on your `PATH`
- Enough disk for one dependency install per worktree

## CLI-first setup with git worktrees

Fetch current `main`, then create one worktree per task:

```bash
git fetch origin

git worktree add -b agent/payments-retry ../repo-payments-retry origin/main
git worktree add -b agent/search-filters ../repo-search-filters origin/main
```

Install dependencies in each worktree if the project needs them, then start Codex inside each directory in separate terminal tabs:

```bash
cd ../repo-payments-retry
codex

# other tab
cd ../repo-search-filters
codex
```

Codex edits files in the directory it starts in. Starting it from inside the worktree is what keeps the two sessions apart. Give each session one task, the files it may change, and the tests it must run.

When a session finishes:

```bash
cd ../repo-payments-retry
git status
git diff
git push -u origin agent/payments-retry
```

## Setup with Treq workspaces

In Treq, create one workspace per task, then pick Codex as the agent. Treq starts `codex` inside the workspace directory with your prompt, so the process cannot write to another task's files.

Treq runs Codex in edit mode only. For broad or unclear tasks, ask Codex in the prompt to propose a plan and stop before editing. You can make Codex a repository's default agent in its settings. You can also start it from a terminal:

```bash
treq agent start payments-retry "Add retry with backoff to the payments client" --agent codex
```

When Codex finishes, review the workspace diff, leave line comments, and send them back to the agent. See [Agent Sessions](/docs/concepts/agent-sessions) and the [CLI reference](/docs/reference/cli).

## Mix agents in one project

Treq is not tied to one agent. Claude Code, Codex, Cursor Agent and GitHub Copilot CLI all run in the same kind of workspace, so you can pick the agent per task instead of per project. The repository setting only sets the default.

Match each task to the model you trust most for that kind of work. For example, give exploratory or unclear tasks to an agent with a plan mode, such as Claude Code or Cursor Agent, and give well-scoped edits to whichever agent is fastest for you. Your own results are the best guide, since model strengths shift with every release.

Agents can share one stack. The database layer can come from one agent and the UI layer from another. Treq still rebases the upper workspace when the lower one changes, and your review comments go back to the agent that owns each workspace.

```bash
treq add db-schema
treq add orders-ui --source-branch db-schema
treq agent start db-schema "Add the orders table and migration" --agent codex
treq agent start orders-ui "Build the orders page on the new API" --agent claude
```

## Common failure cases

**Both sessions share the home checkout.** Symptoms include interleaved edits and agents undoing each other's changes. Move each session into its own worktree before continuing.

**Same branch checked out twice.** Git blocks a second worktree on the same local branch. Use a unique branch name per session.

**Stale base after the other PR merges.** Rebase the remaining worktree with `git -C ../repo-search-filters rebase origin/main`. In Treq, stacked workspaces rebase automatically when the workspace below them changes.

**Review queue overflow.** Cap concurrent sessions to what you can review the same day. Finished branches that wait go stale and conflict later.

## Cleanup

```bash
git worktree remove ../repo-payments-retry
git worktree remove ../repo-search-filters
git branch -d agent/payments-retry agent/search-filters
```

## Related

- [How to Run Multiple Claude Code Sessions](/learn/how-to/how-to-run-multiple-claude-code-sessions)
- [How to Run Multiple Cursor Agents in Parallel](/learn/how-to/how-to-run-multiple-cursor-agents)
- [Git Worktrees for AI Coding Agents](/learn/git-worktrees-for-ai)
- [How to Run Multiple GitHub Copilot CLI Agents in Parallel](/learn/how-to/how-to-run-multiple-copilot-agents)
- [Parallel Coding Agents Without Branch Chaos](/learn/parallel-coding-agents)
