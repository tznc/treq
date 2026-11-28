---
description: "Google Labs' Stitch skills: 16 Agent Skills that drive the Stitch MCP server to generate UI designs and turn them into React, React Native and video."
---

`google-labs-code/stitch-skills` is the Google Labs repository of Agent Skills for Stitch, Google's design tool that generates UI screens from prompts. Its 16 skills, split across three plugins, drive the Stitch MCP server to create designs, sync design systems and convert screens into React, React Native or Remotion videos. The readme marks it as not an officially supported Google product. Install it if you design in Stitch and want your agent to move screens into code, since most skills do nothing without a Stitch project.

## History

David East seeded the repo in January 2026 with a `react-components` skill that converts Stitch screens into React, and Dion Almaer merged the seed. January and February added `design-md`, `stitch-loop`, `enhance-prompt`, a Remotion skill and a `shadcn-ui` skill, and the `v0.1` tag followed in February 2026. In March 2026 Jilin Xia moved generated files into a `.stitch/` folder and created a `stitch-design` skill.

A rewrite landed in May 2026 as "Stitch Skills V2". It split the skills into three plugins, `stitch-design`, `stitch-build` and `stitch-utilities`, and added `code-to-design`, `extract-static-html` and an upload script. A run of commits titled "fix security risk" preceded the `v1.0` tag in May 2026. Codex plugin metadata arrived days later from contributor amourfrei.

June to August 2026 brought skills from first-time contributors: a React Native converter, a React and Vite dashboard skill and `site-md`. Then the repo went quiet for three months. All 14 commits of the last 90 days landed between November 17 and 20, 2026, and most of them harden validators or pin CI actions.

## Depth

The 16 skills have a median `SKILL.md` of 191 lines, with 62 supporting files and 13 scripts behind them. The design skills are prompt pipelines around MCP tools. `generate-design` makes the agent enhance every prompt before it calls `generate_screen_from_text` or `edit_screens`, using a design-mappings reference that swaps vague terms for UI vocabulary. It also forbids colors and fonts in screen prompts, since the design system holds those at the project level.

The build skills carry the real code, and `react-components` runs four gated phases. The agent must download each screen with `fetch-stitch.sh`, view every screenshot and extract Tailwind tokens into `style-guide.json`. An optional last phase runs an AST validator over every component, checking rules such as a typed `Props` interface. `react-native` mirrors that structure, and `extract-static-html` ships three TypeScript scripts that snapshot a running app into self-contained HTML for Stitch.

The edges are thin or loosely tied to Stitch. `site-md` and `code-to-design` have no supporting files, and `shadcn-ui` never mentions Stitch at all. Eight skills use names like `stitch::generate-design`, which do not match their folder names, and the readme warns that OpenCode will not load them until you rename them. `taste-design` writes a `DESIGN.md` of anti-generic design rules for Stitch to follow.

## Tooling

Codex gets first-class support. `.agents/plugins/marketplace.json` lists the three plugins, and each has a `.codex-plugin/plugin.json`, so `codex plugin marketplace add` works. For Claude Code and Cursor the readme uses the `npx plugins add` command, since the repo has no `.claude-plugin` manifest. You can also pull single skills with `npx skills add`, and the readme gives manual steps for OpenCode.

CI is real but narrow. `validate-skills.yml` runs Node's test runner over helper-script tests, then installs the `react-components` and `react-native` packages and validates each gold-standard example. The November 2026 commits pinned every action to a commit SHA and cut the workflow token to read-only. Only the React, React Native and `shadcn-ui` helpers have tests, and there are no evals.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [google-labs-code/stitch-skills](https://github.com/google-labs-code/stitch-skills) |
| Stars / forks | 11.8k / 1.5k |
| Open issues / PRs | 13 / 15 |
| Commits / contributors | 112 / 19 |
| First commit | January 2026 |
| Last 90 days | 14 commits |

As of November 2026 one maintainer carries the repo. Jilin Xia wrote 61 of the 112 commits under three identities and performed 24 of the 31 merges. Once those aliases merge, 19 people have commits, and David East and Dion Almaer did most of the early work.

Pull requests from occasional contributors merge regularly. Emp1500 fixed validator crashes and CI coverage, and Rudy Celekli tightened the React, React Native and `shadcn-ui` checks in November 2026. `CONTRIBUTING.md` asks for a Google CLA. With one person doing most merges, the pause from August to November stopped all intake.

## Utility for target workloads

This repo is for frontend developers and designers who use Stitch. It helps you generate screens or design variants from a text brief, and extract a `DESIGN.md` from an existing codebase. It can also turn a Stitch project into typed React or React Native components, or render a walkthrough video of a project with Remotion.

Prerequisites are a Stitch account and the Stitch MCP server configured in your agent, per the readme's setup guide. The scripts need Node, Python and Bash, and `stitch-loop` uses the Chrome DevTools MCP server to check pages when you have it. Several skills write state into a `.stitch/` folder in your project, so expect new files there.

Google publishes two sibling repos with no overlap. [Google](/skills/google) covers Google Cloud, Ads and Analytics, and [Google Gemini](/skills/gemini) covers code that calls the Gemini API. Verdict: install `stitch-design` and `stitch-build` if Stitch is part of your design workflow, and look elsewhere for general frontend skills.
