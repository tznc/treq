---
description: "Obsidian's official agent skills for vaults: Obsidian Markdown, Bases, JSON Canvas, the Obsidian CLI, Headless Sync and Publish, Defuddle and Knap."
---

Obsidian publishes `obsidian-skills`, seven agent skills that teach an agent to read and write the files in an Obsidian vault and to drive Obsidian's command-line tools. Steph Ango wrote most of it. The skills cover Obsidian Flavored Markdown, Bases, JSON Canvas, the Obsidian CLI, Obsidian Headless, and two companion CLIs, Defuddle and Knap. Install it if an agent works inside your vault, and skip it if you do not use Obsidian.

## History

Steph Ango made the first commits in January 2026, adding the Bases and Obsidian Markdown skills on January 2 and JSON Canvas four days later. The `defuddle` and `obsidian-cli` skills followed in February 2026 and `knap` in September. The repo started under the `kepano` GitHub account, and a November 2026 commit moved every install command to the `obsidianmd` organization.

November 2026 was the busiest month, with 37 commits. The plugin was published as Obsidian 1.1.0 with an icon, then 1.2.0 added `obsidian-headless` for Sync and Publish. That skill grew out of a community pull request for the `ob` CLI that had been open since March. As of November 2026, the repo has three tags, 1.1.0, 1.2.0 and 1.2.1.

## Depth

The seven skills hold 1,419 lines of `SKILL.md`, with a median of 141 lines. Five reference files sit beside them, and no skill ships a script. They fall into two groups: format references for the files Obsidian reads, and thin wrappers around a CLI.

`obsidian-bases` is the largest at 601 lines plus a 173-line function reference. It gives the `.base` YAML schema, the filter grammar, formulas, summaries and the table, card, list, map and kanban views, then a validation checklist. `obsidian-markdown` covers wikilinks, embeds, callouts and properties, and `json-canvas` pairs 253 lines with a 329-line examples file.

The wrappers are short and practical. `defuddle` is 41 lines that check the tool exists and run `defuddle parse <url> --md`. `obsidian-cli` records traps worth knowing, such as an exit status of 0 on failure, `\n` conversion that corrupts backslashes in `content=`, and an unrelated npm package with the same name.

## Tooling

The repo ships manifests for several hosts. `.claude-plugin/marketplace.json` serves Claude Code, `.agents/plugins/marketplace.json` serves Codex, `.cursor-plugin/` serves Cursor, and `gemini-extension.json` serves Gemini CLI. The plugin's `plugin.json` follows the Agent Plugins schema with an OpenAI extension block for the ChatGPT listing, and `npx skills add` also works.

The `README.md` says Obsidian is an official plugin in the ChatGPT and Codex directory, the Cursor marketplace and Grok Bot. `scripts/bump.sh` updates the version in every manifest, and `scripts/package-gemini.sh` builds the Gemini archive. The `README.md` also documents manual installs for Claude Code, Codex and OpenCode.

CI is one workflow. `gemini-release.yml` checks that the manifest version matches the release tag and attaches the archive. Nothing validates skill frontmatter, and there are no tests or evals.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [obsidianmd/obsidian-skills](https://github.com/obsidianmd/obsidian-skills) |
| Stars / forks | 53.4k / 3.8k |
| Open issues / PRs | 3 / 1 |
| Commits / contributors | 104 / 27 |
| First commit | January 2026 |
| Last 90 days | 43 commits |

One maintainer decides what lands. Steph Ango wrote 66 of the 104 commits, and the other 26 contributors each have one to seven, mostly documentation fixes and host support. The contributor committing as `kpeez` and `kpuhger` added the Codex marketplace and the version bump script.

The log shows 28 merged pull requests, and merges come in batches. Twelve landed on November 6 and 11, 2026, including PRs whose commits date from March, April and June. The queue is now down to one open PR and three open issues.

The star count is lopsided. 53.4k stars sit on 104 commits and seven small skills. Those stars reflect interest in Obsidian and say little about the depth of this repo.

## Utility for target workloads

This repo suits people who keep notes, research or project docs in an Obsidian vault and let an agent edit it. It helps the agent write notes with valid wikilinks, callouts and properties, and build a `.base` view such as a reading list or a kanban board. It also covers drawing a canvas of linked ideas, clipping a web page to Markdown with Defuddle, and syncing a vault from a server with `ob`.

Prerequisites depend on the skill. `obsidian-cli` needs the desktop app open with its CLI turned on. `obsidian-headless` needs Node.js 22 or later and an active Sync or Publish subscription, and `defuddle` and `knap` need their npm packages.

Because the vendor maintains it, the format rules follow the official help docs, as commits that match kanban and Headless behavior to those docs show. The verdict: install it if you use Obsidian with an agent, since it is small and ships no code of its own. Expect format references and CLI notes, not workflows.
