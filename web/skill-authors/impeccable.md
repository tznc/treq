---
description: "Paul Bakaus's Impeccable: one frontend design skill with 24 commands, a 62-rule AI slop detector, a CLI and an edit hook for coding agents."
---

Paul Bakaus maintains Impeccable, a single Agent Skill that gives a coding agent 24 named design commands for building, critiquing and refining frontend UI. The same repository holds the Rust engine those commands call, an npm CLI, a Chrome extension and a deterministic detector for the tells of generated UI. Install it if you ship web UI with an agent and want a reviewer, a lint pass and a CI gate in one package. Skip it if you want plain markdown with no binary, hook or network calls.

## History

The first commit landed in November 2025 under the name `vibe-design-plugins`, and the repository became `impeccable` in January 2026. The `README.md` says it started from Anthropic's `frontend-design` skill. Early versions shipped separate skills and the project website from one tree.

On April 10, 2026 one commit folded 18 skills into a single `/impeccable` skill with 20 commands, released as version 3.0. In July 2026 the website and service moved to a private repository, and `skill-v4.0.0` was tagged. In September 2026 a Rust engine replaced the earlier scripts and the JavaScript detector. The 99 tags now span separate skill, CLI, engine and extension release lines, and `skill-v4.10.0` shipped on November 18, 2026.

The 3,795 commits overstate the skill work. Bots made 406 of them, 647 are merges, and only about 1,035 of the 2,742 human non-merge commits touch the skill text. The rest go to the engine, CLI, tests, extension and, until July 2026, the website, and November 2026 alone has 1,100 commits.

## Depth

There is one skill, `impeccable`, and its `SKILL.md` is 89 lines that route to 50 reference files of about 5,600 lines. Each command gets its own playbook: `critique-rubric.md` runs to 496 lines, `harden.md` to 362 and `bolder.md` to 33. The skill first runs `impeccable context` to load `PRODUCT.md` and `DESIGN.md`. It then classifies the surface as Persuade, Operate, Read or Experience and reads `craft-floor.md` before any edit.

The content is opinionated procedure. `bolder.md` opens with "Scope is sovereign" and forbids touching anything outside the named target. `craft-floor.md` bans an eyebrow label above a heading, gradient text and a colored bar down one side of a rounded box.

Five subagent definitions sit under `plugin/agents`, including a finish reviewer and a motion reviewer. The `scripts` folder holds a launcher for the engine binary and a 13,921-line live-mode script. The markdown is a procedure wrapped around a product CLI.

## Tooling

`npx impeccable install` detects your tools, installs the skill and offers the design hook. The plugin hook runs `impeccable hook` on session start and after every `Edit` or `Write`, so findings return before the agent reports done. Plugin manifests exist for Claude Code, Cursor and Grok, Codex gets `.codex/hooks.json`, and a VS Code extension covers Copilot. Generated copies for 17 host folders sit at the repository root.

The detector has 62 rules in `crates/live/assets/antipatterns.json`. `npx impeccable detect` prints JSON, GitHub annotations or a `--sarif` log, and scans a rendered URL for contrast and text size. The engine is a 20-crate Rust workspace that includes an MCP server crate.

`ci.yml` runs the Bun and Node suites, `cargo test --workspace`, an extension build with `web-ext lint`, and a check that generated host output is current. An opt-in suite in `tests/skill-behavior` calls real models to confirm that the setup steps drive the agent. It needs provider keys, so CI does not run it.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [pbakaus/impeccable](https://github.com/pbakaus/impeccable) |
| Stars / forks | 114.9k / 6.8k |
| Open issues / PRs | 49 / 26 |
| Commits / contributors | 3,795 / 69 |
| First commit | November 2025 |
| Last 90 days | 2,138 commits |

As of November 2026, the repository has 114.9k stars against 69 human contributors. Paul Bakaus wrote 2,881 commits, 85% of the human total. Abdul Wahab is the only other steady committer, with 382 commits on the engine, tests and extension since May 2026. Of the 312 pull requests merged with a merge commit, 277 came from Paul Bakaus's branches and 35 from 24 other accounts.

Pull request numbers pass #1,570, so most traffic is the maintainer's own branch flow. The `README.md` routes outside contributions through an issue first, and `sheriff.yml` closes contributor-blocked pull requests after 14 days. Read it as a fast-moving single-maintainer project with one regular collaborator. The 49 open issues are few for the star count, which measures attention more than use.

## Utility for target workloads

Impeccable fits frontend engineers who already ship web UI through an agent. Run `/impeccable critique` on one component for a ranked UX review, or `audit` for P0 to P3 accessibility and performance findings. Gate pull requests with `npx impeccable detect --sarif`, finish a page with `polish` or `harden`, or cycle element variants with `live` in a Vite or Next dev server. A `Platform` entry in `PRODUCT.md` switches the commands to iOS or Android guidance.

The cost is footprint. The skill downloads an engine binary, and the `README.md` warns that installed hooks run even when you deny the model's own commands. New designs fetch visual directions from the `impeccable.style` catalog whether or not telemetry is on. Live mode's Apply can call the `codex` or `claude` CLI with approvals off unless you set `IMPECCABLE_LIVE_COPY_AGENT`.

[Hallmark](/skills/hallmark) and [Taste Skill](/skills/taste-skill) fight the same AI slop but center on new pages built from dealt themes or palettes. Impeccable's `README.md` says it is strongest on interfaces that already exist, and [Emil Kowalski](/skills/emilkowalski) covers motion in more depth with plain markdown. Pick Impeccable when you want a detector and a CI gate beside the prompts, and audit its hooks before unattended runs.
