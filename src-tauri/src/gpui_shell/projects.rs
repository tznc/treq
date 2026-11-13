//! Projects in the desktop app: which saved project each window shows,
//! switching projects (focus the window already showing one, or open a new
//! window), saving a window's unsaved project, and asking to save it when its
//! window closes. The store itself is `core::projects`.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
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
  let id = state.window_projects.lock_or_recover().get(label).cloned();
  let db = state.db.lock_or_recover();
  project_of(db.conn(), id.as_deref())
}

/// The project window `label` keeps its repositories in: its saved project,
/// or the draft of an unsaved window.
pub fn window_repos_project(app: &AppHandle, label: &str) -> Option<Project> {
  let state = app.state::<AppState>();
  let id = state.window_projects.lock_or_recover().get(label).cloned();
  let db = state.db.lock_or_recover();
  any_project_of(db.conn(), id.as_deref())
}

/// Gives unsaved window `label`, now showing `repo`, the draft of `repo`
/// (seeded from its supporting repositories). A saved project is kept.
pub fn assign_draft(app: &AppHandle, label: &str, repo: &str) {
  let state = app.state::<AppState>();
  let mut map = state.window_projects.lock_or_recover();
  let db = state.db.lock_or_recover();
  let current = any_project_of(db.conn(), map.get(label).map(String::as_str));
  if current.as_ref().is_some_and(|p| !p.draft) {
    return;
  }
  if repo.is_empty() {
    map.remove(label);
    return;
  }
  let seed = || {
    crate::core::supporting_repos::list(repo)
      .unwrap_or_default()
      .into_iter()
      .map(|r| r.path)
      .collect()
  };
  match projects::draft_for_repo(db.conn(), repo, seed) {
    Ok(draft) => {
      map.insert(label.to_string(), draft.id);
    }
    Err(err) => log::warn!("no draft project for {repo}: {err}"),
  }
}

/// The saved project `id` names; `None` for an unsaved window (no id or a
/// draft) or a project deleted since.
fn project_of(conn: &Connection, id: Option<&str>) -> Option<Project> {
  any_project_of(conn, id).filter(|p| !p.draft)
}

/// The project `id` names, draft or saved.
fn any_project_of(conn: &Connection, id: Option<&str>) -> Option<Project> {
  projects::get(conn, id?).ok().flatten()
}

/// The windows of `window_projects` that show a saved project.
fn saved_windows(
  conn: &Connection,
  window_projects: &HashMap<String, String>,
) -> HashMap<String, String> {
  window_projects
    .iter()
    .filter(|(_, id)| project_of(conn, Some(id)).is_some())
    .map(|(label, id)| (label.clone(), id.clone()))
    .collect()
}

/// The repositories of saved `project` an agent in `main` may read: the
/// others that are on this machine.
pub fn supporting_repos(project: &Project, main: &str) -> Vec<String> {
  project
    .repos
    .iter()
    .filter(|path| *path != main && std::path::Path::new(path).is_dir())
    .cloned()
    .collect()
}

/// Every saved project, most recently opened first.
pub fn list(app: &AppHandle) -> Vec<Project> {
  let state = app.state::<AppState>();
  let db = state.db.lock_or_recover();
  projects::list(db.conn()).unwrap_or_default()
}

/// The repositories window `label` shows: its main repository, then the
/// others in sidebar order. A remote window's repository is its key
/// (`core::projects::remote_repo`).
pub fn window_repos(app: &AppHandle, label: &str) -> Vec<String> {
  let state = app.state::<AppState>();
  let main = state.window_repo_paths.lock_or_recover();
  let rest = state.window_supporting_repo_paths.lock_or_recover();
  let remote = super::remote_repo::for_window(label).map(|r| r.cache_key);
  repos_of(&main, &rest, remote, label)
}

/// Window `label`'s main repository (local, else `remote`'s key), then its
/// supporting ones.
fn repos_of(
  main: &HashMap<String, String>,
  rest: &HashMap<String, Vec<String>>,
  remote: Option<String>,
  label: &str,
) -> Vec<String> {
  main
    .get(label)
    .cloned()
    .or(remote)
    .into_iter()
    .chain(rest.get(label).cloned().unwrap_or_default())
    .collect()
}

/// What showing project `id` does.
#[derive(Debug, PartialEq)]
enum OpenTarget {
  /// Focus the window with this label, which already shows it.
  Focus(String),
  /// Open a new window on `main`, titled `title`.
  Open {
    project: String,
    main: String,
    title: String,
  },
  /// Nothing to show: no such project, or it has no repositories.
  Nothing,
}

/// Marks project `id` opened and says where to show it. `window_exists`
/// tells whether a window label is still open.
fn open_target(
  conn: &Connection,
  window_projects: &HashMap<String, String>,
  id: &str,
  window_exists: impl Fn(&str) -> bool,
) -> OpenTarget {
  let windows: Vec<(String, Option<String>)> = window_projects
    .iter()
    .map(|(label, project)| (label.clone(), Some(project.clone())))
    .collect();
  projects::mark_opened(conn, id).warn_on_err("mark project opened");
  if let Some(label) = projects::window_for_project(&windows, id, window_exists) {
    return OpenTarget::Focus(label.to_string());
  }
  let Some(project) = project_of(conn, Some(id)) else {
    log::warn!("no project with id {id}");
    return OpenTarget::Nothing;
  };
  let Some(main) = project.repos.first().cloned() else {
    return OpenTarget::Nothing;
  };
  OpenTarget::Open {
    title: format!("Treq - {}", project.name),
    project: project.id,
    main,
  }
}

/// Shows project `id`: focuses the window already showing it, or opens it
/// in a new window. Main thread only.
pub fn open_project(app: &AppHandle, id: &str) {
  let state = app.state::<AppState>();
  let target = {
    let map = state.window_projects.lock_or_recover().clone();
    let db = state.db.lock_or_recover();
    open_target(db.conn(), &map, id, |label| app.get_window(label).is_some())
  };
  match target {
    OpenTarget::Focus(label) => {
      if let Some(window) = app.get_window(&label) {
        window.unminimize().best_effort();
        window.set_focus().warn_on_err("focus project window");
      }
    }
    OpenTarget::Open {
      project,
      main,
      title,
    } => {
      let label = crate::open_new_window::new_repo_window_label();
      state
        .window_projects
        .lock_or_recover()
        .insert(label.clone(), project);
      if let Err(err) = super::open_window(app, &label, Some(main), &title) {
        log::error!("failed to open project window: {err}");
      }
    }
    OpenTarget::Nothing => {}
  }
}

/// Saves window `label`'s project as `name`: renames a saved project, or
/// saves the window's repositories as a new one.
pub fn save_window_project(app: &AppHandle, label: &str, name: &str) -> Result<Project, String> {
  let state = app.state::<AppState>();
  let repos = window_repos(app, label);
  let project = {
    let mut map = state.window_projects.lock_or_recover();
    let db = state.db.lock_or_recover();
    save_for(db.conn(), &mut map, label, &repos, name)?
  };
  if let Some(window) = app.get_window(label) {
    window
      .set_title(&format!("Treq - {}", project.name))
      .best_effort();
  }
  Ok(project)
}

/// Renames window `label`'s saved project to `name`, or saves `repos` as a
/// new project, and records it as the window's project.
fn save_for(
  conn: &Connection,
  window_projects: &mut HashMap<String, String>,
  label: &str,
  repos: &[String],
  name: &str,
) -> Result<Project, String> {
  let project = match window_projects.get(label) {
    Some(id) => {
      // Names a saved project, or saves a draft under the name.
      projects::promote(conn, id, name)?;
      projects::get(conn, id)?.ok_or("The project no longer exists")?
    }
    None => projects::save(conn, name, repos)?,
  };
  window_projects.insert(label.to_string(), project.id.clone());
  Ok(project)
}

/// The window's close request. True holds the close back while the window
/// asks to save its unsaved project.
pub fn close_requested(app: &AppHandle, label: &str) -> bool {
  let Some(state) = app.try_state::<AppState>() else {
    return false;
  };
  let prompt = {
    let confirmed = state.window_close_confirmed.lock_or_recover();
    let main = state.window_repo_paths.lock_or_recover();
    let rest = state.window_supporting_repo_paths.lock_or_recover();
    let projects_map = state.window_projects.lock_or_recover();
    let db = state.db.lock_or_recover();
    asks_to_save(
      &confirmed,
      &main,
      &rest,
      &saved_windows(db.conn(), &projects_map),
      label,
    )
  };
  if prompt {
    Backend::new(app.clone(), label.to_string()).notify(NativeEvent::Open {
      what: OpenRequest::SaveProject { closing: true },
    });
  }
  prompt
}

/// Whether closing window `label` first asks to save its project: a repo
/// window not yet confirmed, on an unsaved project of 2+ repositories.
fn asks_to_save(
  confirmed: &HashSet<String>,
  main: &HashMap<String, String>,
  rest: &HashMap<String, Vec<String>>,
  window_projects: &HashMap<String, String>,
  label: &str,
) -> bool {
  if confirmed.contains(label) || !main.contains_key(label) {
    return false;
  }
  let project = window_projects.get(label).map(String::as_str);
  matches!(
    projects::close_action(project, &repos_of(main, rest, None, label)),
    CloseAction::PromptSave { .. }
  )
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

#[cfg(test)]
mod tests {
  use super::*;

  fn conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    projects::init(&conn).unwrap();
    conn
  }

  fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
  }

  fn map<V: Clone>(pairs: &[(&str, V)]) -> HashMap<String, V> {
    pairs
      .iter()
      .map(|(k, v)| (k.to_string(), v.clone()))
      .collect()
  }

  #[test]
  fn supporting_repos_leave_out_main_and_missing_folders() {
    let a = tempfile::TempDir::new().unwrap();
    let b = tempfile::TempDir::new().unwrap();
    let (a, b) = (
      a.path().to_string_lossy().to_string(),
      b.path().to_string_lossy().to_string(),
    );
    let conn = conn();
    let p = projects::save(
      &conn,
      "P",
      &[a.clone(), "/definitely/missing".into(), b.clone()],
    )
    .unwrap();
    assert_eq!(supporting_repos(&p, &a), vec![b]);
  }

  #[test]
  fn project_of_unsaved_window_is_none() {
    assert_eq!(project_of(&conn(), None), None);
  }

  #[test]
  fn project_of_deleted_project_is_none() {
    assert_eq!(project_of(&conn(), Some("gone")), None);
  }

  #[test]
  fn project_of_saved_window_is_its_project() {
    let conn = conn();
    let saved = projects::save(&conn, "Checkout", &strings(&["/r/web"])).unwrap();
    assert_eq!(project_of(&conn, Some(&saved.id)), Some(saved));
  }

  #[test]
  fn repos_of_lists_main_then_supporting() {
    let main = map(&[("w1", "/r/web".to_string())]);
    let rest = map(&[("w1", strings(&["/r/api", "/r/lib"]))]);
    assert_eq!(
      repos_of(&main, &rest, None, "w1"),
      strings(&["/r/web", "/r/api", "/r/lib"])
    );
  }

  #[test]
  fn repos_of_unknown_window_is_empty() {
    assert!(repos_of(&HashMap::new(), &HashMap::new(), None, "w1").is_empty());
  }

  #[test]
  fn repos_of_window_without_supporting_repos_is_its_main() {
    let main = map(&[("w1", "/r/web".to_string())]);
    assert_eq!(
      repos_of(&main, &HashMap::new(), None, "w1"),
      strings(&["/r/web"])
    );
  }

  #[test]
  fn open_target_focuses_the_window_showing_the_project() {
    let conn = conn();
    let p = projects::save(&conn, "Checkout", &strings(&["/r/web"])).unwrap();
    let windows = map(&[("treq-2", p.id.clone())]);
    assert_eq!(
      open_target(&conn, &windows, &p.id, |_| true),
      OpenTarget::Focus("treq-2".into())
    );
  }

  #[test]
  fn open_target_opens_a_new_window_when_none_shows_it() {
    let conn = conn();
    let p = projects::save(&conn, "Checkout", &strings(&["/r/web", "/r/api"])).unwrap();
    let other = projects::save(&conn, "Other", &strings(&["/r/x"])).unwrap();
    let windows = map(&[("treq-2", other.id)]);
    assert_eq!(
      open_target(&conn, &windows, &p.id, |_| true),
      OpenTarget::Open {
        project: p.id,
        main: "/r/web".into(),
        title: "Treq - Checkout".into(),
      }
    );
  }

  #[test]
  fn open_target_focuses_a_live_window_when_another_is_stale() {
    let conn = conn();
    let p = projects::save(&conn, "Checkout", &strings(&["/r/web"])).unwrap();
    let windows = map(&[
      ("treq-2", p.id.clone()),
      ("treq-3", p.id.clone()),
      ("treq-4", p.id.clone()),
    ]);
    assert_eq!(
      open_target(&conn, &windows, &p.id, |label| label == "treq-3"),
      OpenTarget::Focus("treq-3".into())
    );
  }

  #[test]
  fn a_remote_windows_repos_are_its_remote_repository() {
    let main = map::<String>(&[]);
    let rest = map::<Vec<String>>(&[]);
    let remote = Some("ssh:ep-1:gen0:/srv/app".to_string());
    assert_eq!(
      repos_of(&main, &rest, remote, "treq-2"),
      strings(&["ssh:ep-1:gen0:/srv/app"])
    );
    assert!(repos_of(&main, &rest, None, "treq-2").is_empty());
  }

  #[test]
  fn open_target_opens_anew_when_its_window_is_gone() {
    let conn = conn();
    let p = projects::save(&conn, "Checkout", &strings(&["/r/web"])).unwrap();
    let windows = map(&[("treq-2", p.id.clone())]);
    assert!(matches!(
      open_target(&conn, &windows, &p.id, |_| false),
      OpenTarget::Open { .. }
    ));
  }

  #[test]
  fn open_target_does_nothing_for_unknown_project() {
    assert_eq!(
      open_target(&conn(), &HashMap::new(), "gone", |_| true),
      OpenTarget::Nothing
    );
  }

  #[test]
  fn open_target_marks_the_project_opened() {
    let conn = conn();
    let a = projects::save(&conn, "A", &strings(&["/r/a"])).unwrap();
    let b = projects::save(&conn, "B", &strings(&["/r/b"])).unwrap();
    conn
      .execute("UPDATE projects SET last_opened_at = '2000-01-01'", [])
      .unwrap();
    open_target(&conn, &HashMap::new(), &a.id, |_| true);
    let order: Vec<String> = projects::list(&conn)
      .unwrap()
      .into_iter()
      .map(|p| p.id)
      .collect();
    assert_eq!(order, vec![a.id, b.id]);
  }

  #[test]
  fn save_for_unsaved_window_saves_its_repos_and_records_it() {
    let conn = conn();
    let mut windows = HashMap::new();
    let p = save_for(
      &conn,
      &mut windows,
      "w1",
      &strings(&["/r/web", "/r/api"]),
      "Checkout",
    )
    .unwrap();
    assert_eq!(p.repos, strings(&["/r/web", "/r/api"]));
    assert_eq!(windows.get("w1"), Some(&p.id));
    assert_eq!(projects::list(&conn).unwrap().len(), 1);
  }

  #[test]
  fn save_for_a_draft_window_promotes_its_draft() {
    let conn = conn();
    let d = projects::draft(&conn, &strings(&["/r/web", "/r/api"])).unwrap();
    let mut windows = map(&[("w1", d.id.clone())]);
    let p = save_for(
      &conn,
      &mut windows,
      "w1",
      &strings(&["/r/other"]),
      "Checkout",
    )
    .unwrap();
    assert_eq!(
      (p.id.as_str(), p.name.as_str(), p.draft),
      (d.id.as_str(), "Checkout", false)
    );
    assert_eq!(p.repos, strings(&["/r/web", "/r/api"]));
    assert_eq!(projects::list(&conn).unwrap().len(), 1);
  }

  #[test]
  fn saved_windows_leave_out_drafts() {
    let conn = conn();
    let d = projects::draft(&conn, &strings(&["/r/web"])).unwrap();
    let p = projects::save(&conn, "P", &strings(&["/r/api"])).unwrap();
    let windows = map(&[("w1", d.id), ("w2", p.id.clone())]);
    assert_eq!(saved_windows(&conn, &windows), map(&[("w2", p.id)]));
  }

  #[test]
  fn project_of_a_draft_is_none() {
    let conn = conn();
    let d = projects::draft(&conn, &strings(&["/r/web"])).unwrap();
    assert_eq!(project_of(&conn, Some(&d.id)), None);
    assert_eq!(any_project_of(&conn, Some(&d.id)).map(|p| p.id), Some(d.id));
  }

  #[test]
  fn save_for_saved_window_renames_without_a_new_project() {
    let conn = conn();
    let p = projects::save(&conn, "Old", &strings(&["/r/web"])).unwrap();
    let mut windows = map(&[("w1", p.id.clone())]);
    let renamed = save_for(&conn, &mut windows, "w1", &strings(&["/r/other"]), "New").unwrap();
    assert_eq!(renamed.id, p.id);
    assert_eq!(renamed.name, "New");
    assert_eq!(renamed.repos, strings(&["/r/web"]));
    assert_eq!(projects::list(&conn).unwrap().len(), 1);
  }

  #[test]
  fn save_for_fails_when_the_saved_project_is_gone() {
    let conn = conn();
    let mut windows = map(&[("w1", "gone".to_string())]);
    assert!(save_for(&conn, &mut windows, "w1", &strings(&["/r/web"]), "New").is_err());
  }

  #[test]
  fn save_for_blank_name_fails_and_leaves_window_unsaved() {
    let conn = conn();
    let mut windows = HashMap::new();
    assert!(save_for(&conn, &mut windows, "w1", &strings(&["/r/web"]), "  ").is_err());
    assert!(windows.is_empty());
  }

  fn two_repo_window() -> (HashMap<String, String>, HashMap<String, Vec<String>>) {
    (
      map(&[("w1", "/r/web".to_string())]),
      map(&[("w1", strings(&["/r/api"]))]),
    )
  }

  #[test]
  fn asks_to_save_unsaved_window_of_two_repos() {
    let (main, rest) = two_repo_window();
    assert!(asks_to_save(
      &HashSet::new(),
      &main,
      &rest,
      &HashMap::new(),
      "w1"
    ));
  }

  #[test]
  fn does_not_ask_for_a_single_repo_window() {
    let (main, _) = two_repo_window();
    assert!(!asks_to_save(
      &HashSet::new(),
      &main,
      &HashMap::new(),
      &HashMap::new(),
      "w1"
    ));
  }

  #[test]
  fn does_not_ask_for_a_saved_project() {
    let (main, rest) = two_repo_window();
    let saved = map(&[("w1", "p1".to_string())]);
    assert!(!asks_to_save(&HashSet::new(), &main, &rest, &saved, "w1"));
  }

  #[test]
  fn does_not_ask_again_once_close_is_confirmed() {
    let (main, rest) = two_repo_window();
    let confirmed: HashSet<String> = ["w1".to_string()].into();
    assert!(!asks_to_save(
      &confirmed,
      &main,
      &rest,
      &HashMap::new(),
      "w1"
    ));
  }

  #[test]
  fn does_not_ask_for_a_window_without_a_repo() {
    let (_, rest) = two_repo_window();
    assert!(!asks_to_save(
      &HashSet::new(),
      &HashMap::new(),
      &rest,
      &HashMap::new(),
      "w1"
    ));
  }
}
