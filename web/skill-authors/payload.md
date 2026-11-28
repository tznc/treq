---
description: "Payload's official agent skills: a Payload CMS development guide for collections, hooks and access control, plus a CMS-to-Payload migration workflow."
---

Payload maintains `payloadcms/skills`, two skills for agents working on Payload, which the skill describes as a Next.js native CMS with a TypeScript-first architecture. `payload` is a development guide to collections, fields, hooks, access control, and queries. `cms-migration` is a conversational workflow for designing Payload collections from another CMS's data. Install it if you build on Payload, knowing the repo has been quiet since July 2026.

## History

Elliot DeNolf created the repo in January 2026, and the second commit ported the `payload` skill from the main `payloadcms/payload` repository. January produced eight of the repo's 15 commits, including a rename from "Payload CMS" to "Payload" and a pull request that added proper logger usage. Jarrod Flesch wrote `cms-migration` in March 2026, and it merged in April after the author trimmed scripted phrases and a redundant section.

The last change is a July 2026 commit that synced the `payload` skill "with latest", touching seven files with 230 insertions and 93 deletions. Most of that went into `SKILL.md`, which gained modeling defaults, a gotchas list, and best-practice sections. There are no tags, releases, or changelog, and no commits in the last 90 days.

## Depth

The repo has two skills, but both carry substance. `payload/SKILL.md` runs 518 lines and opens with a quick-reference table of 27 tasks, each linked to a reference file. It then sets defaults such as enabling drafts and using `slugField()` for slugs. Short TypeScript examples cover collections, field and collection hooks, typed access control, Local API queries, and getting a Payload instance in a route or server component.

Its security section is the part most worth reading, with wrong and right code for three pitfalls. Local API calls bypass access control unless you pass `overrideAccess: false`. Nested operations in hooks run in a separate transaction unless you pass `req`, and a hook that updates its own collection loops unless it checks a context flag.

The 11 references total 6,268 lines, led by `PLUGIN-DEVELOPMENT.md` at 1,436 and two access control files near 700 each. They read as pattern libraries with TypeScript examples throughout. Neither skill ships scripts or templates.

`cms-migration` is 149 lines of procedure. The agent asks for a data sample, proposes a collection config, iterates until the user confirms, then plans import order, ID mapping, media, and rich text. Its sharpest rule tells the agent to model repeated strings such as categories and tags as relationships to a collection, and to reserve select fields for fixed enums. A 1,252-line field reference backs it.

## Tooling

Tooling is minimal. The only documented install path is `npx skills add payloadcms/skills`. The repo has no plugin manifests for any agent host, no MCP configuration, no CI workflows, no tests or evals, and no frontmatter validation.

`AGENTS.md` and `CLAUDE.md` are identical files that describe how to write a new skill, including a `scripts/` convention that neither skill uses. The `README.md` states an MIT license, but the repo contains no `LICENSE` file, which matters if your organization reviews licenses before vendoring third-party content. The skills lean on Payload's own tooling. They reference `npx create-payload-app`, `payload generate:types`, and the `payload build` command, which the July update recommends over calling `next build` directly.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [payloadcms/skills](https://github.com/payloadcms/skills) |
| Stars / forks | 174 / 6 |
| Open issues / PRs | 3 / 2 |
| Commits / contributors | 15 / 3 |
| First commit | January 2026 |
| Last 90 days | 0 commits |

As of November 2026 the repo has 174 stars, 6 forks, 3 open issues, and 2 open pull requests. Three people account for every commit: Elliot DeNolf with nine, Jarrod Flesch with five, and Jessica Rynkar with one merge commit. Pull request numbers stop at #5.

The ratio is lopsided. The repo drew more stars than Prisma's or Neon's, with a fraction of their commits and only three authors. The two open pull requests sit against a history with no merge since July 7, 2026, so expect slow review.

## Utility for target workloads

This is for developers building or maintaining a Payload app with an agent. Concrete jobs include modeling a collection with drafts, slugs, and relationships, and writing row-level access control that returns a query constraint. Others are adding an `afterChange` hook without breaking the transaction, building a Payload plugin that adds fields to existing collections, and planning a WordPress or Contentful migration.

Prerequisites are a Payload project and one of its database adapters: MongoDB, Postgres, or SQLite. The skills need no account, API key, or MCP server. Neither calls a hosted service, so the agent works entirely in your codebase.

The gaps are maintenance and verification. Nothing checks the examples against current Payload releases, and the `payload` skill has had one content update since January. `cms-migration` designs collections but stops at offering a seed script. If you run Payload on Postgres, the hosting skills from [Neon](/skills/neon) or [Supabase](/skills/supabase) cover connections and pooling, which this repo leaves out.

Verdict: install `payload` on any Payload codebase for the security pitfalls and the quick reference. Add `cms-migration` only when you plan a migration into Payload. Check the examples against your Payload version, since nothing in the repo does.
