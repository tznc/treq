---
description: "Emil Kowalski's skills: 20 markdown skills for UI animation, gestures, interaction polish and stress-testing components, plus Swift and Expo guides."
---

Emil Kowalski maintains this collection of 20 Agent Skills for designers and engineers. Most of them teach an agent how to decide, build and review UI motion: easing, duration, springs, gestures and choreography. The rest cover interaction polish, worst-case data testing, library choice and modern Swift. The `README.md` says the skills draw on the author's years at companies like Vercel and Linear. Install it if your agent writes interface code and its animations feel slow or generic, and do not expect scripts or automated checks.

## History

The repository started in March 2026 with a single skill, `emil-design-eng`, at 671 lines, plus a short `README.md`. It then sat almost untouched until June 2026, when `review-animations` arrived. From there it grew by nearly one skill a week: `apple-design` and `improve-animations` in July, `animate`, `animate-expo` and `write-swift` in August, and `break-ui` in October.

November 2026 added `choreograph`, `gestures`, `learn-taste` and the router skill `ask-emil`. A commit on November 13 fixed wrong APIs and contradictions between skills. Of the 69 commits, 25 are titled `Update README.md`, and there are no tags or releases. The last 90 days hold 19 commits.

## Depth

The 20 `SKILL.md` files total 4,047 lines, with a median near 170. `emil-design-eng` is the largest at 684 lines and acts as the fallback reference when no narrower skill fits. It sets a frequency rule, no animation for actions seen 100 or more times a day, and an easing decision tree. It also gives duration budgets that keep UI motion under 300ms, with custom cubic-bezier curves to paste in.

The typical skill is an opinionated procedure with hard rules, phases and a required output format. `break-ui` builds a worst-case fixture from realistic values, wires a "Demo data / Worst case" toggle, and reports each break as a table row with a fix. `review-animations` scores a diff against ten numbered standards and tells the agent to flag by default. `learn-taste` turns a session's corrections into rules in `DESIGN.md`, and writes only the rules the user approves.

Supporting material is thin. Only 8 of the 20 skills ship a second file, such as `RECIPES.md` for `animate` and `choreograph`, `CATALOG.md` for `break-ui` and `STANDARDS.md` for `review-animations`. The other 12 are a single `SKILL.md`, and no skill ships a script.

## Tooling

The `README.md` gives one install path, `npx skills@latest add emilkowalski/skills`, and links each skill to a page on `aiforui.dev` with examples and prompts. The repository has no plugin manifest for Claude Code, Codex, Cursor or Gemini, no CI workflow, no tests and no evals. Nothing validates the frontmatter or the links between skills.

Five skills set `disable-model-invocation: true`, so the agent never loads them on its own: `ask-emil`, `learn-taste`, `pick-ui-library`, `prototype` and `review-animations`. You call those by name. Seventeen skills open with a fixed "Initial Response" line and wait for a question before doing anything. `write-swift` exists in `skills/` but is missing from the `README.md` list, so the docs trail the folder.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [emilkowalski/skills](https://github.com/emilkowalski/skills) |
| Stars / forks | 58.8k / 3.5k |
| Open issues / PRs | 3 / not shown |
| Commits / contributors | 69 / 3 |
| First commit | March 2026 |
| Last 90 days | 19 commits |

As of November 2026, the repository has 58.8k stars and three human contributors. Emil Kowalski wrote 67 of the 69 commits under three author names, which the raw metrics count as separate people. The two outside commits are a `.gitattributes` file and a `README.md` typo fix.

This is a one-author collection, and no outside commit has changed a skill. Recent skills land through the author's own pull requests, numbered up to #52. With three open issues, the queue is quiet. The 58.8k stars measure attention, and nothing in the repository tests the skills themselves.

## Utility for target workloads

These skills fit frontend engineers who care how an interface moves and feels. Run `review-animations` on a diff before merging motion code, or `improve-animations` to audit every animation and write prioritized plans another agent can execute. Use `break-ui` to stress a list or card with long names, empty states and huge counts. `pick-ui-library` stops an agent from hand-rolling a toast or dropdown, and `gestures` covers drag, swipe and sheet physics.

The limits are scope and enforcement. Coverage is narrow outside motion, with little on layout, color or page structure beyond `apple-design` and `emil-design-eng`. Every rule is prose, so compliance depends on the model. The recommendations are one person's taste, such as custom curves and specific libraries, stated as rules.

[Impeccable](/skills/impeccable), [Hallmark](/skills/hallmark) and [Taste Skill](/skills/taste-skill) attack AI slop at the page level with linters and browser checks. Emil Kowalski's skills sit underneath them as a motion and interaction layer, and Hallmark's `README.md` lists them as an inspiration. Install them alongside a page-level skill when motion quality matters, and expect to verify the results yourself.
