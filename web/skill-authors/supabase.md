---
description: "Supabase's official agent skills: one skill for Supabase Auth, RLS, CLI and MCP work, plus a library of Postgres performance and schema rules."
---

Supabase maintains `supabase/agent-skills`, a two-skill repo that teaches coding agents to work on Supabase projects and to write sound Postgres. Pedro Rodrigues of Supabase wrote most of it. The `supabase` skill is a short procedure built around a security checklist, and `supabase-postgres-best-practices` is a library of 31 Postgres rules with incorrect and correct SQL. Install it if you build on Supabase, and consider the Postgres skill for any Postgres codebase.

## History

Pedro Rodrigues made the first commit in January 2026, titled "Initial setup: PostgreSQL best practices repository". January alone produced 44 of the repo's 96 commits. They added the first rule files, a Claude Code marketplace manifest, and a custom TypeScript build package under `packages/skills-build` that validated rules. In April 2026 a "clean repo" commit deleted that build package, and the same day the general `supabase` skill landed from pull request #12, opened in January.

From May 2026 the repo moved to Release Please with a GitHub App, publishing tarballs and an `index.json` for `.well-known/agent-skills` discovery on each release. June 2026 split versioning per skill. Tags run from `v0.1.1` in May to `v0.1.9` in September 2026, plus `supabase-postgres-best-practices-v1.6.0` in July. The pace has dropped since: the last 90 days hold three commits, two of them from the release bot, and nothing has landed since early October 2026.

## Depth

The catalog counts two skills, and `supabase` is a 155-line `SKILL.md` with one reference file, `references/skill-feedback.md`, plus an issue template. It reads as an opinionated procedure. Check the Supabase changelog for breaking changes, discover CLI commands through `--help`, and iterate on schema with `execute_sql`. Avoid `apply_migration` during iteration, because it writes migration history on every call.

The security checklist is the most useful part of the skill. It covers views that bypass RLS, `UPDATE` policies that need `WITH CHECK`, `SECURITY DEFINER` functions exposed in `public`, and the deprecated `auth.role()` check that anonymous users pass. Each item names the failure mode and the fix, often with a short SQL policy.

`supabase-postgres-best-practices` is a 64-line index over 31 rule files in eight prefixed categories, from `query-` and `conn-` to `lock-` and `monitor-`. The rules total 1,614 lines and follow one template: an impact rating, an incorrect example, a correct example, and sometimes `EXPLAIN` output. `security-rls-performance.md` shows why wrapping `auth.uid()` in a subselect stops per-row evaluation. The description says the skill applies to Postgres running anywhere, but its RLS rules assume Supabase's `auth` schema, and neither skill ships scripts.

## Tooling

Install with `npx skills add supabase/agent-skills`, or add the repo as a Claude Code plugin marketplace that offers two plugins, `supabase` and `postgres-best-practices`. The `README.md` claims compatibility with 18 or more agents through the skills CLI, but the only host-specific manifest in the repo is for Claude Code. A root `.mcp.json` points at Supabase's hosted MCP server with only its docs feature enabled.

CI runs one Vitest sanity suite that copies `skills/` to a temporary directory and checks that `npx skills add` installs every skill for Claude Code. Release Please builds tarballs and dispatches a sync to `supabase-community/supabase-plugin`. Nothing lints skill content, and the repo has no evals. Frontmatter versions lag the releases: the `SKILL.md` files say 0.1.2 and 1.1.1, while the release manifest says 0.1.9 and 1.6.0.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [supabase/agent-skills](https://github.com/supabase/agent-skills) |
| Stars / forks | 3.1k / 273 |
| Open issues / PRs | 486 / 48 |
| Commits / contributors | 96 / 10 |
| First commit | January 2026 |
| Last 90 days | 3 commits |

As of November 2026 the repo has 3.1k stars and 486 open issues, five times its 96 commits. Read that count with the mechanism in mind: the `supabase` skill tells agents to file a `user-feedback:` issue on this repo, with the user's permission, whenever its guidance proves wrong. The issue tracker doubles as the skill's feedback inbox.

Pedro Rodrigues wrote 70 of the 83 human commits, or 84 percent. Greg Richardson follows with four, and eight others have one or two each, adding pieces such as the `schema-constraints` rule and a GiST index note. This is one maintainer with occasional help. The 48 open pull requests sit against three commits in the last 90 days, so expect slow review on anything you submit.

## Utility for target workloads

This is for teams building on Supabase with an agent that has shell or MCP access. Concrete jobs include writing RLS policies that pass the checklist, iterating on schema and then generating a migration, and diagnosing a table the Data API cannot see. The Postgres rules help with reviewing indexes, pagination, and connection pooling. Prerequisites are a Supabase project, the Supabase CLI at v2.81.3 or later for `supabase db advisors`, and ideally the Supabase MCP server.

The skill fetches the changelog and docs at run time, so the agent needs network access. Two skills is thin coverage for a platform with Auth, Storage, Realtime, Edge Functions, and Queues, and the `supabase` skill defers most product detail to fetched pages. If your database runs on Neon, the [Neon](/skills/neon) skills cover branching and pooled connections, and these Postgres rules still apply to generic query work. For ORM-level work, [Prisma](/skills/prisma) covers its own client and migrations.

Verdict: install `supabase` on any Supabase project for the security checklist alone. Install `supabase-postgres-best-practices` on any Postgres codebase, keeping in mind that the RLS rules assume Supabase's `auth` helpers. Check the CLI version first, since two commands the skill relies on need recent releases.
