---
sidebar_position: 10
---

# Sharing Projects with a Team

_Move a saved project to a Team organization so every member sees it._

A saved project is personal until you move it to a Team organization. Personal projects stay on your computer. A team project syncs to every member of that organization.

## Save a new project to a team

When you save a project, the **Save project** dialog has an **Owner** dropdown above the name. Select the organization's name from it, then select **Save project**. Owner shows only when you belong to a Team organization.

## Move a project to a team

1. Open the project's window.
2. Open the project menu at the top of the sidebar.
3. Select **Move to**, then the organization's name.

Select **Move to** → **Personal** to take a project back. It stays on your computer, and Treq stops syncing it. The copy that members already have stays in the team.

**Move to** shows only after you join a Team organization. See [Cloud Workspaces](/docs/how-to/cloud-workspaces) for how Team billing works.

## Switch between personal and team projects

The **Projects** section on the welcome page has a switcher above the list. Select **Personal** or an organization's name to show that owner's projects. In the sidebar project menu, each organization's projects are in a submenu named after it.

## What syncs

| Field                                | Syncs                               |
| ------------------------------------ | ----------------------------------- |
| Name                                 | Yes                                 |
| Repositories                         | Yes, as `owner/name` or a clone URL |
| Agent instructions and context paths | Yes                                 |
| Local clone folders                  | No                                  |

Each member's computer maps a repository to a clone it already has. If it has none, Treq uses its managed clone folder. Repositories with no GitHub `origin` do not sync.

Treq syncs after you sign in, right after you save or move a project to a team, and every hour. If you leave an organization, Treq removes that organization's projects from your computer at the next sync.

## Next Steps

- [Connecting GitHub](/docs/how-to/connecting-github)
- [Security and Privacy](/docs/security-and-privacy)
