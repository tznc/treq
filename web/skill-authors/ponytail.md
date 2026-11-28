---
description: "Dietrich Gebert's Ponytail: a lazy senior dev mode that steers coding agents to the smallest complete change, plus review, audit and shortcut-debt skills."
---

Ponytail is a minimalism mode for coding agents from Dietrich Gebert, with companion skills that review a diff, audit a repo and list deferred shortcuts. The core skill tells the agent to make the smallest change that fully solves the task and to reuse what the codebase already has. Each reply ends with what the agent skipped or left unchecked. Install it if your agent over-builds and you want one always-on rule set more than a planning workflow.

## History

Gebert published the repo in June 2026 and iterated fast. The first day shows skill versions two through four, a benchmark against a no-skill control, and tags v1.0.0 and v4.0.0. Contributors added Codex, Kiro and Pi support that same day, and Gebert added an OpenCode adapter the next.

June held 155 commits, and activity then slowed to 4 commits in August. Version 5.0.0 in October 2026 rebuilt the core skill along with the review and audit skills, and October brought 145 commits. The latest tag is v5.3.3 from October 2026, the repo has 29 tags in under six months, and November has 3 commits so far.

## Depth

The catalog lists six skills totaling 428 lines, with a median of 76 and `ponytail-audit` the longest at 103. None has a supporting file. `ponytail` is a decision ladder: skip unrequested features, reuse existing code, then the standard library, then an installed dependency, then the minimum new code. It also lists what you must never cut, including validation at trust boundaries, security and accessibility, and it offers lite, full and ultra levels.

`ponytail-review` is a full procedure. It reads the callers of changed code beyond the diff and groups findings as must fix, should fix or nice to have. Each finding needs a concrete failing case and a fix. `ponytail-debt` greps `shortcut:` comments into a ledger of deferred limits and flags the ones that name no upgrade trigger.

Two of the six are thin: `ponytail-gain` prints the benchmark scoreboard, and `ponytail-help` documents the commands. The skills are prompts and short procedures, and the behavior that keeps them active lives in hooks. Read the hooks before you read the skills if you want to know what the install changes.

## Tooling

On Claude Code, a `SessionStart` hook injects the rule set, a `SubagentStart` hook passes it to subagents, and a `UserPromptSubmit` hook tracks the active level. Version 5.3 added a status band above the prompt that shows cost, tokens, agent time and whether tests passed after the last change. A local board page charts the same data across projects and stores it in `~/.config/ponytail/band/` by default.

The repo carries plugin or rule files for more than a dozen agents, among them Claude Code, Codex, Cursor, Gemini CLI, Kiro, Windsurf and Pi, and a `README.md` badge claims 20. A CI workflow publishes an npm package. The test workflow runs a drift check across rule copies, a version consistency check, Node tests and a benchmark self-test. The `benchmarks/` folder holds promptfoo configs and an agentic test runner, so the method behind the published numbers is open to inspection.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail) |
| Stars / forks | 196.8k / 11.1k |
| Open issues / PRs | 145 / 170 |
| Commits / contributors | 372 / 134 |
| First commit | June 2026 |
| Last 90 days | 162 commits |

As of November 2026, Ponytail has the widest contributor base in this group: 134 people after merging duplicate names and emails. Gebert wrote 176 of 372 commits under four identities, about 47%. Of the other contributors, 102 have exactly one commit, and most commits are squash merges that cite a PR number.

The open queue is large next to the code: 145 issues and 170 PRs for six skills that total 428 lines. Stars reached 196.8k, far ahead of the repo's size and of the 9 commits in the last 30 days. Stars here measure attention, and the open PR count points to a review backlog.

## Utility for target workloads

Ponytail fits engineers whose agents add libraries, wrappers and options nobody asked for. You can keep a feature to a few lines by reusing a house component, and run `/ponytail-review` on a PR to find callers the diff breaks. `/ponytail-audit` ranks a repo's risks, and `/ponytail-debt` tracks the limits you deferred. `README.md` reports, from its own benchmark of 39 tasks, 53% less code and 45% fewer tokens than the same agent without the skill.

It needs no accounts or API keys. Its review skill overlaps with code review in [Superpowers](/skills/superpowers), [Matt Pocock](/skills/mattpocock), [Addy Osmani](/skills/addyosmani) and [gstack](/skills/gstack). Ponytail does not plan or execute multi-step work, so pair it with one of those sources or with [Planning with Files](/skills/planning-with-files) for that part. Install it as a style layer that keeps diffs small, and keep a workflow source for planning.
