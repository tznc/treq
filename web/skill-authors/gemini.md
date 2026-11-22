---
description: "Google's gemini-skills repo: four Agent Skills that keep coding agents current on Gemini API models, the Interactions API, Live API and Omni video."
---

`google-gemini/gemini-skills` is a small repository of Agent Skills for building on the Gemini API, published under Google's `google-gemini` GitHub organization. It holds four skills, and their job is narrow: stop a coding agent from writing Gemini code against stale model names and deprecated SDKs. The readme marks it as not an officially supported Google product. Install it if your code calls the Gemini API with an AI Studio key, and skip it if you work only on Google Cloud or use another model provider.

## History

Mark McDonald started the repo in February 2026 by importing a single Gemini API skill file, renamed `gemini-api-dev` three days later. Within weeks, contributors added Java SDK notes, a Vertex AI skill and a Live API skill. Philipp Schmid added a separate Interactions API skill in March 2026. In April 2026 the team removed the Vertex AI skill as a duplicate, and the readme now points to its new home in Google's main skills repo.

From May 2026 many commits follow model launches. June brought `gemini-omni-flash-api` for video generation, and August added manifests for Claude Code, Codex and Cursor. At the end of August the Interactions API skill merged into `gemini-api-dev`, and the plugin version jumped to 2.0.0 in early September. November 2026 added `gemini-api-cli`, and the manifests now read 2.8.0.

The cadence is steady. The repo has 183 commits, 63 of them in the last 90 days, and 50 of the 131 non-merge commits touch `gemini-api-dev`. Changes arrive as pull requests, with numbers reaching #101.

## Depth

The four `SKILL.md` files total 1,470 lines, and two `references/migration.md` files and four Python scripts sit behind them. `gemini-api-dev` is the core at 506 lines. It opens with rules that tell the agent its training data is outdated, lists current model IDs and agents, and bans the legacy `google-generativeai` SDKs. It then gives Python and TypeScript snippets for streaming, Deep Research, managed agents and the Decisions API, and tells the agent to fetch the matching docs page first.

`gemini-live-api-dev` is 408 lines on WebSocket streaming with the Live API, covering voice activity detection, session resumption and ephemeral tokens. `gemini-omni-flash-api` is the only skill with scripts. Its `generate_video.py`, `upload_file.py`, `prep_video.py` and `inspect_video.py` wrap the SDK and ffmpeg for text-to-video, video edits and extensions. `gemini-api-cli` is 144 lines that teach the separate `gemini-api` command-line tool, with a command table and an auth warning about which key wins.

These skills are reference sheets with hard rules, and their value decays fast. The model list in `gemini-api-dev` alone has 17 entries, and 23 commit subjects mention a model. The readme claims the skill raised correct API code to 87% with Gemini 3 Flash and 96% with Gemini 3.1 Pro, citing a Google blog post. The repo ships no eval to reproduce that.

## Tooling

The repo carries one plugin in five manifest formats: a root `plugin.json`, `.claude-plugin/`, `.codex-plugin/`, `.cursor-plugin/` and an `.agents/plugins/marketplace.json` for Codex. All of them share version 2.8.0. The readme gives install commands for each host, plus `npx skills add` and the Context7 `ctx7` CLI. It also says Antigravity bundles these skills already.

There is no CI, no test suite and no validation of skill files. Review happens in pull requests, where two merged commits apply suggestions from the `gemini-code-assist` bot. The omni scripts are the only executable code, and nothing tests them.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [google-gemini/gemini-skills](https://github.com/google-gemini/gemini-skills) |
| Stars / forks | 4.5k / 510 |
| Open issues / PRs | 5 / 10 |
| Commits / contributors | 183 / 17 |
| First commit | February 2026 |
| Last 90 days | 63 commits |

As of November 2026 this is a small team with a clear lead. Seventeen people have commits once name variants merge, and Philipp Schmid wrote 62 of the 183 under two identities. Mark McDonald follows with 45, and Holt Skinner, Patrick Loeber, Trey Nguyen, Thor Schaeff and Guillaume Vernade have 10 to 13 each.

Pull requests from occasional contributors do merge. Fixes from xiaolai, Ryan Swift, Madi and Rody Davis landed in 2026, covering wording, docs setup, the Codex install command and the first plugin manifest. With 5 open issues and 10 open PRs against 4.5k stars, the backlog stays small.

## Utility for target workloads

This repo is for developers who write application code against the Gemini API with the `google-genai` Python SDK or `@google/genai` for JavaScript. It helps with migrating code from `generateContent` to the Interactions API, and with picking current model IDs when you upgrade. It also covers building a voice agent on the Live API, generating or extending video with Gemini Omni, and scripting media generation from a shell with `gemini-api`.

Prerequisites are a `GEMINI_API_KEY`, Python 3.10 or later for the omni scripts, and ffmpeg for video prep. The omni skill warns that video uploads for edits do not work in the EEA, Switzerland, the United Kingdom and some US states. The `gemini-api-cli` skill also needs the `gemini-api` binary, which installs from a script in its own repo.

Google publishes two sibling repos with different scope. [Google](/skills/google) covers Gemini on Google Cloud through its `gemini-api`, `gemini-live-api` and `gemini-interactions-api` skills, which assume Cloud auth. [Google Stitch](/skills/stitch) targets the Stitch design tool and does not touch the Gemini API. Verdict: install `gemini-api-dev` if you ship Gemini API code, and update it often, since its value is how current its model list is.
