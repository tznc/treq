//! The sidebar clicks GPUI handles itself, which `useGpuiSidebar` and
//! Dashboard's handlers ran: the home and workspace row menus, archiving
//! one or several workspaces, and dropping a row onto another to stack it.
//! A remote repository's are read and changed over SSH, off the main
//! thread. The rest go to the window model.

use std::sync::Arc;

use tauri::AppHandle;
use tauri_plugin_gpui::gpui::App;

use super::{
  menus,
  native::{Backend, NativeEvent, OpenRequest, ToastTone},
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
  result_ext::ResultExt,
};

/// Whether GPUI may handle `action` (a window the model does not drive
/// still hands it to React when GPUI cannot find what it is for).
pub fn handles(action: &SidebarAction) -> bool {
  matches!(
    action,
    SidebarAction::HomeMenu { .. }
      | SidebarAction::ProjectMenu
      | SidebarAction::WorkspaceMenu { .. }
      | SidebarAction::WorkspacesMenu
      | SidebarAction::RepoWorkspaceMenu { .. }
      | SidebarAction::MoveWorkspace { .. }
  )
}

/// Runs `action` for window `label`, whose sidebar shows `snapshot`.
/// False when it is not for anything GPUI finds. Main thread only.
pub fn handle(
  app: &AppHandle,
  label: &str,
  snapshot: &SidebarSnapshot,
  action: &SidebarAction,
  cx: &mut App,
) -> bool {
  let backend = Arc::new(Backend::new(app.clone(), label.to_string()));
  let Some(active) = snapshot.repos.iter().find(|r| r.active) else {
    return false;
  };
  match action {
    SidebarAction::ProjectMenu => {
      let current = super::projects::window_project(app, label).map(|p| p.id);
      let entries: Vec<context_menu::ProjectEntry> = super::projects::list(app)
        .into_iter()
        .map(|p| context_menu::ProjectEntry {
          id: p.id,
          name: p.name,
        })
        .collect();
      let items = context_menu::project_items(&entries, current.as_deref());
      show(app, label, backend, items);
      true
    }
    SidebarAction::HomeMenu { repo_path } => {
      let Some(index) = snapshot.repos.iter().position(|r| &r.path == repo_path) else {
        return false;
      };
      let menu = HomeMenu {
        repo_path: repo_path.clone(),
        missing: snapshot.repos[index].missing,
        on_screen: repo_path == &active.path,
        supporting: index > 0,
        // A saved project can drop its first repository; the next one
        // takes over the window.
        removable: index > 0 || (snapshot.project.is_some() && snapshot.repos.len() > 1),
      };
      let items = context_menu::home_items(&menu, editors(&*backend));
      show(app, label, backend, items);
      true
    }
    SidebarAction::WorkspaceMenu { id } => {
      workspace_menu(app, label, backend, &active.path, *id, true, cx)
    }
    SidebarAction::RepoWorkspaceMenu { repo_path, id } => {
      workspace_menu(app, label, backend, repo_path, *id, false, cx)
    }
    SidebarAction::WorkspacesMenu => {
      let ids = selected_ids(active);
      if ids.is_empty() {
        return false;
      }
      let (app, label, repo_path) = (app.clone(), label.to_string(), active.path.clone());
      std::thread::spawn(move || {
        let Some(workspaces) =
          workspaces_or_toast(&backend, &repo_path, "Failed to load the workspace menu")
        else {
          return;
        };
        let selected: Vec<Workspace> = workspaces
          .into_iter()
          .filter(|w| ids.contains(&w.id))
          .collect();
        super::defer_main(&app.clone(), move |app| {
          let items = context_menu::workspaces_items(&selected, &repo_path, editors(&*backend));
          show(app, &label, backend, items);
        });
      });
      true
    }
    SidebarAction::MoveWorkspace { id, onto } => {
      let (id, onto) = (*id, *onto);
      // Onto the list: the home branch (`effectiveDefaultBranch`).
      let home = active
        .home_ref
        .clone()
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| "main".into());
      let repo_path = active.path.clone();
      std::thread::spawn(move || {
        const FAILED: &str = "Failed to move workspace";
        let Some(workspaces) = workspaces_or_toast(&backend, &repo_path, FAILED) else {
          return;
        };
        let find = |id: i64| workspaces.iter().find(|w| w.id == id).cloned();
        let Some(workspace) = find(id) else {
          backend.toast(ToastTone::Error, FAILED, "The workspace no longer exists.");
          return;
        };
        let target = match onto {
          Some(onto) => match find(onto) {
            Some(onto) => onto.branch_name,
            None => {
              backend.toast(
                ToastTone::Error,
                FAILED,
                "The target workspace no longer exists.",
              );
              return;
            }
          },
          None => home,
        };
        workspace_ops::move_workspace(&*backend, &repo_path, &workspace, &target)
      });
      true
    }
    _ => false,
  }
}

/// The ids of `repo`'s selected workspace rows.
fn selected_ids(repo: &super::sidebar::RepoSection) -> Vec<i64> {
  repo
    .workspaces
    .iter()
    .filter(|row| row.selected)
    .map(|row| row.id)
    .collect()
}

/// Archives workspaces `ids` of `repo_path`, off the main thread, closing
/// the terminals of each one archived.
fn archive_all(app: &AppHandle, backend: Arc<Backend>, repo_path: String, ids: Vec<i64>) {
  let app = app.clone();
  std::thread::spawn(move || {
    let Some(workspaces) =
      workspaces_or_toast(&backend, &repo_path, "Failed to archive workspaces")
    else {
      return;
    };
    let selected: Vec<Removed> = workspaces
      .iter()
      .filter(|w| ids.contains(&w.id))
      .map(Removed::of)
      .collect();
    for workspace in workspace_ops::archive_all(&*backend, &selected) {
      super::close_workspace_terminals(&app, backend.label(), workspace.path);
    }
  });
}

/// `repo_path`'s workspaces, over SSH for the window's remote repository.
/// Blocks.
fn workspaces(backend: &Backend, repo_path: &str) -> Result<Vec<Workspace>, String> {
  match backend.remote(repo_path) {
    Some(remote) => remote.workspaces(),
    None => core::list_workspaces(repo_path),
  }
}

/// `workspaces`, or `None` after telling the user why the list could not be
/// read. For actions that already left the menu, so a failure has no other
/// place to show.
fn workspaces_or_toast(backend: &Backend, repo_path: &str, title: &str) -> Option<Vec<Workspace>> {
  workspaces(backend, repo_path)
    .map_err(|err| backend.toast(ToastTone::Error, title, err))
    .ok()
}

/// Workspace `id` of `repo_path`, or `None` after a toast titled `title`.
fn workspace_or_toast(
  backend: &Backend,
  repo_path: &str,
  id: i64,
  title: &str,
) -> Option<Workspace> {
  let found = workspaces_or_toast(backend, repo_path, title)?
    .into_iter()
    .find(|w| w.id == id);
  if found.is_none() {
    backend.toast(ToastTone::Error, title, "The workspace no longer exists.");
  }
  found
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
/// a repository that is not on screen (`archive` false). The workspace
/// comes from what the window last read; one it has not read (and a remote
/// repository's, which has no pull request poller) is read off the main
/// thread, so the menu never waits on the repository or SSH.
fn workspace_menu(
  app: &AppHandle,
  label: &str,
  backend: Arc<Backend>,
  repo_path: &str,
  id: i64,
  archive: bool,
  cx: &App,
) -> bool {
  let remote = backend.remote(repo_path).is_some();
  if !remote {
    if let Some(workspace) = super::window::workspace(label, repo_path, id, cx) {
      let pr_url = pr_url(repo_path, &workspace);
      let items = context_menu::workspace_items(
        &workspace,
        repo_path,
        pr_url.as_deref(),
        editors(&*backend),
        archive,
      );
      show(app, label, backend, items);
      return true;
    }
  }
  let (app, label, repo_path) = (app.clone(), label.to_string(), repo_path.to_string());
  std::thread::spawn(move || {
    let Some(workspace) = workspace_or_toast(
      &backend,
      &repo_path,
      id,
      "Failed to load the workspace menu",
    ) else {
      return;
    };
    let pr_url = (!remote).then(|| pr_url(&repo_path, &workspace)).flatten();
    super::defer_main(&app.clone(), move |app| {
      let items = context_menu::workspace_items(
        &workspace,
        &repo_path,
        pr_url.as_deref(),
        editors(&*backend),
        archive,
      );
      show(app, &label, backend, items);
    });
  });
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
    MenuAction::SaveProject => open(OpenRequest::SaveProject { closing: false }),
    MenuAction::ProjectContext => open(OpenRequest::ProjectContext),
    MenuAction::SwitchProject(id) => {
      let main = app.clone();
      main
        .run_on_main_thread({
          let app = app.clone();
          move || super::projects::open_project(&app, &id)
        })
        .ignore_closed();
    }
    MenuAction::NewWindow => {
      let main = app.clone();
      main
        .run_on_main_thread({
          let app = app.clone();
          move || {
            let label = crate::open_new_window::new_repo_window_label();
            if let Err(err) = super::open_window(&app, &label, None, "Treq") {
              log::error!("failed to open window: {err}");
            }
          }
        })
        .ignore_closed();
    }
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
        let Some(workspace) = workspace_or_toast(
          &backend,
          &repo_path,
          workspace_id,
          "Failed to archive workspace",
        ) else {
          return;
        };
        let workspace = Removed::of(&workspace);
        if workspace_ops::archive(&*backend, &workspace) {
          super::close_workspace_terminals(&app, backend.label(), workspace.path);
        }
      });
    }
    MenuAction::ArchiveWorkspaces {
      repo_path,
      workspace_ids,
    } => archive_all(app, backend, repo_path, workspace_ids),
    _ => {}
  }
}
