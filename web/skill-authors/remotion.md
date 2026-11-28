---
description: "Remotion's official Agent Skills: 13 skills for creating, editing and rendering React videos with Remotion, from markup rules to maps and captions."
---

Remotion publishes these 13 Agent Skills for its framework for making videos in React, and Jonny Burger authored every commit. The skills teach an agent how to write Remotion compositions, preview them in Studio, render them and wire Remotion into an app. Its `package.json` names `packages/skills` in the main Remotion monorepo as the source, and this repository receives synced copies. Install them if you build videos with Remotion, and skip them if you do not, because nothing here applies outside that framework.

## History

The first commit landed in January 2026. On that first day the layout went through several shapes, from 21 small rule and example skills to one `remotion-best-practices` skill with a `rules/` folder. That single skill, with 40 rule files by June, held until July 2026. On July 11 it split into separate `remotion-*` skills for markup, creation, rendering, captions, interactivity and SaaS integration.

Docs search, upgrades, maps, multimedia and Studio followed by early August 2026, and `remotion-license` arrived in November. Of the 100 commits, 99 carry the message "Update template", so the log records syncs and not reasons. Each `SKILL.md` and the `package.json` carry version `4.0.545`, in Remotion's own release numbering. Activity was quiet from March to June 2026 with eight commits, then steady at 7 to 16 a month.

## Depth

The 13 `SKILL.md` files total 1,198 lines with a median of 42. `remotion-markup` at 330 lines and `remotion-interactivity` at 341 carry most of the substance. `remotion-markup` routes to 33 topic files on audio, transitions, Lottie, 3D, FFmpeg, fonts, video matting and more. It uses `MUST`, `SHOULD` and `MAY` markers, and its central rule says CSS transitions and Tailwind animation classes do not render, so motion goes through `useCurrentFrame()` and `interpolate()`.

`remotion-maps` ships the most code of any skill. It offers five techniques, from a static satellite image to MapLibre, Mapbox, MapTiler and CesiumJS flythroughs, each with its own `TECHNIQUE.md` and TypeScript assets. `remotion-interactivity` explains how to structure markup so layers, timing and styles stay editable in Studio.

Several skills are thin wrappers. `remotion-license` is 15 lines of links, and `remotion-studio` and `remotion-upgrade` are about 30 lines each. The router `remotion-best-practices` embeds a copy of the other 12 skills with rewritten links, so about half of the 257 files under `skills/` are duplicates.

## Tooling

You install with `npx remotion skills add` inside a Remotion project, `npx skills add remotion-dev/skills` elsewhere, or `gh skill install`. `npx create-video@latest --skills` adds them to a new project. Each skill ships `agents/openai.yaml` with display metadata for Codex, plus an icon. The repository has no Claude Code plugin manifest.

The `scripts/` folder holds TypeScript that syncs skills, builds the embedded copies, regenerates the `README.md` and validates links. Those scripts resolve paths inside the monorepo, so they do not run from this checkout. The repository has no CI workflow, no tests, no evals and no `LICENSE` file. `remotion-docs` tells the agent to query Remotion's Algolia search index and fetch pages as Markdown.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [remotion-dev/skills](https://github.com/remotion-dev/skills) |
| Stars / forks | 7.8k / 862 |
| Open issues / PRs | 14 / 11 |
| Commits / contributors | 100 / 1 |
| First commit | January 2026 |
| Last 90 days | 32 commits |

As of November 2026, the repository has 7.8k stars and one human contributor. Jonny Burger wrote all 100 commits under two spellings of the same name, which the raw metrics count as two people. There are no merge commits, so no outside pull request has landed in this history.

Since every change arrives as a sync, the 11 open pull requests and 14 open issues have no visible merge path in this history. Community health is better judged on the main Remotion repository. The 7.8k stars are the fewest in this group, which fits a skill set that serves only Remotion users.

## Utility for target workloads

These skills fit developers who already use Remotion or plan to. Ask for a new composition with `/remotion-create`, such as a promo video for a record store. Animate a route between two cities with `remotion-maps`, or transcribe and style captions with `remotion-captions`. Render with input props through `remotion-render`, or embed the Player in a product with `remotion-saas`.

Check the prerequisites before you start. You need Node and a Remotion project. `remotion-render` tells companies that need a Remotion license to pass a license key on every render, and to mark test renders so they are not billed. Map techniques need a Mapbox or MapTiler key, Cesium city scenes need a billing-enabled Google Map Tiles key, and MapLibre needs none.

No other source in this group targets video. [Emil Kowalski](/skills/emilkowalski) writes motion rules for UI built with CSS, and those transitions do not render in Remotion. [Cathryn Lavery](/skills/cathrynlavery) offers optional diagram animation in HTML, not video output. If you build with Remotion, install these first, because they come from the framework's own maintainers and track its releases.
