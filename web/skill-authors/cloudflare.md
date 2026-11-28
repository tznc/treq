---
description: "Cloudflare's official agent skills for Workers, Durable Objects, the Agents SDK, Wrangler and Cloudflare One, bundled with its remote MCP server."
---

Cloudflare maintains `cloudflare/skills`, its official plugin of 22 agent skills for building on Workers and the wider developer platform. Most skills are short routers that send the agent to current pages on developers.cloudflare.com, and the plugin wires in Cloudflare's remote MCP server. Install it if you build on Workers, Durable Objects, the Agents SDK or Cloudflare One and want agents to check the docs before writing config. Code auditing lives in a separate repo, [Cloudflare Security](/skills/cloudflare-security), with a different purpose and maintainer.

## History

The account `katereznykova` made the first commit in December 2025, with two skills for building AI agents and MCP servers. Matt Silverlock added more skills in January 2026, plus the Claude Code plugin layout and a Cursor manifest. Spring 2026 added Sandbox SDK, Workers and email skills. Ade Oshineye landed two correction passes in March and April, one titled "Fix 18 factual errors" covering pricing, CPU limits and CLI syntax.

June 2026 added Cloudflare One skills, contributed as 21 skills and consolidated into two the same day, and the `turnstile-spin` wizard. The largest phase came in September 2026, when Brendan Irvine-Broque wrote most of that month's 178 commits. Those commits replaced copied product documentation with links to current pages. Product folders under `skills/cloudflare/references` fell from 63 in late August to 25 today.

October and November 2026 added the `cf-cli`, `workers-profiling` and `cloudflare-app-bootstrap` skills, a feedback reference in every skill and an Antigravity plugin. The repo has no tags, and the plugin manifests stand at version 1.0.4. Cadence is high, with 271 of 348 commits in the last 90 days.

## Depth

The 22 skills total 2,427 lines of `SKILL.md`, with a median of 94 and a maximum of 340 in `turnstile-spin`. Fourteen ship only a `SKILL.md` plus a shared `skill-feedback.md`, so their substance is the links they route to. `k2` is typical: a 29-line page that tells the agent to fetch current K2 docs and maps eight tasks to eight URLs. `wrangler` follows the same pattern, cut from 887 lines in January to 75, with rules such as treating `wrangler secret put` as a deployment.

The `cloudflare` skill is the umbrella and the heaviest folder, with 130 files. It opens with a choice between the `cf` CLI and Wrangler, then a table that maps needs to products, such as uploads to R2. Its 25 bundled product references share one layout of overview, API, configuration, patterns and gotchas files. `workers-best-practices` adds an anti-pattern table covering floating promises, module-level request state and `Math.random()` for tokens.

`turnstile-spin` is the outlier, a scripted wizard that creates a Turnstile widget, wires Siteverify into the backend and validates the result. Four shell scripts hold the API calls and retries, a Python test covers one of them, and six references cover common frontend frameworks. The thin design elsewhere is deliberate: `CONTRIBUTING.md` tells authors to link docs and not copy limits or pricing.

## Tooling

The repo ships plugin manifests for Claude Code, Codex, Cursor and Antigravity, plus a Codex marketplace file under `.agents/plugins`. The `README.md` documents installs for Codex, Claude Code, Cursor, VS Code Copilot and `npx skills add`. Cursor users also get a `rules/workers.mdc` rule.

Every manifest wires one MCP server, `mcp.cloudflare.com/mcp`, which the `README.md` describes as API and docs access through Code Mode. Each skill's `skill-feedback.md` tells the agent to draft a GitHub issue and strip secrets and account IDs. The agent files it with `gh issue create` only after the user approves the exact text.

CI is light. A Semgrep workflow scans pushes, pull requests and a monthly schedule. A second workflow checks code blocks in the umbrella references, in advisory mode that never fails the job. The repo has no evals and no frontmatter linting.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [cloudflare/skills](https://github.com/cloudflare/skills) |
| Stars / forks | 3.4k / 361 |
| Open issues / PRs | 36 / 8 |
| Commits / contributors | 348 / 46 |
| First commit | December 2025 |
| Last 90 days | 271 commits |

As of November 2026 the repo has 3.4k stars and 361 forks. The log names 46 distinct people once duplicate identities are merged, and Brendan Irvine-Broque wrote 212 of the 348 commits, or 61%. Kylie Czajkowski, Jordan Coeyman, Matt Silverlock and Matt Taylor follow with 10 to 24 commits each.

Outside contributions do land. The log has 108 merge commits plus squash-merged fixes from personal accounts, covering typos, dead links and a D1 migration example. Pull request numbers reach #287, and the #28233 in one subject points at the `cloudflare-docs` repo.

Eight open pull requests against 36 open issues show prompt review. Agent-drafted feedback is filed here under a `skill-feedback:` prefix. The ratio of stars to contributors is ordinary for a vendor repo.

## Utility for target workloads

This plugin fits teams that build on Workers. Typical tasks include choosing between D1, KV, R2 and Durable Objects for a feature, and setting up Workers Previews on the right Wrangler version. It also covers wiring Turnstile into an existing signup form, reviewing a Worker for leaked request state, and planning a Zscaler or VPN migration to Cloudflare One.

The routing design is the main trade-off. Skills stay small and current, but an agent without network access gets little from `k2` or `wrangler` beyond a list of URLs. Anything that deploys needs a Cloudflare account, and `turnstile-spin` needs an API token with `Account.Turnstile:Edit`.

[Microsoft](/skills/microsoft) bundles large procedural skills, while Cloudflare fetches detail from its docs on demand. [Cloudflare Security](/skills/cloudflare-security) audits any codebase and needs no Cloudflare account. Verdict: install this plugin if Cloudflare is your platform, and skip it otherwise, because nearly every skill assumes Cloudflare products.
