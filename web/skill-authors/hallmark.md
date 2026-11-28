---
description: "Hallmark, from Nutlope and Together AI, is one design skill that deals themes, builds web pages and lints out AI slop in a real browser."
---

Hallmark sits under the `Nutlope` GitHub account, and its `README.md` says Together AI made it. Youssef wrote 312 of its 349 commits, and Hassan El Mghari merged the major releases. It ships one skill that builds web pages, components and app screens from a dealt theme, then lints them in headless Chrome for generated-UI tells. Install it if your agent builds pages from short prompts and you want a hard floor, varied output and a linter you can run in CI.

## History

The first commit, in April 2026, was already labeled Hallmark v0.2.0 and shipped 12 themes. Version numbers climbed from 0.2 to 1.0 in eight days, reaching v1.0 on May 5, and May 2026 has 99 commits. Version 1.1 in June 2026 added four themes and a full copy pass. In July 2026 a release reorganized the skill around "one core, three first-class harnesses".

The current line is a v2 rewrite merged on October 22, 2026 through pull request #79. Its commits describe "one build skill, dealt themes, a Floor the linter enforces" and a launch pass that reworked every theme. October 2026 is the busiest month with 117 commits. The last commit is from November 11, 2026, and the repository has no tags.

## Depth

The `SKILL.md` is 154 lines and states its own rules in full. A 16-item Floor binds every build, such as no `transition: all`, no eyebrow labels, no invented metrics and no card inside a card. Most items carry a gate number from `gates.md`, and `sloplint.mjs` reports findings by those numbers. Above the Floor sit Reflex gates, waivable with a stated reason, and Finish checks that never block.

The flow is an opinionated procedure in eight steps. The agent reads the project, asks once for material such as screenshots and real numbers, then plans the page around the visitor's job. `theme.mjs deal` offers three of 50 themes and `draw` deals a page shape, hero posture, nav and footer. The agent builds the hero first and sends it to a fresh-context reviewer before writing the rest.

The skill folder holds 187 files in 7.2 MB. References run about 2,100 lines plus 21 page macrostructures, four genre files and 50 theme files of at most 8 KB, each with a screenshot. `sloplint.mjs` is 4,426 lines and its render tier adds 1,283 more, checking pages at eight widths from 320 to 1920 pixels. `photos.json` lists 755 Wikimedia Commons photographs, and `capture.mjs` screenshots your real product from a URL or dev server.

## Tooling

`npx skills add nutlope/hallmark -g` installs for Claude Code, Codex, Cursor, OpenCode and GitHub Copilot. A Claude Code marketplace lives in `.claude-plugin/marketplace.json`, `agents/openai.yaml` describes the skill to Codex, and frontmatter enables an OpenCode slash command. `install-hook.mjs` adds an edit-time Floor check to Claude Code. The scripts need only Node 18, and the browser tier adds `puppeteer-core` and Chrome.

The `verify.yml` workflow runs `refcheck.mjs` to confirm that references, gates, fixtures, versions and links agree. It also validates every theme file, checks that the theme index is current and runs every `*.test.mjs` file. An `eval/` folder, kept out of the published package, runs briefs through models, scores pages with `sloplint` and grades screenshots with a vision judge. Generated imagery is opt-in and needs a `TOGETHER_API_KEY`.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [Nutlope/hallmark](https://github.com/Nutlope/hallmark) |
| Stars / forks | 59.4k / 3.3k |
| Open issues / PRs | 30 / 24 |
| Commits / contributors | 349 / 11 |
| First commit | April 2026 |
| Last 90 days | 147 commits |

As of November 2026, the repository has 59.4k stars and 11 human contributors. Youssef's 312 commits are 89% of the history, and nine of Hassan El Mghari's 12 commits are pull request merges. Rudy Celekli is the most active outside contributor, with ten commits on capture, render and theme fixes.

Outside fixes do land. Of 34 merge commits, 22 bring in branches from outside accounts, mostly small script fixes. Reference numbers only reach #142, so traffic is light for the star count, and 24 open pull requests make a large backlog next to 22 outside merges. `CONTRIBUTING.md` asks for an issue before any new gate or theme and warns that cosmetic pull requests get closed.

## Utility for target workloads

Hallmark suits developers who ask an agent for whole pages and want them to differ. Ask for a landing or pricing page with a dealt theme, or say "custom" for a design from the prompt alone. Run `/hallmark audit <path>` for a report without changes, or `/hallmark redesign <path>`, which records every link, field and handler first and fails if one goes missing. You can also run `sloplint.mjs <file> --render` on pages your agent built without the skill.

The limits are real. The skill refuses to touch an existing design unless you name it, so routine edits stay outside its rules. Without your own photographs it places library stand-ins, and the Floor forbids invented proof, so a page without real numbers, quotes or logos ships without them. A full sweep with the fresh-context reviewer adds subagent runs and tokens to every build.

Hallmark's `README.md` lists [Impeccable](/skills/impeccable), [Taste Skill](/skills/taste-skill) and [Emil Kowalski](/skills/emilkowalski) as inspirations, and it overlaps them most. Taste Skill also deals palettes and gates in a browser but serves only company sites, while Impeccable works best on interfaces that already exist. Pick Hallmark for new pages across many categories, with a linter you can also run on its own.
