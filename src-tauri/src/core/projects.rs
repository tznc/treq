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
  /// Instructions added to every agent started in the project's windows.
  pub prompt: String,
  /// Files, folders or URLs agents may read for context.
  pub context_paths: Vec<String>,
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
  )?;
  // Added after the first prototype: an older table gains them here.
  for (column, ddl) in [
    (
      "prompt",
      "ALTER TABLE projects ADD COLUMN prompt TEXT NOT NULL DEFAULT ''",
    ),
    (
      "context_paths",
      "ALTER TABLE projects ADD COLUMN context_paths TEXT NOT NULL DEFAULT '[]'",
    ),
  ] {
    let has: i64 = conn.query_row(
      "SELECT COUNT(*) FROM pragma_table_info('projects') WHERE name = ?1",
      [column],
      |r| r.get(0),
    )?;
    if has == 0 {
      conn.execute(ddl, [])?;
    }
  }
  Ok(())
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
      "SELECT name, created_at, last_opened_at, prompt, context_paths FROM projects WHERE id = ?1",
      [id],
      |r| {
        Ok((
          r.get::<_, String>(0)?,
          r.get::<_, String>(1)?,
          r.get::<_, String>(2)?,
          r.get::<_, String>(3)?,
          r.get::<_, String>(4)?,
        ))
      },
    )
    .optional()
    .map_err(err)?;
  let Some((name, created_at, last_opened_at, prompt, context)) = row else {
    return Ok(None);
  };
  Ok(Some(Project {
    id: id.to_string(),
    name,
    repos: repos_of(conn, id)?,
    created_at,
    last_opened_at,
    prompt,
    context_paths: serde_json::from_str(&context).unwrap_or_default(),
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

/// Sets the project's agent instructions and context paths. Blank and
/// repeated paths are dropped.
pub fn set_context(
  conn: &Connection,
  id: &str,
  prompt: &str,
  context_paths: &[String],
) -> Result<(), String> {
  exists(conn, id)?;
  let mut paths: Vec<String> = Vec::new();
  for path in context_paths.iter().map(|p| p.trim()) {
    if !path.is_empty() && !paths.iter().any(|p| p == path) {
      paths.push(path.to_string());
    }
  }
  let json = serde_json::to_string(&paths).map_err(|e| e.to_string())?;
  conn
    .execute(
      "UPDATE projects SET prompt = ?2, context_paths = ?3 WHERE id = ?1",
      params![id, prompt.trim(), json],
    )
    .map_err(err)
    .map(drop)
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

/// `owner/name` alone means a GitHub repository; anything else is a clone URL
/// as given.
pub fn clone_url(spec: &str) -> String {
  let spec = spec.trim();
  let parts: Vec<&str> = spec.split('/').collect();
  if parts.len() == 2
    && !spec.contains(':')
    && parts
      .iter()
      .all(|p| !p.is_empty() && *p != "." && *p != "..")
  {
    return format!("https://github.com/{spec}.git");
  }
  spec.to_string()
}

/// Where Treq keeps its own clone of `url`: `<root>/<host>/<owner>/<name>`.
/// `None` for a local path or anything it cannot read as a repository URL.
pub fn managed_clone_path(root: &Path, url: &str) -> Option<std::path::PathBuf> {
  let url = clone_url(url);
  let rest = if let Some((_, rest)) = url.split_once("://") {
    // ssh://git@host/owner/name, https://host/owner/name
    let rest = rest.rsplit_once('@').map(|(_, r)| r).unwrap_or(rest);
    rest.to_string()
  } else if let Some((user_host, path)) = url.split_once(':') {
    // git@host:owner/name
    let host = user_host
      .rsplit_once('@')
      .map(|(_, h)| h)
      .unwrap_or(user_host);
    if host.contains('/') || host.is_empty() {
      return None;
    }
    format!("{host}/{path}")
  } else {
    return None;
  };
  let parts: Vec<&str> = rest
    .trim_end_matches('/')
    .trim_end_matches(".git")
    .split('/')
    .filter(|p| !p.is_empty())
    .collect();
  if parts.len() < 3 || parts.iter().any(|p| *p == "." || *p == "..") {
    return None;
  }
  let mut path = root.to_path_buf();
  for part in parts {
    path.push(part);
  }
  Some(path)
}

/// Where Treq clones repositories: `$TREQ_CLONES_DIR`, else
/// `~/.treq/repos`.
pub fn clones_root() -> std::path::PathBuf {
  if let Some(dir) = std::env::var_os("TREQ_CLONES_DIR").filter(|d| !d.is_empty()) {
    return dir.into();
  }
  let home = std::env::var_os("HOME")
    .or_else(|| std::env::var_os("USERPROFILE"))
    .unwrap_or_else(|| ".".into());
  std::path::PathBuf::from(home).join(".treq").join("repos")
}

/// A repository on this machine for `spec`: an existing folder as it is, or
/// a URL (or `owner/name`) cloned once under `root` and reused after.
/// Returns the folder and whether this call cloned it.
pub fn ensure_repo(spec: &str, root: &Path) -> Result<(String, bool), String> {
  let as_path = Path::new(spec.trim());
  if as_path.is_dir() {
    let path = std::fs::canonicalize(as_path).map_err(|e| e.to_string())?;
    return Ok((path.to_string_lossy().to_string(), false));
  }
  let dest = managed_clone_path(root, spec)
    .ok_or_else(|| format!("'{spec}' is not a folder or a repository URL"))?;
  if dest.join(".git").exists() {
    return Ok((dest.to_string_lossy().to_string(), false));
  }
  if let Some(parent) = dest.parent() {
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
  }
  let dest_str = dest.to_string_lossy().to_string();
  crate::core::remote::clone_repository(&clone_url(spec), &dest_str)?;
  Ok((dest_str, true))
}

/// One repository of a project as `treq project status` reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoOverview {
  pub path: String,
  pub exists: bool,
  pub workspaces: Vec<WorkspaceOverview>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOverview {
  pub branch: String,
  pub title: String,
  pub target_branch: Option<String>,
  pub path: String,
}

/// Every repository of `project` and the workspaces each one lists, read
/// from the repositories' own databases. A missing repository lists none.
pub fn overview(project: &Project) -> Vec<RepoOverview> {
  project
    .repos
    .iter()
    .map(|path| {
      let exists = Path::new(path).is_dir();
      let workspaces = if exists {
        crate::local_db::get_workspaces(path)
          .unwrap_or_default()
          .into_iter()
          .filter(|w| !w.archived)
          .map(|w| WorkspaceOverview {
            branch: w.branch_name,
            title: w.title,
            target_branch: w.target_branch,
            path: w.workspace_path,
          })
          .collect()
      } else {
        Vec::new()
      };
      RepoOverview {
        path: path.clone(),
        exists,
        workspaces,
      }
    })
    .collect()
}

/// Saved projects that list repository `repo`.
pub fn projects_with_repo(conn: &Connection, repo: &str) -> Result<Vec<Project>, String> {
  Ok(
    list(conn)?
      .into_iter()
      .filter(|p| p.repos.iter().any(|r| r == repo))
      .collect(),
  )
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
  fn new_project_has_no_prompt_or_context() {
    let conn = conn();
    let p = save(&conn, "P", &repos(&["/r/a"])).expect("save");
    assert_eq!(p.prompt, "");
    assert!(p.context_paths.is_empty());
  }

  #[test]
  fn stores_prompt_and_context_paths() {
    let conn = conn();
    let p = save(&conn, "P", &repos(&["/r/a"])).expect("save");
    set_context(
      &conn,
      &p.id,
      "  Use pnpm.\nKeep API and web in sync.  ",
      &repos(&[
        "/docs/style.md",
        " ",
        "/docs/style.md",
        "https://x.dev/spec",
      ]),
    )
    .expect("set");
    let p = get(&conn, &p.id).unwrap().unwrap();
    assert_eq!(p.prompt, "Use pnpm.\nKeep API and web in sync.");
    assert_eq!(
      p.context_paths,
      repos(&["/docs/style.md", "https://x.dev/spec"])
    );
  }

  #[test]
  fn setting_context_on_unknown_project_fails() {
    let conn = conn();
    assert!(set_context(&conn, "missing", "x", &[]).is_err());
  }

  #[test]
  fn init_adds_context_columns_to_an_older_table() {
    let conn = Connection::open_in_memory().expect("open");
    conn
      .execute_batch(
        "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL,
           created_at TEXT NOT NULL, last_opened_at TEXT NOT NULL);
         INSERT INTO projects VALUES ('p', 'Old', 'a', 'a');",
      )
      .expect("old schema");
    init(&conn).expect("init");
    let p = get(&conn, "p").unwrap().unwrap();
    assert_eq!(p.prompt, "");
  }

  #[test]
  fn managed_clone_path_follows_host_owner_and_name() {
    let root = Path::new("/home/me/.treq/repos");
    for url in [
      "https://github.com/tznc/my-repo.git",
      "https://github.com/tznc/my-repo",
      "git@github.com:tznc/my-repo.git",
      "ssh://git@github.com/tznc/my-repo",
    ] {
      assert_eq!(
        managed_clone_path(root, url).expect(url),
        root.join("github.com/tznc/my-repo"),
        "{url}"
      );
    }
    assert_eq!(
      managed_clone_path(root, "tznc/my-repo").unwrap(),
      root.join("github.com/tznc/my-repo")
    );
  }

  #[test]
  fn managed_clone_path_rejects_what_is_not_a_repo_url() {
    let root = Path::new("/r");
    assert!(managed_clone_path(root, "/home/me/code/app").is_none());
    assert!(managed_clone_path(root, "https://github.com/../../etc").is_none());
    assert!(managed_clone_path(root, "").is_none());
  }

  #[test]
  fn clone_url_expands_owner_slash_name_to_github() {
    assert_eq!(
      clone_url("tznc/my-repo"),
      "https://github.com/tznc/my-repo.git"
    );
    assert_eq!(
      clone_url("git@github.com:a/b.git"),
      "git@github.com:a/b.git"
    );
  }

  #[test]
  fn finds_projects_that_hold_a_repo() {
    let conn = conn();
    let a = save(&conn, "A", &repos(&["/r/api", "/r/web"])).expect("a");
    save(&conn, "B", &repos(&["/r/ios"])).expect("b");
    let found: Vec<String> = projects_with_repo(&conn, "/r/api")
      .unwrap()
      .into_iter()
      .map(|p| p.id)
      .collect();
    assert_eq!(found, vec![a.id]);
    assert!(projects_with_repo(&conn, "/r/none").unwrap().is_empty());
  }

  #[test]
  fn ensure_repo_takes_an_existing_folder_as_is() {
    let dir = tempfile::TempDir::new().unwrap();
    let (path, cloned) = ensure_repo(dir.path().to_str().unwrap(), Path::new("/unused")).unwrap();
    assert!(!cloned);
    assert_eq!(Path::new(&path), std::fs::canonicalize(dir.path()).unwrap());
  }

  #[test]
  fn ensure_repo_reuses_an_earlier_clone() {
    let root = tempfile::TempDir::new().unwrap();
    let dest = root.path().join("github.com/tznc/app");
    std::fs::create_dir_all(dest.join(".git")).unwrap();
    let (path, cloned) = ensure_repo("tznc/app", root.path()).unwrap();
    assert!(!cloned, "no second clone");
    assert_eq!(Path::new(&path), dest);
  }

  #[test]
  fn ensure_repo_clones_a_url_once() {
    // A local bare repository stands in for the remote: no network.
    let src = tempfile::TempDir::new().unwrap();
    let work = src.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let git = |args: &[&str], dir: &Path| {
      let ok = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .unwrap()
        .success();
      assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"], &work);
    std::fs::write(work.join("README.md"), "hi").unwrap();
    git(&["add", "."], &work);
    git(&["commit", "-qm", "init"], &work);
    let url = format!("file://{}", work.display());
    // file:// URLs have no owner/name, so clone into an explicit layout.
    let root = tempfile::TempDir::new().unwrap();
    let spec = format!("{url}/x");
    assert!(managed_clone_path(root.path(), &spec).is_some());
    let dest = root.path().join("clone");
    crate::core::remote::clone_repository(&url, dest.to_str().unwrap()).unwrap();
    assert!(dest.join("README.md").exists());
  }

  #[test]
  fn overview_lists_missing_repos_without_workspaces() {
    let project = Project {
      id: "p".into(),
      name: "P".into(),
      repos: repos(&["/definitely/missing/repo"]),
      created_at: String::new(),
      last_opened_at: String::new(),
      prompt: String::new(),
      context_paths: Vec::new(),
    };
    let overview = overview(&project);
    assert_eq!(overview.len(), 1);
    assert!(!overview[0].exists && overview[0].workspaces.is_empty());
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
