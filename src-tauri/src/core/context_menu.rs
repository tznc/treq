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
  if menu.supporting {
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

#[cfg(test)]
mod tests {
  use super::*;

  const NO_EDITORS: EditorApps = EditorApps {
    cursor: false,
    vscode: false,
    zed: false,
  };

  /// Menu labels, with "---" for separators and submenus as `text: [...]`.
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
  fn offers_settings_for_a_present_supporting_repository() {
    let menu = HomeMenu {
      repo_path: "/s".into(),
      supporting: true,
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
