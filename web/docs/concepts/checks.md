---
sidebar_position: 9
---

# Workspace Checks

_How Treq runs shell-based check workflows inside a workspace and records pass or fail for each step._

Workspace Checks run the commands you define, such as install, lint, typecheck, and tests, inside a workspace. You write them as YAML workflows in your repository. Each step passes or fails on its exit code, and Treq keeps a run history with the output of every step.

:::note[Preview in v0.3.0]

Checks is a feature preview and is off by default. Turn on **Checks** in Settings under **Feature Preview**. While it is off, Treq refuses to run workflows. Workspace Checks are not part of the v0.2.0 release.

:::

## Defining Workflows

Treq reads every `.yaml` and `.yml` file in `.treq/workflows/` at the root of the repository. A workflow has a `name` and a set of `jobs`. Each job has an optional `name` and a list of `steps`. Each step has a `name` and a `run` command, and may set `working-directory` and `env`.

```yaml
name: CI
jobs:
  verify:
    name: Verify
    steps:
      - name: Install
        run: npm ci
      - name: Typecheck
        run: npm run typecheck
      - name: Unit tests
        run: npm test
        env:
          CI: "true"
```

A file that is not valid YAML in this shape is skipped. `working-directory` must be a relative path inside the workspace and cannot contain `..`. Treq runs each `run` command with `sh -c` in the workspace directory.

## Running Checks

Open a pull request in the GitHub panel. The **Treq checks** section lists each workflow with its jobs and steps. **Run All** runs every job in a workflow. **Run** followed by a job name runs one job. The section appears when the pull request belongs to a Treq workspace.

| Behavior | Detail |
| --- | --- |
| Step order | Steps in a job run one after another. The first failing step stops the job. |
| Job concurrency | **Run All** runs up to 4 jobs at the same time. |
| Step timeout | A step that runs longer than 60 seconds is terminated and counts as failed. |
| Run history | **Run All** records one history entry for the whole workflow. Running a single job records its own entry. |
| Output | Treq captures stdout and stderr for each step and stores it with the run. |

## Trusting a Repository

Workflows run arbitrary shell commands from files in the repository. Treq does not run them until you select **Trust Repository** in the checks section. The trust decision is stored per repository on your machine.

## Setup Script

If `.treq/config.yaml` sets a `setup_script`, Treq runs it when it creates a workspace. Checks wait until that script has finished. If the script fails, the checks section shows **Rerun Setup**, and checks can still run.

## Autosave on Pass

When a run passes and the workspace has uncommitted changes, Treq commits them with a message that starts with `treq-autosave:` and lists up to two changed files. A failing run commits nothing.

## Logs

Check output is stored per run. The **Logs** view reads it when the **Logs** preview is on. Choose **Checks** as the log source to browse and filter it.

## Scope

Workspace Checks run on your machine, inside the workspace. They are separate from GitHub Actions and do not replace your CI. For why agent output needs gates like these, see [Quality Gates for Agent Output](/learn/concepts/ai-engineering/quality-gates-for-agent-output).
