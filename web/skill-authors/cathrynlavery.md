---
description: "Cathryn Lavery's diagram-design skill draws 51 diagram types as branded, self-contained HTML and SVG files, and redraws Mermaid or draw.io sources."
---

Cathryn Lavery maintains `diagram-design`, one Agent Skill that turns a plain request into an editorial diagram saved as a single HTML file with inline SVG. `PRIVACY.md` names LittleMight as the publisher. The skill covers 51 diagram types, matches your brand from a website, and redraws existing Mermaid, draw.io, Excalidraw and PlantUML diagrams. Install it if you publish architecture or concept diagrams in posts, docs or slides and want them to look designed.

## History

The first commit, in April 2026, was a v1.0 release of the skill. Claude Code plugin manifests followed three days later, and outside contributors added Codex support and fixed the plugin layout that same month. In July 2026 the maintainer merged curated work from the `aristide/diagram-design` fork as version 2.0. That merge added 12 diagram types for 26 in total, plus an icon set, PNG and SVG export and a second onboarding flow.

August 2026 brought 110 commits, including draw.io import, Pi packaging and sequence diagram fragments. The 2.6 line started on August 20, and the manifests now read 2.6.170 because a workflow bumps them after every merge. October 2026 is the busiest month with 202 commits. The last commit is from November 22, 2026, and there are no git tags.

## Depth

There is one skill, but it is large. `SKILL.md` runs 385 lines and routes to 70 reference files totalling about 10,400 lines, including one `type-*.md` file per diagram type. The `assets/` folder holds 242 files, mostly worked examples and five templates, and `scripts/` ships eight Python helpers for import, export and checking.

The rules are opinionated and measurable. A diagram gets at most 9 nodes, 12 arrows and 2 accent-colored elements, and structural geometry sits on a 4px grid. Six connector rules ban diagonal lines and labels touching their stroke, and `verify_geometry.py` checks them. A table names "AI slop" schematics: dark mode with cyan glow, shadows, `rounded-2xl` boxes and blanket monospace.

Before the first diagram in a project, the skill checks whether `style-guide.md` still holds the default tokens. If so, it stops and offers onboarding from a website URL, a local design system or pasted tokens. Imports take four settings for format, size, detail and audience, and each ends with a ledger of what was merged or dropped. The docs drift in one spot, since `SKILL.md` opens with "Fifty visual types" while its own guide lists 51.

## Tooling

`npx skills add cathrynlavery/diagram-design` installs the skill for any Agent Skills host. Native install paths exist for Claude Code, Codex, GitHub Copilot, Factory Droid, Qoder and Pi, and the `README.md` documents Kiro and OpenCode installs. Seven commands in `commands/` handle export, the four importers, saved brand profiles and a `doctor` check. PNG export needs Python and Playwright with Chromium.

`ci.yml` runs `claude plugin validate . --strict`, rejects manifest version changes in pull requests, and runs a lint and verify matrix on Linux, Windows and macOS. That matrix checks skin conformance, text contrast, connector geometry, every importer and per-type verifiers for charts such as Sankey and treemap. The `scripts/` folder holds 111 entries, most of them verifiers and their `test-*.py` counterparts. `pages.yml` publishes the gallery, and `PRIVACY.md` states the skill has no server or telemetry.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [cathrynlavery/diagram-design](https://github.com/cathrynlavery/diagram-design) |
| Stars / forks | 70.2k / 4.7k |
| Open issues / PRs | 18 / 27 |
| Commits / contributors | 477 / 74 |
| First commit | April 2026 |
| Last 90 days | 348 commits |

As of November 2026, the repository has 70.2k stars and 74 human contributors. Bots made 160 of the 477 commits, nearly all of them automatic version bumps. Cathryn Lavery wrote 143 of the remaining 317, or 45%, and outside authors wrote the rest. Rudy Mizrahi Celekli, Matt Van Horn and Praveen Bishnoi lead them with 16 to 27 commits each.

That spread is real collaboration by the standards of this group. Pull request references run past #530, and the July fork merge credits its outside author as co-author. `CONTRIBUTING.md` asks for an issue before any non-trivial change and caps contributors at five open pull requests. Of the 348 commits in the last 90 days, 160 are bot bumps, so human activity is closer to 188.

## Utility for target workloads

The skill fits engineers and writers who publish diagrams. Ask for an architecture diagram of an app, a quadrant of projects by impact and effort, or a sequence diagram of token refresh on a 401. Redraw a Mermaid block from a `README.md` at slide size, or export a diagram to PNG for a deck. Brand profiles let one install serve several clients through a `.diagram-design` marker file.

The `README.md` states the trade-offs plainly. Output quality depends on the model, the same prompt can lay out differently twice, and the HTML is not a diffable source format. For diagrams that live in Git and change every sprint, it recommends Mermaid. Default output loads fonts from Google Fonts unless you ask for system fonts.

None of the other sources in this group draws diagrams. [Impeccable](/skills/impeccable), [Hallmark](/skills/hallmark) and [Taste Skill](/skills/taste-skill) share its stance against generated defaults, but they target web pages. [Remotion](/skills/remotion) covers motion graphics as video. Install `diagram-design` for publishable diagrams, and keep Mermaid for docs that change weekly.
