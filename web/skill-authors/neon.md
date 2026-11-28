---
description: "Neon's official agent skills for Lakebase Postgres, branching, Neon Auth, Functions, Realtime, object storage and AI Gateway, with MCP plugins."
---

Neon maintains `neondatabase/agent-skills`, seven skills that teach coding agents to use Neon's backend services. The `README.md` describes Neon as backend primitives built around Lakebase Postgres "from Databricks", and the skills reach past the database into Auth, Functions, Realtime, object storage, and an AI Gateway. Andre Landgraf leads the work. Install it if your app runs on Neon or you are evaluating its newer services.

## History

Andre Landgraf made the first commit in January 2026, a proof of concept that ported existing `ai-rules` content and then folded it into "one skill to rule them all". It became `neon-postgres` within a week, and a Claude Code plugin with the Neon MCP server followed that month. February 2026 added Cursor packaging and a claimable Postgres quickstart skill. In March, Mikael Lirbank built evals for a separate egress-optimizer skill.

June 2026 was the busiest month, with 60 commits, as skills arrived for Functions, AI Gateway, and object storage. In July the database was renamed Lakebase Postgres across all public copy. Releases live in `package.json` versions without git tags: 1.1.2 in August, 2.0.0 in September, and 3.0.0 in November 2026, when the branching and egress skills were folded into `neon-postgres`. Activity stays high, with 116 commits in the last 90 days.

## Depth

The catalog counts seven skills under `skills/`, and `plugins/neon-postgres/skills/` holds identical copies for plugins. The `SKILL.md` files run from 166 lines for `neon-auth` to 727 for `neon-functions`, 2,553 lines in all, backed by 28 reference files. `neon` is the parent router, covering the services, skill selection, and setup through the `neon` CLI, the MCP server, and `neon init`.

These are opinionated procedures with heavy product reference. `neon-functions` covers deployment, region limits, WebSocket fan-out across isolates, SSE, and Function Triggers, with eight references. `neon-postgres` pairs a setup flow and a pooled versus direct connection table with Neon-specific diagnostics through `neon inspect db`. Generic query and index work goes to a `postgres-best-practices` skill in `neondatabase/postgres-skills`.

`neon-realtime` has the largest references, five guides with the longest at 745 lines. `neon-ai-gateway`, at 376 lines, and `neon-object-storage` have none. Many sections link to neon.com docs pages for the agent to fetch as markdown, so the skills assume network access.

## Tooling

Install with `npx skills add neondatabase/agent-skills`, the Claude Code marketplace, Cursor, Antigravity, Kimi Code, or Pi through the npm package `@neon/skills`. The repo root is also an Agent Plugins v1 package, with `plugin.json` and `mcp.json` declaring the Neon MCP server. According to the `README.md`, plugin installs prompt for Neon OAuth the first time the agent calls an MCP tool.

One GitHub workflow, `validate.yml`, runs `npm run validate:ci`. It checks the Claude, Cursor, Antigravity, and Agent Plugins manifests, runs `skills-ref validate` on each skill, and confirms plugin copies match `skills/`. A reference-graph script fails the build on broken links or orphaned reference files.

The `evals/` folder holds the repo's one eval, for egress optimization. A script runs Claude Code against a Hono and Drizzle fixture and scores the diff against a rubric. Its `README.md` reports that detection of missing pagination rose from 0 of 89 baseline runs to 57 percent across 42 runs of skill version v003. No other skill has an eval.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [neondatabase/agent-skills](https://github.com/neondatabase/agent-skills) |
| Stars / forks | 118 / 26 |
| Open issues / PRs | 17 / 26 |
| Commits / contributors | 333 / 18 |
| First commit | January 2026 |
| Last 90 days | 116 commits |

As of November 2026 the repo has 118 stars, a small number for its volume of work. The 26 open pull requests and 17 open issues look heavy next to the star count and light next to the throughput of 50 commits in the last 30 days. Here the star count understates the activity.

Andre Landgraf wrote 197 of the 327 human commits, about 60 percent. Tony Dang and Mikael Lirbank add 30 each, Dominik K. and James Arthur 17 and 14, and Philip Olson 10. A name-based count finds 20 authors, but two pairs share an identity, leaving 18 people.

Commit emails include neon.tech and databricks.com addresses next to personal accounts. Outside the top six, 12 people have eight or fewer commits each. Pull requests land both as merge commits and as squashes, and their numbers pass #210.

## Utility for target workloads

This is for teams whose app runs on Neon. Concrete jobs include picking pooled or direct connection strings, running migrations over the right one, and creating a branch per pull request. It also helps with cutting egress from `SELECT *` and missing pagination, deploying an API or MCP server on Neon Functions, and wiring Neon Auth into a Next.js app.

Prerequisites are a Neon account, or Claimable Neon for a database with no signup, plus the `neon` CLI or the Neon MCP server. Neon Functions run only in four AWS regions, and the skill tells the agent to check the region first. Realtime is unavailable on Claimable projects until they are claimed.

The pace cuts both ways: names, CLI commands, and whole skills changed within months, and the `neon` skill documents a `neon skills status` check for stale copies. [Supabase](/skills/supabase) offers the same bundle of Postgres, Auth, storage, and functions, so the two repos are alternatives for a new project. If you use Prisma on Neon, pair this with [Prisma](/skills/prisma) for client and migration commands.

Verdict: install it if you run on Neon. Start with `neon` and `neon-postgres`, and add the service skills only for services you use. Pair `neon-postgres` with a generic Postgres skill, such as Supabase's rule library, for query and index work.
