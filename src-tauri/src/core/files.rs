use crate::core::WorkspaceEntry;
use crate::jj;
use crate::local_db;
use std::collections::HashSet;

/// The indexed files of a workspace (or the home repo) whose path matches
/// `query`, as paths relative to it; the shortest paths first when it is empty.
pub fn search_workspace_files(
  repo_path: &str,
  workspace_id: Option<i64>,
  query: &str,
  limit: usize,
) -> Result<Vec<String>, String> {
  Ok(
    local_db::search_workspace_files(repo_path, workspace_id, query, limit)?
      .into_iter()
      .map(|file| file.relative_path)
      .collect(),
  )
}

/// Lists the top-level entries of a workspace (or the home repo when
/// `workspace_id` is `None`), each annotated with its file status for the
/// Code tab's file list. Costs three jj computations on top of a plain
/// `ls_workspace`, so callers that don't need status use that instead.
pub fn list_workspace_files(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<Vec<WorkspaceEntry>, String> {
  let workspace_root = crate::core::workspaces::resolve_workspace_root(repo_path, workspace_id)?;
  let mut entries = crate::core::ls_workspace(repo_path, workspace_id)?;

  let target_branch = workspace_id.and_then(|id| {
    local_db::get_workspace_by_id(repo_path, id)
      .ok()
      .flatten()
      .and_then(|ws| ws.target_branch)
  });
  annotate_entry_statuses(&workspace_root, &mut entries, target_branch.as_deref());

  Ok(entries)
}

/// Status of a file (or, for a directory, the most severe status among its
/// descendants) relative to the working copy and target branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileStatus {
  /// Unresolved conflict somewhere in the tree.
  Conflicted,
  /// Modified by the mutable working-copy change (`@`).
  WorkingCopy,
  /// Modified by an already-committed ancestor, relative to the target branch.
  Committed,
  /// No change in the working copy or the workspace's commits.
  Untouched,
}

impl FileStatus {
  fn as_str(self) -> &'static str {
    match self {
      FileStatus::Conflicted => "conflict",
      FileStatus::WorkingCopy => "workingCopy",
      FileStatus::Committed => "committed",
      FileStatus::Untouched => "untouched",
    }
  }
}

/// Populate `status` on each entry (file or directory) in place, based on
/// each entry's path relative to `workspace_path`.
pub fn annotate_entry_statuses(
  workspace_path: &str,
  entries: &mut [WorkspaceEntry],
  target_branch: Option<&str>,
) {
  let conflicted: HashSet<String> = jj::get_conflicted_files(workspace_path, target_branch)
    .unwrap_or_default()
    .into_iter()
    .collect();
  let working_copy: HashSet<String> = jj::jj_get_changed_files(workspace_path)
    .unwrap_or_default()
    .into_iter()
    .map(|f| f.path)
    .collect();
  let committed: HashSet<String> = jj::jj_get_committed_files(workspace_path, target_branch)
    .unwrap_or_default()
    .into_iter()
    .collect();

  for entry in entries.iter_mut() {
    let rel_path = relative_path(workspace_path, &entry.path);
    let status = if entry.is_directory {
      directory_status(&rel_path, &conflicted, &working_copy, &committed)
    } else {
      file_status(&rel_path, &conflicted, &working_copy, &committed)
    };
    entry.status = Some(status.unwrap_or(FileStatus::Untouched).as_str().to_string());
  }
}

fn relative_path(workspace_path: &str, full_path: &str) -> String {
  let prefix = format!("{}/", workspace_path.trim_end_matches('/'));
  full_path
    .strip_prefix(&prefix)
    .unwrap_or(full_path)
    .to_string()
}

fn file_status(
  rel_path: &str,
  conflicted: &HashSet<String>,
  working_copy: &HashSet<String>,
  committed: &HashSet<String>,
) -> Option<FileStatus> {
  if conflicted.contains(rel_path) {
    Some(FileStatus::Conflicted)
  } else if working_copy.contains(rel_path) {
    Some(FileStatus::WorkingCopy)
  } else if committed.contains(rel_path) {
    Some(FileStatus::Committed)
  } else {
    None
  }
}

fn directory_status(
  rel_dir: &str,
  conflicted: &HashSet<String>,
  working_copy: &HashSet<String>,
  committed: &HashSet<String>,
) -> Option<FileStatus> {
  let prefix = format!("{}/", rel_dir);
  let contains = |set: &HashSet<String>| set.iter().any(|path| path.starts_with(&prefix));

  if contains(conflicted) {
    Some(FileStatus::Conflicted)
  } else if contains(working_copy) {
    Some(FileStatus::WorkingCopy)
  } else if contains(committed) {
    Some(FileStatus::Committed)
  } else {
    None
  }
}

/// When `path` was last modified, as RFC 3339; `None` when it is missing.
pub fn file_modified_at(path: &str) -> Option<String> {
  std::fs::metadata(path)
    .ok()
    .and_then(|metadata| metadata.modified().ok())
    .map(|modified| chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339())
}

/// The immediate children of the directory at `path`, leaving out what the
/// repository's ignore files ignore: directories first, then by name.
pub fn list_directory(path: &str) -> Result<Vec<WorkspaceEntry>, String> {
  use std::path::Path;

  let base_path = Path::new(path);
  if !base_path.is_dir() {
    return Err(format!("Directory does not exist: {}", path));
  }
  let mut files = Vec::new();

  let walker = ignore::WalkBuilder::new(path)
    .max_depth(Some(1))
    .hidden(false)
    .git_ignore(true)
    .git_global(true)
    .git_exclude(true)
    .parents(true)
    .build();

  for entry in walker.flatten() {
    let entry_path = entry.path();
    if entry_path == base_path {
      continue;
    }
    if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
      // The type comes with the directory listing; only a symlink needs a
      // stat to tell whether it leads to a directory. No caller reads the
      // entries' times, and a stat per entry was most of a large listing.
      let is_directory = match entry.file_type() {
        Some(t) if !t.is_symlink() => t.is_dir(),
        _ => entry_path.is_dir(),
      };
      files.push(WorkspaceEntry {
        name: name.to_string(),
        modified_at: None,
        path: entry_path.to_string_lossy().to_string(),
        is_directory,
        submodule_pin: None,
        submodule_synced: None,
        status: None,
      });
    }
  }

  files.sort_by(|a, b| match (a.is_directory, b.is_directory) {
    (true, false) => std::cmp::Ordering::Less,
    (false, true) => std::cmp::Ordering::Greater,
    _ => a.name.cmp(&b.name),
  });

  Ok(files)
}

#[cfg(test)]
mod list_directory_tests {
  use super::*;
  use tempfile::TempDir;

  #[test]
  fn lists_directories_before_files() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.txt"), "a").unwrap();
    std::fs::create_dir(temp.path().join("z")).unwrap();
    let entries = list_directory(&temp.path().to_string_lossy()).unwrap();
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["z", "a.txt"]);
    assert!(entries[0].is_directory);
  }

  #[cfg(unix)]
  #[test]
  fn a_symlink_to_a_directory_lists_as_one() {
    let temp = TempDir::new().unwrap();
    std::fs::create_dir(temp.path().join("real")).unwrap();
    std::fs::write(temp.path().join("file"), "a").unwrap();
    std::os::unix::fs::symlink(temp.path().join("real"), temp.path().join("link")).unwrap();
    std::os::unix::fs::symlink(temp.path().join("file"), temp.path().join("file-link")).unwrap();
    let entries = list_directory(&temp.path().to_string_lossy()).unwrap();
    let dir = |name: &str| {
      entries
        .iter()
        .find(|e| e.name == name)
        .unwrap()
        .is_directory
    };
    assert!(dir("real") && dir("link"));
    assert!(!dir("file") && !dir("file-link"));
  }

  #[test]
  fn returns_error_when_directory_missing() {
    let temp = TempDir::new().unwrap();
    assert!(list_directory(&temp.path().join("gone").to_string_lossy()).is_err());
  }

  #[test]
  fn modified_at_is_none_for_missing_file() {
    let temp = TempDir::new().unwrap();
    assert_eq!(
      file_modified_at(&temp.path().join("gone").to_string_lossy()),
      None
    );
  }
}
