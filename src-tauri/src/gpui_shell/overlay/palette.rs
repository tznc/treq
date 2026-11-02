//! `CommandPalette`'s commands: which show, in React's order, with their
//! copy and icons. Most run Dashboard handlers, so a pick goes back to React
//! as a `palette.<id>` press; opening the PR in the browser runs here.

use serde::Deserialize;

use super::data::{PaletteChecks, PrLink};
use crate::gpui_shell::{
  actions::{
    self, Action, GO_TO_FILE, MAXIMIZE_TERMINAL, NEW_AGENT_SESSION, NEW_AGENT_WITH_PROMPT,
    NEW_SHELL, TOGGLE_TERMINAL,
  },
  theme,
};

/// What React sends for the palette: what it knows of the Dashboard.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  /// `dataRepoPath`; empty with no repository open.
  pub repo_path: String,
  /// "View stashed changes" is offered (not for a remote repository).
  pub stash: bool,
  /// A workspace is selected, so the terminal commands show.
  pub has_selected_workspace: bool,
  /// `currentWorkspace`, for its checks and pull request.
  pub workspace: Option<RouteWorkspace>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteWorkspace {
  pub id: i64,
  /// The workspace's own `repo_path`, which its checks run in.
  pub repo_path: String,
  pub branch: String,
}

/// One `CommandItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
  pub id: String,
  pub label: String,
  pub description: Option<String>,
  pub icon: &'static str,
  /// Its shortcut's keys, empty for none, as the shortcuts sheet shows them.
  pub keys: &'static [&'static str],
}

impl Command {
  fn new(id: &str, label: &str, description: Option<&str>, icon: &'static str) -> Self {
    Self {
      id: id.into(),
      label: label.into(),
      description: description.map(Into::into),
      icon,
      keys: &[],
    }
  }

  /// A command for one of the app's named actions, with its shortcut.
  fn action(id: &str, action: Action, icon: &'static str) -> Self {
    Self {
      id: id.into(),
      label: action.label.into(),
      description: action.description.map(Into::into),
      icon,
      keys: action.keys,
    }
  }
}

/// Opens the workspace's pull request in the browser; GPUI runs it.
pub const OPEN_PR_IN_BROWSER: &str = "open-workspace-pr-browser";
/// Opens the pull request too: in the browser, as there is no in-app page.
pub const OPEN_PR_HERE: &str = "open-workspace-pr-current-window";

/// The palette's commands, as `CommandPalette` builds them from
/// Dashboard's props, `useChecksCommands` and `usePrInfoViaGh`.
pub fn commands(
  route: &Route,
  checks: Option<&PaletteChecks>,
  pr: Option<&PrLink>,
) -> Vec<Command> {
  let mut list = vec![
    Command::new("dashboard", "Go to home", None, theme::ICON_HOUSE),
    Command::new("settings", "Go to settings", None, theme::ICON_SETTINGS),
    Command::new(
      "open-repository",
      "Open repository",
      Some("Open a different repository"),
      theme::ICON_FOLDER_OPEN,
    ),
    Command::new(
      "open-repository-new-window",
      "Open repository in new window",
      Some("Open a repository in a separate window"),
      theme::ICON_APP_WINDOW,
    ),
  ];
  if !route.repo_path.is_empty() {
    list.extend([
      Command::new(
        "switch-branch",
        "Switch branch",
        Some("Switch the home repository's branch"),
        theme::ICON_BRANCH,
      ),
      Command::action("search-files", GO_TO_FILE, theme::ICON_FILE_SEARCH),
      Command::new(
        "go-to-workspace",
        "Go to workspace",
        Some("Open and filter workspaces"),
        theme::ICON_BRANCH,
      ),
      Command::action("create-workspace", actions::NEW_WORKSPACE, theme::ICON_PLUS),
      Command::new(
        "delete-workspace",
        "Delete workspace",
        Some("Delete a workspace"),
        theme::ICON_TRASH,
      ),
      Command::action(
        "start-agent-with-prompt",
        NEW_AGENT_WITH_PROMPT,
        theme::ICON_AGENT_CLAUDE,
      ),
      Command::action(
        "start-agent-terminal",
        NEW_AGENT_SESSION,
        theme::ICON_AGENT_CLAUDE,
      ),
      Command::new(
        "open-prompt-history",
        "View prompt history",
        Some("See every prompt ever sent, labeled by workspace"),
        theme::ICON_HISTORY,
      ),
    ]);
    if route.stash {
      list.push(Command::new(
        "open-stash",
        "View stashed changes",
        Some("Browse and apply immutable stashed change sets"),
        theme::ICON_ARCHIVE,
      ));
    }
  }
  if route.has_selected_workspace {
    list.extend([
      Command::action(
        "toggle-terminal",
        TOGGLE_TERMINAL,
        theme::ICON_CHEVRONS_UP_DOWN,
      ),
      Command::action("maximize-terminal", MAXIMIZE_TERMINAL, theme::ICON_MAXIMIZE),
      Command::action("new-shell-terminal", NEW_SHELL, theme::ICON_TERMINAL),
    ]);
  }
  if route.workspace.is_some() {
    if let Some(checks) = checks {
      list.extend(checks_commands(checks));
    }
    if let Some(pr) = pr {
      list.push(Command {
        id: OPEN_PR_IN_BROWSER.into(),
        label: "Open Workspace PR in Browser".into(),
        description: Some(format!("Open PR #{} on GitHub", pr.number)),
        icon: theme::ICON_EXTERNAL_LINK,
        keys: &[],
      });
      list.push(Command {
        id: OPEN_PR_HERE.into(),
        label: "Open Workspace PR".into(),
        description: Some(format!("Navigate to PR #{}", pr.number)),
        icon: theme::ICON_APP_WINDOW,
        keys: &[],
      });
    }
  }
  list
}

/// `useChecksCommands`.
fn checks_commands(checks: &PaletteChecks) -> Vec<Command> {
  let mut list = Vec::new();
  let workflows = &checks.workflows;
  if !workflows.is_empty() && !checks.trusted {
    list.push(Command::new(
      "trust-repo-checks",
      "Trust repository to run checks",
      Some("Allow workflows in .treq/workflows to run"),
      theme::ICON_SHIELD_CHECK,
    ));
  }
  let can_run = checks.trusted && !checks.setup_running;
  if can_run && !workflows.is_empty() {
    list.push(Command {
      id: "run-all-checks".into(),
      label: "Run checks…".into(),
      description: Some(format!(
        "Run all {} workflows in this workspace",
        workflows.len()
      )),
      icon: theme::ICON_LIST_CHECKS,
      keys: &[],
    });
    for (filename, name) in workflows {
      list.push(Command {
        id: format!("run-check-{filename}"),
        label: format!("Run check: {name}"),
        description: Some(filename.clone()),
        icon: theme::ICON_PLAY,
        keys: &[],
      });
    }
  }
  if checks.setup_configured {
    list.push(Command::new(
      "rerun-setup-script",
      "Re-run setup script",
      Some("Run the workspace setup script again"),
      theme::ICON_ROTATE_CW,
    ));
  }
  list
}

#[cfg(test)]
mod tests {
  use super::*;

  fn route(repo_path: &str) -> Route {
    Route {
      repo_path: repo_path.into(),
      stash: true,
      has_selected_workspace: false,
      workspace: None,
    }
  }

  fn ids(list: &[Command]) -> Vec<&str> {
    list.iter().map(|c| c.id.as_str()).collect()
  }

  #[test]
  fn lists_only_navigation_without_a_repository() {
    assert_eq!(
      ids(&commands(&route(""), None, None)),
      [
        "dashboard",
        "settings",
        "open-repository",
        "open-repository-new-window"
      ]
    );
  }

  #[test]
  fn lists_repository_commands_in_react_order() {
    let list = commands(&route("/repo"), None, None);
    assert_eq!(
      ids(&list)[4..],
      [
        "switch-branch",
        "search-files",
        "go-to-workspace",
        "create-workspace",
        "delete-workspace",
        "start-agent-with-prompt",
        "start-agent-terminal",
        "open-prompt-history",
        "open-stash"
      ]
    );
    assert_eq!(
      list[4].description.as_deref(),
      Some("Switch the home repository's branch")
    );
  }

  fn find<'a>(list: &'a [Command], id: &str) -> &'a Command {
    list.iter().find(|c| c.id == id).expect(id)
  }

  #[test]
  fn names_commands_as_the_action_table_does() {
    use crate::gpui_shell::actions;
    let route = Route {
      has_selected_workspace: true,
      ..route("/repo")
    };
    let list = commands(&route, None, None);
    for (id, action) in [
      ("search-files", actions::GO_TO_FILE),
      ("create-workspace", actions::NEW_WORKSPACE),
      ("start-agent-terminal", actions::NEW_AGENT_SESSION),
      ("start-agent-with-prompt", actions::NEW_AGENT_WITH_PROMPT),
      ("new-shell-terminal", actions::NEW_SHELL),
      ("toggle-terminal", actions::TOGGLE_TERMINAL),
      ("maximize-terminal", actions::MAXIMIZE_TERMINAL),
    ] {
      let command = find(&list, id);
      assert_eq!(command.label, action.label, "{id}");
      assert_eq!(command.keys, action.keys, "{id}");
    }
    assert!(find(&list, "dashboard").keys.is_empty());
  }

  #[test]
  fn hides_the_stash_for_a_remote_repository() {
    let route = Route {
      stash: false,
      ..route("/repo")
    };
    assert!(!ids(&commands(&route, None, None)).contains(&"open-stash"));
  }

  #[test]
  fn adds_terminal_checks_and_pr_commands_for_a_workspace() {
    let route = Route {
      has_selected_workspace: true,
      workspace: Some(RouteWorkspace {
        id: 3,
        repo_path: "/repo".into(),
        branch: "feat/alpha".into(),
      }),
      ..route("/repo")
    };
    let checks = PaletteChecks {
      workflows: vec![("ci.yml".into(), "CI".into())],
      trusted: true,
      setup_configured: true,
      setup_running: false,
    };
    let pr = PrLink {
      number: 12,
      url: "https://github.com/o/r/pull/12".into(),
    };
    let list = commands(&route, Some(&checks), Some(&pr));
    assert_eq!(
      ids(&list)[13..],
      [
        "toggle-terminal",
        "maximize-terminal",
        "new-shell-terminal",
        "run-all-checks",
        "run-check-ci.yml",
        "rerun-setup-script",
        OPEN_PR_IN_BROWSER,
        OPEN_PR_HERE
      ]
    );
    let run = &list[16];
    assert!(run.keys.is_empty());
    assert_eq!(run.label, "Run checks…");
    assert_eq!(
      run.description.as_deref(),
      Some("Run all 1 workflows in this workspace")
    );
    assert_eq!(list[17].label, "Run check: CI");
    assert_eq!(
      list[19].description.as_deref(),
      Some("Open PR #12 on GitHub")
    );
  }

  #[test]
  fn asks_to_trust_the_repository_before_running_checks() {
    let route = Route {
      workspace: Some(RouteWorkspace {
        id: 3,
        repo_path: "/repo".into(),
        branch: "feat/alpha".into(),
      }),
      ..route("/repo")
    };
    let mut checks = PaletteChecks {
      workflows: vec![("ci.yml".into(), "CI".into())],
      trusted: false,
      setup_configured: false,
      setup_running: false,
    };
    let list = commands(&route, Some(&checks), None);
    assert_eq!(ids(&list).last(), Some(&"trust-repo-checks"));
    checks.trusted = true;
    checks.setup_running = true;
    let list = commands(&route, Some(&checks), None);
    assert!(!ids(&list).iter().any(|id| id.starts_with("run-")));
  }
}
