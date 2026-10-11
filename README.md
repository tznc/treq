<div align="center">

![treq](./assets/combined-horizontal.png)

</div>

# treq

Treq is the open-source Stacking Agent Development Environment (ADE). It gives each agent its own workspace, stacks branches, and rebases when the base moves.

The goal of Treq is to provide a full suite of high-productivity tooling with the software developer firmly in the driver seat. Features are centered around ensuring high code quality and reducing delegation friction, while integrating with existing development platforms and tooling.

![Treq code overview](./assets/screenshots/code.png)

## Getting Started

Download the latest release [here](https://github.com/tznc/treq/releases/latest). Install steps are in the [installation docs](https://treq.dev/docs/getting-started/installation).

## Features

- Works with [Claude Code, Codex, and Cursor Agent](https://treq.dev/docs/concepts/agent-sessions).
- [Review diffs, commits, and files](https://treq.dev/docs/concepts/changes-and-reviews) locally and send line comments to an agent.
- Gives each agent an [isolated workspace](https://treq.dev/docs/concepts/workspaces) with its own checkout.
- [Auto-rebases](https://treq.dev/docs/concepts/workspaces) dependent workspaces when a target branch moves.
- Lets you [split a feature into a stack](https://treq.dev/docs/tutorials/merging-workspaces) of workspaces that rebase together as you land each one.
- Lets agents [spawn other agents](https://treq.dev/docs/reference/cli#treq-agent) into their own workspaces to split up work.
- [Moves commits, files, and hunks](https://treq.dev/docs/how-to/moving-files-between-workspaces) between workspaces.
- Lets you [break up a large workspace](https://treq.dev/docs/tutorials/managing-workspaces) into smaller stacked or separate ones.
- Resolves [merge conflicts](https://treq.dev/docs/concepts/changes-and-reviews#conflict-management) with an agent, either as a follow-up commit or rewritten in place.
- Exposes workspace management to agents through the [treq CLI](https://treq.dev/docs/reference/cli).
- Agents can send assets as [reviewable artifacts](https://treq.dev/docs/reference/cli#treq-send).
- Handles [commit management](https://treq.dev/docs/concepts/commit-management), reviewing either a single commit or the cumulative stack.
- Lets you [schedule work](https://treq.dev/docs/concepts/workspaces) by hiding a workspace or shifting commit timestamps.
- Manages [GitHub issues and pull requests](https://treq.dev/docs/concepts/github-integration) in the app, including starting an agent from an issue.
- Runs shells and agents in [terminal sessions](https://treq.dev/docs/concepts/terminal-sessions) attached to the workspace, with automatic skill installation.
- [Local by default](https://treq.dev/docs/security-and-privacy), with no code uploaded.
- Sends [no telemetry](https://treq.dev/docs/security-and-privacy#telemetry), privacy focused with application logs kept local.
- Uses [Jujutsu under the hood and is Jujitsu CLI compatible](https://treq.dev/docs/under-the-hood) while staying Git compatible.

## Developer

### Regenerating screenshots

The committed overview shot is `assets/screenshots/code.png`. Site crops are generated in the Deploy Web `landing-screenshots` job and are not committed.

Regenerate locally with `npm run screenshot:readme`. Docs-only CI sets `SKIP_LANDING_SCREENSHOTS=1`.

```bash
npm run screenshot:readme
# same as
npm run screenshot:landing
```

Specs live at `scripts/screenshot/specs/readme-*.spec.tsx`.

### Bumping the version

```bash
make bump           # prompts for the new version
make bump VERSION=0.1.3   # non-interactive
```

Updates `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `Cargo.lock` in one step.

```bash
# to profile benchmark code
samply record cargo bench --bench sync_workspaces
```

## License

Licensed under the Apache License, Version 2.0
