---
sidebar_position: 2
---

# Security and Privacy

_How Treq handles your data, what it never sends, and which operations the CLI refuses to perform._

Treq is privacy-focused by default. It is a local desktop application. Repository data, workspace metadata, and settings stay on your machine. The base application does not upload your code or usage data to Treq servers.

## Telemetry

Treq does not collect **telemetry**. The application does not send **feature usage**, **crash reports**, or **performance metrics** to Treq or any third party.

**Local logs** can exist on disk for debugging. They stay on your machine and are not uploaded.

## Local Data

The base Treq app stores nothing on remote servers. Workspace checkouts, review comments, terminal session metadata, and app preferences live in local files on your computer.

Repository history remains in your Git and Jujutsu stores. Treq metadata lives under `.treq` inside the repository and in the app data directory. See [Under the Hood](/docs/under-the-hood) for the storage layout.

When Treq connects to a remote Git host, the connection goes directly to that host. Traffic does not pass through Treq servers.

Optional [GitHub Integration](/docs/concepts/github-integration) uses two extra paths. Pull request, issue, CI, and review-thread actions run through the local `gh` CLI with your GitHub credentials on the machine. Repository linking uses the Treq GitHub App and Treq's backend after you sign in and install the App. Local review comments, application logs, and telemetry all stay locally on disk unless you copy or send them to the Treq developers explicitly.

## CLI Safety

The `treq` CLI is non-destructive. It can create and inspect workspaces, move changes, and start agent sessions. It does not delete workspaces. It does not force-push to remotes.

Workspace deletion stays in the desktop UI, where you confirm context before removing local state. The CLI has no force-push path.

## Website Analytics

The documentation site at treq.dev uses Google Analytics to measure traffic. IP addresses are anonymized before Google records them.

This analytics applies only to the website. Visiting the docs does not connect to your local Treq install or repositories.

## Open Source

Treq is fully open source. You can read every line of the desktop app, CLI, and documentation site in the [public repository](https://github.com/tznc/treq).

Audit the code yourself, or follow the public history of changes. These files are the direct sources for the claims on this page:

| Claim | Source |
|---|---|
| Telemetry stays on disk | [`src-tauri/src/telemetry.rs`](https://github.com/tznc/treq/blob/main/src-tauri/src/telemetry.rs) |
| App and repository databases | [`src-tauri/src/db.rs`](https://github.com/tznc/treq/blob/main/src-tauri/src/db.rs), [`src-tauri/src/local_db.rs`](https://github.com/tznc/treq/blob/main/src-tauri/src/local_db.rs) |
| CLI command set | [`src-tauri/src/cli/mod.rs`](https://github.com/tznc/treq/blob/main/src-tauri/src/cli/mod.rs) |

## Learn More

- [Under the Hood](/docs/under-the-hood)
- [GitHub Integration](/docs/concepts/github-integration)
- [Connecting GitHub](/docs/how-to/connecting-github)
- [CLI](/docs/reference/cli)
- [Contributing](/docs/reference/contributing)
