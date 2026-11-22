---
description: "Expo's official agent skills for building, upgrading and shipping Expo and React Native apps, from Expo Router to EAS Build, Update and simulators."
---

The Expo team maintains `expo/skills`, 23 skills for agents that build, ship, and operate Expo and React Native apps. The skills split into open-source framework work, such as Expo Router, native UI, and data fetching, and paid Expo Application Services, such as store submission, over-the-air updates, and remote simulators. Thirty-one people have contributed, and CI enforces size limits, cost notices, and routing. Install it if you build mobile apps with Expo.

## History

James Ide started the repo in November 2025 with one EAS CI/CD workflows skill, then added a release script for a stable branch in December. In January 2026 Evan Bacon restructured it for the Claude Code plugin marketplace, and Brent Vatne renamed the project from "Expo Router Skills" to "Expo Skills". A run of skills followed for SDK upgrades, native tabs, and `expo-av` migration. In March 2026 Kudo Chien merged several plugins into a single `expo` plugin.

Mid-2026 added infrastructure: the Expo MCP server in May, then an eval skill and CI limits on skill size in June. In July a rename sorted skills under `expo-*` and `eas-*` prefixes. Since then much of the work has gone into paid services, and 27 commit subjects name `eas-simulator` alone. Cadence is steady, with 43 commits in the last 90 days and four on November 20, 2026.

## Depth

The 23 `SKILL.md` files total 4,824 lines, with a median of 182. `expo-data-fetching` is the largest at 477 lines and `eas-observe` the smallest at 49. Behind them sit 64 reference files, three helper scripts, and an `agents/openai.yaml` per skill for Codex. Five skills, including `expo-dom` and `eas-hosting`, have no reference files.

The skills are opinionated procedures. `expo-overview` is a router that tells the agent to classify the user's goal before loading a leaf skill. `expo-data-fetching` prefers `expo/fetch` over axios and requires every data screen to design loading, error, empty, and content states. `expo-router` fixes file naming and route rules, and `eas-observe` is a short index into five references.

Every services skill opens with an "EAS service - costs apply" callout, and CI enforces it. One defect shows: `expo-overview` still routes Tailwind work to `expo-tailwind-setup`, a skill removed in August 2026. The CI routing check confirms that every skill appears in the router, but it does not confirm that every router entry still exists.

## Tooling

Claude Code users install `expo@claude-plugins-official` and Codex users `expo@openai-curated`, so both hosts pull from their curated marketplaces. Other agents use `npx skills@latest add expo/skills --skill '*'`. The `expo` plugin bundles the hosted Expo MCP server, and a separate `expo-experiments` plugin carries one skill for unfinished APIs.

The `check.yml` workflow runs Bun scripts that cap descriptions at 1,024 characters and bodies at 500 lines. The same scripts require the paid-service callout, reject bare EAS commands in runnable examples, check router coverage, and check plugin version bumps. A second workflow comments install commands on each pull request.

Evals run on EAS Workflows against a private `eval-harness` submodule, only for pull requests labeled `eval`, and never block a merge. The repo-local `expo-skill-eval` skill adds scripts that snapshot iOS, Android, and web builds for a visual grader. Each skill ends with a feedback command, `npx --yes @expo/agent-cli@latest feedback`, which the agent may run only after the user agrees.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [expo/skills](https://github.com/expo/skills) |
| Stars / forks | 4.2k / 279 |
| Open issues / PRs | 31 / 74 |
| Commits / contributors | 164 / 31 |
| First commit | November 2025 |
| Last 90 days | 43 commits |

As of November 2026 the repo has 4.2k stars, 279 forks, 31 open issues, and 74 open pull requests. Authorship is spread wide. Evan Bacon has 23 commits under two author names, Kudo Chien 22, Gabe Debes 17, and James Ide 14, so no one holds more than 15 percent of the total.

Several authors commit from expo.dev or expo.io addresses, and the log also holds one-commit fixes such as a Cursor install note and a YAML quoting fix for descriptions. Changes land as squashed pull requests, with numbers past #300. The 74 open pull requests are more than double the open issues and outnumber the 43 commits of the last 90 days. That backlog is the main caution for contributors, though the merge rate shows review is active.

## Utility for target workloads

This is for teams building iOS and Android apps with Expo, or adding React Native to an existing native app. Concrete jobs include building Expo Router screens with native tabs, sheets, and headers, and upgrading an app across Expo SDK versions. Others are writing a native module in Swift or Kotlin, submitting builds to TestFlight and Google Play, and driving a remote simulator from a Linux machine with `eas-simulator`.

The 16 `expo-*` framework skills need only an Expo project. The seven `eas-*` services skills need an Expo account, and each opens with a note that costs apply. Store submission also needs Apple or Google developer accounts.

Backend work is out of scope. `eas-hosting` itself lists cloud databases such as Supabase and Neon for API routes, and suggests Firebase or Supabase for managed backends. Pair it with [Firebase](/skills/firebase), [Supabase](/skills/supabase), or [Neon](/skills/neon) for auth and data. Those repos teach their backends, not Expo Router, EAS, or native modules.

Verdict: install it for any Expo app. Start with `expo-overview`, which routes the agent to the right leaf skill. Skip the `eas-*` skills if you build and submit without EAS.
