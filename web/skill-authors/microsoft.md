---
description: "Microsoft's Azure Skills plugin pairs MCP-backed skills for preparing, validating and deploying Azure apps with diagnostics, cost and Foundry work."
---

Microsoft publishes `microsoft/azure-skills`, the Azure Skills Plugin, which installs Azure skills together with the Azure MCP Server. The repo is a distribution mirror: a bot syncs skill content from Microsoft's `GitHub-Copilot-for-Azure` repo, and this repo adds manifests, install docs and release tags. Install it if you deploy or operate Azure workloads with an agent and accept a Node-launched MCP server and default-on telemetry.

## History

Fan Yang created the repo in February 2026, and the first bot sync from `GitHub-Copilot-for-Azure` merged the same day. March and April 2026 were the busiest months, with 77 and 87 commits, three quarters of them syncs and merges. Maintainers added VS Code support, swapped early symlinks for copies, and documented installs for Cursor, Gemini CLI, IntelliJ and Codex.

In late April 2026 the plugin was renamed to `azure` and gained an `apm.yml` for Microsoft's APM installer. A May 2026 workflow started cutting a GitHub release after each sync, and the first tag, `v1.1.37`, dates from May 7. An Astro landing site followed in May, and an installation test workflow arrived in October.

The log reflects the mirror model. Of 325 commits, 163 are titled "Sync plugin files from GitHub-Copilot-for-Azure" and 176 come from bots. Since late May 2026 a sync bot commits directly, so 70 of the 75 commits in the last 90 days are automated. The repo has 100 tags, up to `v1.2.133` on November 20, 2026.

## Depth

The catalog indexes the root `skills/` folder, a byte-identical copy of the `azure-skills` plugin payload. It holds 28 top-level skills and 35 `SKILL.md` files, because `azure-kubernetes` and `microsoft-foundry` nest seven sub-skills. Behind them sit 1,026 files, including 104 Python, shell and PowerShell scripts. The median `SKILL.md` is 106 lines, and the largest, `azure-reliability`, is 387.

These are opinionated procedures bound to MCP tools. `azure-prepare` forces a plan file at `.azure/deployment-plan.md` before any code, then hands off to `azure-validate` and `azure-deploy`, which read that file. `azure-reliability` checks Functions and App Service apps for zone redundancy and offers a CLI fix path or a Bicep or Terraform patch path. `microsoft-foundry`, with 231 files, begins by running a dependency script that installs missing tools.

Depth is uneven. `azure-messaging` and `azure-kusto` ship only a `SKILL.md`, and `azure-messaging` points to troubleshooting guides stored in `azure-diagnostics`. The tone is loud, and 11 of the 28 top-level files use `MANDATORY` or stop-sign markers to force step order. Seven more plugins in `.github/plugins`, including `azure-cost` and `aks-skills`, sit in the marketplace but outside the catalog roots.

## Tooling

The repo ships plugin files for Claude Code, Copilot CLI, VS Code, Cursor and Gemini CLI, plus the `apm.yml`. The `README.md` adds Codex marketplace steps, IntelliJ steps and an `npx skills add` path. Claude Code users install `azure@claude-plugins-official`.

The MCP config starts one server through `npx`, pinned to `@azure/mcp@4.0.0-beta.7`. The `README.md` claims more than 200 tools across more than 40 services and also describes a Foundry MCP, which `microsoft-foundry` reaches through the Azure MCP `foundry` tool. Hook files for Claude Code, Copilot and Cursor run a telemetry script at session start and after tool calls, and `AZURE_MCP_COLLECT_TELEMETRY=false` turns it off.

CI has three workflows. One cuts a GitHub release from the plugin version and changelog, and one deploys the landing site. The third installs Copilot CLI and Claude Code on each pull request and verifies that every plugin installs. Skill evals do not live here, and the `eval` folders under `azure-prepare` belong to Azure Functions templates.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [microsoft/azure-skills](https://github.com/microsoft/azure-skills) |
| Stars / forks | 1.7k / 272 |
| Open issues / PRs | 7 / 2 |
| Commits / contributors | 325 / 14 |
| First commit | February 2026 |
| Last 90 days | 75 commits |

As of November 2026 the repo has 1.7k stars and 272 forks. Fourteen people have authored commits once duplicate identities merge, and Fan Yang leads with 56. Most human commits merge sync pull requests or edit install docs, so this log says little about who writes the skills.

Skill authorship happens upstream. Each changelog entry links a pull request in `GitHub-Copilot-for-Azure`, where numbers pass #3500, while this repo's reach #306. Outside contributions here are rare, such as a symlink path fix merged from a contributor's fork in March 2026.

Seven open issues and two open pull requests make for a quiet tracker. File skill behavior bugs against the upstream repo, since fixes land there and reach this repo through the sync. The star count reflects interest in Azure's official plugin more than activity in this mirror.

## Utility for target workloads

This plugin targets developers and platform engineers who ship to Azure from an agent. It generates `azure.yaml`, Bicep or Terraform and Dockerfiles for an `azd` project, then validates and deploys it. Other tasks include diagnosing Container Apps or Functions failures from logs and resource health, listing resources across subscriptions with Resource Graph, and checking quotas or zone redundancy before a release.

Prerequisites are an Azure subscription, Node.js 18 or later for `npx`, the Azure CLI signed in with `az login`, and the Azure Developer CLI for deploy workflows. Several skills need Reader or Contributor rights on the target scope. Telemetry is on by default, so set the opt-out variable before first use if your policy requires it.

The trade-offs are size and voice. Skills route to each other, so the plugin works best installed whole, and forced plan-first steps add turns to small changes. [HashiCorp](/skills/hashicorp) covers Azure Terraform module standards in `azure-verified-modules`, while this plugin acts on live resources. Verdict: install it if Azure is your primary cloud, review the hooks first, and skip it for one-off Azure questions.
