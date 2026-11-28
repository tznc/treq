---
description: "Google's official Agent Skills for Google Cloud, Ads and Analytics: over 200 skills for GKE, BigQuery, IAM, SecOps, Agent Platform and gcloud work."
---

`google/skills` is Google's official repository of Agent Skills for its products, and Google Cloud dominates it. It holds more than 200 skills, most of them under `skills/cloud/`, covering GKE, BigQuery, Spanner, IAM, Cloud Monitoring, SecOps and Google's Agent Platform. Most of its commits are exported from Google's internal source tree, and it does not accept outside pull requests. Install it if you run workloads on Google Cloud and want an agent that knows current `gcloud` syntax and product gotchas.

## History

The repo started in April 2026 with a run of commits from the Cloud IX Team account, many titled "No public description". By the end of that month it held 13 skills and had gone through two restructures that flattened skill folders and nested `cloud/` under a top-level `skills/` directory. The commit trailers explain the pattern. Of the 645 commits, 633 carry a `PiperOrigin-RevId` line and most come from a Copybara export address, so the public repo mirrors an internal one.

Growth since then has been fast. The skill count reached 69 by June 2026, 127 by August, 178 by October and 219 by late November. Monthly commits rose from 28 in April 2026 to 131 in the first 20 days of November. In July 2026 the team began adding Data Cloud plugins as git submodules, now 16, that point at separate GoogleCloudPlatform repositories.

The repo took 369 commits in the last 90 days, far more than the Gemini and Stitch repos. Recent ones are detailed fixes, such as a November 2026 commit that corrects GKE ComputeClasses version gates. There are no tags, but every skill carries a `metadata.version` field.

## Depth

The median `SKILL.md` runs 240 lines. Of the 219 skills at the end of November 2026, 214 ship a `references/` folder, 48 add scripts and five are a lone `SKILL.md`. Most are procedures with product opinions that hand off to each other by name. `gke-basics` defaults to Autopilot, requires CPU requests in 250m steps and sends upgrade questions to `gke-upgrades`.

The largest skill, `agent-platform-tuning`, runs 881 lines and works as a gated decision tree. It stops tool execution until you name a project and region, and it forbids the agent from proposing a new storage bucket. At the small end, `google-mobile-ads-banner` is 47 lines that pick a banner type and send the agent to an Android, iOS or Unity reference. `gcloud` opens by telling the agent its memory of `gcloud` flags is stale and requires a `gcloud help` lookup before any command.

One pattern runs through 188 skills: a `references/attribution.md` file. It tells the agent to prefix each `gcloud` call with `CLOUDSDK_METRICS_REQUEST_ATTRIBUTION` and to append an `agent-skills/1.0` tag to `User-Agent` headers. The file says the tag carries only the skill name and version, but you should know your agent's traffic is labeled.

## Tooling

The readme's main install path is `npx skills add google/skills`, which copies skills into the agent of your choice. Beyond plain skills, `.claude-plugin/marketplace.json` and `.agents/plugins/marketplace.json` list 17 plugins for Claude Code and Codex, and the readme gives an `agy plugin install` command for Antigravity. The in-repo `google-cloud-developer` plugin also ships `gemini-extension.json` and configures the Developer Knowledge MCP server, which needs an API key.

CI is thin. The only workflow runs the lychee link checker every Monday and files broken links to an issue instead of failing. Forty-two `_test.py` files sit inside skill scripts, such as those for `agent-platform-alert-configuration`, but no workflow runs them.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [google/skills](https://github.com/google/skills) |
| Stars / forks | 24.5k / 2.0k |
| Open issues / PRs | 41 / 1 |
| Commits / contributors | 645 / 7 |
| First commit | April 2026 |
| Last 90 days | 369 commits |

As of November 2026 the seven named contributors understate who writes this repo. The Cloud IX Team export account wrote 621 of the 645 commits, so the author list says nothing about how many internal teams feed it. The named people, all with google.com addresses or merge rights, account for the other 24, led by Holt Skinner with nine, who also commits to the Gemini repo.

`CONTRIBUTING.md` states that the repo does not accept external pull requests and that skills go through internal review. Feedback goes through issues, and 41 are open against 24.5k stars, which is a light load. You cannot see review discussion, since changes land as exported commits.

## Utility for target workloads

This repo is for engineers and SREs who run production work on Google Cloud. It helps with tasks such as creating a private GKE Autopilot cluster, triaging a pending GKE workload against capacity limits, and tuning BigQuery queries. It also covers writing Logging and PromQL queries for alerts and investigating a SecOps alert. The 15 Ads skills cover the Google Mobile Ads SDK, the IMA SDK, the Google Ads API and the Data Manager API.

Prerequisites are a Google Cloud project with billing, the `gcloud` CLI and the IAM roles each skill names. Some skills, such as `gke-basics`, document a tool order across Google's MCP servers, `gcloud` and `kubectl`. Outside Google Cloud the repo offers little, and at more than 200 skills it rewards picking one by one.

Two other Google teams publish separate skills repos. [Google Gemini](/skills/gemini) covers the Gemini API from AI Studio and moved its Vertex AI skill here as `gemini-api`. [Google Stitch](/skills/stitch) covers the Labs design tool Stitch and has no overlap with this repo. Verdict: install the GKE, BigQuery, IAM and observability skills you need if Google Cloud is your platform, and skip the repo otherwise.
