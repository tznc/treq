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

The Linear integration is a Pro feature. On the Free plan, Settings shows an upgrade prompt in place of the Linear controls, and the app refuses Linear requests. A repository that already has an API key or OAuth connection stops working on Free and resumes once Pro is back; nothing is deleted.

Pro supports two authentication methods:

**Personal API key:** Paste your Linear API key into Settings, stored locally per repository. The key stays on your machine and is never sent to Treq's servers.

**OAuth:** Connect your Linear account. Requests go through Treq's servers, so the raw token never reaches your machine. The server refuses OAuth requests from accounts without Pro. If both an API key and OAuth are set, the API key takes priority.

## Repository Settings

| Setting | Type | Purpose |
|---|---|---|
| `linear_api_key` | String | Personal Linear API key (Pro) |
