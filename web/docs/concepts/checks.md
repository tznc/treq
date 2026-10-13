---
sidebar_position: 9
---

# Workspace Checks

_How Treq runs shell-based check workflows inside a workspace and records pass or fail for each step._

Workspace Checks run the commands you define, such as install, lint, typecheck, and tests, inside a workspace. You write them as YAML workflows in your repository. Each step passes or fails on its exit code, and Treq keeps a run history with the output of every step.

Checks are on in every build. There is no preview switch.

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

Checks run against one workspace. You can start them from four places:

- **Workspace header.** The **Checks** button shows a summary of the latest runs, such as `Checks 2/3`, `Checks running`, or `Checks not run`. When the branch has a pull request with GitHub checks, the button also shows their count, such as `GH 4/5`. Click it to open a popover. The **Treq checks** section lists each workflow with its latest run, such as `run #3 passed`, and a button that runs or re-runs that workflow. **Run all** runs every workflow, **Setup** re-runs the setup script when one is configured, and **View logs** opens the **Logs** tab with **All checks** selected. The **GitHub checks** section lists the pull request's checks, failed first, and each one opens its run on GitHub. The button is hidden when there are no workflows and no pull request checks.
- **Logs tab.** With **Checks** as the log source, the **Filter by check** dropdown shows the output of one workflow or **All checks**. The Logs tab does not run checks.
- **GitHub panel.** Open a pull request whose head branch has a Treq workspace. The **Treq checks** section lists each workflow and job with its latest status and run buttons. The button next to a job runs that job alone. **Open in Logs** switches to that workspace's Logs tab with **All checks** selected.
- **Command palette.** **Run checks…** runs every workflow in the current workspace. **Run check:** followed by a workflow name runs one workflow. Treq shows a notification when the run finishes.

| Behavior | Detail |
| --- | --- |
| Step order | Steps in a job run one after another. The first failing step stops the job. |
| Job concurrency | Running a workflow runs up to 4 of its jobs at the same time. |
| Step timeout | A step that runs longer than 60 seconds is terminated and counts as failed. |
| Run history | Running a workflow records one history entry. Running a single job records its own entry. |
| Output | Treq captures stdout and stderr for each step and stores it with the run. |

## Trusting a Repository

Workflows run arbitrary shell commands from files in the repository. Treq does not run them until you select **Trust Repository** in the header Checks popover or the GitHub panel, or **Trust repository to run checks** in the command palette. The trust decision is stored per repository on your machine.

## Setup Script

If `.treq/config.yaml` sets a `setup_script`, Treq runs it when it creates a workspace. Checks wait until that script has finished. If the script fails, checks can still run. **Setup** in the header Checks popover, **Re-run setup script** in the GitHub panel, or the command palette runs it again.

## Autosave on Pass

When a run passes and the workspace has uncommitted changes, Treq commits them with a message that starts with `treq-autosave:` and lists up to two changed files. A failing run commits nothing.

## Logs

Check output is stored per run. The **Logs** tab reads it when the **Logs** preview is on. Choose **Checks** as the log source to browse and filter it, or open **Logs Explorer** to query it with SQL. **Filter by check** limits the feed and the chart to the runs of one workflow.

## Scope

Workspace Checks run on your machine, inside the workspace. They are separate from GitHub Actions and do not replace your CI. For why agent output needs gates like these, see [Quality Gates for Agent Output](/learn/concepts/ai-engineering/quality-gates-for-agent-output).
