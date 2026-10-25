---
sidebar_position: 2
---

# Customizing Settings

_Personalize Treq to match your preferences and workflow._

Open settings with the gear icon (⚙️) in the sidebar. Settings split into repository, application, account, integrations, skills, and Feature Preview. Repository settings apply to the current Git repository. Application settings apply everywhere. Both are saved when you save them, but repository settings live in the `.treq` directory and are lost if you delete it.

The **Skills** tab browses the Treq skill registry when **Skills installation** is on in Feature Preview. **Install…** opens a dialog where you choose application or repository storage. Treq copies installed skills into new workspaces. See [Installing Skills from the Library](/docs/how-to/installing-skills).

## Feature Preview

Feature Preview lists experimental features. Each title opens the docs page for that feature. The switch stores your choice in application settings.

| Feature | Docs |
| --- | --- |
| Skills installation | [Installing Skills from the Library](/docs/how-to/installing-skills) |
| Remote SSH | [Remote SSH Workspaces](/docs/how-to/remote-ssh-workspaces) |
| Logs (on by default) | [Logs](/docs/concepts/logs) |

Release builds start from the shipped default. Dev builds start with every preview feature on. Turning a switch off hides the UI and rejects the matching backend commands.

## Repository Settings

**Auto-push to remote** pushes after every commit in this repository when enabled. It is off by default. Turn it on when you want each commit on the remote without a separate push step. Create PR still pushes on its own when the branch is missing remotely.

**Branch naming pattern** builds branch names from variables.

| Variable | Value |
| --- | --- |
| `{name}` | The intent taken from your plan |
| `{user}` | Your git username |
| `{date}` | Today's date as `YYYY-MM-DD` |

Common patterns are `feature/{name}`, `dev/{user}`, and `bugfix/{name}-{date}`.

**Copy files** copies specific files listed in `.gitignore`, such as `.env`, into each new workspace. Use it for small config files that should be independent copies.

**Symlink from home repo** is a per-workspace create option under Advanced. Pick heavy directories such as `node_modules/`, `target/`, or `.venv/` so the new workspace links them from the home working copy instead of copying. The dialog suggests root paths from `.gitignore`.

## Terminal Settings

| Setting | Options |
| --- | --- |
| Font size | 12-16px recommended |
| Default shell | Auto-detect, `bash`, `zsh`, `fish`, or a custom path |
| Scrollback buffer | 1000-10000 lines |
| Cursor style | Block, underline, or bar |
| Cursor blink | On or off |

Add shell arguments such as `--login` or `-i` for interactive sessions.

## Appearance

| Setting | Options |
| --- | --- |
| Theme | Light, dark, or system |
| UI density | Compact, normal, or comfortable |
| Font scaling | 12-18px, for interface elements only |

Font scaling does not affect the terminal. Custom themes are planned for a future release.

## Diff Viewer

Toggle line numbers, the minimap, word wrap, and whitespace visibility. Pick a syntax highlighting theme from GitHub, VS Code, Monokai, or the Solarized variants.

## Git Preferences

**Commit settings** turn on auto-stage, which we do not recommend, set a commit message template, and add validation rules such as a maximum length or a requirement for conventional commits.

**Merge settings** choose the default strategy, the [conflict style](/learn/concepts/git/zdiff3), and whether Treq stashes your work before an operation.

| Setting | Options |
| --- | --- |
| Default strategy | Regular, squash, no-ff, or ff-only |
| Conflict style | Standard or diff3 |
| Auto-stash | On or off |

[Merge vs Rebase](/learn/concepts/git/merge-vs-rebase) explains what each strategy does to your history.

## Performance

Set **file watching** to ignore the paths you never need, such as `node_modules/`, `.git/`, and `dist/`, and set the polling interval between 100 and 1000ms. The **git cache** holds 100 to 1000 entries, and you can clear it to force a refresh. For large repositories, consider shallow [clones](/learn/concepts/git/git-worktrees-vs-clones), sparse checkout, or LFS support.

## Account

Settings → Account shows the signed-in Treq account, its plan, and the cloud workspace. **Sign Out** ends the session on this computer.

To delete the account, select **Delete Account** in the **Delete account** section, type your email in the **Delete your Treq account?** dialog, and select **Delete Account**. Your Pro subscription is cancelled at once, without a refund, and your [cloud workspace](/docs/how-to/cloud-workspaces) machine and disk are deleted. Treq keeps billing records. Repositories and workspaces on your computer stay as they are. While you are the last owner of any organization, the dialog refuses: make another member an owner, or delete the organization, first.

## Integrations

Settings → Integrations manages the Treq account link to the GitHub App for Pro users. Sign in with the browser, open **Manage GitHub** to install or adjust the Treq GitHub App, and review connected repositories.

Day-to-day create PR, CI, and review-thread actions on Free still use the local `gh` CLI. See [Connecting GitHub](/docs/how-to/connecting-github).

## Notifications

**Notify when an agent finishes** is in the Application tab and is on by default. Treq shows a system notification when an agent runs [`treq notify`](/docs/reference/cli#treq-notify) or exits on its own. It only does this while no Treq window is focused, so you hear about agents when you have switched away. **Save Settings** applies the change.

## Privacy

Anonymous usage data covers feature statistics, error reports, and performance metrics. You can turn it off in Privacy settings. Plan history is stored in `.treq/plans/`. **Clear All Data** resets everything, and it cannot be undone.

## Advanced

**Developer mode** turns on debug logs and experimental features, which may be unstable. The **update channel** is stable, beta for early features, or nightly for the latest and least tested build. Database operations cover backup, restore, and rebuild.

## Import/Export

Export your settings as JSON from Settings → Advanced → Export to share them with the team or keep a backup. To import, select a JSON file and choose which settings to apply. You can reset one category or all of them from Advanced settings.
