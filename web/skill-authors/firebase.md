---
description: "Google's official Firebase agent skills for project setup, Auth, Firestore security rules, SQL Connect, Hosting, AI Logic and Crashlytics."
---

Google's Firebase team maintains `firebase/agent-skills`, 14 skills that teach coding agents to set up and build on Firebase. They cover CLI and project setup, Auth, Firestore and its security rules, SQL Connect, App Hosting, Remote Config, Crashlytics, Analytics, and AI Logic across web, Android, iOS, and Flutter. The repo packages them for six agent hosts and bundles the Firebase MCP server. Install it if you build on Firebase, and treat the security rules skills as its strongest part.

## History

Anna Eilering made the initial commit in January 2026, and two days later Joe Hanley added the first skills with configs for Claude and Gemini CLI. A "launch cut" followed in February 2026, along with a Kiro power, a Cursor plugin, and a token counting script. From March to May 2026 a sync job imported Genkit skills from `genkit-ai/skills` through bot pull requests. In May a commit removed them with their sync workflow, and `firebase-basics` now points Genkit users to that repo.

Consolidation came in waves. Local environment setup merged into `firebase-basics` in March, Firestore Standard and Enterprise merged into one `firebase-firestore` skill with edition detection in April, and a Codex plugin arrived in June. September 2026 brought a `firestore-rules-author` subagent and six commits titled "Fix objectively incorrect" code snippets, CLI flags, or schemas. Cadence holds at 33 commits in the last 90 days.

## Depth

The 14 `SKILL.md` files total 2,242 lines, with a median of 130. Behind them sit 79 supporting files, mostly per-platform setup guides in markdown. The only code is a small Swift package in `xcode-project-setup` that adds Firebase to an Xcode project. Seven skills carry a `-basics` suffix and read that way: `firebase-auth-basics` explains users and identity providers, then hands platform code to five references.

Depth is uneven. `firestore-rules-creation` is the largest skill at 600 lines and has no references. Its four phases scan the codebase for every collection and query, write default-deny rules with validator functions, attack them in a devil's advocate pass, and validate with a dry-run deploy.

The same skill tells the agent to present its rules as a prototype that needs review. `firebase-security-rules-auditor` complements it with a red-team checklist for update bypasses and user-supplied role fields. At the thin end, `firebase-crashlytics` is 48 lines plus three setup references. Most skills are procedures that drive `npx -y firebase-tools@latest` commands.

## Tooling

Install through `npx skills add`, the Gemini CLI extension in `gemini-extension.json`, the Claude Code or Codex plugin marketplaces, Kimi Code, a Kiro power under `kiro/`, or a Cursor plugin. Most of these bundle the Firebase MCP server, which runs through `npx firebase-tools mcp`. The root `.mcp.json` pins it to version 15.32.1, while the Gemini extension uses `latest`.

The repo also ships an agent definition, `agents/firestore-rules-author.md`. `firestore-rules-creation` tells the agent to delegate to it when the host supports subagents and to author rules directly otherwise. The subagent's persona frames the work as a penetration test of the rules.

CI is thin. The single workflow, `format.yml`, runs `mdformat` in check mode over the markdown, and nothing validates frontmatter. One skill shows the gap: the folder `firebase-data-connect-basics` declares `name: firebase-data-connect`. Commit titles mention eval prompts and activation evals, but no eval files or tests exist in the public tree.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [firebase/agent-skills](https://github.com/firebase/agent-skills) |
| Stars / forks | 522 / 118 |
| Open issues / PRs | 13 / 55 |
| Commits / contributors | 114 / 21 |
| First commit | January 2026 |
| Last 90 days | 33 commits |

As of November 2026 the repo has 522 stars and 118 forks, with 13 open issues and 55 open pull requests. Twenty-one people wrote the 82 human commits. Joe Hanley leads with 24 and Charlotte Liang has 16, so no single author dominates, and several contributors commit from google.com addresses.

The public log understates the work. Of the 114 commits, 27 come from `copybara-service[bot]`, which imports changes made outside GitHub, and five from a Google sync bot. A `CODEOWNERS` file added in October 2026 maps each skill to product owners and technical writers, but its own comment says those reviews do not block merges. The 55 open pull requests outnumber the open issues four to one, so review is the bottleneck for anyone who contributes.

## Utility for target workloads

This is for developers building on Firebase with an agent that can run `npx firebase-tools`. Concrete jobs include creating or selecting a project and fetching `google-services.json` through the CLI, and adding Firebase Auth to a web or Flutter app. Others are generating and then red-teaming Firestore security rules, building a SQL Connect schema with GraphQL queries and mutations, and calling Gemini through Firebase AI Logic with App Check.

Prerequisites are a Google account with a Firebase project, Node for `npx`, and the Firebase MCP server for Crashlytics and SQL Connect operations. The `-basics` skills explain concepts and defer code to per-platform references, so an agent reads several files per task. They also tell the agent to fetch app config files through the CLI and not to send the user to the Firebase Console.

Firebase overlaps [Supabase](/skills/supabase) as a backend platform, and SQL Connect on Cloud SQL for PostgreSQL puts it next to [Neon](/skills/neon). Each repo teaches only its own product, so pick by platform. For mobile clients built with Expo, [Expo](/skills/expo) covers the app side and leaves the backend to you.

Verdict: install it on Firebase projects. `firestore-rules-creation` and `firebase-security-rules-auditor` justify the install on their own. Install the set together, since the other skills point back to `firebase-basics` for CLI setup and login.
