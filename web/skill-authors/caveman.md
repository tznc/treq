---
description: "Julius Brussee's Caveman: terse-reply skills for coding agents plus a local LLM proxy, CLI and gateway that cut and measure token spend."
---

Julius Brussee maintains `caveman`, a monorepo that started as one skill to make coding agents answer in fewer words. It now ships 22 skills alongside a Go proxy, a token-compression engine, SDKs, a browser extension and a CLI installer. The `README.md` says the installer works with more than 30 agents. Install the `caveman` skill alone if you want shorter replies, and evaluate the proxy and installer as separate infrastructure before you let them touch your agent setup.

## History

Julius Brussee published the first `caveman` skill for Claude Code in April 2026 and tagged v1.0.0 the same day. In the first week outside contributors added Codex support, intensity levels, a rule to drop the style for security warnings, and the `caveman-compress` skill. Activity stayed modest from May through July 2026, at 28 to 50 commits a month.

Version 2.0.0 in August 2026 added the engine, the proxy, SDKs and a unified CLI, and commits jumped to 252 that month. Version 3.0.0 in September relicensed the whole repo under Apache-2.0, replacing a split of MIT for the skill surfaces and Business Source License 1.1 for the runtime. Version 4.0.0 in October bundled six components the `README.md` calls rocks, and October alone produced 1,035 commits.

As of November 2026, the repo has 90 tags, ending at `cli-v4.2.0`, and the `README.md` now leads with a Mac app called Caveman Code. Only 145 of the 1,784 non-merge commits touch `skills/`, so most of the history is product work. `packages/`, `proxy/` and `engine/` hold 1,182 of the repo's 1,857 tracked files, against 63 under `skills/`.

## Depth

The 22 skills hold 1,623 lines of `SKILL.md`, with a median of 63 lines and a maximum of 222. Seven of them run 18 lines or fewer. They fall into four groups: three response styles, eight workflow helpers, six one-page coding disciplines and five skills for the hosted Caveman Cloud service.

`caveman` is the core at 89 lines. It sets rules such as answer first, keep code and errors verbatim, never drop a negation, and cap sentences at 20 words. It also lists when to switch back to full sentences, such as security warnings and irreversible actions. `ultracave` and `megacave` push the same rules further, the second into Classical Chinese.

`investigate-first` is typical of the discipline skills, five bullet points between two short lines. `caveman-compress` is the only skill that ships runtime code, seven Python files that send a memory file to your model provider and refuse secret-looking files. `caveman-setup`, the largest, walks an agent through routing an app's LLM calls through the Caveman Cloud gateway in record mode.

## Tooling

The main install is `npx @caveman-ai/cli@latest`, which needs Node.js 22.13 or later, and `INSTALL.md` also offers a tag-pinned `install.sh`. According to `INSTALL.md`, the installer detects each supported agent, runs that agent's native install path, and wires hooks for Claude Code, Codex, Cursor and Copilot. It accepts `--dry-run`, `--minimal` and `--no-hooks`. `npx skills add JuliusBrussee/caveman -g` installs the skills alone.

The repo carries manifests for Claude Code, Codex, Cursor and Gemini CLI. The proxy runs on `127.0.0.1:8787`, forwards original bytes in record mode, and logs spend to `~/.caveman/caveman.db`. A browser extension adds the caveman instruction to messages on ChatGPT, Claude and Gemini.

CI is extensive, with 17 workflows. They cover installer and hook tests, agent conformance, engine and middleware suites, CodeQL, OpenSSF Scorecard and supply-chain checks. A sync job keeps the copies of `SKILL.md` identical. `evals/` compares each response skill against a plain "Answer concisely." prompt, and `docs/HONEST-NUMBERS.md` publishes the results, losses included.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [JuliusBrussee/caveman](https://github.com/JuliusBrussee/caveman) |
| Stars / forks | 122.1k / 7.1k |
| Open issues / PRs | 70 / 42 |
| Commits / contributors | 2,055 / 120 |
| First commit | April 2026 |
| Last 90 days | 1,657 commits |

This is a one-maintainer project. Julius Brussee wrote 1,504 of the 2,011 non-bot commits, and another 196 came from a Claude agent identity between September and November 2026. No other human contributor has more than 21 commits.

Outside work does land. The log has 85 PR merge commits from 27 fork owners and 70 squash merges, with PR numbers past #1500. The open queue holds 42 PRs and 70 issues.

The 122.1k stars measure attention to the idea. One person and one agent identity wrote 1,700 of the 2,011 non-bot commits, and the skill content totals 1,623 lines. That ratio is lopsided, so weigh the code and the published numbers over the star count.

## Utility for target workloads

This repo suits developers who pay per token for Claude Code, Codex or similar agents and want shorter replies and smaller tool output. Concrete uses include terse replies with `/caveman`, shrinking a `CLAUDE.md` with `/caveman-compress`, terse commit messages with `caveman-commit` and one-line review findings with `caveman-review`. Teams can also route an app's LLM traffic through the proxy to measure spend per request.

Read the repo's own numbers before you expect savings. Against a plain "Answer concisely." prompt, `/caveman` cut output by 3% at the median and `/ultracave` by 35%, in a 10-question run. The `README.md` adds that the rules cost about 1,160 input tokens and save nothing on per-request pricing.

The five Caveman Cloud skills need an account on the hosted service, and `caveman-compress` sends file text to your model provider. The verdict: install the skills with `npx skills add` and try `/ultracave` if terse output matters to you. Run the full installer with `--dry-run` first, because it changes the configuration of every agent it detects.
