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
  conn.execute_batch(
    "CREATE TABLE IF NOT EXISTS projects (
       id TEXT PRIMARY KEY,
       name TEXT NOT NULL,
       created_at TEXT NOT NULL,
       last_opened_at TEXT NOT NULL
     );
     CREATE TABLE IF NOT EXISTS project_repos (
       project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
       repo_path TEXT NOT NULL,
       position INTEGER NOT NULL,
       PRIMARY KEY (project_id, repo_path)
     );",
  )
}

fn now() -> String {
  // Nanoseconds keep two saves in one second in order.
  chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

fn err(e: rusqlite::Error) -> String {
  e.to_string()
}

fn exists(conn: &Connection, id: &str) -> Result<(), String> {
  let found: Option<i64> = conn
    .query_row("SELECT 1 FROM projects WHERE id = ?1", [id], |r| r.get(0))
    .optional()
    .map_err(err)?;
  found
    .map(drop)
    .ok_or_else(|| format!("No project with id '{id}'"))
}

fn clean_name(name: &str) -> Result<String, String> {
  let name = name.trim();
  if name.is_empty() {
    return Err("A project needs a name".to_string());
  }
  Ok(name.to_string())
}

fn repos_of(conn: &Connection, id: &str) -> Result<Vec<String>, String> {
  let mut stmt = conn
    .prepare("SELECT repo_path FROM project_repos WHERE project_id = ?1 ORDER BY position")
    .map_err(err)?;
  let rows = stmt.query_map([id], |r| r.get(0)).map_err(err)?;
  rows.collect::<Result<_, _>>().map_err(err)
}

pub fn save(conn: &Connection, name: &str, repos: &[String]) -> Result<Project, String> {
  let name = clean_name(name)?;
  let mut unique: Vec<String> = Vec::new();
  for repo in repos {
    if !unique.contains(repo) {
      unique.push(repo.clone());
    }
  }
  if unique.is_empty() {
    return Err("A project needs at least one repository".to_string());
  }
  let id = uuid::Uuid::new_v4().to_string();
  let at = now();
  conn
    .execute(
      "INSERT INTO projects (id, name, created_at, last_opened_at) VALUES (?1, ?2, ?3, ?3)",
      params![id, name, at],
    )
    .map_err(err)?;
  for (position, repo) in unique.iter().enumerate() {
    conn
      .execute(
        "INSERT INTO project_repos (project_id, repo_path, position) VALUES (?1, ?2, ?3)",
        params![id, repo, position as i64],
      )
      .map_err(err)?;
  }
  get(conn, &id)?.ok_or_else(|| "The project was not saved".to_string())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Project>, String> {
  let row = conn
    .query_row(
      "SELECT name, created_at, last_opened_at FROM projects WHERE id = ?1",
      [id],
      |r| {
        Ok((
          r.get::<_, String>(0)?,
          r.get::<_, String>(1)?,
          r.get::<_, String>(2)?,
        ))
      },
    )
    .optional()
    .map_err(err)?;
  let Some((name, created_at, last_opened_at)) = row else {
    return Ok(None);
  };
  Ok(Some(Project {
    id: id.to_string(),
    name,
    repos: repos_of(conn, id)?,
    created_at,
    last_opened_at,
  }))
}

/// Saved projects, most recently opened first.
pub fn list(conn: &Connection) -> Result<Vec<Project>, String> {
  let mut stmt = conn
    .prepare("SELECT id FROM projects ORDER BY last_opened_at DESC, name")
    .map_err(err)?;
  let ids: Vec<String> = stmt
    .query_map([], |r| r.get(0))
    .map_err(err)?
    .collect::<Result<_, _>>()
    .map_err(err)?;
  ids
    .iter()
    .filter_map(|id| get(conn, id).transpose())
    .collect()
}

pub fn rename(conn: &Connection, id: &str, name: &str) -> Result<(), String> {
  let name = clean_name(name)?;
  exists(conn, id)?;
  conn
    .execute(
      "UPDATE projects SET name = ?2 WHERE id = ?1",
      params![id, name],
    )
    .map_err(err)
    .map(drop)
}

pub fn add_repo(conn: &Connection, id: &str, repo: &str) -> Result<(), String> {
  exists(conn, id)?;
  conn
    .execute(
      "INSERT OR IGNORE INTO project_repos (project_id, repo_path, position)
       VALUES (?1, ?2, (SELECT COALESCE(MAX(position), -1) + 1 FROM project_repos WHERE project_id = ?1))",
      params![id, repo],
    )
    .map_err(err)
    .map(drop)
}

pub fn remove_repo(conn: &Connection, id: &str, repo: &str) -> Result<(), String> {
  let repos = {
    exists(conn, id)?;
    repos_of(conn, id)?
  };
  if repos.len() <= 1 && repos.iter().any(|r| r == repo) {
    return Err("A project keeps at least one repository".to_string());
  }
  conn
    .execute(
      "DELETE FROM project_repos WHERE project_id = ?1 AND repo_path = ?2",
      params![id, repo],
    )
    .map_err(err)
    .map(drop)
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), String> {
  conn
    .execute("DELETE FROM project_repos WHERE project_id = ?1", [id])
    .map_err(err)?;
  conn
    .execute("DELETE FROM projects WHERE id = ?1", [id])
    .map_err(err)
    .map(drop)
}

pub fn mark_opened(conn: &Connection, id: &str) -> Result<(), String> {
  exists(conn, id)?;
  conn
    .execute(
      "UPDATE projects SET last_opened_at = ?2 WHERE id = ?1",
      params![id, now()],
    )
    .map_err(err)
    .map(drop)
}

/// The name offered when saving an unsaved project: its repository folder
/// names joined with " + ".
pub fn suggested_name(repos: &[String]) -> String {
  let names: Vec<&str> = repos
    .iter()
    .map(|repo| {
      Path::new(repo.trim_end_matches(['/', '\\']))
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(repo)
    })
    .collect();
  if names.is_empty() {
    "Untitled project".to_string()
  } else {
    names.join(" + ")
  }
}

/// Closing a window on an unsaved project of two or more repositories asks
/// to save it; a saved project or a single repository just closes.
pub fn close_action(project_id: Option<&str>, repos: &[String]) -> CloseAction {
  // A plain single-repository window is today's behaviour; it closes quietly.
  if project_id.is_some() || repos.len() < 2 {
    return CloseAction::Close;
  }
  CloseAction::PromptSave {
    suggested_name: suggested_name(repos),
  }
}

/// The window already showing `project_id`, which switching focuses instead
/// of opening another. `windows` pairs each window label with its project.
pub fn window_for_project<'a>(
  windows: &'a [(String, Option<String>)],
  project_id: &str,
) -> Option<&'a str> {
  windows
    .iter()
    .find(|(_, project)| project.as_deref() == Some(project_id))
    .map(|(label, _)| label.as_str())
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
    let names: Vec<String> = list(&conn)
      .expect("list")
      .into_iter()
      .map(|p| p.name)
      .collect();
    assert_eq!(names, vec!["A".to_string(), "B".to_string()]);
    mark_opened(&conn, &b.id).expect("open b");
    let names: Vec<String> = list(&conn)
      .expect("list")
      .into_iter()
      .map(|p| p.name)
      .collect();
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
    assert_eq!(
      get(&conn, &p.id).unwrap().unwrap().repos,
      repos(&["/r/a", "/r/b"])
    );
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
    assert_eq!(
      get(&conn, &a.id).unwrap().unwrap().repos,
      repos(&["/r/api"])
    );
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
    assert_eq!(
      suggested_name(&repos(&["/r/web", "/code/api/"])),
      "web + api"
    );
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
    assert_eq!(
      close_action(Some("p1"), &repos(&["/r/web"])),
      CloseAction::Close
    );
  }

  #[test]
  fn closing_unsaved_single_repo_just_closes() {
    assert_eq!(close_action(None, &repos(&["/r/web"])), CloseAction::Close);
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
