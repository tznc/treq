---
description: "Prisma's official agent skills for Prisma ORM setup, CLI and Client API reference, version upgrades, Prisma Postgres, and Prisma Compute deploys."
---

Prisma maintains `prisma/skills`, eleven skills for agents working with Prisma ORM, Prisma Postgres, and Prisma Compute. The repo is now partly a router: Prisma 8 ships version-matched skills inside its npm packages, and the setup skills here tell the agent to sync and read those. The rest cover Prisma 7 commands, the Client API, upgrades, and deployment. Install it if you run Prisma, especially during a major-version upgrade.

## History

Aman Varshney's first commit in January 2026 brought four Prisma 7 skills: `prisma-cli`, `prisma-client-api`, `prisma-database-setup`, and `prisma-upgrade-v7`. A `prisma-driver-adapter-implementation` skill followed in February. At the end of March, Kristof Siket added `prisma-postgres-setup` after what its commits call two rounds of dogfooding.

June 2026 added `prisma-compute`, which Aman Varshney revised seven times through mid-July as the Compute CLI changed. September 2026 brought the main restructure. Sampo Lahtinen consolidated the setup skills around Prisma 8, folded `prisma-database-setup` into `prisma-orm-setup`, and replaced references to "Prisma Next" with Prisma 8. The old names survive as deprecation stubs.

The newest skill, from November 2026, is `prisma-postgres-upgrade`, which moves PostgreSQL apps from Prisma 6 or 7 to Prisma 8 in one pass. The repo has no tags, releases, or changelog. Cadence is steady and modest, with 20 commits in the last 90 days.

## Depth

Eleven folders hold a `SKILL.md`, but two of them, `prisma-database-setup` and `prisma-postgres`, are 14-line redirects with no recipes. The other nine run from 47 to 270 lines, and all eleven total 1,557 lines, backed by 65 reference files. `prisma-cli` has the most support, with 20 references. One of them, `agent-safety.md`, explains Prisma's consent checkpoint, which blocks `migrate reset` and `db push --force-reset` in agent environments until the user's exact consent text is passed in.

The skills fall into two kinds. `prisma-cli`, `prisma-client-api`, and `prisma-upgrade-v7` are Prisma 7 reference material, organized as rule tables and one page per command or topic. `prisma-orm-setup`, `prisma-postgres-setup`, and `prisma-compute` are procedures: detect the starting version, pin a release, run `prisma skills sync`, then read the package-owned skill. The Prisma 8 paths are verified against release candidates, such as `8.0.0-rc.21` in `prisma-compute`.

`prisma-driver-adapter-implementation` is the most specialized skill, at 270 lines with no references. It gives the adapter interfaces, a transaction protocol with savepoints, error mapping that preserves original codes for `P2039`, and a 13-item verification checklist. Few agents need it, but adapter authors will.

## Tooling

Install with `npx skills add prisma/skills`, all at once or one skill at a time with `--skill`. The repo has no plugin manifests for Claude Code, Cursor, Codex, or Gemini, and no MCP configuration, although `prisma-cli` documents the `prisma mcp` command. The `README.md` lists a separate `--skill` command for each of the nine active skills.

Two GitHub workflows run. `validate-skills.yml` parses frontmatter with `gray-matter` and checks that `name` and `description` exist and that `references` is a directory. `pr-code-security.yml` calls Prisma's shared secret detection and code scanning workflows. There are no evals or tests, and no skill ships a script.

The heavier tooling lives in Prisma's own CLI. According to `prisma-compute`, `prisma skills sync` copies version-matched skills from installed packages into `.claude/skills`, `.cursor/skills`, `.agents/skills`, and `.devin/skills`. That design keeps Prisma 8 guidance pinned to the installed version, and it leaves this repo to handle routing and older versions.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [prisma/skills](https://github.com/prisma/skills) |
| Stars / forks | 80 / 8 |
| Open issues / PRs | 1 / 4 |
| Commits / contributors | 47 / 6 |
| First commit | January 2026 |
| Last 90 days | 20 commits |

As of November 2026 the repo has 80 stars and 8 forks, with one open issue and four open pull requests. Six people wrote the 46 human commits, and no one dominates: Aman Varshney has 16, Sampo Lahtinen 10, Kristof Siket 9, and Gregory Boch 8. The remaining commit, which added `prisma-mongodb-upgrade`, came from an account named `clanky.exe`.

Pull request numbers stop at #38, and some merged branches carry follow-up commits that address review feedback. Attention is low for an official vendor repo, but the authorship is more even than in [Supabase](/skills/supabase), where one person wrote 84 percent of human commits. Expect a small team that updates the skills as Prisma's CLI changes, with little visible outside traffic.

## Utility for target workloads

This is for teams on Prisma ORM, and more so for teams moving between majors. Concrete jobs include upgrading a Prisma 6 app to 7 with driver adapters and `prisma.config.ts`, and deciding whether a Prisma 6 MongoDB app stays put or moves to Prisma 8. Other jobs are provisioning Prisma Postgres through `create-db` or the Vercel Marketplace, deploying an app with `prisma deploy`, and writing a custom SQL driver adapter.

Prerequisites depend on the skill. `create-db` gives a temporary database with no account, which is deleted after 24 hours unless claimed with a free Prisma account. Prisma Compute needs `prisma auth login`, or a service token and workspace ID in CI.

The split between this repo and package-owned skills means an agent needs both to get Prisma 8 right. The `README.md` says not to use the Prisma 7 skills' recipes for Prisma 8 setup. Database hosts overlap here: [Neon](/skills/neon) covers pooled and direct connections for migrations on Neon, and Prisma's setup skill keeps whatever database you already have.

Verdict: install `prisma-orm-setup` and the upgrade skill that matches your migration. Add `prisma-postgres-setup` and `prisma-compute` only if you use Prisma's hosted services. Skip the two deprecated stubs, which only redirect.
