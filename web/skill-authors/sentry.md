---
description: "Sentry's internal engineering skills for commits, PR writing, CI iteration, code and security review, Django performance and skill authoring."
---

Sentry maintains `getsentry/skills`, a Claude Code plugin of 31 skills and two subagents that its engineers use for daily development work. These skills do not instrument apps with Sentry or debug production errors, and the `README.md` points that work to a separate `getsentry/sentry-for-ai` repo. Install it if you want opinionated workflows for commits, pull requests, CI loops and security review, and leave the Sentry-specific skills out.

## History

David Cramer made the first commit in January 2026 and, the same day, added skills for finding bugs, iterating on pull requests and creating pull requests. Josh Ferge added a `deslop` skill that day too. The repo took its current `getsentry/skills` name on its second day. January closed with a `security-review` skill and Greg Pstrucha's Django performance review.

February 2026 was the busiest month, with 73 commits. It added Django access-control review, a scanner for agent skills, a skill authoring workflow and Warden, which runs the scanner on pull requests. Evan Hicks added two SR&ED tax-incentive skills and a review-request fetcher the same month.

From March 2026 the pace slowed and shifted to consolidation. The canonical `pr-writer` and `skill-writer` names replaced older aliases, `prompt-optimizer` arrived in April, and `@netlify/axis` evals arrived in June. The repo-local Warden workflow was removed in July 2026. September and October added `secret-serialization`, `security-model` and `pr-screenshots`, and the last 90 days hold 39 commits.

## Depth

The 31 skills total 5,081 lines of `SKILL.md`, with a median of 141 and a maximum of 396 in `django-perf-review`. Thirteen ship only a `SKILL.md`, including `commit`, `code-review` and `doc-coauthoring`. Eleven carry a `SPEC.md` that records intent and scope, which the `README.md` asks of new or changed skills.

Most skills are opinionated procedures tied to Sentry's conventions. `commit` is a 62-line rule set for conventional commits with Sentry's type list and `Fixes` footers. In `iterate-pr`, the strongest workflow, four Python scripts fetch CI status, sort review feedback into JSON buckets and post thread replies. The skill fixes high and medium items in a loop until checks pass.

The review skills carry the most reference weight. `security-review` adds 17 vulnerability-class references plus Python, JavaScript and Docker guides, and it requires tracing input to an attacker before reporting. `django-perf-review` ranks N+1 queries and unbounded querysets above micro-optimizations and accepts zero findings. Around ten skills are Sentry-internal, such as `brand-guidelines`, `blog-writing-guide`, `document-api-endpoint`, the `sred-*` pair and `triage-frontend-issues`.

## Tooling

The repo installs as a Claude Code plugin through its own marketplace, and the `README.md` also documents `npx skills add getsentry/skills`. Two skills include `agents/openai.yaml` metadata for Codex, but the repo has no Codex, Cursor or Gemini plugin manifest. The plugin also ships two subagents, `code-simplifier` and `senpai`, a mentor for new Sentry hires.

Scripts are Python and run through `uv`. Beyond `iterate-pr`, `skill-scanner` ships a static scanner for prompt injection, malicious code and excessive permissions, and `skill-writer` ships `quick_validate.py`. `security-model` ships a resolver with a self-test, and `pr-writer` and `skill-writer` carry eval scenarios that run Codex through `npx @netlify/axis`.

The repo has no GitHub Actions workflows. A `warden.toml` asks Warden to run `skill-scanner` on pull requests that touch skills and to fail on high-severity findings. No MCP server ships, and `claude-settings-audit` only suggests a Sentry MCP entry when it detects the Sentry SDK.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [getsentry/skills](https://github.com/getsentry/skills) |
| Stars / forks | 1.2k / 71 |
| Open issues / PRs | 8 / 24 |
| Commits / contributors | 275 / 26 |
| First commit | January 2026 |
| Last 90 days | 39 commits |

As of November 2026 the repo has 1.2k stars and 71 forks. Twenty-six people have authored commits once duplicate identities merge, and David Cramer leads with 88, about a third of human commits. Greg Pstrucha, Priscila Oliveira, Josh Ferge, Evan Hicks and Josh Goldberg follow with 12 to 28 each.

Half of the authors commit from sentry.io addresses, so this is a team repo with a wide internal bench. `CONTRIBUTING.md` makes self-review the default and formal review optional. The log holds 70 merge commits, and two bots, `sentry-junior` and `fix-it-felix-sentry`, wrote 13 commits.

Open pull requests outnumber open issues 24 to 8. The `README.md` describes the skills as tools for Sentry employees and names no support path for outside users. Stars measure attention, and 1.2k stars against 26 contributors is an ordinary ratio for a company team repo.

## Utility for target workloads

Outside Sentry, the useful core is about a dozen general workflow and review skills. Concrete tasks include iterating on a pull request until CI passes and review threads are answered, and writing a pull request description with `pr-writer`. Others are reviewing Django code for N+1 queries or object-level access flaws, auditing GitHub Actions workflows for injection paths, and scanning a third-party skill before you install it.

Prerequisites are the GitHub CLI signed in, `uv` for the Python scripts and a git repo. Several skills hard-code Sentry conventions, such as commit types, branch names and Linear ticket links, so expect to fork and edit them. `claude-settings-audit` generates Claude Code permissions, which ties it to that host.

For security work these are single-pass reviewers with strict reporting rules. [Cloudflare Security](/skills/cloudflare-security) runs a multi-agent audit with validators and a coverage ledger, which suits full audits better. Verdict: install selectively, taking `iterate-pr`, `security-review`, `gha-security-review` and `skill-scanner`, and leave the brand, blog and `sred-*` skills behind.
