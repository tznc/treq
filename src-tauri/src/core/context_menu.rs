//! The right-click and dropdown menus of the workspace sidebar and header,
//! as data (`lib/native-context-menu.ts`): which items show, in which
//! order, and what each one does. The GPUI shell draws them and runs the
//! picked item's [`MenuAction`].

use crate::local_db::Workspace;

/// The lucide icon beside an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuIcon {
  Archive,
  Copy,
  FolderOpen,
  FolderPlus,
  FolderSearch,
  GitBranch,
  Link,
  Pencil,
  RotateCcw,
  Settings,
  Trash,
}

/// What picking an item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
  /// Puts the text on the clipboard.
  Copy(String),
  /// Shows the path in the file manager.
  Reveal(String),
  /// Opens a URL, such as an editor's `cursor://file/...`.
  OpenUrl(String),
  /// The window's branch switcher.
  OpenBranchSwitcher,
  AddRepository,
  OpenRepoSettings(String),
  LocateRepo(String),
  RemoveRepo(String),
  /// The rename dialog for a workspace of the repository.
  RenameWorkspace {
    repo_path: String,
    workspace_id: i64,
  },
  ArchiveWorkspace {
    repo_path: String,
    workspace_id: i64,
  },
  /// Archives several workspaces of the repository at once.
  ArchiveWorkspaces {
    repo_path: String,
    workspace_ids: Vec<i64>,
  },
  /// Runs each action in turn, such as opening several paths.
  Each(Vec<MenuAction>),
  DeleteWorkspace {
    repo_path: String,
    workspace_id: i64,
  },
  /// Points the workspace's branch back at `origin`, dropping local work.
  ResetToRemote,
  /// The workspace header's Create PR menu.
  CreatePr {
    draft: bool,
  },
  CreatePrManually,
  /// Shows saved project `id`: focuses its window or opens a new one.
  SwitchProject(String),
  /// Saves (or renames) this window's project.
  SaveProject,
  /// Edits the saved project's agent instructions and context.
  ProjectContext,
  /// Moves this window's saved project to a Team organization, or back to
  /// personal with `None`.
  SetProjectOwner(Option<String>),
  /// A new window on the welcome page, to open another project or repository.
  NewWindow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
  Item {
    text: String,
    icon: Option<MenuIcon>,
    destructive: bool,
    action: MenuAction,
  },
  Separator,
  Submenu {
    text: String,
    icon: Option<MenuIcon>,
    items: Vec<MenuItem>,
  },
}

impl MenuItem {
  pub fn item(text: &str, action: MenuAction) -> Self {
    MenuItem::Item {
      text: text.into(),
      icon: None,
      destructive: false,
      action,
    }
  }

  fn icon(mut self, value: MenuIcon) -> Self {
    if let MenuItem::Item { icon, .. } | MenuItem::Submenu { icon, .. } = &mut self {
      *icon = Some(value);
    }
    self
  }

  fn destructive(mut self) -> Self {
    if let MenuItem::Item { destructive, .. } = &mut self {
      *destructive = true;
    }
    self
  }
}

/// Installed editors, which "Open in…" lists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorApps {
  pub cursor: bool,
  pub vscode: bool,
  pub zed: bool,
}

/// The item that shows a path in the platform's file manager.
pub const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
  "Open in Finder"
} else if cfg!(target_os = "windows") {
  "Open in File Explorer"
} else {
  "Open in File Manager"
};

/// `pathMenuEntries`: copy the path, or open it.
pub fn path_items(relative_path: &str, full_path: &str, editors: EditorApps) -> Vec<MenuItem> {
  let mut open = vec![
    MenuItem::item(REVEAL_LABEL, MenuAction::Reveal(full_path.into())).icon(MenuIcon::FolderOpen),
  ];
  for (on, label, scheme) in [
    (editors.cursor, "Open in Cursor", "cursor"),
    (editors.vscode, "Open in VSCode", "vscode"),
    (editors.zed, "Open in Zed", "zed"),
  ] {
    if on {
      open.push(MenuItem::item(
        label,
        MenuAction::OpenUrl(format!("{scheme}://file/{full_path}")),
      ));
    }
  }
  vec![
    MenuItem::item("Copy relative path", MenuAction::Copy(relative_path.into()))
      .icon(MenuIcon::Copy),
    MenuItem::item("Copy full path", MenuAction::Copy(full_path.into())).icon(MenuIcon::Copy),
    MenuItem::Submenu {
      text: "Open in…".into(),
      icon: Some(MenuIcon::FolderOpen),
      items: open,
    },
  ]
}

/// Which of the home row's repository items apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HomeMenu {
  pub repo_path: String,
  /// The folder is gone: no path items.
  pub missing: bool,
  /// The repository on screen, whose branch can be switched.
  pub on_screen: bool,
  /// A supporting repository (not the window's main one).
  pub supporting: bool,
  /// The window can let this repository go.
  pub removable: bool,
}

/// `homeMenuEntries`, with the items `useGpuiSidebar` passes handlers for.
pub fn home_items(menu: &HomeMenu, editors: EditorApps) -> Vec<MenuItem> {
  let path = &menu.repo_path;
  let mut items = Vec::new();
  if menu.on_screen {
    items.push(
      MenuItem::item("Switch Branch...", MenuAction::OpenBranchSwitcher).icon(MenuIcon::GitBranch),
    );
    items.push(MenuItem::Separator);
  }
  if !menu.missing {
    items.extend(path_items(".", path, editors));
  }
  let mut repo = Vec::new();
  if !menu.supporting {
    repo.push(
      MenuItem::item("Add Repository…", MenuAction::AddRepository).icon(MenuIcon::FolderPlus),
    );
  }
  if menu.supporting && !menu.missing {
    repo.push(
      MenuItem::item(
        "Repository Settings",
        MenuAction::OpenRepoSettings(path.clone()),
      )
      .icon(MenuIcon::Settings),
    );
  }
  if menu.supporting && menu.missing {
    repo.push(
      MenuItem::item("Locate…", MenuAction::LocateRepo(path.clone())).icon(MenuIcon::FolderSearch),
    );
  }
  if menu.removable {
    repo.push(
      MenuItem::item("Remove Repository", MenuAction::RemoveRepo(path.clone()))
        .icon(MenuIcon::Trash)
        .destructive(),
    );
  }
  if !repo.is_empty() {
    items.push(MenuItem::Separator);
    items.extend(repo);
  }
  items
}

/// `getFullWorkspacePath`.
pub fn full_workspace_path(workspace: &Workspace) -> String {
  if workspace.workspace_path.starts_with('/') {
    workspace.workspace_path.clone()
  } else {
    format!(
      "{}/.treq/workspaces/{}",
      workspace.repo_path, workspace.workspace_path
    )
  }
}

/// `workspaceRelativePath`: where a workspace lives, relative to its
/// repository.
pub fn workspace_relative_path(workspace_path: &str, repo_path: &str) -> String {
  if !workspace_path.starts_with('/') {
    return format!(".treq/workspaces/{workspace_path}");
  }
  match workspace_path
    .strip_prefix(repo_path)
    .and_then(|rest| rest.strip_prefix('/'))
  {
    Some(rest) if !repo_path.is_empty() => rest.to_string(),
    _ => workspace_path.to_string(),
  }
}

/// `workspaceMenuEntries`. `archive` is false for a repository that is not
/// on screen.
pub fn workspace_items(
  workspace: &Workspace,
  repo_path: &str,
  pr_url: Option<&str>,
  editors: EditorApps,
  archive: bool,
) -> Vec<MenuItem> {
  let mut items = vec![MenuItem::item(
    "Copy branch name",
    MenuAction::Copy(workspace.branch_name.clone()),
  )
  .icon(MenuIcon::GitBranch)];
  if let Some(url) = pr_url {
    items.push(
      MenuItem::item("Copy link to GitHub PR", MenuAction::Copy(url.into())).icon(MenuIcon::Link),
    );
  }
  items.push(
    MenuItem::item(
      "Rename workspace",
      MenuAction::RenameWorkspace {
        repo_path: repo_path.into(),
        workspace_id: workspace.id,
      },
    )
    .icon(MenuIcon::Pencil),
  );
  items.push(MenuItem::Separator);
  items.extend(path_items(
    &workspace_relative_path(&workspace.workspace_path, repo_path),
    &full_workspace_path(workspace),
    editors,
  ));
  if archive {
    items.push(MenuItem::Separator);
    items.push(
      MenuItem::item(
        "Archive Workspace",
        MenuAction::ArchiveWorkspace {
          repo_path: workspace.repo_path.clone(),
          workspace_id: workspace.id,
        },
      )
      .icon(MenuIcon::Archive)
      .destructive(),
    );
  }
  items
}

/// The menu for several selected workspaces of `repo_path`: every item
/// acts on all of them, and copies join their texts with newlines.
pub fn workspaces_items(
  workspaces: &[Workspace],
  repo_path: &str,
  editors: EditorApps,
) -> Vec<MenuItem> {
  let lines = |text: fn(&Workspace, &str) -> String| {
    workspaces
      .iter()
      .map(|w| text(w, repo_path))
      .collect::<Vec<_>>()
      .join("\n")
  };
  let full: Vec<String> = workspaces.iter().map(full_workspace_path).collect();
  let mut items = vec![
    MenuItem::item(
      "Copy branch names",
      MenuAction::Copy(lines(|w, _| w.branch_name.clone())),
    )
    .icon(MenuIcon::GitBranch),
    MenuItem::item(
      "Copy relative paths",
      MenuAction::Copy(lines(|w, repo| {
        workspace_relative_path(&w.workspace_path, repo)
      })),
    )
    .icon(MenuIcon::Copy),
    MenuItem::item("Copy full paths", MenuAction::Copy(full.join("\n"))).icon(MenuIcon::Copy),
  ];
  // The single-workspace "Open in…" items, each run for every path.
  if let Some(MenuItem::Submenu {
    text,
    icon,
    items: open,
  }) = path_items("", "", editors).pop()
  {
    let open = open
      .into_iter()
      .map(|item| match item {
        MenuItem::Item {
          text,
          icon,
          destructive,
          action,
        } => MenuItem::Item {
          text,
          icon,
          destructive,
          action: MenuAction::Each(
            full
              .iter()
              .map(|path| match &action {
                MenuAction::Reveal(_) => MenuAction::Reveal(path.clone()),
                MenuAction::OpenUrl(url) => MenuAction::OpenUrl(format!("{url}{path}")),
                other => other.clone(),
              })
              .collect(),
          ),
        },
        other => other,
      })
      .collect();
    items.push(MenuItem::Submenu {
      text,
      icon,
      items: open,
    });
  }
  items.push(MenuItem::Separator);
  items.push(
    MenuItem::item(
      &format!("Archive {} workspaces", workspaces.len()),
      MenuAction::ArchiveWorkspaces {
        repo_path: repo_path.into(),
        workspace_ids: workspaces.iter().map(|w| w.id).collect(),
      },
    )
    .icon(MenuIcon::Archive)
    .destructive(),
  );
  items
}

/// The workspace header's More menu; Reset to remote only when the branch
/// is on `origin`.
pub fn workspace_more_items(repo_path: &str, workspace_id: i64, on_remote: bool) -> Vec<MenuItem> {
  let mut items = Vec::new();
  if on_remote {
    items.push(
      MenuItem::item("Reset to remote", MenuAction::ResetToRemote)
        .icon(MenuIcon::RotateCcw)
        .destructive(),
    );
    items.push(MenuItem::Separator);
  }
  items.push(
    MenuItem::item(
      "Delete workspace",
      MenuAction::DeleteWorkspace {
        repo_path: repo_path.into(),
        workspace_id,
      },
    )
    .icon(MenuIcon::Trash)
    .destructive(),
  );
  items
}

/// `CreatePrButtonGroup`'s dropdown.
pub fn create_pr_items() -> Vec<MenuItem> {
  vec![
    MenuItem::item("Create draft PR", MenuAction::CreatePr { draft: true }),
    MenuItem::item("Create PR manually", MenuAction::CreatePrManually),
  ]
}

/// A saved project as the project menu lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
  pub id: String,
  pub name: String,
  /// The owning Team organization's name; `None` for personal.
  pub team: Option<String>,
}

/// The sidebar's project menu: every saved project (this window's one
/// marked), then Save or Rename, then a new window.
pub fn project_items(projects: &[ProjectEntry], current: Option<&str>) -> Vec<MenuItem> {
  let row = |p: &ProjectEntry| {
    let text = if current == Some(p.id.as_str()) {
      format!("✓ {}", p.name)
    } else {
      p.name.clone()
    };
    MenuItem::item(&text, MenuAction::SwitchProject(p.id.clone())).icon(MenuIcon::FolderOpen)
  };
  // Personal projects inline, then one submenu per team, in first-seen order.
  let mut items: Vec<MenuItem> = projects
    .iter()
    .filter(|p| p.team.is_none())
    .map(row)
    .collect();
  let mut teams: Vec<&str> = Vec::new();
  for team in projects.iter().filter_map(|p| p.team.as_deref()) {
    if !teams.contains(&team) {
      teams.push(team);
    }
  }
  for team in teams {
    items.push(MenuItem::Submenu {
      text: team.to_string(),
      icon: Some(MenuIcon::FolderOpen),
      items: projects
        .iter()
        .filter(|p| p.team.as_deref() == Some(team))
        .map(row)
        .collect(),
    });
  }
  if !items.is_empty() {
    items.push(MenuItem::Separator);
  }
  let save = if current.is_some() {
    "Rename Project…"
  } else {
    "Save Project…"
  };
  items.push(MenuItem::item(save, MenuAction::SaveProject).icon(MenuIcon::Pencil));
  if current.is_some() {
    items.push(
      MenuItem::item("Project Instructions…", MenuAction::ProjectContext).icon(MenuIcon::Settings),
    );
  }
  items.push(MenuItem::item("New Window…", MenuAction::NewWindow).icon(MenuIcon::FolderPlus));
  items
}

/// "Move to" for this window's saved project: Personal, then each Team
/// organization as (id, name), the current owner marked. `None` without teams.
pub fn project_owner_item(teams: &[(String, String)], owner: Option<&str>) -> Option<MenuItem> {
  if teams.is_empty() {
    return None;
  }
  let mark = |on: bool, name: &str| {
    if on {
      format!("✓ {name}")
    } else {
      name.to_string()
    }
  };
  let mut items = vec![MenuItem::item(
    &mark(owner.is_none(), "Personal"),
    MenuAction::SetProjectOwner(None),
  )];
  items.extend(teams.iter().map(|(id, name)| {
    MenuItem::item(
      &mark(owner == Some(id.as_str()), name),
      MenuAction::SetProjectOwner(Some(id.clone())),
    )
  }));
  Some(MenuItem::Submenu {
    text: "Move to".into(),
    icon: Some(MenuIcon::FolderOpen),
    items,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn project_owner_menu_marks_the_owner_and_lists_teams() {
    let teams = vec![("o1".to_string(), "Acme".to_string())];
    assert_eq!(project_owner_item(&[], None), None);
    let item = project_owner_item(&teams, Some("o1")).unwrap();
    assert_eq!(
      labels(std::slice::from_ref(&item)),
      ["Move to: [Personal, ✓ Acme]"]
    );
    let MenuItem::Submenu { items, .. } = item else {
      panic!()
    };
    assert!(
      matches!(&items[1], MenuItem::Item { action: MenuAction::SetProjectOwner(Some(id)), .. } if id == "o1")
    );
  }

  fn entry(id: &str, name: &str) -> ProjectEntry {
    ProjectEntry {
      id: id.into(),
      name: name.into(),
      team: None,
    }
  }

  #[test]
  fn project_menu_puts_each_teams_projects_in_a_submenu() {
    let mut team = entry("t", "Billing");
    team.team = Some("Acme".into());
    let items = project_items(&[entry("a", "Checkout"), team], Some("t"));
    assert_eq!(
      labels(&items)[..3],
      ["Checkout", "Acme: [✓ Billing]", "---"]
    );
  }

  #[test]
  fn project_menu_offers_save_for_an_unsaved_window() {
    let items = project_items(&[entry("a", "Checkout"), entry("b", "Refunds")], None);
    assert_eq!(
      labels(&items),
      ["Checkout", "Refunds", "---", "Save Project…", "New Window…"]
    );
  }

  #[test]
  fn project_menu_marks_this_windows_project_and_offers_rename() {
    let items = project_items(&[entry("a", "Checkout"), entry("b", "Refunds")], Some("b"));
    assert_eq!(
      labels(&items),
      [
        "Checkout",
        "✓ Refunds",
        "---",
        "Rename Project…",
        "Project Instructions…",
        "New Window…"
      ]
    );
  }

  #[test]
  fn project_menu_without_projects_only_saves_and_opens() {
    assert_eq!(
      labels(&project_items(&[], None)),
      ["Save Project…", "New Window…"]
    );
  }

  #[test]
  fn picking_a_project_switches_to_it() {
    let items = project_items(&[entry("a", "Checkout")], None);
    assert!(matches!(
      &items[0],
      MenuItem::Item { action: MenuAction::SwitchProject(id), .. } if id == "a"
    ));
  }

  const NO_EDITORS: EditorApps = EditorApps {
    cursor: false,
    vscode: false,
    zed: false,
  };

  /// Menu labels, with "---" for separators and submenus as `text: [...]`.
  mod project_props {
    use super::*;
    use crate::test_repo::proptest_config;
    use proptest::prelude::*;

    proptest! {
      #![proptest_config(proptest_config(64))]

      /// One switch per project, a mark only on this window's one, and
      /// Instructions only for a saved project.
      #[test]
      fn project_menu_lists_each_project_once(
        names in prop::collection::vec("\\PC{0,6}", 0..5),
        current in prop::option::of(0usize..6),
      ) {
        let projects: Vec<ProjectEntry> = names
          .iter()
          .enumerate()
          .map(|(i, n)| ProjectEntry { id: format!("p{i}"), name: n.clone(), team: None })
          .collect();
        let current = current.map(|i| format!("p{i}"));
        let items = project_items(&projects, current.as_deref());
        let switches: Vec<&str> = items
          .iter()
          .filter_map(|i| match i {
            MenuItem::Item { action: MenuAction::SwitchProject(id), .. } => Some(id.as_str()),
            _ => None,
          })
          .collect();
        let ids: Vec<&str> = projects.iter().map(|p| p.id.as_str()).collect();
        prop_assert_eq!(switches, ids);
        let marked = labels(&items).iter().take(projects.len()).filter(|l| l.starts_with("✓ ")).count();
        let expected = current.as_deref().is_some_and(|c| projects.iter().any(|p| p.id == c));
        prop_assert!(marked <= usize::from(expected) + names.iter().filter(|n| n.starts_with("✓ ")).count());
        let labels = labels(&items);
        prop_assert_eq!(labels.contains(&"Project Instructions…".to_string()), current.is_some());
        prop_assert_eq!(labels.contains(&"---".to_string()), !projects.is_empty());
        prop_assert_eq!(labels.last().map(String::as_str), Some("New Window…"));
      }
    }
  }

  fn labels(items: &[MenuItem]) -> Vec<String> {
    items
      .iter()
      .map(|item| match item {
        MenuItem::Separator => "---".to_string(),
        MenuItem::Item { text, .. } => text.clone(),
        MenuItem::Submenu { text, items, .. } => format!("{text}: [{}]", labels(items).join(", ")),
      })
      .collect()
  }

  fn workspace(path: &str) -> Workspace {
    Workspace {
      id: 4,
      repo_path: "/r".into(),
      workspace_name: "feat-a".into(),
      workspace_path: path.into(),
      branch_name: "feat/a".into(),
      created_at: String::new(),
      refreshed_at: None,
      metadata: None,
      target_branch: None,
      title: "Feature A".into(),
      description: None,
      moved_files: None,
      not_on_remote: false,
      sparse_patterns: None,
      archived: false,
    }
  }

  #[test]
  fn lists_only_the_editors_that_are_installed() {
    let editors = EditorApps {
      cursor: true,
      vscode: false,
      zed: true,
    };
    let items = path_items(".", "/r", editors);
    assert_eq!(
      labels(&items),
      [
        "Copy relative path",
        "Copy full path",
        format!("Open in…: [{REVEAL_LABEL}, Open in Cursor, Open in Zed]").as_str()
      ]
    );
    let MenuItem::Submenu { items, .. } = &items[2] else {
      panic!("submenu");
    };
    assert_eq!(
      items[1],
      MenuItem::item(
        "Open in Cursor",
        MenuAction::OpenUrl("cursor://file//r".into())
      )
    );
  }

  #[test]
  fn matches_the_react_home_row_menu() {
    let menu = HomeMenu {
      repo_path: "/r".into(),
      on_screen: true,
      ..Default::default()
    };
    assert_eq!(
      labels(&home_items(&menu, NO_EDITORS)),
      [
        "Switch Branch...",
        "---",
        "Copy relative path",
        "Copy full path",
        format!("Open in…: [{REVEAL_LABEL}]").as_str(),
        "---",
        "Add Repository…"
      ]
    );
  }

  #[test]
  fn drops_path_items_for_a_missing_supporting_repository() {
    let menu = HomeMenu {
      repo_path: "/r".into(),
      missing: true,
      supporting: true,
      removable: true,
      on_screen: false,
    };
    let items = home_items(&menu, NO_EDITORS);
    assert_eq!(labels(&items), ["---", "Locate…", "Remove Repository"]);
    assert!(matches!(
      &items[2],
      MenuItem::Item { action: MenuAction::RemoveRepo(path), destructive: true, .. } if path == "/r"
    ));
  }

  #[test]
  fn offers_to_remove_the_main_repository_when_removable() {
    let menu = HomeMenu {
      repo_path: "/m".into(),
      removable: true,
      ..Default::default()
    };
    let items = home_items(&menu, NO_EDITORS);
    assert_eq!(
      labels(&items)[3..],
      ["---", "Add Repository…", "Remove Repository"]
    );
    let fixed = HomeMenu {
      removable: false,
      ..menu
    };
    assert!(!labels(&home_items(&fixed, NO_EDITORS))
      .iter()
      .any(|l| l == "Remove Repository"));
  }

  #[test]
  fn offers_settings_for_a_present_supporting_repository() {
    let menu = HomeMenu {
      repo_path: "/s".into(),
      supporting: true,
      removable: true,
      ..Default::default()
    };
    let items = home_items(&menu, NO_EDITORS);
    assert_eq!(
      labels(&items)[3..],
      ["---", "Repository Settings", "Remove Repository"]
    );
  }

  #[test]
  fn matches_the_react_workspace_row_menu() {
    let ws = workspace("feat-a");
    let items = workspace_items(
      &ws,
      "/r",
      Some("https://github.com/o/r/pull/1"),
      NO_EDITORS,
      true,
    );
    assert_eq!(
      labels(&items),
      [
        "Copy branch name",
        "Copy link to GitHub PR",
        "Rename workspace",
        "---",
        "Copy relative path",
        "Copy full path",
        format!("Open in…: [{REVEAL_LABEL}]").as_str(),
        "---",
        "Archive Workspace"
      ]
    );
    assert_eq!(
      items[0],
      MenuItem::item("Copy branch name", MenuAction::Copy("feat/a".into()))
        .icon(MenuIcon::GitBranch)
    );
    assert!(matches!(
      &items[5],
      MenuItem::Item { action: MenuAction::Copy(path), .. } if path == "/r/.treq/workspaces/feat-a"
    ));
  }

  #[test]
  fn several_workspaces_menu_acts_on_every_workspace() {
    let mut b = workspace("/r/wt/b");
    b.id = 5;
    b.branch_name = "feat/b".into();
    let items = workspaces_items(&[workspace("feat-a"), b], "/r", NO_EDITORS);
    assert_eq!(
      labels(&items),
      [
        "Copy branch names",
        "Copy relative paths",
        "Copy full paths",
        format!("Open in…: [{REVEAL_LABEL}]").as_str(),
        "---",
        "Archive 2 workspaces"
      ]
    );
    let action = |i: usize| match &items[i] {
      MenuItem::Item { action, .. } => action.clone(),
      _ => panic!("not an item"),
    };
    assert_eq!(action(0), MenuAction::Copy("feat/a\nfeat/b".into()));
    assert_eq!(
      action(1),
      MenuAction::Copy(".treq/workspaces/feat-a\nwt/b".into())
    );
    assert_eq!(
      action(2),
      MenuAction::Copy("/r/.treq/workspaces/feat-a\n/r/wt/b".into())
    );
    let MenuItem::Submenu { items: open, .. } = &items[3] else {
      panic!("no Open in submenu");
    };
    assert!(matches!(
      &open[0],
      MenuItem::Item { action: MenuAction::Each(each), .. } if *each == [
        MenuAction::Reveal("/r/.treq/workspaces/feat-a".into()),
        MenuAction::Reveal("/r/wt/b".into()),
      ]
    ));
    assert_eq!(
      action(5),
      MenuAction::ArchiveWorkspaces {
        repo_path: "/r".into(),
        workspace_ids: vec![4, 5],
      }
    );
  }

  #[test]
  fn leaves_out_the_pr_link_and_archive_when_absent() {
    let items = workspace_items(&workspace("feat-a"), "/r", None, NO_EDITORS, false);
    let labels = labels(&items);
    assert!(!labels.contains(&"Copy link to GitHub PR".to_string()));
    assert!(!labels.contains(&"Archive Workspace".to_string()));
  }

  #[test]
  fn finds_a_workspace_path_relative_to_its_repository() {
    assert_eq!(
      workspace_relative_path("feat-a", "/r"),
      ".treq/workspaces/feat-a"
    );
    assert_eq!(workspace_relative_path("/r/wt/a", "/r"), "wt/a");
    assert_eq!(workspace_relative_path("/x/a", "/r"), "/x/a");
    assert_eq!(workspace_relative_path("/rr/a", "/r"), "/rr/a");
  }

  #[test]
  fn builds_the_header_menus() {
    assert_eq!(
      labels(&workspace_more_items("/r", 4, true)),
      ["Reset to remote", "---", "Delete workspace"]
    );
    assert_eq!(
      labels(&workspace_more_items("/r", 4, false)),
      ["Delete workspace"]
    );
    assert_eq!(
      labels(&create_pr_items()),
      ["Create draft PR", "Create PR manually"]
    );
  }
}
