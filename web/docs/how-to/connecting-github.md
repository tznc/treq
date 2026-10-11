---
sidebar_position: 5
---

# Connecting GitHub

_Set up GitHub access for pull requests, CI, and Pro repository linking._

Free users connect through the local [GitHub CLI](https://cli.github.com/) (`gh`). Pro users can connect through the Treq GitHub App without installing `gh`. When `gh` is available on a Pro machine, Treq still uses it for some data fetching.

:::note[Work in progress]

The Treq GitHub App is still under development. App-backed flows such as connected-repo management are incomplete.

:::

## Confirm a GitHub Remote

Treq only enables GitHub UI when `origin` is a `github.com` remote:

```bash
git remote -v
```

SSH and HTTPS GitHub URLs both work. Remotes on other hosts keep ordinary [push](/docs/how-to/pushing-to-remote) behavior without Create PR or CI pills.

## Free: Install and Authenticate `gh`

Install the GitHub CLI, then authenticate on the machine that runs Treq:

```bash
gh auth login
gh auth status
```

Without a working `gh` session, Free-tier Create PR, View PR, the GitHub panel, CI status, and inline review threads stay empty or fail when invoked.

## Pro: Sign In and Install the Treq GitHub App

1. Open Settings → Integrations in the desktop app. If you are signed out, choose **Sign in with Browser** and finish the Treq account flow.
2. Choose **Manage GitHub**. That opens the web dashboard Integrations tab.
3. Start the GitHub App install from the dashboard. Treq mints a single-use install intent, then sends you through GitHub's install screen.
4. Pick the organization or user and the repositories the App may access, then authorize the App when GitHub asks.
5. Complete the callback. Treq uses that authorization once to check that your GitHub account can access the installation, then records the installation and repository list.

Back in the desktop app, Connected repositories should list the repos the App can see for your plan. If a repo is missing, re-open Manage GitHub and adjust the App's repository access on GitHub.

Pro users do not need `gh` installed for App-backed connection. If `gh` is present and authenticated, Treq may still use it for some fetches.

## Verify the Connection

1. Open a non-default-branch [workspace](/docs/concepts/workspaces) that already has a commit.
2. Confirm **Create PR** appears in the workspace header, or **View PR** if a PR already exists.
3. Open **GitHub** in the sidebar and load Pull Requests for the repo.

## Next Steps

- [Creating and Viewing Pull Requests](/docs/how-to/creating-and-viewing-pull-requests)
- [GitHub Integration](/docs/concepts/github-integration)
- [Customizing Settings](/docs/how-to/customizing-settings)
- [Security and Privacy](/docs/security-and-privacy)
