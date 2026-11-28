---
description: "Anthropic's reference Agent Skills for Claude: Word, Excel, PowerPoint and PDF skills, a Claude API reference, skill-creator and design examples."
---

`anthropics/skills` is Anthropic's public repository of Agent Skills, the folder format with a `SKILL.md` file that Claude loads on demand. Its 19 skills run from the Office document skills behind Claude's file creation to a large Claude API reference and a set of design examples. The readme calls them demonstrations and tells you to test them before you rely on them. Install the Office skills and `claude-api` if you build on Claude, and read the rest as worked examples of how to write a skill.

## History

Anthropic opened the repo in October 2025 with an `init repo` commit from Keith Lazuka that added 290 files. That first drop held the four document skills under `document-skills/`, examples such as `canvas-design`, `mcp-builder`, `slack-gif-creator` and `webapp-testing`, and the first Agent Skills spec. A Claude Code marketplace file arrived the next day. In December 2025 a restructure moved every skill into `skills/`, the spec into `spec/` and the starter into `template/`, and the spec later moved out to agentskills.io.

Since March 2026 the repo has mostly tracked one skill. `claude-api` landed that month, and 31 of the 64 commits touch it, each syncing new models, SDK changes or Managed Agents features. Other milestones are sparse. The Office skills were refreshed in February, July and November 2026, and in November 2026 they moved from a source-available license to Apache 2.0.

Nearly every commit is a squash-merged pull request, with numbers in commit subjects reaching #2221. There are no tags and no changelog. The last 90 days brought 12 commits, seven of them `claude-api` updates.

## Depth

The median `SKILL.md` runs 147 lines, and the substance behind each one varies widely. `claude-api` is the outlier: a 616-line router that detects your project language and sends the agent into 95 supporting files. Those hold per-SDK docs for seven languages plus cURL, Managed Agents quickstarts and eval guides. It also defines subcommands such as `migrate`, `prompt-audit` and `cost-optimize`, so it works as both reference and procedure.

The Office skills are the most engineered. `docx` keeps its `SKILL.md` to 92 lines of gotchas, such as docx-js defaulting to A4 and tables needing widths on both the table and every cell. Behind it sit 60 files, including Office Open XML schemas, a `validate.py` that checks edits against them and a `comment.py` that writes the six files a Word comment needs. `pptx` and `xlsx` follow the same pattern with 17 and 12 scripts.

`skill-creator` is a 485-line authoring loop. It runs test prompts with and without the skill, grades the results and opens a review viewer, using scripts like `run_eval.py` and `improve_description.py`. At the thin end, five skills ship nothing beyond the `SKILL.md` and a license file. `frontend-design` is 71 lines of opinionated design guidance that lists the visual tells of generated pages, and `internal-comms` is a 32-line router to four example files.

## Tooling

The only manifest is `.claude-plugin/marketplace.json`, which registers the repo as a Claude Code marketplace with five plugins: `document-skills`, `example-skills`, `claude-api`, `academy-guide` and `discernment-nudge`. You add it with `/plugin marketplace add anthropics/skills`. The readme also points to Claude.ai, where it says paid plans already include these skills, and to the Skills API. Nothing in the repo targets Codex, Cursor or Gemini CLI, though the plain folders work in any host that reads `SKILL.md`.

There is no CI, no test suite and no lint step for skills. The validation that exists lives inside the skills. `skill-creator` ships `quick_validate.py` and `package_skill.py`, and its grading scripts call `claude -p`, so they need Claude Code installed. The Office skills validate their own output against XSD schemas and expect LibreOffice, Pandoc and Poppler on the machine.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [anthropics/skills](https://github.com/anthropics/skills) |
| Stars / forks | 198.9k / 23.3k |
| Open issues / PRs | 464 / ~1.1k |
| Commits / contributors | 64 / 16 |
| First commit | October 2025 |
| Last 90 days | 12 commits |

As of November 2026 the repo has close to 200,000 stars, and its intake is lopsided. About 1,100 pull requests sit open against 64 commits that ever landed. Sixteen people have commits, and half of the 62 human commits come from anthropic.com addresses. Lance Martin and CJ Avilla account for 24 commits between them, all of them `claude-api` updates.

Outside fixes do land, such as a link correction in #159 and a YAML rendering fix in #898, but they are rare. Two more commits come from a `cc-skill-sync` bot that mirrors `claude-api`. Read the star count as attention to the Agent Skills format. It does not mean the repo reviews community work at that scale.

## Utility for target workloads

This repo serves developers who build on the Claude API and engineers who write their own skills. It handles redlining a Word contract with tracked changes, and building a PowerPoint deck then rendering it to images for review. It also covers migrating code to a newer Claude model with `claude-api migrate`, scaffolding an MCP server with `mcp-builder`, and benchmarking a new skill with `skill-creator`.

The gaps are scope and fit. `claude-api` stops and asks when it finds OpenAI or other provider code, by design. `brand-guidelines`, `academy-guide` and `internal-comms` encode Anthropic's own brand, courses or formats, so they need rewriting before they help you. Code that `claude-api` produces needs an Anthropic API key to run.

The sibling repo [Claude Plugins](/skills/claude-plugins), also run by Anthropic, is the place for Claude Code plugins with hooks, agents and commands. It ships the same `frontend-design` and `skill-creator` skills, so one copy is enough. Verdict: install `document-skills` and `claude-api` if you work with Claude, and treat the rest as reference material for your own skills.
