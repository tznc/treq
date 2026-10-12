---
sidebar_position: 7
---

# Linear Integration

_How Treq connects workspaces to Linear issues and creates workspaces from them._

Treq's Linear integration links workspaces to Linear issues, letting you view and manage issues from within Treq and kick off a workspace directly from an issue without leaving the app.

## Viewing Issues

Open the Linear panel in the workspace sidebar to list Linear issues. The list view shows all team issues with their status, assignee, and labels. You can view issue details, including the description, sub-issues, and full context.

## Kicking Off Workspaces from Issues

When you view a Linear issue, a **Create Workspace** action immediately kicks off a new workspace using the issue's suggested branch name, title, and description. Workspaces created this way stay linked to the issue.

If the issue has sub-issues, you can optionally include them as separate nested workspaces in a single action. Each sub-issue becomes a child workspace in the stack.

## Authentication

Treq supports two authentication methods for Linear:

**Free tier:** Paste your personal Linear API key into Settings and store it locally per repository. Your key stays on your machine and is never sent to Treq's servers.

**Pro tier:** Use OAuth2 to connect your Linear account. Authentication is proxied through Treq's servers, so the raw API token never reaches your browser or local machine. If both an API key and OAuth token are set, the API key takes priority.

## Repository Settings

| Setting | Type | Purpose |
|---|---|---|
| `linear_api_key` | String | Personal Linear API key (free tier) |

## Availability

| Features | Free | Pro |
|---|---|---|
| View Linear issues | ✅* | ✅ |
| Create workspace from issue | ✅* | ✅ |
| Include sub-issues as nested workspaces | ✅* | ✅ |

\* Requires personal Linear API key
