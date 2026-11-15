---
sidebar_position: 1
description: Install Claude Code in VS Code, give it project context, and use it to plan, build, and test a TypeScript utility.
---

# Setting Up Claude Code

_Install Claude Code in VS Code, give it project context, and use it to build a tested TypeScript utility._

This tutorial takes you through one complete coding task. You will plan the change, review the plan, let Claude edit the files, run tests, and inspect the final diff. That review loop is [human-in-the-loop development](/learn/concepts/ai-engineering/human-in-the-loop-development) in miniature.

## What You Will Build

You will add a `formatDate` utility and its tests to an existing TypeScript project. You will also add project instructions that Claude reads in future sessions.

By the end, your project will contain:

- `src/lib/formatDate.ts`
- `test/formatDate.test.ts`
- `AGENTS.md`
- `CLAUDE.md`

## Before You Begin

You need VS Code 1.98 or later, an Anthropic account with Claude Code access, and a local Git repo. The repo must have a working TypeScript test command.

The VS Code extension includes Claude Code for its chat panel. Install the standalone command-line tool only if you also want to run `claude` in a terminal.

## Install the VS Code Extension

Open the Extensions view with `Cmd+Shift+X` on macOS or `Ctrl+Shift+X` on Windows and Linux. Search for **Claude Code** and install the extension published by Anthropic.

Open a source file, then click the Spark icon in the editor toolbar. You can also open the Command Palette and run **Claude Code: Open in New Tab**. Sign in through the browser when prompted.

If the panel does not appear, run **Developer: Reload Window** from the Command Palette.

## Add Project Instructions

Create `AGENTS.md` at the repo root. Keep shared commands and team rules here so other [coding agents](/learn/concepts/ai-engineering/coding-agents) can use the same source.

```markdown
# Project instructions

## Commands
- Install dependencies: `npm install`
- Run tests: `npm test`
- Type check: `npm run typecheck`

## Code style
- Use TypeScript strict mode
- Keep utility functions small
- Add unit tests for new behavior

## Workflow
- Plan changes that span more than one file
- Run tests before finishing a task
```

Create `CLAUDE.md` beside it. Import the shared file, then add guidance specific to Claude Code.

```markdown
@AGENTS.md

## Claude Code notes
- Place utilities in `src/lib/`
- Match the test patterns in `test/`
- Ask before changing CI or deployment files
```

Claude Code reads `CLAUDE.md` when a session starts. The import keeps shared rules in `AGENTS.md` instead of copying them into two files.

## Plan the Change

Click the mode indicator below the prompt box and choose **Plan**. In Plan mode, Claude explores the repo and prepares a plan before it edits files.

Send this prompt:

```text
Add a formatDate utility at src/lib/formatDate.ts.
It accepts an ISO date string and returns a date such as "Jan 5, 2026".
Add tests at test/formatDate.test.ts for valid and invalid input.
Inspect the existing code and plan the change before editing files.
```

Review the plan as a change contract. Check the target files, public behavior, invalid input case, and test command. Add comments to the plan if Claude made a wrong assumption.

Approve the plan when it matches the task.

## Implement the Plan

Choose **Edit automatically** from the mode indicator. This mode lets Claude edit files without asking for approval on each change. Commands and sensitive actions can still require approval.

Send:

```text
Implement the approved plan. Run the relevant tests and type checks.
```

Watch the edits and command output. Claude should create the utility and test files without changing unrelated code.

## Give Claude Exact Context

Use file references when a follow-up concerns known code. Type `@` and select `src/lib/formatDate.ts`, or select a line range in the editor and press `Option+K` on macOS or `Alt+K` on Windows and Linux.

Ask Claude to refine the invalid input behavior:

```text
@src/lib/formatDate.ts Return null for an invalid date string.
Update the tests to define this behavior.
```

The file reference anchors the request to the code that must change. Claude can still inspect callers and tests when the task requires more context.

## Verify the Result

Run the project's checks yourself:

```bash
npm test
npm run typecheck
```

Read the new tests before trusting the green result. Confirm that they assert the required output and fail when the implementation is wrong.

Test at least these cases:

- A valid ISO date returns the expected text.
- An invalid string returns `null`.
- The function does not depend on the machine's local time zone.

Date formatting often changes across time zones. Use an explicit time zone or a date-only parsing rule in the contract so local settings do not change the output.

## Review the Diff

Switch to **Manual** mode for the final pass. Manual mode asks before file edits and most shell commands.

Review every changed file in VS Code. Check for unrelated edits, weak assertions, new dependencies, and behavior that the plan did not cover. Ask Claude for a summary if the diff is larger than expected:

```text
Summarize each changed file and explain why it changed.
```

The summary helps you navigate. The diff remains the source of truth.

## What You Practiced

Project instructions gave Claude the commands and local rules it needed before the task began. Plan mode exposed its assumptions while changes were still cheap to correct. File references narrowed follow-up work, while tests and diff review checked the result.

Permission modes control when Claude stops for approval. Use Plan while the approach is unsettled, Edit automatically after you accept the direction, and Manual around sensitive work or final review.

## Next Steps

- [AI Feature Development Workflow](/learn/workflows/ai/ai-feature-development): define a bounded feature task.
- [Human-in-the-Loop Review Workflow](/learn/workflows/git/human-in-the-loop-review): choose review checkpoints.
- [What are Coding Agents?](/learn/concepts/ai-engineering/coding-agents): understand how agents plan and act.
- [Claude Code for VS Code](https://code.claude.com/docs/en/vs-code): read the current extension reference.
