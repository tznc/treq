---
description: "Corey Haines's 53 marketing skills for AI agents: CRO, copywriting, SEO and AI search, paid ads, analytics, pricing and outbound sales workflows."
---

Corey Haines, whose `README.md` calls Conversion Factory "Corey's agency", maintains `marketingskills`, a library of 53 marketing skills. They cover conversion optimization, copywriting, SEO and AI search, paid ads, analytics, pricing, outbound sales and launches, and every skill reads a shared product context file first. A set of 69 zero-dependency CLIs for marketing platforms ships next to the skills. Install it if you are a technical founder or marketer who wants an agent to audit pages, write copy and wire up tracking.

## History

Corey Haines published the first 14 skills on January 15, 2026, and tagged v1.0.0 the next day, after restructuring the folders for `npx skills` and adding a Claude Code marketplace file. The library grew from 25 skills at the start of February 2026 to 40 in May and 50 by September. Release 2.0.0 in May renamed 17 skills and merged `page-cro` and `form-cro` into one `cro` skill.

October 2026 changed the pace. That month alone produced 885 commits, including 290 merge commits, as Rudy Celekli joined with 224 commits on CLIs, packaging and integration guides. The same month added `reverse-engineer-marketing` and began publishing per-skill ZIPs for Claude on the web and Perplexity.

As of November 2026, the repo carries 125 tags, ending at v2.14.25 on November 19. `VERSIONS.md` tracks a separate version for every skill. The last 90 days hold 1,232 commits, so expect frequent small updates.

## Depth

The 53 skills hold 16,684 lines of `SKILL.md`, with a median of 314 lines, and `ai-seo` sits exactly at the 500-line limit in `CONTRIBUTING.md`. Every skill ships supporting files and an `evals/evals.json` with test prompts and assertions. Only two skills ship code, because helper code lives in `tools/`.

`product-marketing` is the foundation: it writes `.agents/product-marketing.md`, and the other skills read that file before they ask you questions. `cro` is typical at 198 lines, an opinionated checklist that ranks value proposition, headline, CTA and friction. It returns quick wins, larger changes and test ideas. `ai-seo` routes 15 reference files by question and tells the agent to weigh platform documentation above vendor studies.

`conversion-tracking` is the deepest skill, with 75 files. It ships 26 detection recipes for forms, booking tools and clicks as Google Tag Manager JSON, plus browser snippets the agent must hand over verbatim. Its first rule is to fire on confirmed success, never on a submit click, and to prove a test conversion arrived.

## Tooling

You can install through `npx skills add`, the Claude Code plugin marketplace, the Codex plugin marketplace, SkillKit, a git submodule, or ZIPs from each GitHub release. `tools/clis/` holds the 69 single-file Node CLIs for platforms such as GA4, Ahrefs, Apollo and Resend. `tools/integrations/` adds 128 integration guides.

Five of the seven CI workflows are checks. They validate skill frontmatter, require a version bump when a skill changes, and run validator regression tests. They also run `node --check` and unit tests on the CLIs, test ZIP packaging, and warn on hidden content such as HTML comments, invisible Unicode and pipe-to-shell commands.

The `evals/triggers` suite runs headless `claude -p` to check that the right skill fires for each prompt. It keeps held-out prompt sets for tuning descriptions. Its `README.md` reports that identical runs scored between 80.2% and 87.7%, and tells you not to read meaning into a change of a few points.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [coreyhaines31/marketingskills](https://github.com/coreyhaines31/marketingskills) |
| Stars / forks | 62.0k / 9.1k |
| Open issues / PRs | 35 / 14 |
| Commits / contributors | 1,670 / 90 |
| First commit | January 2026 |
| Last 90 days | 1,232 commits |

Corey Haines wrote 1,190 of the 1,669 human commits, and Rudy Celekli and Aaron Beashel add 274 more between them. The other 87 contributors mostly sent one or two commits. The log shows 277 PR merge commits and 92 squash merges, and PR numbers reach #1019. Of the merge commits, 76 came from forks owned by 73 other accounts.

The queue is in good shape for a repo this size, with 14 open PRs and 35 open issues. The `README.md` says paid, disclosed partners fund the work. `tools/PARTNERS.md` states that sponsorship buys a labeled integration guide and a listing in the `README.md`, and never a change to what a skill recommends.

## Utility for target workloads

This library suits founders, growth engineers and technical marketers at software companies. Concrete tasks include auditing a landing page with `cro`, rewriting page copy with `copywriting` and planning comparison pages with `competitors`. `conversion-tracking` sets up or debugs GA4 and ad-platform conversions, and `ai-seo` checks whether AI answer engines can crawl and cite your site.

Most skills need nothing but the agent. The CLIs read credentials such as `GA4_ACCESS_TOKEN` from environment variables, so each one needs an account on its platform. Many skills are written for software products, as the `marketing-ideas` description states, so retail and local-business marketing get less coverage.

For research before a launch or a competitor teardown, [Matt Van Horn](/skills/mvanhorn)'s `last30days` collects what people said about a topic in the last month. That output complements `competitor-profiling`, which builds dated profiles of rivals. The verdict: install the whole pack if marketing is part of your job, fill in `product-marketing` first, and treat the partner integrations as optional extras.
