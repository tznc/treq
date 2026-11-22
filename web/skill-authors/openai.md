---
description: "OpenAI's deprecated Codex skills catalog: 44 skills for GitHub, Figma, Notion, deploys, security reviews and image generation, frozen since June 2026."
---

`openai/skills` is OpenAI's catalog of Agent Skills for Codex, its coding agent. It holds 44 skills in two hidden tiers: five `.system` skills that ship inside Codex and 39 `.curated` skills you install by name. In June 2026 OpenAI marked the repo deprecated and pointed readers to its plugins repository, and nothing has changed since. Read it for patterns or copy a skill you need, but do not expect updates or reviews.

## History

Gav Verma created the repo in November 2025, and the first skills landed in December 2025. That month brought `skill-creator`, `skill-installer`, a `plan` skill, Notion and Linear skills and two GitHub skills, sorted into `.system`, `.curated` and `.experimental` folders. Figma skills arrived and left within a day, then returned at the end of January 2026.

January and February 2026 were the busiest stretch, with 44 of the 114 commits. A bulk sync added `imagegen`, `pdf`, `screenshot`, `speech` and `yeet`, then deploy skills for Cloudflare, Netlify, Render and Vercel and three security skills followed. The experimental tier was emptied in February 2026. Each skill gained an `agents/openai.yaml` file with a display name, icons and a default prompt for the Codex interface.

From March to May 2026 the catalog churned. New skills such as `chatgpt-apps`, `playwright-interactive`, `migrate-to-codex` and `hatch-pet` arrived, while `sora`, `slides`, `spreadsheet` and `doc` were removed. In all, 14 skill names were added and later deleted. The last commits came in June 2026: a docs update, the deprecation notice in the readme and one installer fix.

## Depth

The 44 skills have a median `SKILL.md` of 144 lines, with 736 supporting files behind them. Those files are uneven. `cloudflare-deploy` alone carries 311 of them, a reference tree with decision trees that route the agent to Workers, Pages, D1 or Durable Objects docs. Most other skills have a handful of references, an icon and a license.

`gh-address-comments` is the smallest at 25 lines. It tells the agent to run a bundled `fetch_comments.py`, number the review threads and ask you which to fix. `security-threat-model` is a typical procedure at 81 lines: eight steps from system model to mitigations, a pause to confirm assumptions with you, and an output contract in `references/prompt-template.md`. `playwright-interactive` is the largest at 693 lines, a QA checklist for driving a browser or Electron app through a persistent `js_repl` session.

Partner skills lean on MCP servers. The eight Figma skills and four Notion skills wrap those vendors' MCP tools, and `figma-use` describes itself as a mandatory step before any `use_figma` call. Compare `pdf` with the [Anthropic](/skills/anthropic) version: OpenAI's is 67 lines of library advice with no scripts, while Anthropic's ships eight.

## Tooling

Codex is the only host the repo targets. The readme says Codex installs the `.system` skills itself, and you add curated ones with the `$skill-installer` skill, which runs `install-skill-from-github.py` against a name or a GitHub URL. Every skill carries `agents/openai.yaml`, and 17 of those files declare MCP dependencies, such as the Figma server at `mcp.figma.com`. There is no Claude Code, Cursor or Gemini manifest.

The `.system/skill-creator` skill ships `init_skill.py`, `quick_validate.py` and `generate_openai_yaml.py` for authors. The repo itself has no CI, no tests and no validation on pull requests. The four Notion skills each include an `evaluations/` folder, and no other skill does. A contributor license agreement was added in January 2026 and removed 18 days later.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [openai/skills](https://github.com/openai/skills) |
| Stars / forks | 28.9k / 2.0k |
| Open issues / PRs | 90 / not shown |
| Commits / contributors | 114 / 35 |
| First commit | November 2025 |
| Last 90 days | 0 commits |

As of November 2026 the repo is a team effort that has stopped. Thirty-five people have commits, 26 of them with openai.com addresses, and no author holds more than 15 percent. Dominik Kundel and Gav Verma lead with 17 commits each, and Vaibhav Srivastav follows with 15.

Outside work did land while the repo was active. Simon Willison added a new image model to the `openai-docs` model list in #374, and an outside author moved the Sentry skill to the Sentry CLI in #336. Issue and pull request numbers reach #507 against 114 commits. With 90 issues open and no commits since June 2026, the stars measure past interest in Codex skills.

## Utility for target workloads

This repo is for Codex users. Its skills handle concrete tasks: addressing PR review comments with `gh-address-comments`, fixing failing GitHub Actions checks with `gh-fix-ci`, and deploying a project to Vercel, Netlify, Render or Cloudflare. It also covers writing a threat model for a repository and implementing a Figma frame in code with `figma-implement-design`.

Prerequisites add up. You need the `gh` CLI logged in for the GitHub skills, and the Figma, Notion or Linear MCP servers for their skills. `speech`, `transcribe` and the `imagegen` command-line fallback need an `OPENAI_API_KEY`, and `playwright-interactive` asks you to run Codex with the sandbox disabled.

The skills also speak Codex. They reference `sandbox_permissions=require_escalated`, `js_repl` and `$skill-installer`, so other agents need edits before the instructions make sense. Verdict: install nothing new from here. Copy `security-threat-model` or `gh-fix-ci` into your own setup if they fit, and look to OpenAI's plugins repository for maintained versions.
