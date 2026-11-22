---
description: "Matt Van Horn's last30days skill: a Python research engine that searches Reddit, X, YouTube, Hacker News, Polymarket and more from the last 30 days."
---

Matt Van Horn maintains `last30days-skill`, which ships one skill, `last30days`, backed by a large Python research engine. Given a topic, person or company, it searches Reddit, X, YouTube, TikTok, Hacker News, Polymarket, GitHub and the web for the last 30 days. It scores results by engagement and writes a cited brief. Install it if you need fast recency research before a meeting, a launch or a purchase, and skip it if you want a library of many small skills.

## History

Matt Van Horn made the first commit in January 2026. It already held 31 files and 3,773 lines, including a Python engine with unit tests that found Reddit threads through OpenAI and X posts through xAI. Version 3.0.0 in April 2026 added pre-research that resolves handles and subreddits before searching, an engine design the changelog credits to Jeffrey Sperling. The v4 line began in November 2026, and the `README.md` describes it as reading careers pages and US patent filings for signs of unannounced work.

The busiest stretch ran from May to July 2026, with 206, 293 and 227 commits, and Trevin Chow did much of the merging, authoring 132 of the repo's 187 PR merge commits. As of November 2026, the repo has 92 tags ending at v4.6.0 and a `CHANGELOG.md` with an entry for every release. A `changelog-guard.yml` workflow requires a changelog fragment on every pull request unless a label waives it.

## Depth

The catalog counts one skill, so judge the depth by the code behind it. `SKILL.md` is 261 lines and works as a dispatcher. It tells the agent to bind the skill directory, resolve Python 3.12 or later, and run a first-run gate before any research. It then routes to 18 reference files, 2,684 lines in total, covering the setup wizard, synthesis rules, comparisons, discovery mode, hiring signals and an agent JSON mode.

The engine under `scripts/` holds 157 Python modules, about 101,000 lines, plus a vendored JavaScript client for X search. The largest module, `lib/pipeline.py`, runs 8,412 lines, and other modules cover YouTube transcripts, job boards and a `doctor` command that diagnoses broken sources. The `README.md` lists 30 sources, and only some of them work without keys.

This is an opinionated procedure wrapped around a real application. The skill forbids passing off a WebSearch-only report as an engine run. It treats retrieved posts and pages as untrusted evidence, never as instructions.

## Tooling

Install paths cover a Claude Code plugin marketplace, `npx skills add`, a Codex plugin, a Grok plugin, a Gemini CLI extension and ClawHub for OpenClaw. The Claude Code plugin registers a `Stop` hook that archives finished briefs after each turn. `mcp/` holds a Go MCP server that embeds the Python engine and packages it as a `.mcpb` bundle for Claude Desktop.

Testing is the strongest part. The repo has 380 test files, and `validate.yml` runs the full pytest suite with coverage on every pull request. An offline eval suite replays recorded HTTP fixtures and scores citation grounding, recency compliance, cluster coherence and determinism against fixed floors.

Other workflows run `uv audit` and Semgrep, OSV-Scanner, OpenSSF Scorecard and zizmor for Actions security. A daily Reddit canary checks that the keyless Reddit path still works. It runs with no session, credentials or config, so it measures the path every free user gets.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [mvanhorn/last30days-skill](https://github.com/mvanhorn/last30days-skill) |
| Stars / forks | 72.6k / 6.4k |
| Open issues / PRs | 6 / 13 |
| Commits / contributors | 1,587 / 182 |
| First commit | January 2026 |
| Last 90 days | 398 commits |

Not every commit is skill work: 732 of the 1,356 non-merge commits touch `skills/`, and the rest go to tests, docs, the MCP server and CI. Matt Van Horn wrote 571 of the 1,464 human commits, Trevin Chow 350 and Ilia Alshanetsky 111, so three people carry most of the work. The other 179 contributors account for the remaining 432 human commits.

Outside contributions land often. The log has 187 PR merge commits and 670 squash merges, with PR numbers past #1500, and changelog entries thank contributors by handle. With 6 open issues and 13 open PRs, the queue is close to empty.

## Utility for target workloads

This skill suits founders, analysts, salespeople and anyone who needs to know what changed on a topic this month. It helps you brief yourself on a person before a meeting, and compare tools with an "X vs Y" query. You can also scan a company's job postings and patent filings for unannounced work, or find trending topics in AI with discovery mode.

Setup takes real choices. The engine needs Python 3.12 or later and Node.js, and its frontmatter names `SCRAPECREATORS_API_KEY` as the main credential, with optional OpenAI, xAI, OpenRouter, Perplexity and Brave keys. Richer X and Reddit results come from your own logged-in browser sessions, which the skill reads only after you consent, so decide whether that fits your accounts.

When the research turns into positioning work, [Corey Haines](/skills/coreyhaines)'s `competitor-profiling` and `customer-research` skills take over. The verdict: install it if recency research is a weekly task for you. Start with the keyless sources, and add credentials one source at a time.
