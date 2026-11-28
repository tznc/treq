---
description: "K-Dense AI's 177 scientific agent skills for bioinformatics, cheminformatics, clinical research, scientific databases and research writing, with tests."
---

K-Dense Inc., which also publishes an open-source desktop co-scientist built on these skills, maintains `scientific-agent-skills`, a library of 177 skills for research work. Each skill covers one scientific package, database, platform or method, from `scanpy` and `rdkit` to ClinicalTrials.gov lookups and manuscript audits. It ships per-skill test suites and builds an isolated environment for each skill under test. Install a topical subset if you do computational biology, chemistry or clinical research, and leave it alone if you do not.

## History

Timothy Kassis started the repo as Claude Scientific Skills in October 2025. By the end of its first day it held 66 skills in three folders: `scientific-databases`, `scientific-packages` and `scientific-thinking`. In November 2025 the folders merged into one flat `scientific-skills/` tree, and in March 2026 the separate database skills collapsed into a single `database-lookup` skill.

April 2026 brought the rename to Scientific Agent Skills and the removal of the Claude-only marketplace file. The tree moved to `skills/` in May 2026, and the root became an Agent Plugins package in August. Release 2.72.0 in October 2026 dropped the vendored Anthropic `docx`, `pdf`, `pptx` and `xlsx` skills, taking the count from 181 to 177.

The repo has 109 tags, from v1.50.0 in October 2025 to v2.72.0. As of November 2026, the last human commit is dated October 1, 2026. Every commit since then is the weekly security-scan report from a bot.

## Depth

The 177 skills hold 49,653 lines of `SKILL.md`, with a median of 277 and a maximum of 499. CI warns past 500 lines, so long material sits in `references/`. All but one skill ship supporting files, the `skills/` tree holds 2,025 files, and 122 skills include scripts. Every skill records a `last-reviewed` date in its frontmatter, and 173 declare their environment needs in a `compatibility` field.

`scanpy` is typical of the package skills. Its 360 lines state data contracts, such as requiring raw counts as input, then route the agent to 14 tested CLI scripts that chain from QC to pseudobulk. `database-lookup` pairs a retrieval contract with 82 reference files covering 80 databases, and it tells the agent to treat API responses as untrusted data. `scientific-writing` adds offline CLIs that audit claims, check references and lint a manuscript.

Scope and licensing vary at the edges. `what-if-oracle` and `consciousness-council`, both contributed by AHK Strategies, are prompt-only methods with no code. `what-if-oracle` carries a CC BY-NC-SA 4.0 license, and `deepspot-m` uses PolyForm Noncommercial, so the repo's MIT license does not cover every skill.

## Tooling

You install with `npx skills add`, with `gh skill install` and a pinned tag, or by loading the root `plugin.json` as an Agent Plugins package in Cursor or Codex. The `README.md` also documents Hermes taps and manual clones into `.agents/skills`. There is no Claude Code marketplace file, so Claude Code users rely on `npx skills add`, `gh skill` or a manual copy.

Four of its five workflows guard the tree. `skill-spec-validation.yml` runs `skills-ref validate` on every skill and checks the repo's frontmatter rules. `skill-tests.yml` runs a structural guard that fails any skill with scripts but no suite, then tests changed skills in throwaway `uv` environments built from `tests/skill-requirements.toml`. `pr-skill-scan.yml` runs Cisco's skill scanner on changed skills and fails on high findings, and a weekly job publishes `docs/security-report.md`.

That report, generated November 16, 2026, marks 18 of 177 skills unsafe, nine at critical and nine at high. The flagged set includes `scientific-slides`, `literature-review` and `modal`. `AGENTS.md` lists known false-positive classes, so read each finding before you discard or trust a skill.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [K-Dense-AI/scientific-agent-skills](https://github.com/K-Dense-AI/scientific-agent-skills) |
| Stars / forks | 58.1k / 5.1k |
| Open issues / PRs | 13 / 83 |
| Commits / contributors | 765 / 57 |
| First commit | October 2025 |
| Last 90 days | 69 commits |

One maintainer carries the repo. Timothy Kassis wrote 558 of the 724 human commits, and the next contributor has 25. The log shows 56 merged pull requests from 40 fork owners, plus a September 2026 batch of squash merges that Kassis followed with fix-up commits.

The queue has stalled since then. There are 83 open PRs against 13 open issues, and no human commit has landed in seven weeks. The `README.md` asks readers to star the repo and says community skills may get less review than K-Dense's own. The 58.1k stars measure reach and say little about review capacity.

## Utility for target workloads

This library suits computational scientists who already script in Python. It helps you run single-cell QC and clustering with the `scanpy` scripts, and pull ChEMBL, UniProt or PubChem records with logged provenance through `database-lookup`. It also covers cheminformatics with `rdkit` and `datamol`, PK/PD modelling with `pkpd-modeling`, and pre-submission manuscript checks with `scientific-writing`.

Plan for setup. Repo tooling needs Python 3.13 and `uv`, and `AGENTS.md` warns that skill dependencies conflict, so you need one environment per workflow. Integration skills need their own accounts, such as a Benchling tenant or Exa, OpenRouter and Rowan API accounts.

Overlap with other sources is narrow. [Hugging Face](/skills/huggingface) owns model training and publishing on the Hub, and `consciousness-council` resembles the persona panel in [Corey Haines](/skills/coreyhaines)'s `marketing-council`. The verdict: install only the skills for your domain, check each skill's license and scan result, and run its tests before you trust its scripts.
