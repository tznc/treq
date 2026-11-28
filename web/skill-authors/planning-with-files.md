---
description: "OthmanAdi's Planning with Files: one hook-driven skill that keeps a coding agent's plan, findings and progress in Markdown files that survive /clear."
---

Planning with Files is a single skill maintained by Ahmad Othman Ammar Adi, who publishes as OthmanAdi. It makes the agent keep three Markdown files, `task_plan.md`, `findings.md` and `progress.md`, and uses lifecycle hooks to bring the plan back into context. The `SKILL.md` opens with "Work like Manus" and treats the context window as RAM and the filesystem as disk. Install it if your agent loses track during long, multi-step tasks or after `/clear`, whatever workflow skills you already run.

## History

The first commit in January 2026 was a plain Claude Code skill. Within a week a contributor converted it to a Claude Code plugin, and v2.0.0 added hooks and templates the same day. The v2 line ran to v2.43.0 in May 2026, mostly host ports, security hardening and fixes. In March 2026, for example, v2.21.0 removed web tools from `allowed-tools` after an audit found the plan re-injection could amplify prompt injection.

Version 3.0.0 in June 2026 added opt-in autonomous and gated modes, a per-agent run ledger and plan attestation for unattended runs. The repo has 126 tags, the most in this group, and v3.29.0 shipped in November 2026. September 2026 was the busiest month with 134 commits, and the last 90 days hold 284.

## Depth

The catalog lists one skill. Translations into Arabic, German, Spanish and two Chinese variants sit in an `i18n` folder that the catalog excludes. The `SKILL.md` runs 534 lines, and its frontmatter registers hooks for prompt submit, tool use, stop and pre-compact events. Behind it sit 33 supporting files: 22 scripts in shell, PowerShell and Python, 7 templates, a reference, examples and an eval definition.

This is a procedure backed by working scripts. `init-session.sh` creates the files, `check-complete.sh` validates the phase status markers, and `session-catchup.py` reads past session records only when you ask. Gated mode can block the agent from stopping while a phase is in progress, with a cap on blocks. The `CHANGELOG.md` documents the security model in detail, including nonce-framed plan injection and a refusal to inject plans that were edited after attestation.

## Tooling

Host support is wide. The repo carries configuration folders for Claude Code, Codex, Cursor, Gemini CLI, OpenCode, Pi, Hermes, Kiro, Factory, CodeBuddy, Continue and Qoder, among others. `README.md` claims the skill works across more than 60 agents through the Agent Skills standard. Hook strength varies by host, and `MIGRATION.md` sorts hosts into those that can hard-block a stop, those that inject a follow-up and those that only notify.

CI runs pytest on several operating systems plus vitest suites for the Pi, OpenCode and DSH plugins, across 108 test files. Two more workflows run Tessl's skill review action. `docs/evals.md` reports evals built on Anthropic's skill-creator: a 96.7% assertion pass rate with the skill and 3 of 3 wins in blind comparisons. The same document reports about 68% more tokens and 17% more time per task, which the author calls the intended trade.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [OthmanAdi/planning-with-files](https://github.com/OthmanAdi/planning-with-files) |
| Stars / forks | 28.2k / 2.3k |
| Open issues / PRs | 13 / 4 |
| Commits / contributors | 648 / 73 |
| First commit | January 2026 |
| Last 90 days | 284 commits |

As of November 2026, OthmanAdi has written 478 of 648 commits, about 74%, under several names and emails. The other 72 contributors mostly add host ports, tests and fixes. The history has only 17 merge commits, so most outside work lands as squashed or rebased commits that keep the original author.

The queue is small: 13 open issues and 4 open PRs against 28.2k stars, the lowest star count in this group. Issue numbers in commit subjects reach #361, which is modest traffic next to the other sources here. The maintainer ships often, so pin a version if you depend on exact hook behavior.

## Utility for target workloads

This skill fits anyone who runs long agent sessions. You can resume a multi-hour refactor after `/clear` from `task_plan.md`, and log findings in `findings.md` during a research task with dozens of tool calls. Parallel workers in separate worktrees can append to their own ledgers, and `check-complete.sh` validates a plan before substantive work starts. The author's own recovery benchmark reports that a session resumed after a context wipe in 5.0 turns on average, against 13.3 for an agent without the skill.

It targets a problem the other sources in this group handle only in part. [Superpowers](/skills/superpowers) keeps a similar progress ledger, but only inside `subagent-driven-development`. [Matt Pocock](/skills/mattpocock), [Addy Osmani](/skills/addyosmani) and [gstack](/skills/gstack) plan through specs, tickets or design docs, and this skill can hold the running state for any of them. [Ponytail](/skills/ponytail) shapes how code is written and does not track plans.

You need Python for the catchup script and a Unix shell or PowerShell for the rest. Per-tool-call injection costs tokens, which is why v3 added an autonomous mode that drops it. Install this skill as a small, focused addition, and enable gated mode only on hosts that can block a stop.
