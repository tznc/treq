---
description: "Addy Osmani's 26 lifecycle skills for coding agents: spec, plan, build, test, review and ship, with slash commands, reviewer personas and routing evals."
---

Addy Osmani maintains `addyosmani/agent-skills`, a pack of 26 skills that walks a coding agent through the software lifecycle from idea to launch. Ten slash commands such as `/spec`, `/plan`, `/build`, `/review` and `/ship` route into the skills, and five persona files run targeted reviews. The skills read like a staff engineer's handbook, and `README.md` credits Google's published engineering practices as a source. Install it if you want broad written coverage of engineering practice, including instrumentation and deprecation, which the other workflow sources here have no skills for.

## History

Osmani made the first commit in February 2026 and added 18 skills that day, along with commands, hooks and agent personas. A contributor added the Claude Code plugin manifest a week later, and tags started at 0.5.0 in April 2026. Outside contributors wrote several later skills: Federico Bartoli added `doubt-driven-development` in May 2026 and `observability-and-instrumentation` in June.

Osmani added `interview-me` in May and a three-tier eval framework in July 2026. Activity has climbed since then, from 94 commits in August to 210 in October, the busiest month. The latest tag is 0.6.17 from November 2026, and the skill count has grown from 21 at 0.5.0 to 26.

## Depth

The 26 `SKILL.md` files are long and uniform: 7,552 lines in total, a median of 307, and `code-review-and-quality` the longest at 402. Each follows the same anatomy: overview, when to use, a stepwise process, red flags and verification. All but the `using-agent-skills` router include a table of rationalizations an agent uses to skip steps. `code-review-and-quality`, for example, defines a five-axis review, change sizing around 100 lines and severity labels.

Supporting files are thin. Only five skills have any, nine files in all, and the other 21 are a single `SKILL.md`. Shared checklists for security, performance, accessibility, observability and testing live in a root `references/` folder, and `README.md` warns that a per-skill `npx skills` install does not copy it.

`README.md` calls them workflows, not reference docs. In practice they are procedures that carry a lot of reference material, such as Hyrum's Law in `api-and-interface-design` and Chesterton's Fence in `code-simplification`, and few ship scripts for the agent to run. `doubt-driven-development` stands out as a process: it spawns a fresh-context reviewer told to disprove each non-trivial decision, for at most three cycles.

## Tooling

The repo ships a Claude Code marketplace, a Codex plugin, Gemini CLI commands as `.toml` files, and OpenCode and Antigravity support. Setup docs cover Cursor, Windsurf, Copilot and Kiro, and `npx skills add addyosmani/agent-skills` reaches other agents. Hooks add a session-start script, a citation cache for `source-driven-development` and a filter that hides marked blocks from `/code-simplify`.

One workflow, `test-plugin-install.yml`, runs six jobs: skill validation, command parity across tool folders, a tooling check on several operating systems, hook regression tests, plugin structure checks and an install test. The validation job also runs a TF-IDF routing eval and fails when the rank-one score drops below 95%. A behavioral tier runs each skill's `evals.json` cases through headless Claude Code on demand, and `evals/cases/` holds a case file for all 26 skills.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [addyosmani/agent-skills](https://github.com/addyosmani/agent-skills) |
| Stars / forks | 119.7k / 12.4k |
| Open issues / PRs | 61 / 68 |
| Commits / contributors | 866 / 107 |
| First commit | February 2026 |
| Last 90 days | 401 commits |

As of November 2026, Addy Osmani wrote 426 of the 866 commits, about half, and roughly 106 other people wrote the rest. The history holds 126 merge commits from 58 distinct branch owners. Federico Bartoli, Joan León, Rudy Celekli and ayobamiseun each have dozens of commits, and outside authors still wrote a large share of October and November commits.

The queue is moderate for the attention: 61 open issues and 68 open PRs against 119.7k stars. Recent cadence is high, with 401 commits in 90 days and 127 in the last 30. Merge commits from contributors' forks appear in every month from March to September 2026, so outside pull requests do get reviewed and merged.

## Utility for target workloads

This pack fits teams that want an agent to follow written engineering standards across the whole lifecycle. You can write a PRD with `spec-driven-development` before coding and split it into ordered tasks with `planning-and-task-breakdown`. `code-review-and-quality` reviews a diff on five axes, `accessibility-engineering` audits a page with axe-core and pa11y, and `observability-and-instrumentation` adds structured logging and tracing. `browser-testing-with-devtools` expects the Chrome DevTools MCP server, and the other skills need no accounts.

The long skills cost context: 7,552 lines across 26 files, against 3,358 lines for the 15 in [Superpowers](/skills/superpowers). Superpowers enforces a narrower loop more strictly, and [Matt Pocock](/skills/mattpocock) covers interviews and TDD in a fraction of the lines. The `evals/README.md` here names Superpowers as prior art for its behavioral tests.

[gstack](/skills/gstack) covers the same review and ship steps with runnable tools and a browser, at the cost of a heavy install. [Ponytail](/skills/ponytail) makes the opposite trade on length: its review skill runs 101 lines against 402 for `code-review-and-quality`. [Planning with Files](/skills/planning-with-files) re-injects the plan through hooks, which goes further than the `tasks/plan.md` file that `planning-and-task-breakdown` writes. Install this pack as a reference library and enable the skills that match gaps in your team's process.
