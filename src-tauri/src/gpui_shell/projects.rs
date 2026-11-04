//! Projects in the desktop app: which saved project each window shows,
//! switching projects (focus the window already showing one, or open a new
//! window), saving a window's unsaved project, and asking to save it when its
//! window closes. The store itself is `core::projects`.

use tauri::{AppHandle, Manager};

use super::native::{Backend, NativeEvent, OpenRequest};
use crate::{
  core::projects::{self, CloseAction, Project},
  lock_ext::LockExt,
  result_ext::ResultExt,
  AppState,
};

/// The saved project window `label` shows, if any.
pub fn window_project(app: &AppHandle, label: &str) -> Option<Project> {
  let state = app.state::<AppState>();
  let id = state
    .window_projects
    .lock_or_recover()
    .get(label)
    .cloned()?;
  let db = state.db.lock_or_recover();
  projects::get(db.conn(), &id).ok().flatten()
}

/// Every saved project, most recently opened first.
pub fn list(app: &AppHandle) -> Vec<Project> {
  let state = app.state::<AppState>();
  let db = state.db.lock_or_recover();
  projects::list(db.conn()).unwrap_or_default()
}

/// The repositories window `label` shows: its main repository, then the
/// others in sidebar order.
pub fn window_repos(app: &AppHandle, label: &str) -> Vec<String> {
  let state = app.state::<AppState>();
  let main = state
    .window_repo_paths
    .lock_or_recover()
    .get(label)
    .cloned();
  let rest = state
    .window_supporting_repo_paths
    .lock_or_recover()
    .get(label)
    .cloned()
    .unwrap_or_default();
  main.into_iter().chain(rest).collect()
}

/// Shows project `id`: focuses the window already showing it, or opens it
/// in a new window. Main thread only.
pub fn open_project(app: &AppHandle, id: &str) {
  let state = app.state::<AppState>();
  let windows: Vec<(String, Option<String>)> = {
    let map = state.window_projects.lock_or_recover();
    map
      .iter()
      .map(|(label, project)| (label.clone(), Some(project.clone())))
      .collect()
  };
  {
    let db = state.db.lock_or_recover();
    projects::mark_opened(db.conn(), id).warn_on_err("mark project opened");
  }
  if let Some(label) = projects::window_for_project(&windows, id) {
    if let Some(window) = app.get_window(label) {
      window.unminimize().best_effort();
      window.set_focus().warn_on_err("focus project window");
      return;
    }
  }
  let project = {
    let db = state.db.lock_or_recover();
    projects::get(db.conn(), id).ok().flatten()
  };
  let Some(project) = project else {
    log::warn!("no project with id {id}");
    return;
  };
  let Some(main) = project.repos.first().cloned() else {
    return;
  };
  let label = crate::open_new_window::new_repo_window_label();
  state
    .window_projects
    .lock_or_recover()
    .insert(label.clone(), project.id.clone());
  let title = format!("Treq - {}", project.name);
  if let Err(err) = super::open_window(app, &label, Some(main), &title) {
    log::error!("failed to open project window: {err}");
  }
}

/// Saves window `label`'s project as `name`: renames a saved project, or
/// saves the window's repositories as a new one.
pub fn save_window_project(app: &AppHandle, label: &str, name: &str) -> Result<Project, String> {
  let state = app.state::<AppState>();
  let current = state.window_projects.lock_or_recover().get(label).cloned();
  let repos = window_repos(app, label);
  let db = state.db.lock_or_recover();
  let project = match current {
    Some(id) => {
      projects::rename(db.conn(), &id, name)?;
      projects::get(db.conn(), &id)?.ok_or("The project no longer exists")?
    }
    None => projects::save(db.conn(), name, &repos)?,
  };
  drop(db);
  state
    .window_projects
    .lock_or_recover()
    .insert(label.to_string(), project.id.clone());
  if let Some(window) = app.get_window(label) {
    window
      .set_title(&format!("Treq - {}", project.name))
      .best_effort();
  }
  Ok(project)
}

/// The window's close request. True holds the close back while the window
/// asks to save its unsaved project.
pub fn close_requested(app: &AppHandle, label: &str) -> bool {
  let Some(state) = app.try_state::<AppState>() else {
    return false;
  };
  if state
    .window_close_confirmed
    .lock_or_recover()
    .contains(label)
  {
    return false;
  }
  if !state
    .window_repo_paths
    .lock_or_recover()
    .contains_key(label)
  {
    return false;
  }
  let project = state.window_projects.lock_or_recover().get(label).cloned();
  match projects::close_action(project.as_deref(), &window_repos(app, label)) {
    CloseAction::Close => false,
    CloseAction::PromptSave { .. } => {
      Backend::new(app.clone(), label.to_string()).notify(NativeEvent::Open {
        what: OpenRequest::SaveProject { closing: true },
      });
      true
    }
  }
}

/// Closes window `label` without asking again.
pub fn close_confirmed(app: &AppHandle, label: &str) {
  app
    .state::<AppState>()
    .window_close_confirmed
    .lock_or_recover()
    .insert(label.to_string());
  if let Some(window) = app.get_window(label) {
    window.close().warn_on_err("close window");
  }
}
