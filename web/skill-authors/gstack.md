---
description: "Garry Tan's gstack: 62 role-based skills that run a Claude Code sprint, from office hours and plan review to browser QA, code review and shipping."
---

gstack is the Claude Code setup of Garry Tan, whom the `README.md` introduces as President and CEO of Y Combinator. It models a virtual team: `/office-hours` and `/plan-ceo-review` challenge the product, `/review` and `/qa` check the work, and `/ship` opens the PR. The skills depend on a local runtime of compiled tools, a browser daemon and hooks, so you adopt the whole system and not individual files. Install it if you work in Claude Code on macOS or Linux and want an opinionated sprint with real browser QA.

## History

Tan released v0.0.1 in March 2026 with six skills, including `browse`, `review`, `ship` and `retro`. Within days Garry Tan added a template system that generates each `SKILL.md` from a `SKILL.md.tmpl` file, plus an eval suite with planted bugs. The catalog grew to 31 skills by the end of March, 52 by the end of May and 62 today. `CHANGELOG.md` lists 835 versions, but git tags only start in October 2026, when Tan added tagged releases and a stable update channel.

Read the commit counts with care. Only 466 of the 1,418 commits touch a skill's `SKILL.md` or template, and the rest change the browser, tests, CI and release tooling. October and November 2026 alone hold 1,006 commits, and 956 of them carry a `Co-authored-by: capy-ai[bot]` trailer. `CONTRIBUTING.md` explains that AI agents maintain the repo day to day and commit under Tan's account.

## Depth

The 62 skills are the longest in this group: 29,174 lines in total, a median of 446 and `design-review` at 1,239. Much of that length is shared machinery. Each generated `SKILL.md` opens with a preamble that calls gstack binaries to load shared sections, tune questions and log local analytics, then runs the skill's own steps. `review` alone adds a specialists folder, a checklist, a design checklist and a gate-integrity guide.

Behind the skills sit 800 supporting files, 392 of them scripts, and `browse` accounts for 338. These are procedures wired to tools: `careful` registers a `PreToolUse` hook that asks before destructive shell commands, and `qa` drives a real browser to reproduce bugs. Some skills serve Tan's own stack, such as `captain` and `runaway-stopper` for the Capy agent platform and `setup-gbrain` for Tan's GBrain knowledge base.

## Tooling

You install by cloning into `~/.claude/skills/gstack` and running `./setup`, which needs Bun 1.3.3 or later and builds the browser and helper binaries. The `--host` flag installs for ten other agents, including Codex, Cursor and Gemini CLI, but `README.md` rates only Claude Code as full support and the others as experimental. New installs have followed the stable channel by default since November 2026, and team mode commits a hook so every teammate's session checks for updates. Telemetry stays off until you opt in, but the update check is on by default, and outside reviews send code to Codex or Claude Code under your login.

CI is the heaviest in this group, with 38 workflows. They run free test suites, end-to-end evals and Windows smoke tests, plus a check that generated skill docs match their templates. Release and version gates sit beside OSV-Scanner and OpenSSF Scorecard security scans. The `test/` folder holds 3,090 tracked files.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [garrytan/gstack](https://github.com/garrytan/gstack) |
| Stars / forks | 142.4k / 21.0k |
| Open issues / PRs | 38 / 4 |
| Commits / contributors | 1,418 / 17 |
| First commit | March 2026 |
| Last 90 days | 1,042 commits |

Commit authorship hides the contributor base. As of November 2026, Garry Tan's account holds 1,370 of 1,418 commits, but squash merges credit about 330 other email addresses in `Co-authored-by` trailers. `CONTRIBUTING.md` reports that in the month to 15 November 2026, 4 of 124 outside PRs merged as written and 54 outside contributors were credited as co-authors. Agents rebuild accepted patches on `main` and close the original PR, so GitHub shows most taken PRs as closed.

The open queue is the smallest relative to stars in this group: 38 issues and 4 PRs against 142.4k stars. The stated goal is a first reply within 48 hours, posted by `capy-ai[bot]`. You get fast triage, but the policy favors changes that matter to most users or to the maintainer's own setup.

## Utility for target workloads

gstack fits founders and tech leads who ship web products from Claude Code and want review, QA and release steps bundled. You can pressure-test a product idea with `/office-hours` and run CEO, design and engineering reviews of a plan with `/autoplan`. `/qa` reproduces and fixes bugs on a staging URL, and `/ship` runs tests and review, then opens the PR. `/careful` and `/freeze` add guardrails even if you use nothing else.

Prerequisites are real: Bun, Git, Claude Code, the bundled Chromium or the Aside browser on macOS, Docker for runtime `/cso` checks, and a signed-in Codex CLI for outside reviews. `README.md` also reports its own test: on 100 GitHub tasks, the agent fixed 58 with gstack and 61 of 99 without. It concludes that you should use gstack for the workflow and safety guards, and not for a higher fix rate.

Compared with [Superpowers](/skills/superpowers) or [Addy Osmani](/skills/addyosmani), gstack trades portability for automation: its skills call real tools, where theirs are mostly Markdown procedures. [Matt Pocock](/skills/mattpocock) covers a similar path from spec to PR with small skills you call one at a time. [Planning with Files](/skills/planning-with-files) and [Ponytail](/skills/ponytail) each do one job and install far less. Install gstack for the sprint and its guardrails, and pick lighter sources if you need full support outside Claude Code.
