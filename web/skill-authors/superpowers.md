---
description: "Jesse Vincent's Superpowers: 15 workflow skills that make a coding agent brainstorm, plan, test first, run subagents and review before it ships."
---

Superpowers is a skills library and plugin from Jesse Vincent and the team at Prime Radiant, published as `obra/superpowers`. It turns a coding agent into one fixed workflow: draw out a design, write a plan, execute it through subagents, and review every task. A session-start hook loads a bootstrap skill that tells the agent to check for a matching skill before any response. Install it if you want one opinionated process enforced by default and can pay the token cost of per-task review.

## History

Jesse Vincent made the first commit in October 2025 as "Superpowers plugin v1.0.0", with 22 skills in category folders and a conversation-search tool. Within a week, v2.0 moved every skill into a separate `obra/superpowers-skills` repository. Days later, v3.0 brought them back onto Claude Code's native skills system. Each major version since then has rebuilt one stage of the workflow.

Version 4.0 in December 2025 added two-stage review to subagent execution. Version 5.0 in March 2026 made subagent execution mandatory on agents that support it and added a browser-based visual companion for brainstorming. Version 6.0 in June 2026 merged the two task reviewers into one, and 7.0 in October 2026 rebuilt `brainstorming`. The repo has 39 tags, from v3.1.0 to v7.0.2 in November 2026.

Since August 2026, `main` has received almost nothing but one squash commit per release, and `README.md` tells contributors to branch from `dev`. That is why the last 90 days show only 6 commits on `main` while five releases shipped. The commit count understates activity, so read the release notes for cadence.

## Depth

The catalog lists 15 skills with 3,358 lines of `SKILL.md` in total, a median of 205, and `writing-skills` the largest at 399. Behind them sit 65 supporting files, clustered in a few skills. `subagent-driven-development` carries 14, including implementer and reviewer prompts and the `sdd-workspace`, `task-brief` and `review-package` scripts. `diagnosing-superpowers` has 19 reference files for auditing a session that went wrong.

Six skills, including `verification-before-completion` and `writing-plans`, are a single `SKILL.md`. All of them are opinionated procedures written to resist an agent's shortcuts. Four state an "Iron Law", such as the `test-driven-development` rule that code written before its test gets deleted. Ten carry tables of rationalizations with rebuttals.

`using-superpowers` tells the agent to invoke a skill if there is even a 1% chance it applies. `writing-skills` applies the same discipline to authoring: run a pressure scenario without the skill, watch the agent fail, then write the skill that fixes the failure. Its `references/` folder still carries the persuasion-principles notes Vincent added on the first day.

## Tooling

The repo holds plugin manifests or extensions for Claude Code, Codex, Cursor, Gemini CLI, OpenCode, Pi, Antigravity, Devin, Hermes, Kimi Code and Muse. `README.md` gives install steps for 15 agents. On Claude Code you install from Anthropic's official plugin marketplace or from `obra/superpowers-marketplace`. On most of them, a hook or extension injects the `using-superpowers` bootstrap at session start.

The repo has no GitHub Actions workflows. Tests in `tests/` are shell, Node and Python scripts that check hooks, per-agent packaging, skill layout and skill size. Behavioral evals run real agent sessions from a separate `superpowers-evals` repository that contributors clone into `evals/`, so no eval results live here. The visual companion loads a logo from Prime Radiant's site with the plugin version, and `SUPERPOWERS_DISABLE_TELEMETRY` turns that off.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [obra/superpowers](https://github.com/obra/superpowers) |
| Stars / forks | 324.0k / 28.7k |
| Open issues / PRs | 203 / 159 |
| Commits / contributors | 687 / 41 |
| First commit | October 2025 |
| Last 90 days | 6 commits |

As of November 2026, Jesse Vincent has written 542 of the 687 commits under three identities. Drew Ritter, who also commits from a Prime Radiant address, wrote 86. The remaining 57 commits come from 39 people, most with one or two each, and 10 of the 20 merge commits come from outside forks.

The contribution policy is strict. `README.md` says the team does not generally accept new skills and rejects agent-written PRs for issues someone else filed. With 324.0k stars, the repo carries 203 open issues and 159 open PRs, and release PR numbers reach #2657. The stars show reach, while the commit history shows a small core team that ships the workflow in large, versioned releases.

## Utility for target workloads

Superpowers suits engineers who want the agent to run a full loop on feature work without stepping in between tasks. You can turn a vague request into a design doc with `brainstorming` and split it into 2-5 minute tasks with `writing-plans`. `subagent-driven-development` then runs those tasks in an isolated worktree, and `systematic-debugging` ships `find-polluter.sh` to bisect which test leaves unwanted files or state behind.

No accounts or API keys are needed, but per-task subagents and reviews cost tokens. The v6.0 release notes report that the review redesign cut token spend almost in half in the team's own evals. `subagent-driven-development` states the trade itself: a fresh context per task and per review, against one context and one final review for inline `executing-plans`.

It overlaps with the other methodology sources. [Matt Pocock](/skills/mattpocock) covers interviews, TDD and review with small skills you call yourself, and [Addy Osmani](/skills/addyosmani) spreads a similar lifecycle across 26 longer reference skills. [gstack](/skills/gstack) wraps its own sprint in a Claude Code runtime, [Planning with Files](/skills/planning-with-files) keeps plan state on disk across `/clear`, and [Ponytail](/skills/ponytail) also injects rules at session start. Install Superpowers if you want the process enforced, and skip it if you prefer to choose the practice for each task.
