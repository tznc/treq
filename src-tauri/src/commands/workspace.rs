use crate::file_indexer::FileIndex;
use crate::local_db::Workspace;
use crate::lock_ext::LockExt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

// The file indexes of the workspaces searched lately, least recently used
// dropped past `INDEXES`: an index holds every tracked path (about 200 bytes
// each), and a day's work visits more workspaces than are searched at once.
// Each workspace has its own slot, whose lock is held while the first index
// builds, so a search waits for an index another thread (the warm-up when a
// workspace shows) is still building.
const INDEXES: usize = 4;

#[derive(Default)]
struct IndexSlot {
  index: Mutex<Option<Arc<FileIndex>>>,
  /// A thread is building the index again.
  rebuilding: AtomicBool,
  /// The files changed since the last build began.
  stale: AtomicBool,
}

#[derive(Default)]
struct Indexes {
  slots: HashMap<String, (Arc<IndexSlot>, u64)>,
  used: u64,
}

static FILE_INDEXES: OnceLock<Mutex<Indexes>> = OnceLock::new();

impl Indexes {
  /// The workspace's slot, made if need be; the least recently used slots
  /// past [`INDEXES`] are dropped.
  fn slot(&mut self, workspace_path: &str) -> Arc<IndexSlot> {
    self.used += 1;
    let used = self.used;
    let slot = self.slots.entry(workspace_path.to_string()).or_default();
    slot.1 = used;
    let slot = slot.0.clone();
    while self.slots.len() > INDEXES {
      let oldest = self
        .slots
        .iter()
        .min_by_key(|(_, (_, used))| *used)
        .map(|(path, _)| path.clone());
      let Some(oldest) = oldest else { break };
      self.slots.remove(&oldest);
    }
    slot
  }
}

fn index_slot(workspace_path: &str) -> Arc<IndexSlot> {
  FILE_INDEXES
    .get_or_init(Mutex::default)
    .lock_or_recover()
    .slot(workspace_path)
}

/// Fires the repo's `.treq/config.yaml` `setup_script` (if any) for a newly
/// created workspace, in the background so workspace creation itself isn't
/// slowed down. Checks are blocked for the workspace until this finishes —
/// see `core::checks::ensure_setup_script_complete`.
pub(crate) fn spawn_setup_script_if_configured(workspace: &Workspace) {
  let script = match crate::core::configured_setup_script(&workspace.repo_path) {
    Ok(Some(script)) => script,
    Ok(None) => return,
    Err(e) => {
      log::warn!("Failed to read .treq/config.yaml for setup_script: {}", e);
      return;
    }
  };

  let repo_path = workspace.repo_path.clone();
  let workspace_id = workspace.id;
  let workspace_path = crate::auto_rebase::get_full_workspace_path(workspace);
  std::thread::spawn(move || {
    if let Err(e) =
      crate::core::run_setup_script_sync(&repo_path, workspace_id, &workspace_path, &script)
    {
      log::warn!(
        "Setup script failed for workspace {} in {}: {}",
        workspace_id,
        repo_path,
        e
      );
    }
  });
}

/// The workspace's file index, built on first use. Later calls return the
/// index as last built; [`refresh_workspace_file_index`] reads the files again.
pub fn workspace_file_index(workspace_path: &str) -> Result<Arc<FileIndex>, String> {
  let slot = index_slot(workspace_path);
  let mut index = slot.index.lock_or_recover();
  if let Some(index) = index.as_ref() {
    return Ok(index.clone());
  }
  let built = Arc::new(FileIndex::build(workspace_path)?);
  *index = Some(built.clone());
  Ok(built)
}

/// Builds the workspace's index, or builds it again so files created or
/// removed since appear. Searches meanwhile use the index they had. Calls
/// that arrive while a build runs share the one build that follows it.
pub fn refresh_workspace_file_index(workspace_path: &str) -> Result<(), String> {
  refresh_with(workspace_path, FileIndex::build)
}

fn refresh_with(
  workspace_path: &str,
  build: impl Fn(&str) -> Result<FileIndex, String>,
) -> Result<(), String> {
  let slot = index_slot(workspace_path);
  if slot.index.lock_or_recover().is_none() {
    // Nothing to search yet: the first build is the one searches wait for.
    let mut index = slot.index.lock_or_recover();
    if index.is_none() {
      *index = Some(Arc::new(build(workspace_path)?));
    }
    return Ok(());
  }
  slot.stale.store(true, Ordering::SeqCst);
  loop {
    if slot.rebuilding.swap(true, Ordering::SeqCst) {
      // The thread building sees `stale` when it finishes.
      return Ok(());
    }
    while slot.stale.swap(false, Ordering::SeqCst) {
      match build(workspace_path) {
        Ok(built) => *slot.index.lock_or_recover() = Some(Arc::new(built)),
        Err(error) => {
          slot.rebuilding.store(false, Ordering::SeqCst);
          return Err(error);
        }
      }
    }
    slot.rebuilding.store(false, Ordering::SeqCst);
    // A change that arrived between the last check and the release.
    if !slot.stale.load(Ordering::SeqCst) {
      return Ok(());
    }
  }
}

#[cfg(test)]
mod tests {
  // TODO: Add unit tests when a mockable workspace DB abstraction exists.
  use super::*;
  use std::fs;
  use std::sync::atomic::AtomicUsize;
  use tempfile::TempDir;

  /// Lists the files in `dir`, as the jj snapshot the real index is built from.
  fn list(dir: &str) -> Result<FileIndex, String> {
    let mut paths: Vec<String> = fs::read_dir(dir)
      .map_err(|e| e.to_string())?
      .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
      .collect();
    paths.sort();
    Ok(FileIndex::new(paths))
  }

  #[test]
  fn a_file_created_after_the_index_was_built_appears_once_it_is_refreshed() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    fs::write(dir.path().join("old.rs"), "").unwrap();
    refresh_with(path, list).unwrap();
    assert_eq!(workspace_file_index(path).unwrap().len(), 1);

    fs::write(dir.path().join("new.rs"), "").unwrap();
    // Until the refresh, searches keep the index they had.
    assert!(workspace_file_index(path)
      .unwrap()
      .search("new", 10)
      .is_empty());
    refresh_with(path, list).unwrap();
    assert_eq!(
      workspace_file_index(path).unwrap().search("new", 10),
      vec!["new.rs".to_string()]
    );
  }

  #[test]
  fn overlapping_refreshes_share_one_follow_up_build() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap().to_string();
    refresh_with(&path, list).unwrap();
    let builds = Arc::new(AtomicUsize::new(0));
    let slot = index_slot(&path);
    slot.rebuilding.store(true, Ordering::SeqCst);
    for _ in 0..5 {
      let counted = builds.clone();
      refresh_with(&path, move |dir| {
        counted.fetch_add(1, Ordering::SeqCst);
        list(dir)
      })
      .unwrap();
    }
    // A build is running, so none started; it will see the file change flag.
    assert_eq!(builds.load(Ordering::SeqCst), 0);
    assert!(slot.stale.load(Ordering::SeqCst));
  }

  #[test]
  fn only_the_recently_searched_workspaces_keep_an_index() {
    let mut indexes = Indexes::default();
    for n in 0..INDEXES + 2 {
      indexes.slot(&format!("/w/{n}"));
      // Workspace 0 is searched again and again.
      indexes.slot("/w/0");
    }
    assert_eq!(indexes.slots.len(), INDEXES);
    assert!(indexes.slots.contains_key("/w/0"));
    assert!(indexes.slots.contains_key(&format!("/w/{}", INDEXES + 1)));
    assert!(!indexes.slots.contains_key("/w/1"));
  }

  // Unit tests cover DB cleanup only; full jj+directory cleanup is e2e-tested.
  #[test]
  fn test_delete_workspace_cleans_up_db_entry() {
    use crate::local_db;

    // Setup: Create a temp directory with a fake workspace under .treq/workspaces/
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();
    let workspaces_dir = temp_dir.path().join(".treq").join("workspaces");
    let workspace_dir = workspaces_dir.join("test_workspace");
    fs::create_dir_all(&workspace_dir).unwrap();

    // Add workspace to DB with just the directory name (matching production behavior)
    local_db::add_workspace(
      repo_path,
      "test".to_string(),
      "test_workspace".to_string(),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .unwrap();

    // Get the workspace ID
    let workspaces = local_db::get_workspaces(repo_path).unwrap();
    assert_eq!(workspaces.len(), 1);
    let workspace_id = workspaces[0].id;

    // Act: delete workspace; jj forget is expected best-effort without a real jj repo.
    let result = crate::core::delete_workspace(repo_path, &workspace_id);

    // Assert: Should succeed (jj errors are non-fatal)
    assert!(
      result.is_ok(),
      "delete_workspace should succeed: {:?}",
      result
    );

    // Assert: DB entry should be removed
    let workspaces_after = local_db::get_workspaces(repo_path).unwrap();
    assert_eq!(
      workspaces_after.len(),
      0,
      "Workspace should be removed from database"
    );
  }

  #[test]
  fn test_delete_workspace_removes_db_even_if_directory_missing() {
    use crate::local_db;

    // Setup: Create a temp directory for the repo
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();

    // Create .treq/workspaces/ but NOT the workspace directory itself
    let workspaces_dir = temp_dir.path().join(".treq").join("workspaces");
    fs::create_dir_all(&workspaces_dir).unwrap();

    // Add workspace to DB with just the directory name (orphaned entry - directory doesn't exist)
    local_db::add_workspace(
      repo_path,
      "test".to_string(),
      "nonexistent_workspace".to_string(),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .unwrap();

    let workspaces = local_db::get_workspaces(repo_path).unwrap();
    assert_eq!(workspaces.len(), 1);
    let workspace_id = workspaces[0].id;

    // Act: Delete the workspace (directory doesn't exist)
    let result = crate::core::delete_workspace(repo_path, &workspace_id);

    // Assert: Should still succeed (core::delete_workspace handles missing directories)
    assert!(
      result.is_ok(),
      "delete_workspace should succeed even when directory missing: {:?}",
      result
    );

    // Assert: DB entry should be removed
    let workspaces_after = local_db::get_workspaces(repo_path).unwrap();
    assert_eq!(
      workspaces_after.len(),
      0,
      "Workspace should be removed from database even if directory was missing"
    );
  }

  #[test]
  fn test_archive_workspace_keeps_db_entry() {
    use crate::local_db;

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_str().unwrap();
    let workspaces_dir = temp_dir.path().join(".treq").join("workspaces");
    let workspace_dir = workspaces_dir.join("test_workspace");
    fs::create_dir_all(&workspace_dir).unwrap();

    local_db::add_workspace(
      repo_path,
      "test".to_string(),
      "test_workspace".to_string(),
      "test-branch".to_string(),
      None,
      None,
      None,
    )
    .unwrap();

    let workspaces = local_db::get_workspaces(repo_path).unwrap();
    assert_eq!(workspaces.len(), 1);
    let workspace_id = workspaces[0].id;

    let result = crate::core::archive_workspace_removing_directory_later(repo_path, workspace_id);
    assert!(
      result.is_ok(),
      "archive_workspace should succeed: {:?}",
      result
    );

    assert!(
      local_db::get_workspaces(repo_path).unwrap().is_empty(),
      "Archived workspace should be hidden from get_workspaces"
    );
    let stored = local_db::get_workspace_by_id(repo_path, workspace_id)
      .unwrap()
      .expect("record should remain");
    assert!(stored.archived);
  }
}
