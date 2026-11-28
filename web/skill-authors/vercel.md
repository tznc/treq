---
description: "Vercel Labs' agent-browser ships one discovery skill for its Rust browser automation CLI, which serves versioned usage guides to coding agents."
---

Vercel Labs maintains `vercel-labs/agent-browser`, a browser automation CLI written in Rust that drives Chrome over the DevTools Protocol. The repo is a product monorepo, and the catalog lists one skill from it: a 51-line stub that tells the agent to load the real guide from the installed CLI. Install it if you want agents to open pages, fill forms and run QA passes from the shell. Judge it as a CLI, because the skill is only the entry point.

## History

Chris Tate started the repo in January 2026 and bumped it to version 0.1.1 on the first day. A Rust CLI replaced the TypeScript one the next day, and a 143-line `browsing-web` skill followed on day three. Later that month contributors added CDP connections, video recording, a Claude Code marketplace plugin and a skill split into references and templates.

March 2026 was the busiest month, with 249 commits. Its central pull request, titled `native`, replaced the Node.js and Playwright daemon with a single Rust binary that speaks CDP directly. April 2026 changed how skills ship: an `agent-browser skills` command arrived with evals, specialized skills moved to `skill-data/`, and the main guide was renamed `core` and served by the CLI.

Skill work since then is steady but secondary. Of 871 commits, 225 touch `skills/` or `skill-data/`, and only 87 touch `skills/`, the folder the catalog indexes. November 2026 slimmed the `core` guide, added loading of single references with `--ref` and added a Modern Web Guidance skill. The project has 122 release tags, from 0.6.0 in January to 0.43.0 in November 2026.

## Depth

The catalog sees one skill, `agent-browser`, with 51 lines and no supporting files. It is marked `hidden: true`, allows only `Bash(agent-browser:*)`, and lists the CLI commands that load real guidance. The stub explains the design: the CLI serves skill text that matches the installed version, and a static file cannot.

The substance sits in `skill-data/`, which the npm package ships and the CLI prints on demand. It holds ten skills and 5,224 lines. `core` is the main guide at 266 lines, backed by ten references and three shell templates, including a 717-line command reference and a 413-line authentication guide.

`core` is an opinionated procedure. It makes the agent take a session name scoped to the worktree, so parallel agents do not share one browser. It then teaches a loop of open, snapshot, act on `@eN` refs and snapshot again, and it prefers snapshots over screenshots. Specialized skills cover Electron apps, Slack, exploratory QA with `dogfood`, API clients derived from HAR files, WebMCP tools, Vercel Sandbox, protected Vercel deployments and AWS Bedrock AgentCore browsers.

## Tooling

The product is the tooling. `npm i -g agent-browser` links a prebuilt native binary, `agent-browser install` downloads Chrome for Testing, and Homebrew and `npx` paths also exist. `agent-browser mcp` starts an MCP server over stdio for clients that prefer tools to shell commands.

For agent hosts, the repo ships a Claude Code marketplace file that points at the stub. The `README.md` also documents `npx skills add vercel-labs/agent-browser` and a two-line `AGENTS.md` snippet as alternatives. The stub names Cursor, Claude Code, Codex, Continue and Windsurf as compatible hosts.

Skill quality has its own tests. The `evals/` suite runs Claude Code or Codex through the Vercel AI Gateway and scores skill loading, skill selection, command usage, reference routing and context footprint. CI runs version-sync, launcher, docs, Rust, cross-compilation, native end-to-end, Windows and global install jobs. The release workflow builds binaries for seven platform targets.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [vercel-labs/agent-browser](https://github.com/vercel-labs/agent-browser) |
| Stars / forks | 53.4k / 3.7k |
| Open issues / PRs | 166 / 412 |
| Commits / contributors | 871 / 119 |
| First commit | January 2026 |
| Last 90 days | 233 commits |

As of November 2026 the repo has 53.4k stars and 3.7k forks. Those numbers measure interest in the CLI, and only about a quarter of commits touch skill content. Chris Tate wrote 426 commits, 53% of human-authored work, followed by Railly Hugo at 103 and Lahfir at 77.

Contribution is broad. About 119 people have authored commits, and 808 of 871 commit subjects carry a squash-merged pull request number, reaching #2700. Bots account for 70 commits.

The review queue is the weak point. The tracker holds 412 open pull requests and 166 open issues, while three people wrote about three quarters of human commits. Releases are frequent, with 122 tags in ten months, but outside pull requests compete for a small pool of reviewers.

## Utility for target workloads

This fits engineers who want an agent to drive a real browser from the terminal, such as front-end developers and QA engineers. Concrete tasks include filling and submitting a logged-in form and capturing screenshots for a pull request. Others are running a `dogfood` bug hunt with a structured report, checking unread Slack channels in a browser session and generating an API client from a recorded HAR file.

Prerequisites are light: Node.js 22 or later for the npm package, or Homebrew, plus a Chrome download on first run. Cloud browsers through AgentCore or Vercel Sandbox need those accounts. The evals need an `AI_GATEWAY_API_KEY` for the Vercel AI Gateway, but normal use does not.

The main weakness for a skills catalog is that the installed skill does almost nothing alone. Without the CLI on the `PATH`, the agent gets a page of commands it cannot run. [Cloudflare](/skills/cloudflare) routes browser tasks to its hosted Browser Run product, while agent-browser runs a local browser. Verdict: install the CLI first and the stub second, and evaluate it as a browser tool.
