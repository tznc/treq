---
sidebar_position: 1
description: Stop two coding agents from overwriting each other's files by giving each its own git worktree or workspace instead of one shared checkout.
---

# Stop Two Coding Agents From Overwriting Each Other

_Give each agent its own working directory. Two agents in one checkout share every file, every build and every test run._

## The Fix

Create one worktree per agent and start each agent inside its own directory:

```bash
git worktree add -b agent/api ../repo-api main
git worktree add -b agent/ui ../repo-ui main
cd ../repo-api && claude
```

## Why It Happens

An agent edits files and runs commands in the directory it starts in. Two agents in the same directory see each other's half-finished edits, and one agent's failing build becomes the other agent's problem. `git status` shows changes from both, so neither can commit only its own work. Separate branches in the same directory do not help, because switching branches changes the files under the other agent.

## How Treq Handles It

Treq creates a separate [workspace](/docs/concepts/workspaces) per task and starts the agent inside it. Any supported agent works, and different workspaces can run different agents.

## Related

- [Run multiple Claude Code sessions](/learn/how-to/how-to-run-multiple-claude-code-sessions)
- [Parallel Coding Agents](/learn/parallel-coding-agents)
