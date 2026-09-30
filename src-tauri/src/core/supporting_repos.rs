//! Supporting repositories: other local repositories linked to a main
//! repository so one Treq window can work on all of them. The list lives in
//! the main repository's `.treq/local.db`. The link is one-way; a supporting
//! repository keeps its own `.treq/local.db` for its workspaces and sessions.

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

/// Validates `candidate` and links it to `main_repo`. Returns the stored
/// (canonical) path.
pub fn add(main_repo: &str, candidate: &str) -> Result<String, String> {
  let main = canonical(Path::new(main_repo))?;
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
  if path == main {
    return Err("This is the main repository".to_string());
  }
  if path.starts_with(&main) {
    return Err("A repository inside the main repository cannot be added".to_string());
  }
  if main.starts_with(&path) {
    return Err("A repository that contains the main repository cannot be added".to_string());
  }
  let path_str = path.to_string_lossy().to_string();
  if local_db::list_supporting_repo_paths(main_repo)?.contains(&path_str) {
    return Err("This repository is already added".to_string());
  }
  local_db::add_supporting_repo_path(main_repo, &path_str)?;
  Ok(path_str)
}

pub fn remove(main_repo: &str, path: &str) -> Result<(), String> {
  local_db::remove_supporting_repo_path(main_repo, path)
}

/// Resolves a CLI `-r` value to one of `main_repo`'s supporting repositories.
/// Accepts a path (absolute, or relative to `cwd`), a directory name, or a
/// trailing path such as `org/app`.
pub fn resolve(main_repo: &str, spec: &str, cwd: &Path) -> Result<String, String> {
  let paths = local_db::list_supporting_repo_paths(main_repo)?;
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
    if as_path.is_absolute() && spec.contains(std::path::MAIN_SEPARATOR) {
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

  #[test]
  fn adds_and_lists_supporting_repo() {
    let temp = TempDir::new().unwrap();
    let main = make_repo(temp.path(), "main");
    let svc = make_repo(temp.path(), "svc");

    assert_eq!(add(&main, &svc).unwrap(), svc);
    assert_eq!(
      list(&main).unwrap(),
      vec![SupportingRepo {
        path: svc,
        exists: true
      }]
    );
  }

  #[test]
  fn rejects_main_duplicate_nested_containing_and_non_repo() {
    let temp = TempDir::new().unwrap();
    let outer = make_repo(temp.path(), "outer");
    let main = make_repo(temp.path(), "outer/main");
    let nested = make_repo(temp.path(), "outer/main/nested");
    let svc = make_repo(temp.path(), "svc");
    let plain = temp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();

    assert!(add(&main, &main).unwrap_err().contains("main repository"));
    assert!(add(&main, &nested).unwrap_err().contains("inside"));
    assert!(add(&main, &outer).unwrap_err().contains("contains"));
    assert!(add(&main, plain.to_str().unwrap())
      .unwrap_err()
      .contains("not a repository"));
    add(&main, &svc).unwrap();
    assert!(add(&main, &svc).unwrap_err().contains("already"));
  }

  #[test]
  fn rejects_treq_workspace_dir() {
    let temp = TempDir::new().unwrap();
    let main = make_repo(temp.path(), "main");
    let ws = temp.path().join("other/.treq/workspaces/feat");
    std::fs::create_dir_all(ws.join(".jj")).unwrap();

    assert!(add(&main, ws.to_str().unwrap())
      .unwrap_err()
      .contains("workspace"));
  }

  #[test]
  fn remove_unlinks_repo_and_marks_missing_paths() {
    let temp = TempDir::new().unwrap();
    let main = make_repo(temp.path(), "main");
    let a = make_repo(temp.path(), "a");
    let b = make_repo(temp.path(), "b");
    add(&main, &a).unwrap();
    add(&main, &b).unwrap();

    remove(&main, &a).unwrap();
    std::fs::remove_dir_all(&b).unwrap();

    assert_eq!(
      list(&main).unwrap(),
      vec![SupportingRepo {
        path: b,
        exists: false
      }]
    );
  }

  #[test]
  fn resolves_by_name_suffix_and_path() {
    let temp = TempDir::new().unwrap();
    let main = make_repo(temp.path(), "main");
    let svc = make_repo(temp.path(), "svc");
    let org_app = make_repo(temp.path(), "org/app");
    let other_app = make_repo(temp.path(), "other/app");
    for repo in [&svc, &org_app, &other_app] {
      add(&main, repo).unwrap();
    }
    let cwd = Path::new(&main);

    assert_eq!(resolve(&main, "svc", cwd).unwrap(), svc);
    assert_eq!(resolve(&main, "../svc", cwd).unwrap(), svc);
    assert_eq!(resolve(&main, &svc, cwd).unwrap(), svc);
    assert_eq!(resolve(&main, "org/app", cwd).unwrap(), org_app);
    assert!(resolve(&main, "app", cwd).unwrap_err().contains("several"));
    assert!(resolve(&main, "nope", cwd)
      .unwrap_err()
      .contains("not a supporting repository"));
  }

  #[test]
  fn resolve_rejects_repo_that_is_not_linked() {
    let temp = TempDir::new().unwrap();
    let main = make_repo(temp.path(), "main");
    let stranger = make_repo(temp.path(), "stranger");

    assert!(resolve(&main, &stranger, Path::new(&main))
      .unwrap_err()
      .contains("none are added"));
  }
}
