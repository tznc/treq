//! Repositories alongside one another in a window. A project
//! (`core::projects`) holds the list; this module checks a repository can
//! join one, and resolves `treq -r`. The list a repository's `.treq/local.db`
//! kept before projects is still read, to seed an unsaved window's draft.

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::local_db;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportingRepo {
  pub path: String,
  /// False when the directory no longer exists.
  pub exists: bool,
}

fn canonical(path: &Path) -> Result<PathBuf, String> {
  std::fs::canonicalize(path).map_err(|e| format!("Cannot open '{}': {}", path.display(), e))
}

fn is_repo_root(dir: &Path) -> bool {
  dir.join(".git").exists() || dir.join(".jj").is_dir()
}

fn is_treq_workspace_dir(dir: &Path) -> bool {
  dir.components().any(|c| c.as_os_str() == ".treq")
}

pub fn list(main_repo: &str) -> Result<Vec<SupportingRepo>, String> {
  Ok(
    local_db::list_supporting_repo_paths(main_repo)?
      .into_iter()
      .map(|path| SupportingRepo {
        exists: Path::new(&path).is_dir(),
        path,
      })
      .collect(),
  )
}

/// Checks `candidate` can join a window on `main_repo` (empty: a project
/// with no repository yet). Returns its canonical path.
pub fn validate(main_repo: &str, candidate: &str) -> Result<String, String> {
  let path = canonical(Path::new(candidate))?;
  if !path.is_dir() {
    return Err(format!("'{}' is not a directory", path.display()));
  }
  if is_treq_workspace_dir(&path) {
    return Err("A Treq workspace cannot be added as a repository".to_string());
  }
  if !is_repo_root(&path) {
    return Err(format!(
      "'{}' is not a repository root (no .git or .jj)",
      path.display()
    ));
  }
  if !main_repo.is_empty() {
    let main = canonical(Path::new(main_repo))?;
    if path == main {
      return Err("This is the main repository".to_string());
    }
    if path.starts_with(&main) {
      return Err("A repository inside the main repository cannot be added".to_string());
    }
    if main.starts_with(&path) {
      return Err("A repository that contains the main repository cannot be added".to_string());
    }
  }
  Ok(path.to_string_lossy().to_string())
}

/// Resolves a CLI `-r` value to one of `paths`, the repositories alongside
/// the current one. Accepts a path (absolute, or relative to `cwd`), a
/// directory name, or a trailing path such as `org/app`.
pub fn resolve(paths: &[String], spec: &str, cwd: &Path) -> Result<String, String> {
  let unknown = || {
    let available = paths
      .iter()
      .map(|p| p.as_str())
      .collect::<Vec<_>>()
      .join(", ");
    if available.is_empty() {
      format!("'{spec}' is not a supporting repository: none are added")
    } else {
      format!("'{spec}' is not a supporting repository. Available: {available}")
    }
  };

  if let Ok(as_path) = std::fs::canonicalize(cwd.join(spec)) {
    let as_str = as_path.to_string_lossy().to_string();
    if paths.contains(&as_str) {
      return Ok(as_str);
    }
    // An absolute path names one directory; do not fall back to suffixes.
    if Path::new(spec).is_absolute() {
      return Err(unknown());
    }
  }

  let suffix: Vec<&str> = spec.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
  let matches: Vec<&String> = paths
    .iter()
    .filter(|path| {
      let parts: Vec<String> = Path::new(path.as_str())
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
      !suffix.is_empty()
        && parts.ends_with(&suffix.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    })
    .collect();
  match matches.as_slice() {
    [one] => Ok((*one).clone()),
    [] => Err(unknown()),
    many => Err(format!(
      "'{spec}' matches several supporting repositories: {}. Use a longer name or a path",
      many
        .iter()
        .map(|p| p.as_str())
        .collect::<Vec<_>>()
        .join(", ")
    )),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn make_repo(root: &Path, rel: &str) -> String {
    let dir = root.join(rel);
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::canonicalize(dir)
      .unwrap()
      .to_string_lossy()
      .to_string()
  }

  fn link(main: &str, repo: &str) {
    local_db::add_supporting_repo_path(main, repo).unwrap();
  }

  #[test]
  fn lists_linked_repos_and_marks_missing_paths() {
    let temp = TempDir::new().unwrap();
    let main = make_repo(temp.path(), "main");
    let a = make_repo(temp.path(), "a");
    link(&main, &a);
    std::fs::remove_dir_all(&a).unwrap();
    assert_eq!(
      list(&main).unwrap(),
      vec![SupportingRepo {
        path: a,
        exists: false
      }]
    );
  }

  #[test]
  fn validates_a_repo_to_add() {
    let temp = TempDir::new().unwrap();
    let outer = make_repo(temp.path(), "outer");
    let main = make_repo(temp.path(), "outer/main");
    let nested = make_repo(temp.path(), "outer/main/nested");
    let svc = make_repo(temp.path(), "svc");
    let plain = temp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();

    assert_eq!(validate(&main, &svc).unwrap(), svc);
    assert!(validate(&main, &main)
      .unwrap_err()
      .contains("main repository"));
    assert!(validate(&main, &nested).unwrap_err().contains("inside"));
    assert!(validate(&main, &outer).unwrap_err().contains("contains"));
    assert!(validate(&main, plain.to_str().unwrap())
      .unwrap_err()
      .contains("not a repository"));
  }

  #[test]
  fn an_empty_project_takes_any_repo() {
    let temp = TempDir::new().unwrap();
    let svc = make_repo(temp.path(), "svc");
    assert_eq!(validate("", &svc).unwrap(), svc);
  }

  #[test]
  fn rejects_treq_workspace_dir() {
    let temp = TempDir::new().unwrap();
    let main = make_repo(temp.path(), "main");
    let ws = temp.path().join("other/.treq/workspaces/feat");
    std::fs::create_dir_all(ws.join(".jj")).unwrap();

    assert!(validate(&main, ws.to_str().unwrap())
      .unwrap_err()
      .contains("workspace"));
  }

  #[test]
  fn resolves_by_name_suffix_and_path() {
    let temp = TempDir::new().unwrap();
    let main = make_repo(temp.path(), "main");
    let svc = make_repo(temp.path(), "svc");
    let org_app = make_repo(temp.path(), "org/app");
    let other_app = make_repo(temp.path(), "other/app");
    let paths = vec![svc.clone(), org_app.clone(), other_app];
    let cwd = Path::new(&main);

    assert_eq!(resolve(&paths, "svc", cwd).unwrap(), svc);
    assert_eq!(resolve(&paths, "../svc", cwd).unwrap(), svc);
    assert_eq!(resolve(&paths, &svc, cwd).unwrap(), svc);
    assert_eq!(resolve(&paths, "org/app", cwd).unwrap(), org_app);
    assert!(resolve(&paths, "app", cwd).unwrap_err().contains("several"));
    assert!(resolve(&paths, "nope", cwd)
      .unwrap_err()
      .contains("not a supporting repository"));
  }

  #[test]
  fn resolve_suffix_is_not_shadowed_by_same_named_dir_under_cwd() {
    let temp = TempDir::new().unwrap();
    let main = make_repo(temp.path(), "main");
    let org_app = make_repo(temp.path(), "org/app");
    std::fs::create_dir_all(Path::new(&main).join("org/app")).unwrap();

    assert_eq!(
      resolve(std::slice::from_ref(&org_app), "org/app", Path::new(&main)).unwrap(),
      org_app
    );
  }

  #[test]
  fn resolve_rejects_repo_that_is_not_linked() {
    let temp = TempDir::new().unwrap();
    let main = make_repo(temp.path(), "main");
    let stranger = make_repo(temp.path(), "stranger");

    assert!(resolve(&[], &stranger, Path::new(&main))
      .unwrap_err()
      .contains("none are added"));
  }
}
