---
description: "Cloudflare's security-audit skill runs a multi-phase, multi-agent source code audit with JSON findings, a coverage ledger and Node validators."
---

Cloudflare publishes `cloudflare/security-audit-skill`, a single skill that turns a coding agent into a source-first security auditor for any codebase. It is unrelated to the platform skills in [Cloudflare](/skills/cloudflare), which teach agents to build on Workers, and it needs no Cloudflare account. The `README.md` says it seeded the multi-stage vulnerability discovery system that Cloudflare describes on its blog. Install it if you want repeatable, evidence-gated audits from an agent that can run parallel sub-agents, and skip it if you want quick lint-style checks.

## History

Dan Jones, who commits as `literally-dan`, made the initial commit in June 2026. It held nine files: `SKILL.md`, guides for reconnaissance, hunting, attack classes and validation, a JSON report schema and a 194-line findings validator. Six days later an outside contributor moved the files under `skills/security-audit/` so `npx skills add` could find them.

July 2026 merged companion files by attack domain. One outside contributor supplied memory-safety, binary and kernel hunting classes, and another added trace step-order validation. Dan Jones added AI and LLM, HTTP protocol and auth, and client-side companions, then rewrote the whole skill into one house style.

The largest change landed in September 2026 as pull request #12, which touched 21 files with 4,792 insertions. It reworked the workflow and findings contract and added a coverage ledger with its own validator and tests. It also added six companions for cloud, data isolation, native apps, protocols, resource exhaustion and supply chain. A commit four days later split guidance mode from full audit mode, and nothing has landed since.

## Depth

This is one skill with real weight behind it. `SKILL.md` runs 192 lines, and 14 companion markdown files add about 1,590 more, led by `HUNTING.md` at 251 lines. A 461-line JSON schema defines three verdicts: `confirmed`, `needs_validation` and `rejected`. Two zero-dependency Node validators, `validate-findings.cjs` and `validate-coverage-ledger.cjs`, total about 1,650 lines.

The skill is an opinionated procedure with six phases. Reconnaissance builds an architecture map and a coverage ledger, hunters work the ledger in waves, and a fresh verifier tries to disprove each candidate. The parent then writes `findings.json`, sends final records to independent verifiers and derives the reports from verified records only.

The rules are strict and specific. The parent alone writes shared files, each sub-agent gets its own scratch directory, and the agent that finds a candidate never validates it. Target code runs only inside an OS-enforced sandbox with no network and hard resource limits, or the lead stays `needs_validation`. Severity applies only to `confirmed` records, and the `quick`, `standard` and `deep` profiles change breadth without lowering the evidence bar.

## Tooling

Beyond markdown, the repo ships the schema, the two validators and their `node:test` suites, which hold 65 test cases between them. The skill tells the parent to run `validate-coverage-ledger.cjs` after each ledger update and `validate-findings.cjs` in phases 4 and 5. The validators need only Node.js.

Installation is `npx skills add` with `--skill security-audit`, plus `--global` for a user-level install. The repo has no plugin manifests for Claude Code, Codex, Cursor or Gemini, no MCP server and no CI workflows, so the tests run only when someone runs them locally. The skill text is agent-neutral and names a parent, a task tool and two delegated roles that you map to your host's sub-agent feature.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [cloudflare/security-audit-skill](https://github.com/cloudflare/security-audit-skill) |
| Stars / forks | 77.0k / 5.2k |
| Open issues / PRs | 55 / 88 |
| Commits / contributors | 14 / 4 |
| First commit | June 2026 |
| Last 90 days | 3 commits |

As of November 2026 the repo shows 77.0k stars and 5.2k forks against 14 commits, five of them merge commits. Four people have authored commits: Dan Jones and three outside contributors with one commit each. Dan Jones merged every pull request.

The ratio is lopsided. Pull request numbers reach #12 and five have merged, while 88 sit open alongside 55 open issues. The last commit is from September 2026, so outside work is piling up faster than a single maintainer reviews it.

Stars measure attention, and this repo drew far more of it than its contributor base can absorb. The 5.2k forks point the same way, with many copies of the skill and few merged changes flowing back. Treat the star count as interest in the method, and judge the skill by its files.

## Utility for target workloads

This skill suits security engineers and teams that want an agent to audit a repository with evidence gates and machine-readable output. Typical workloads are a full audit of a service before launch and a scoped audit of the diff between two refs. It also handles repeat runs that target gaps from earlier ledgers, guidance-mode triage of one suspected bug, and audits of LLM-backed apps through the AI and LLM companion.

Prerequisites are heavy. You need an agent with tool use and parallel sub-agents, Node.js, and an OS-enforced sandbox for any finding that depends on running target code. A full run reserves at least four reconnaissance calls plus critics and verifiers, so token spend grows with the coverage ledger. The `README.md` reports that in Cloudflare's test runs one pass found roughly half of what repeated runs found.

[Sentry](/skills/sentry) overlaps with lighter `security-review` and `gha-security-review` skills that report high-confidence findings in a single pass. Verdict: install this skill for serious audits where false positives cost you time, and bring your own sandbox and budget. For building on Cloudflare products, use [Cloudflare](/skills/cloudflare) instead.
