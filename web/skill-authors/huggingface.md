---
description: "Hugging Face's official agent skills for the Hub: the hf CLI, datasets, TRL and Unsloth training on Jobs, Spaces, Gradio and SageMaker deployment."
---

Hugging Face maintains `huggingface/skills`, a set of 25 agent skills for work on the Hugging Face Hub. They teach an agent the `hf` CLI, the Dataset Viewer API, fine-tuning on Hugging Face Jobs, Spaces and Gradio apps, and model deployment to Amazon SageMaker. Install them if your models, datasets or demos already live on the Hub and you want an agent to drive that work. Skip them if you do not use Hugging Face, since almost every skill assumes a Hub account.

## History

Ben Burtenshaw created the repo in November 2025 with four skills: a dataset creator, an LLM trainer, a model evaluator and a paper publisher. The first weeks also shipped a contributor hackathon, "Humanity's Last Hackathon (of 2025)", whose quests and leaderboard apps still sit under `apps/`. In January 2026 the team moved every skill under `skills/` with `hugging-face-*` names, then renamed them to `huggingface-*` in March 2026.

New skills arrived in batches. Gradio came in February 2026, Transformers.js and two vision trainers in March, and TRL and a LoRA Space builder in June. Six `hf-cloud-*` SageMaker skills followed in July 2026.

Human activity has slowed since spring. Monthly commits fell from 59 in March 2026 to 7 in October, and as of November 2026, 13 of the 16 commits in the last 90 days came from bots. Most of those bot commits regenerate the `hf-cli` skill from a new `huggingface_hub` release. The repo has no tags or changelog, and the version lives in the plugin manifests, currently 1.0.38.

## Depth

The 25 skills under `skills/` hold 6,362 lines of `SKILL.md`, with a median of 207 lines. The largest, `huggingface-llm-trainer`, runs 738 lines and ships ten reference files plus eight PEP 723 scripts for SFT, DPO and `GRPO` training, dataset inspection, cost estimates and `GGUF` conversion. It reads as an opinionated procedure. Submit through the `hf_jobs()` MCP tool, log to Trackio, set timeouts above the 30-minute default, and push results to the Hub before the job ends.

Eight of the 25 skills are a single `SKILL.md` with no supporting files. `hf-cli` is a 243-line command reference generated from `huggingface_hub` v2.4.0, and `hf-mem` is a 31-line wrapper around `uvx hf-mem`. A bot syncs `trl-training` and `train-sentence-transformers` from their upstream library repos, so those libraries own the content. `huggingface-datasets` is a typical mid-size skill, a 107-line map of Dataset Viewer endpoints and upload commands.

The six `hf-cloud-*` skills form a chain. They discover the AWS context, set up Python, check the IAM role and pick a serving image. Then `hf-cloud-sagemaker-production-defaults` deploys through bundled `deploy.py` scripts, with autoscaling and CloudWatch alarms on by default. The chain ships 11 scripts across four of its six skills.

## Tooling

Claude Code installs the repo as a plugin marketplace, but since July 2026 the Claude and Cursor marketplaces expose only `hf-cli`. You install the other skills with `hf skills add <skill-name>`, which reads a full manifest that CI uploads to a Hub bucket. Codex users copy folders into `.agents/skills`, Gemini CLI installs the repo through `gemini-extension.json`, and a generated `agentsmd/AGENTS.md` covers agents without skill support. The Gemini and Cursor manifests also register the hosted Hugging Face MCP server.

The `generate-agents.yml` workflow runs on pull requests. It checks that generated manifests are current with `publish.sh --check`, enforces a version bump when published content changes, and builds the skills index and SEP-2640 distribution artifacts. One test file covers that distribution builder. No skill has its own tests or evals, so the correctness of the training and deployment scripts rests on review alone.

## Community engagement

| Signal | Value |
| --- | --- |
| Repository | [huggingface/skills](https://github.com/huggingface/skills) |
| Stars / forks | 11.4k / 801 |
| Open issues / PRs | 21 / 66 |
| Commits / contributors | 353 / 40 |
| First commit | November 2025 |
| Last 90 days | 16 commits |

This is a team repo with one anchor. Ben Burtenshaw wrote 126 of the 310 human commits, and Shaun Smith added 50, much of it on manifest generation and distribution tooling. The other 38 contributors split between authors who each added one or two skills and outside contributors who sent typo fixes, input sanitization and install docs.

Pull requests do get merged. The log shows 61 merge commits and 89 squash merges with PR numbers, the highest being #303. The queue is the weak spot: 66 PRs are open, while humans made three commits in the last 90 days.

## Utility for target workloads

These skills suit ML engineers and researchers who already publish to the Hub. Concrete jobs include fine-tuning a small model with SFT or DPO on Hugging Face Jobs and checking whether a model fits a GPU with `hf-mem`. You can also filter a dataset through the Dataset Viewer API, build a Gradio demo on Spaces, or deploy a SageMaker endpoint with alarms.

The prerequisites cost money. The skills themselves state that Jobs training and Gradio or Docker Spaces need a paid Hugging Face plan. The SageMaker chain needs an AWS account that can create roles and endpoints. The trainer skill expects the `hf_jobs()` MCP tool and falls back to the `hf jobs` CLI when the tool is missing.

For scientific Python beyond the Hub, [K-Dense AI](/skills/k-dense) covers `transformers`, PyTorch Lightning and many domain libraries, with per-skill tests this repo lacks. The verdict: install `hf-cli` anywhere you touch the Hub, and add the trainer and Spaces skills if you pay for compute there. Read the other skills before you rely on their scripts, because none of them has tests.
