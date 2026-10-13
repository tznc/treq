---
sidebar_position: 6
---

# Creating and Viewing Pull Requests

_Create, open, and inspect GitHub pull requests from a Treq workspace._

Once the repository has a GitHub `origin` remote and GitHub access is set up, each workspace can create and open its pull request without leaving the app. See [Connecting GitHub](/docs/how-to/connecting-github).

## Create a Pull Request

1. Open a workspace that is not the default branch.
2. Make at least one commit. Working-copy changes alone do not enable Create PR.
3. In the workspace header, click **Create PR**.

Treq pushes the branch when it is missing on the remote, then creates the PR. From the header **Create PR** control (including draft and manual compare), the title comes from a conventional-commit derivation of the workspace title or branch name. The body uses the workspace description when one exists.

Use the chevron next to **Create PR** for:

| Action | Result |
|---|---|
| Create draft PR | Same push-and-create path with a draft PR |
| Create PR manually | Push if needed, then open GitHub's compare URL with title and body filled in |

After the PR exists, **Create PR** hides. A toast offers **Open in Web**.

From the Review tab, open the Commit split button and choose **Commit and create PR** to commit, push if needed, and create the PR in one action. That path uses the workspace title or branch name as the PR title and does not run the conventional-commit derivation. **Commit and push** pushes without creating a PR.

## View a Pull Request

Click **View PR** in the workspace header to open the in-app GitHub panel on that PR.

Right-click the workspace in the sidebar and choose **Copy link to GitHub PR** when Treq already has the PR URL.

From the command palette (`Cmd+K`):

| Command | Behavior |
|---|---|
| Open Workspace PR in Browser | Opens the PR URL with the system browser helper |
| Open Workspace PR | Navigates the app webview to the PR URL on GitHub |

## Use the GitHub Panel

Open **GitHub** in the sidebar, then **Pull Requests**. Select Draft, Open, Closed, or All. Draft lists only draft PRs. Click a row to open detail beside the list. From detail you can manage the PR: read the conversation, add comments, change draft state, close or reopen, inspect checks, and open the PR on GitHub.

Use **New** on the PR list when you need a manual title, body, base, and head instead of the workspace Create PR flow. Issues under the same GitHub panel follow the same list and detail pattern.

## Read CI Status

When CI checks are available, Treq shows the status of each check in the PR detail panel. In a workspace whose branch has a pull request, the **Checks** button in the header shows a count such as `GH 4/5`, and its popover has a **GitHub checks** section that lists each check.

Treq does not merge a PR automatically once CI checks pass.

## Read GitHub Review Threads

Treq interleaves GitHub PR comments with code changes when reviewing a branch workspace, allowing users to optionally quote and include external comments when delegating work to agents. Local comments within Treq are for agentic usage. See [Changes and Reviews](/docs/concepts/changes-and-reviews).

## Next Steps

- [Connecting GitHub](/docs/how-to/connecting-github)
- [Code Review Workflow](/docs/tutorials/code-review-workflow)
- [Pushing to Remote](/docs/how-to/pushing-to-remote)
- [GitHub Integration](/docs/concepts/github-integration)
