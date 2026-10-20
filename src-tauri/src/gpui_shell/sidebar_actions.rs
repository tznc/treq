//! The sidebar clicks GPUI handles itself, which `useGpuiSidebar` and
//! Dashboard's handlers ran: the home and workspace row menus, archiving
//! one or several workspaces, and dropping a row onto another to stack it.
//! The rest, and everything for a remote repository, go to React.

use std::sync::Arc;

use tauri::AppHandle;
use tauri_plugin_gpui::gpui::App;

use super::{
  menus,
  native::{Backend, NativeEvent, OpenRequest},
  remote_repo,
  screen::{
    workspace_ops::{self, Removed},
    ChangesHost, WorkspaceHost,
  },
  sidebar::{SidebarAction, SidebarSnapshot},
};
use crate::{
  core::{
    self,
    context_menu::{self, EditorApps, HomeMenu, MenuAction},
  },
  local_db::Workspace,
};

/// Whether GPUI may handle `action` (it still hands it to React when it
/// cannot find what it is for).
pub fn handles(action: &SidebarAction) -> bool {
  matches!(
    action,
    SidebarAction::HomeMenu { .. }
      | SidebarAction::WorkspaceMenu { .. }
      | SidebarAction::RepoWorkspaceMenu { .. }
      | SidebarAction::BulkArchive
      | SidebarAction::MoveWorkspace { .. }
  )
}

/// Runs `action` for window `label`, whose sidebar shows `snapshot`.
/// False leaves it to React. Main thread only.
pub fn handle(
  app: &AppHandle,
  label: &str,
  snapshot: &SidebarSnapshot,
  action: &SidebarAction,
  _: &mut App,
) -> bool {
  if remote_repo::for_window(label).is_some() {
    return false;
  }
  let backend = Arc::new(Backend::new(app.clone(), label.to_string()));
  let Some(active) = snapshot.repos.iter().find(|r| r.active) else {
    return false;
  };
  match action {
    SidebarAction::HomeMenu { repo_path } => {
      let Some(index) = snapshot.repos.iter().position(|r| &r.path == repo_path) else {
        return false;
      };
      let menu = HomeMenu {
        repo_path: repo_path.clone(),
        missing: snapshot.repos[index].missing,
        on_screen: repo_path == &active.path,
        supporting: index > 0,
      };
      let items = context_menu::home_items(&menu, editors(&*backend));
      show(app, label, backend, items);
      true
    }
    SidebarAction::WorkspaceMenu { id } => {
      workspace_menu(app, label, backend, &active.path, *id, true)
    }
    SidebarAction::RepoWorkspaceMenu { repo_path, id } => {
      workspace_menu(app, label, backend, repo_path, *id, false)
    }
    SidebarAction::BulkArchive => {
      let Ok(workspaces) = core::list_workspaces(&active.path) else {
        return false;
      };
      let selected: Vec<Removed> = active
        .workspaces
        .iter()
        .filter(|row| row.selected)
        .filter_map(|row| workspaces.iter().find(|w| w.id == row.id))
        .map(Removed::of)
        .collect();
      if selected.is_empty() {
        return false;
      }
      let app = app.clone();
      std::thread::spawn(move || {
        for workspace in workspace_ops::archive_all(&*backend, &selected) {
          super::close_workspace_terminals(&app, backend.label(), workspace.path);
        }
      });
      true
    }
    SidebarAction::MoveWorkspace { id, onto } => {
      let Ok(workspaces) = core::list_workspaces(&active.path) else {
        return false;
      };
      let find = |id: i64| workspaces.iter().find(|w| w.id == id).cloned();
      let Some(workspace) = find(*id) else {
        return false;
      };
      let target = match onto {
        Some(onto) => match find(*onto) {
          Some(onto) => onto.branch_name,
          None => return true,
        },
        // Onto the list: the home branch (`effectiveDefaultBranch`).
        None => active
          .home_ref
          .clone()
          .filter(|r| !r.is_empty())
          .unwrap_or_else(|| "main".into()),
      };
      let repo_path = active.path.clone();
      std::thread::spawn(move || {
        workspace_ops::move_workspace(&*backend, &repo_path, &workspace, &target)
      });
      true
    }
    _ => false,
  }
}

fn editors(host: &dyn ChangesHost) -> EditorApps {
  let apps = host.editor_apps();
  EditorApps {
    cursor: apps.cursor,
    vscode: apps.vscode,
    zed: apps.zed,
  }
}

/// `workspaceMenuEntries` for workspace `id` of `repo_path`, which may be
/// a repository that is not on screen (`archive` false).
fn workspace_menu(
  app: &AppHandle,
  label: &str,
  backend: Arc<Backend>,
  repo_path: &str,
  id: i64,
  archive: bool,
) -> bool {
  let Some(workspace) = core::list_workspaces(repo_path)
    .ok()
    .and_then(|list| list.into_iter().find(|w| w.id == id))
  else {
    return false;
  };
  let pr_url = pr_url(repo_path, &workspace);
  let items = context_menu::workspace_items(
    &workspace,
    repo_path,
    pr_url.as_deref(),
    editors(&*backend),
    archive,
  );
  show(app, label, backend, items);
  true
}

fn pr_url(repo_path: &str, workspace: &Workspace) -> Option<String> {
  crate::pr_status::global()
    .get_cached(repo_path, &workspace.branch_name)
    .flatten()
    .map(|pr| pr.url)
}

/// Opens `items` at the sidebar's last right-click.
fn show(app: &AppHandle, label: &str, backend: Arc<Backend>, items: Vec<context_menu::MenuItem>) {
  let run_app = app.clone();
  let entries = menus::entries(
    items,
    std::rc::Rc::new(move |action, cx: &mut App| {
      if let Some(action) = menus::run_common(action, &*backend, cx) {
        run(&run_app, backend.clone(), action);
      }
    }),
  );
  super::show_menu(app, label, entries);
}

/// A pick the menu's owner carries out.
fn run(app: &AppHandle, backend: Arc<Backend>, action: MenuAction) {
  let open = |what| WorkspaceHost::notify(&*backend, NativeEvent::Open { what });
  match action {
    MenuAction::OpenBranchSwitcher => open(OpenRequest::BranchSwitcher),
    MenuAction::AddRepository => open(OpenRequest::AddRepository),
    MenuAction::OpenRepoSettings(repo_path) => open(OpenRequest::RepoSettings { repo_path }),
    MenuAction::LocateRepo(repo_path) => open(OpenRequest::LocateRepo { repo_path }),
    MenuAction::RemoveRepo(repo_path) => open(OpenRequest::RemoveRepo { repo_path }),
    MenuAction::RenameWorkspace {
      repo_path,
      workspace_id,
    } => open(OpenRequest::RenameWorkspace {
      repo_path,
      workspace_id,
    }),
    MenuAction::ArchiveWorkspace {
      repo_path,
      workspace_id,
    } => {
      let app = app.clone();
      std::thread::spawn(move || {
        let Some(workspace) = core::list_workspaces(&repo_path)
          .ok()
          .and_then(|list| list.into_iter().find(|w| w.id == workspace_id))
        else {
          return;
        };
        let workspace = Removed::of(&workspace);
        if workspace_ops::archive(&*backend, &workspace) {
          super::close_workspace_terminals(&app, backend.label(), workspace.path);
        }
      });
    }
    _ => {}
  }
}
