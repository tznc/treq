---
description: "Leon Lin's Taste Skill: one prescriptive skill plus a browser-checked kit for agents that build company marketing websites from a brief."
---

Leon Lin, who publishes as `Leonxlnx`, maintains Taste Skill, which now ships one skill, `design-taste-frontend`, for agents that build company marketing websites. It pairs a 381-line `SKILL.md` with a 40 MB kit of CSS, page templates, photographs and Node scripts that check the page in headless Chrome. Install it if your agent builds landing or company pages from a brief and you want strict, checkable rules. Skip it for app screens, dashboards or design systems, which it does not target.

## History

The first commit, in February 2026, was an "Anti-Slop Frontend" skill. Two days later a rewrite added configuration dials for entropy and motion, React-first rules and a ban on purple accents. In March 2026 the repository split into three skills and kept growing, reaching 13 `SKILL.md` files by September 2026 with minimalist, brutalist and image-generation variants. Much of the mid-2026 activity was sponsor listings, and 90 of the 269 non-merge commits touch only `README.md` or `assets/`.

October 2026 reset the project. A commit describes a new default skill "built from a 247-site study", `v2.0.0` was tagged on October 13, and the other skills moved to `taste-skill-classics` and `imagegen-skills`. Version 2.5.0 on November 5 rebuilt the skill around company websites at what it calls the `figma.com` and `x.ai` bar, and 2.5.1 followed two days later. October 2026 is the busiest month with 117 commits, and the repository has 10 tags.

## Depth

The single skill is a procedure bound to its kit. `SKILL.md` opens with nine house rules, then classifies the brief as `software`, `gaming`, `tech`, `product`, `service` or `place`. That kind decides the page plan, the photo count and whether shader effects are allowed at all. Section 4 lists the exact commands to run, and the checklist names the checker ID behind each rule.

The skill tells the agent to "deal, never choose by taste". `scripts/start.mjs` deals a palette for the trade and a Google Fonts pairing, `assets/layout/deal.mjs` deals one of eleven hero layouts, and a history file keeps consecutive pages apart. The 22 reference files hold about 5,500 lines, including `copy-voice.md`, `layout-v4.md` and `photos.md`. The skill folder holds 1,631 files, 153 of them scripts, and the checker in `scripts/consistency.mjs` alone is 11,485 lines.

The rules are specific enough to check. Each section fills one or two screens at 1440x900, images carry no captions, violet is banned, and street addresses never show a house number. That precision cuts both ways, because the skill pushes back on any brand that wants those things.

## Tooling

You install with `npx skills add` naming `design-taste-frontend`, through the Claude Code marketplace in `.claude-plugin/marketplace.json`, or through the Codex manifest in `.codex-plugin/plugin.json`. `install.sh` symlinks a clone into Claude Code or Codex. The `SKILL.md` frontmatter registers `gate.mjs` as a Claude Code Stop hook, so the finish gate runs when the agent tries to end. The `CHANGELOG.md` says Codex and Antigravity read the same command from their own hook files.

The `README.md` lists Node 22 or newer and Chrome, Edge or Chromium, with ffmpeg optional, and says the skill needs no API keys. The one workflow, `check-skills.yml`, parses every frontmatter block as Claude Code and Codex would, checks text encoding and runs `node --check` on every script. `tests/kit-clean.mjs` runs the checker over the kit's own page models but is not wired into CI.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [Leonxlnx/taste-skill](https://github.com/Leonxlnx/taste-skill) |
| Stars / forks | 120.0k / 8.0k |
| Open issues / PRs | 14 / 4 |
| Commits / contributors | 321 / 26 |
| First commit | February 2026 |
| Last 90 days | 161 commits |

As of November 2026, Taste Skill has 120.0k stars, the most in this group, and 26 human contributors. Leon Lin wrote 277 of the 321 commits under two author names, which is 86% of the history. Agent identities for Cursor, Claude and Devin add 11 commits, mostly sponsor list edits. The other 25 people have between one and five commits each.

Outside work gets merged in batches. On October 11, 2026 Leon Lin merged 12 community pull requests within three minutes, including fixes whose commits date from May 2026. The 2.5.1 changelog credits four contributors by handle for ported fixes. Four open pull requests and 14 open issues make a small queue for 120.0k stars, a number that measures attention more than the skill itself.

## Utility for target workloads

Taste Skill suits developers and agencies who turn a short brief into a finished company site. It can build a cafe, hotel or shop page with credited stand-in photographs, or a SaaS page that shows the product as live HTML in one `app-frame`. You can also lint page copy for stock phrases with `copy-lint.mjs`, or check any built page at five widths with `consistency.mjs`. The `README.md` says a redesign of an existing page starts with a browser audit.

The limits follow from the precision. Output defaults to a static page built from `assets/starter.html`, and React, Vue or Astro projects load the kit from their public folder. Fetching extra photographs, fonts or brand icons needs network access. The kit adds 40 MB to every install, 12 MB of it photographs.

[Hallmark](/skills/hallmark) is the closest relative. It deals one of 50 themes where Taste Skill deals trade palettes, lints in a real browser, and also covers components and app screens. [Impeccable](/skills/impeccable) checks each edit with a 62-rule detector and works best on interfaces that already exist. Pick Taste Skill when marketing pages are the whole job and you want a Stop hook that holds the agent for up to three fix rounds.
