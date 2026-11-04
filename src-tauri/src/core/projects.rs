//! Projects: the set of repositories one Treq window shows. A saved project
//! has a name and is listed on the welcome page; a window opened on a plain
//! repository holds an unsaved project until the user saves it.
//!
//! Projects live in the app database (`treq.db`), so every window and the CLI
//! see the same list. See `docs/design/projects.md`.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
  pub id: String,
  pub name: String,
  /// Repository paths (each repo's clone directory), in sidebar order.
  pub repos: Vec<String>,
  pub created_at: String,
  pub last_opened_at: String,
}

/// What closing a window should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseAction {
  /// Close without asking.
  Close,
  /// Ask the user to save the unsaved project, with this name prefilled.
  PromptSave { suggested_name: String },
}

pub fn init(conn: &Connection) -> rusqlite::Result<()> {
  todo!()
}

pub fn save(conn: &Connection, name: &str, repos: &[String]) -> Result<Project, String> {
  todo!()
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Project>, String> {
  todo!()
}

/// Saved projects, most recently opened first.
pub fn list(conn: &Connection) -> Result<Vec<Project>, String> {
  todo!()
}

pub fn rename(conn: &Connection, id: &str, name: &str) -> Result<(), String> {
  todo!()
}

pub fn add_repo(conn: &Connection, id: &str, repo: &str) -> Result<(), String> {
  todo!()
}

pub fn remove_repo(conn: &Connection, id: &str, repo: &str) -> Result<(), String> {
  todo!()
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), String> {
  todo!()
}

pub fn mark_opened(conn: &Connection, id: &str) -> Result<(), String> {
  todo!()
}

/// The name offered when saving an unsaved project: its repository folder
/// names joined with " + ".
pub fn suggested_name(repos: &[String]) -> String {
  todo!()
}

/// Closing a window on an unsaved project asks to save it; a saved project,
/// or a window with no repositories, just closes.
pub fn close_action(project_id: Option<&str>, repos: &[String]) -> CloseAction {
  todo!()
}

/// The window already showing `project_id`, which switching focuses instead
/// of opening another. `windows` pairs each window label with its project.
pub fn window_for_project<'a>(
  windows: &'a [(String, Option<String>)],
  project_id: &str,
) -> Option<&'a str> {
  todo!()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn conn() -> Connection {
    let conn = Connection::open_in_memory().expect("open");
    init(&conn).expect("init");
    conn
  }

  fn repos(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| p.to_string()).collect()
  }

  #[test]
  fn saves_project_with_repos_in_order() {
    let conn = conn();
    let saved = save(&conn, "Checkout", &repos(&["/r/web", "/r/api"])).expect("save");
    let loaded = get(&conn, &saved.id).expect("get").expect("exists");
    assert_eq!(loaded.name, "Checkout");
    assert_eq!(loaded.repos, repos(&["/r/web", "/r/api"]));
  }

  #[test]
  fn rejects_blank_project_name() {
    let conn = conn();
    assert!(save(&conn, "  ", &repos(&["/r/web"])).is_err());
  }

  #[test]
  fn rejects_project_without_repos() {
    let conn = conn();
    assert!(save(&conn, "Empty", &[]).is_err());
  }

  #[test]
  fn drops_duplicate_repos_when_saving() {
    let conn = conn();
    let saved = save(&conn, "P", &repos(&["/r/web", "/r/web", "/r/api"])).expect("save");
    assert_eq!(saved.repos, repos(&["/r/web", "/r/api"]));
  }

  #[test]
  fn returns_none_for_unknown_project() {
    let conn = conn();
    assert_eq!(get(&conn, "missing").expect("get"), None);
  }

  #[test]
  fn lists_most_recently_opened_first() {
    let conn = conn();
    let a = save(&conn, "A", &repos(&["/r/a"])).expect("a");
    let b = save(&conn, "B", &repos(&["/r/b"])).expect("b");
    mark_opened(&conn, &a.id).expect("open a");
    let names: Vec<String> = list(&conn).expect("list").into_iter().map(|p| p.name).collect();
    assert_eq!(names, vec!["A".to_string(), "B".to_string()]);
    mark_opened(&conn, &b.id).expect("open b");
    let names: Vec<String> = list(&conn).expect("list").into_iter().map(|p| p.name).collect();
    assert_eq!(names, vec!["B".to_string(), "A".to_string()]);
  }

  #[test]
  fn renames_project() {
    let conn = conn();
    let p = save(&conn, "Old", &repos(&["/r/a"])).expect("save");
    rename(&conn, &p.id, "New").expect("rename");
    assert_eq!(get(&conn, &p.id).unwrap().unwrap().name, "New");
  }

  #[test]
  fn rejects_blank_rename() {
    let conn = conn();
    let p = save(&conn, "Old", &repos(&["/r/a"])).expect("save");
    assert!(rename(&conn, &p.id, "").is_err());
  }

  #[test]
  fn adds_repo_at_end_once() {
    let conn = conn();
    let p = save(&conn, "P", &repos(&["/r/a"])).expect("save");
    add_repo(&conn, &p.id, "/r/b").expect("add");
    add_repo(&conn, &p.id, "/r/b").expect("add again is a no-op");
    assert_eq!(get(&conn, &p.id).unwrap().unwrap().repos, repos(&["/r/a", "/r/b"]));
  }

  #[test]
  fn removes_repo() {
    let conn = conn();
    let p = save(&conn, "P", &repos(&["/r/a", "/r/b"])).expect("save");
    remove_repo(&conn, &p.id, "/r/a").expect("remove");
    assert_eq!(get(&conn, &p.id).unwrap().unwrap().repos, repos(&["/r/b"]));
  }

  #[test]
  fn refuses_to_remove_last_repo() {
    let conn = conn();
    let p = save(&conn, "P", &repos(&["/r/a"])).expect("save");
    assert!(remove_repo(&conn, &p.id, "/r/a").is_err());
  }

  #[test]
  fn same_repo_can_be_in_two_projects() {
    let conn = conn();
    let a = save(&conn, "A", &repos(&["/r/api"])).expect("a");
    let b = save(&conn, "B", &repos(&["/r/api", "/r/ios"])).expect("b");
    remove_repo(&conn, &b.id, "/r/api").expect("remove from b");
    assert_eq!(get(&conn, &a.id).unwrap().unwrap().repos, repos(&["/r/api"]));
  }

  #[test]
  fn deletes_project_and_its_repos() {
    let conn = conn();
    let p = save(&conn, "P", &repos(&["/r/a"])).expect("save");
    delete(&conn, &p.id).expect("delete");
    assert_eq!(get(&conn, &p.id).expect("get"), None);
    let rows: i64 = conn
      .query_row("SELECT COUNT(*) FROM project_repos", [], |r| r.get(0))
      .expect("count");
    assert_eq!(rows, 0);
  }

  #[test]
  fn changes_to_unknown_project_fail() {
    let conn = conn();
    assert!(rename(&conn, "missing", "X").is_err());
    assert!(add_repo(&conn, "missing", "/r/a").is_err());
    assert!(mark_opened(&conn, "missing").is_err());
  }

  #[test]
  fn suggests_name_from_repo_folders() {
    assert_eq!(suggested_name(&repos(&["/r/web", "/code/api/"])), "web + api");
  }

  #[test]
  fn suggests_fallback_name_without_repos() {
    assert_eq!(suggested_name(&[]), "Untitled project");
  }

  #[test]
  fn closing_unsaved_project_prompts_to_save() {
    assert_eq!(
      close_action(None, &repos(&["/r/web", "/r/api"])),
      CloseAction::PromptSave {
        suggested_name: "web + api".to_string()
      }
    );
  }

  #[test]
  fn closing_saved_project_just_closes() {
    assert_eq!(close_action(Some("p1"), &repos(&["/r/web"])), CloseAction::Close);
  }

  #[test]
  fn closing_window_without_repos_just_closes() {
    assert_eq!(close_action(None, &[]), CloseAction::Close);
  }

  #[test]
  fn finds_window_already_showing_project() {
    let windows = vec![
      ("main".to_string(), None),
      ("treq-1".to_string(), Some("p1".to_string())),
    ];
    assert_eq!(window_for_project(&windows, "p1"), Some("treq-1"));
    assert_eq!(window_for_project(&windows, "p2"), None);
  }
}
