---
description: "Anthropic's official Claude Code plugin marketplace, with skills for plugin authoring, MCP servers, manufacturing workflows and Claude Code rollout."
---

`anthropics/claude-plugins-official` is Anthropic's repository for its own Claude Code plugins and the `claude-plugins-official` marketplace. Skills are a minority of what it holds: 57 `SKILL.md` folders spread across 21 plugins, next to LSP configs, hooks, slash commands and agents. The marketplace file also lists 262 third-party plugins hosted elsewhere. Install from it if you use Claude Code and build plugins or MCP servers, and skip it if your agent is something else.

## History

Noah Zweben created the repo in November 2025 with a scaffold of 13 Anthropic plugins, including `plugin-dev`, `security-guidance`, `hookify` and `frontend-design`. The same scaffold listed 17 partner plugins such as Figma, Linear and Supabase under `external_plugins/`. December 2025 brought LSP plugins with inline server configs, and January 2026 added a workflow that closes pull requests from anyone without write access.

The marketplace grew from 51 entries in January 2026 to 254 by June 2026 and 317 in November 2026. In April and May 2026 the team added nightly automation that bumps the pinned commit of each external plugin, plus a Claude-driven policy scan of every bump. That automation explains the commit count. Of 4,878 commits, 3,446 are `github-actions` bumps, and June 2026 alone saw 1,186 of them.

In November 2026 the readme changed direction. Third-party submissions now go through claude.ai/directory/manage, and the readme says `marketplace.json` will shrink to Anthropic's own plugins. The same month added the two largest skill bundles, `claude-for-manufacturing` with 17 skills and `champion-kit` with seven.

## Depth

The 57 skills have a median `SKILL.md` of 234 lines, and they cluster in a few plugins. `claude-for-manufacturing` holds 17 plant workflows such as `ncr-writeup`, `rfq-to-quote` and `root-cause-capa`. `ncr-writeup` is a strict procedure: it cites the violated requirement by document and locator, scopes suspect parts across five locations and never invents a requirement it cannot find. Data contracts and a `plant_setup.py` script sit in a `shared/` folder, and the plugin readme calls the bundle an unmaintained reference implementation.

`plugin-dev` carries seven reference-heavy skills for building Claude Code extensions. `command-development` is the largest skill in the repo at 884 lines plus a `references/` folder and examples, and `hook-development` follows at 712 lines. `mcp-server-dev` adds three skills, led by `build-mcp-server`, which interviews you about users, actions and auth before it picks a remote HTTP server, an MCP bundle or local stdio.

At the thin end, `champion-kit` ships seven prompts of 40 to 83 lines that walk a Claude Code champion through an FAQ, office hours and a hackathon. The Discord, Telegram and iMessage channel plugins each carry `access` and `configure` skills that edit JSON allowlists and refuse requests that arrive through the channel itself. Two skills, `frontend-design` and `skill-creator`, are copies of the ones in [Anthropic](/skills/anthropic).

## Tooling

Plugins install through Claude Code with `/plugin install <name>@claude-plugins-official`. Each plugin carries a `.claude-plugin/plugin.json`, and many add `.mcp.json`, hooks, agents or `lspServers` configs. There are no Codex, Cursor or Gemini manifests, so the repo targets Claude Code, plus other Claude products that the readme says serve plugins from the directory.

Seven workflows run in CI. `validate-plugins` checks every plugin folder, the frontmatter of skills, agents and commands, and the marketplace file. `validate-licenses` requires an Apache 2.0 license in each plugin, and `check-mcp-urls` probes declared MCP endpoints daily. `scan-plugins` has Claude review changed external entries against Anthropic's directory and acceptable use policies, and `revert-failed-bumps` drops entries that fail.

Tests exist in a few plugins. `security-guidance` has 82 test files and `code-modernization` has its own suite. On the skill side, only `math-olympiad` ships an eval file, a trigger eval in JSON.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [anthropics/claude-plugins-official](https://github.com/anthropics/claude-plugins-official) |
| Stars / forks | 48.0k / 5.0k |
| Open issues / PRs | 1.1k / 222 |
| Commits / contributors | 4,878 / 46 |
| First commit | November 2025 |
| Last 90 days | 1,454 commits |

As of November 2026 the raw numbers overstate human activity on skills. Bots wrote 3,449 commits, and 89 more carry the author name Claude. The remaining 1,340 come from 46 people, and 1,027 of those commits use anthropic.com addresses. The top authors are jordanecker-ant with 370 commits, Mohamed Hegazy with 293 and Bryan Thompson with 242.

Recent effort sits outside the skills. Of 807 non-bot commits in the last 90 days, 298 touched `security-guidance`, a hook-based plugin, and 11 touched any skills folder. Outside contributions are now closed by design, since the readme says pull requests go unmonitored and a workflow closes them. Meanwhile 1.1k issues sit open, and the readme names no process for them.

## Utility for target workloads

This repo is for Claude Code users and for teams that build Claude Code extensions. The skills help with writing a plugin's commands and hooks through `plugin-dev`, and with choosing a transport and auth model for a new MCP server. A quality engineer can draft a nonconformance report or a supplier quote with `claude-for-manufacturing`. A team lead can prepare a Claude Code rollout FAQ with `champion-kit`, and a developer can pair a Telegram bot with the channel plugins.

Everything needs Claude Code. `claude-for-manufacturing` reads from Microsoft 365 and Oracle NetSuite or Infor M3, and its `design-space-explore` skill needs a live Autodesk Fusion connection. The LSP plugins expect the matching language server on your machine.

Compared with [Anthropic](/skills/anthropic), which holds standalone skills that also run in Claude.ai and the API, this repo is Claude Code plumbing first. Much of what it offers has no skill at all, such as the `security-guidance` hooks, the `code-review` commands and 12 LSP configs. Verdict: add the marketplace if you live in Claude Code, install `plugin-dev` or `mcp-server-dev` when you build extensions, and look elsewhere for general-purpose skills.
