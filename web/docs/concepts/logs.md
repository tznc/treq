---
sidebar_position: 8
---

# Logs

_How the Logs tab collects check, agent chat, and app setup output in one place._

The Logs tab shows output that Treq records while it works on a repository. It is on by default. You can turn it off under **Settings > Feature Preview**, which hides the tab.

## Sources

The **Log source** menu at the top of the tab picks what to show.

| Source | What it shows |
| --- | --- |
| Checks | Output from workflow checks run in any workspace of the repository, with run number and job. A **Filter by check** menu picks one or more workflows. With none ticked it reads **All checks**. To run checks, use the **Checks** button in the workspace header. See [Workspace Checks](/docs/concepts/checks). |
| Agent | The recorded conversation of one agent terminal. Pick the terminal from the **Agent terminal** menu. Shell terminals are not recorded. |
| App setup | Output from the app setup script set in Application settings, with run number. |

## Browsing logs

Each source shows a log feed. Every line has a timestamp, the source's own columns (run, job, or role), a level, and the message. Errors show in red and warnings in amber.

For checks and app setup logs, **Filter by level** limits the feed to the levels you choose, and the search box matches text across all runs. For checks, **Filter by check** keeps only lines from runs of the workflows you tick, in any workspace. Tick several to see them together. With none ticked it reads **All checks** and shows every workflow. The agent source searches only the selected conversation.

Check logs also show a stacked bar chart above the feed. It counts info, warn, and error lines per second, using the same filters as the feed.

Click a line to open its full text in a details panel. **Multi-select** and **Select all** pick several lines at once. **Send to agent** passes the chosen lines to an agent as a prompt. You can also select text inside a line and use **Send selection to agent**.

## Logs Explorer

For checks, **Logs Explorer** runs SQL against a `logs` view of the recorded records. The view follows the OpenTelemetry log fields, such as `timestamp`, `severityText`, `traceId`, `spanId`, `body`, and `attributes`. Only read-only statements are accepted.

The template menu fills in common queries:

- **Recent lines** lists the newest records across every run.
- **Errors by job** counts error records per job.
- **Severity by run** counts records per run, split by level.
- **Slowest steps** ranks steps by wall-clock time.
- **Trace overview** gives one row per trace and span with error counts.
