---
sidebar_position: 6
---

# Devcontainer Environments

:::note[Work in progress]

Devcontainer environments are a Feature Preview. Turn **Devcontainer environments** on in Settings → Feature Preview. Dev builds start with the flag on.

:::

A repository with a `.devcontainer/devcontainer.json` (or `.devcontainer.json`) can run its agent sessions and terminals inside that container instead of on your machine. The Treq window, repository state and workspaces stay local; only the processes move into the container.

## Prerequisites

- A running Docker engine (Docker Desktop or equivalent).
- The Dev Containers CLI on `PATH`: `npm install -g @devcontainers/cli`.
- The agent CLIs you want to run (Claude Code, Codex, and so on) installed and signed in **inside the container**. Add them to your `devcontainer.json`, for example with a Dev Container Feature.

## Switch environment

1. Open the repository and find the environment button in the task composer, next to the agent picker. It reads **Local** by default.
2. Choose **Devcontainer**. Treq runs `devcontainer up` for the repository and shows a spinner until the container is running. The first start builds the image and can take several minutes.
3. Start an agent or open a terminal. New sessions in every workspace of the repository now start inside the container.

The same picker sits in the terminal pane controls. Switching back to **Local** affects only new sessions; sessions already running stay where they started. The choice is saved per repository.

## How it works

Treq starts its own container for the repository, labelled `treq.repo=<repository path>`, separate from any container your editor opened for the same folder. It bind-mounts two host directories at the same paths inside the container:

- the repository, which contains every Treq workspace under `.treq/workspaces/`
- the system temp directory, where Treq writes agent prompt and settings files

Because the paths match, Treq passes host paths to the container unchanged. Each terminal runs `devcontainer exec` with a login shell in the session's working directory.

## Limitations

- The `treq` CLI is not installed in the container, so agents cannot run `treq` commands there.
- Remote SSH repositories ignore this setting.
- Treq does not stop the container. Stop it with `docker stop` when you are done.
