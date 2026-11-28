---
description: "Matt Pocock's agent skills for real engineering: grilling interviews, specs and tickets, TDD at agreed seams, two-axis code review and domain modeling."
---

Matt Pocock maintains `mattpocock/skills`, skills the `README.md` describes as the ones the author uses every day for real engineering work. `README.md` positions them against frameworks that own the whole process and promises small, composable skills you adapt to your project. The core is a handful of workflows: grilling sessions that interrogate a plan, spec and ticket generation on your issue tracker, and TDD at agreed test seams. Install it if you want to drive each step yourself with slash commands and keep a domain glossary in the repo.

## History

The first commit landed in February 2026 with five skills, among them `tdd` and `write-a-prd`, and the repo gained a `README.md` and an MIT license in March 2026. April brought renames that still echo in the changelog, such as `write-a-prd` becoming `to-prd` and later `to-spec`. The same month added the `setup-matt-pocock-skills` onboarding skill and the category folders. Changesets and the v1.0.0 tag arrived in June 2026.

In July 2026 Pocock added a plugin marketplace so the set installs as a native Claude Code plugin, and added Codex metadata to every skill. July was the busiest month, with 182 commits. The latest tag is v1.3.2 from October 2026, and the repo logged 103 commits in the last 90 days and 13 in the last 30.

## Depth

The catalog lists 39 skills across four folders: 20 in `engineering`, 7 in `productivity`, 4 in `misc` and 8 in `in-progress`. They are short, with a median of 75 lines and `pr` the longest at 170. Of the 39, 23 ship nothing beyond `SKILL.md` and a Codex `agents/openai.yaml` file. Several are thin routers: `grill-me` is one line that calls `grilling`, `grill-with-docs` calls `grilling` and `domain-modeling`, and `wait-what` is a single prompt.

The substance sits in the longer procedures, and they are opinionated. `tdd` makes you confirm test seams with the user before any test exists and rejects writing all tests before any code. `wayfinder` charts work too big for one agent session as a map of decision tickets on your tracker. `setup-matt-pocock-skills` ships templates for GitHub through `gh`, GitLab through `glab`, and local Markdown files.

`README.md` splits the skills on one axis. User-invoked skills such as `/to-spec` orchestrate, and model-invoked skills such as `tdd` and `codebase-design` hold reusable discipline. A user-invoked skill may call model-invoked ones but never another user-invoked skill, which keeps the call graph shallow.

## Tooling

Install paths cover the official Claude Code plugin marketplace, a Codex marketplace, the Copilot CLI, `gemini skills install` and `npx skills add` for other agents. The Claude Code plugin ships 27 skills. The `misc` skills are frozen and the `in-progress` skills are beta, so you install those one at a time with `npx skills`.

CI runs four workflows. `check.yml` parses every frontmatter block with the same YAML parser the `skills` installer uses, and `release.yml` opens changeset version PRs. Two more workflows label new issues `needs-triage` and close `needs-info` issues after 14 days without a reply. The repo has no tests or evals of skill behavior, so quality rests on the author's daily use.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [mattpocock/skills](https://github.com/mattpocock/skills) |
| Stars / forks | 330.3k / 27.8k |
| Open issues / PRs | 238 / 26 |
| Commits / contributors | 559 / 1 |
| First commit | February 2026 |
| Last 90 days | 103 commits |

This is a one-person repo. As of November 2026, Matt Pocock's account wrote 509 of 559 commits, and the other author names are agents or placeholders such as `Claude`, `Remote Box Agent` and `Test`. All 116 merge commits come from branches in the maintainer's own repo. `CONTRIBUTING.md` states that pull requests are open to collaborators only, so outside input arrives as issues that the changelog credits by number.

The star count is the most lopsided in this group: 330.3k stars against one maintainer, 238 open issues and 26 open PRs. Triage is structured, with issue templates, a `needs-triage` label and an `.out-of-scope` folder that records rejected requests and the reasons. Expect fixes on the author's schedule, and expect requests for first-class support of other issue trackers to be declined, since `.out-of-scope` keeps the templates to GitHub, GitLab and local Markdown.

## Utility for target workloads

These skills fit engineers who want structure at the edges of a task and control in the middle. You can interview yourself into a sharper plan with `/grill-me`, then turn the discussion into a spec and tracer-bullet tickets with `/to-spec` and `/to-tickets`. `/improve-codebase-architecture` surveys a codebase for deep-module refactors, and `code-review` checks a diff against both your standards and the originating spec in parallel subagents. Prerequisites are light: an issue tracker or local Markdown files, plus `gh` or `glab` if you use those trackers.

[Superpowers](/skills/superpowers) and [gstack](/skills/gstack) chain their skills into a fixed process, and [Ponytail](/skills/ponytail) runs as an always-on mode, while these skills wait until you call them. [Addy Osmani](/skills/addyosmani) covers the same topics with longer reference skills and runs evals in CI, which this repo lacks. [Planning with Files](/skills/planning-with-files) keeps plan state on disk during a session, while this repo's `handoff` skill only writes a summary when you end one. Install this source for the grilling and ticket workflows, and skip `misc` and `in-progress` unless you plan to maintain your own copy.
