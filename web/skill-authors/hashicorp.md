---
description: "HashiCorp's official Terraform and Packer agent skills cover HCL style, modules, provider development, tests, Stacks, policy and image builds."
---

HashiCorp maintains `hashicorp/agent-skills`, 20 skills split into a `terraform` bundle of 16 and a `packer` bundle of four. The skills are long reference guides with HCL and Go examples, aimed at writing Terraform configuration, developing providers and building machine images. Install the Terraform bundle if you write modules or providers, and treat the Packer bundle as a set of starting templates.

## History

A HashiCorp service account created the repo in November 2025, and Gautam pushed the first content three days later. The early layout was organized per coding agent, with docs for Kiro, Q, Gemini and Claude Code. A December 2025 refactor reorganized folders by use case, and eight days later the repo pivoted to Terraform only.

January 2026 brought provider-development skills from Baraa Basata, a restructure for the `npx skills add` pattern and the return of Packer as its own plugin. From February to April 2026 the team added Terraform Search import, provider test patterns and provider docs skills, plus a CI job that scored skills with Tessl. Policy-as-code skills for the `tfpolicy` engine in HCP Terraform arrived in July 2026.

Version 1.0.0 shipped in August 2026. Six days later a governance pull request folded six legacy plugin IDs into the `terraform` and `packer` bundles, removed the Tessl workflows and limited contributions to HashiCorp staff. Since then the work is maintenance: `tfpolicy` version updates and, in November 2026, root manifests that let Kiro install each bundle as a Power.

## Depth

These are the longest skills in this group. The 20 `SKILL.md` files total 5,976 lines with a median of 301, and `azure-verified-modules` reaches 615. Most are opinionated reference guides that pair rules with HCL or Go examples, and nine of the 20 ship only a `SKILL.md`.

`terraform-style-guide` is typical: a file layout table, a full example configuration, naming rules and a security section, built on HashiCorp's public style guide. `azure-verified-modules` encodes the Azure Verified Modules certification rules, from variable ordering to `lifecycle` syntax. `run-acceptance-tests` is short and procedural. It covers `TF_ACC=1`, explicit timeouts and cumulative debug steps, then a "flip" check that edits an assertion to prove a passing test can fail.

`terraform-policy` carries the most supporting material: three references and 16 worked conversions from Sentinel to `.policy.hcl`, each beside its original policy. It tracks `tfpolicy` CLI versions 0.3 through 0.5 and tells the agent to check the installed version first. Its troubleshooting block still prints `npx skills add` paths that the August 2026 consolidation removed.

## Tooling

Each product root carries a Claude Code manifest, a Codex manifest and an Agent Plugins `plugin.json`, and the repo root has Claude Code and Codex marketplace files. The `README.md` documents `claude plugin install terraform@hashicorp`, Codex repository marketplaces, Kiro Powers and per-skill `npx skills add` paths. Two skills carry small extras: Go scaffold files in `new-terraform-provider` and a shell script in `terraform-search-import`.

There is no MCP server and no helper CLI. CI runs one workflow with two jobs. `validate-structure.sh` checks manifest JSON, marketplace entries, frontmatter names and lifecycle status, and that `SKILLS.md` lists exactly 20 skills with a `CODEOWNERS` owner for each. `check-links.py` validates local and external markdown links.

Evaluation moved out of public view. The changelog says the Waza evaluation assets now live in a private repository, and the structure check fails any change that adds public eval files. `SUPPORTED_MODELS.md` names the Claude and GPT models the team evaluates against, but you cannot rerun those evals.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [hashicorp/agent-skills](https://github.com/hashicorp/agent-skills) |
| Stars / forks | 936 / 137 |
| Open issues / PRs | 11 / 1 |
| Commits / contributors | 69 / 18 |
| First commit | November 2025 |
| Last 90 days | 6 commits |

As of November 2026 the repo has 936 stars and 137 forks. Eighteen people have authored commits, and Gautam leads with 28 of the 65 human commits. Every pull request is squash-merged, so the log has no merge commits, and pull request numbers reach #104.

The authors are mostly HashiCorp staff, plus a few early outside contributors such as a CI fix from a Tessl contributor. Since August 2026 `CONTRIBUTING.md` rejects general external contributions and points internal contributors to a private guide. Five issue templates and a `CODEOWNERS` entry for every skill route reports to named owners.

One open pull request and 11 open issues show a tracker that stays clear. Cadence is slow, with six commits in the last 90 days, half of them `tfpolicy` updates. The star count is modest for an official vendor repo, and outside contributors have no path to merge changes.

## Utility for target workloads

The Terraform bundle serves module authors, who get skills such as `terraform-style-guide`, `terraform-test` and `refactor-module`, and provider developers, who get nine skills from scaffolding to acceptance runs. Concrete tasks include migrating a provider from Plugin SDKv2 to the Plugin Framework and writing `.tftest.hcl` tests with mock providers. Others are bulk imports with Terraform Search, converting a Sentinel library to `tfpolicy`, and building an AMI with the `amazon-ebs` builder.

Prerequisites depend on the skill. `terraform-search-import` requires Terraform 1.14 or newer, Stacks and `tfpolicy` target HCP Terraform, and acceptance tests create real, billable infrastructure. With no MCP server, the agent relies on the skill text and your local CLIs.

The Packer bundle is thin by comparison: four single-file skills built around templates for AWS, Azure, Windows and the HCP Packer registry. The [Microsoft](/skills/microsoft) plugin deploys to Azure from the Azure side, and `azure-verified-modules` complements it for module authors. Verdict: install the Terraform bundle if you maintain modules or providers, and pick Packer skills only if their templates match your builders.
