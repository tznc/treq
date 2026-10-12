mod common;

use common::TestRepo;

/// Forces jj to scan disk into the working-copy commit's tree, the same way
/// the app's file-list loader (`jj_get_changed_files`) does on every poll.
/// `snapshot_working_copy`/`jj_restore_file`/`jj_restore_all` all operate on
/// whatever tree jj-lib last recorded for the working copy -- they don't
/// rescan disk themselves -- so tests must trigger this explicitly after
/// writing files directly to disk.
fn snapshot_disk_into_wc(workspace_path: &str) -> Vec<treq_lib::jj::JjFileChange> {
  treq_lib::jj::jj_get_changed_files(workspace_path).expect("jj_get_changed_files should not panic")
}

/// `snapshot_working_copy` + `restore_working_copy_snapshot` are the undo
/// mechanism behind the "Undo" toast action on discard: capture a token for
/// the working copy before a discard, then restore it after.
#[test]
fn test_snapshot_restore_undoes_discard_all() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/snapshot-restore-all",
    Some("snapshot restore all".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  let changed_content = "# CHANGED CONTENT\n";
  TestRepo::write_workspace_file(ws_dir_str, "README.md", changed_content)
    .expect("Failed to overwrite README.md");
  assert!(
    !snapshot_disk_into_wc(ws_dir_str).is_empty(),
    "expected jj to see the change before snapshotting"
  );

  let snapshot_id = treq_lib::core::snapshot_working_copy(ws_dir_str)
    .expect("snapshot_working_copy should succeed");
  assert!(
    !snapshot_id.is_empty(),
    "expected a non-empty snapshot token"
  );

  // Discard the change (what the "Discard all changes" button does).
  treq_lib::jj::jj_restore_all(ws_dir_str).expect("jj_restore_all should succeed");
  assert!(
    snapshot_disk_into_wc(ws_dir_str).is_empty(),
    "expected clean working copy after discard"
  );

  // Undo the discard via the captured snapshot.
  treq_lib::core::restore_working_copy_snapshot(ws_dir_str, &snapshot_id)
    .expect("restore_working_copy_snapshot should succeed");

  let restored = std::fs::read_to_string(ws_dir.join("README.md"))
    .expect("README.md should exist on disk after restore");
  assert_eq!(
    restored, changed_content,
    "README.md should be back to its pre-discard content after undo"
  );
  assert!(
    !snapshot_disk_into_wc(ws_dir_str).is_empty(),
    "expected working copy to show the restored change again"
  );
}

/// The snapshot must capture every file that was modified, not just one --
/// undoing a "Discard all changes" restores the whole working copy at once.
#[test]
fn test_snapshot_restore_covers_multiple_files() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/snapshot-restore-multi",
    Some("snapshot restore multi".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  TestRepo::write_workspace_file(ws_dir_str, "README.md", "# CHANGED\n")
    .expect("Failed to overwrite README.md");
  TestRepo::write_workspace_file(ws_dir_str, "newfile.txt", "brand new\n")
    .expect("Failed to create newfile.txt");
  assert_eq!(
    snapshot_disk_into_wc(ws_dir_str).len(),
    2,
    "expected jj to see both file changes before snapshotting"
  );

  let snapshot_id = treq_lib::core::snapshot_working_copy(ws_dir_str)
    .expect("snapshot_working_copy should succeed");

  treq_lib::jj::jj_restore_all(ws_dir_str).expect("jj_restore_all should succeed");
  assert!(
    !ws_dir.join("newfile.txt").exists(),
    "newfile.txt should be removed from disk after discard"
  );

  treq_lib::core::restore_working_copy_snapshot(ws_dir_str, &snapshot_id)
    .expect("restore_working_copy_snapshot should succeed");

  let readme = std::fs::read_to_string(ws_dir.join("README.md")).expect("README.md missing");
  assert_eq!(readme, "# CHANGED\n");
  let newfile = std::fs::read_to_string(ws_dir.join("newfile.txt")).expect("newfile.txt missing");
  assert_eq!(newfile, "brand new\n");
}

/// A snapshot taken before discarding a *single* file (the per-file "Discard"
/// action) must restore that file without disturbing other pending changes.
#[test]
fn test_snapshot_restore_after_single_file_discard() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/snapshot-restore-single",
    Some("snapshot restore single".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  TestRepo::write_workspace_file(ws_dir_str, "README.md", "# CHANGED\n")
    .expect("Failed to overwrite README.md");
  TestRepo::write_workspace_file(ws_dir_str, "other.txt", "other content\n")
    .expect("Failed to create other.txt");
  assert_eq!(snapshot_disk_into_wc(ws_dir_str).len(), 2);

  let snapshot_id = treq_lib::core::snapshot_working_copy(ws_dir_str)
    .expect("snapshot_working_copy should succeed");

  // Discard only README.md (the per-file "Discard" action).
  treq_lib::jj::jj_restore_file(ws_dir_str, "README.md").expect("jj_restore_file should succeed");

  let discarded = std::fs::read_to_string(ws_dir.join("README.md")).expect("README.md missing");
  assert_eq!(
    discarded, "# Test Repository\n",
    "README.md should be back to parent content after the single-file discard"
  );
  let untouched = std::fs::read_to_string(ws_dir.join("other.txt")).expect("other.txt missing");
  assert_eq!(
    untouched, "other content\n",
    "other.txt should be unaffected by discarding only README.md"
  );

  treq_lib::core::restore_working_copy_snapshot(ws_dir_str, &snapshot_id)
    .expect("restore_working_copy_snapshot should succeed");

  let restored = std::fs::read_to_string(ws_dir.join("README.md")).expect("README.md missing");
  assert_eq!(
    restored, "# CHANGED\n",
    "README.md should be back to its pre-discard content after undo"
  );
  let still_there = std::fs::read_to_string(ws_dir.join("other.txt")).expect("other.txt missing");
  assert_eq!(still_there, "other content\n");
}

/// Restoring with a syntactically invalid token must return a clear error,
/// not panic.
#[test]
fn test_restore_snapshot_rejects_invalid_id() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/snapshot-restore-invalid",
    Some("snapshot restore invalid".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  let err = treq_lib::core::restore_working_copy_snapshot(ws_dir_str, "not-a-valid-hex-id")
    .expect_err("restore_working_copy_snapshot should reject a malformed token");
  assert!(
    err.contains("Invalid snapshot id"),
    "expected 'Invalid snapshot id' in error, got: {err}"
  );
}

/// A well-formed but unknown commit id (never captured by
/// `snapshot_working_copy`) must fail gracefully rather than panic.
#[test]
fn test_restore_snapshot_rejects_unknown_commit() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/snapshot-restore-unknown",
    Some("snapshot restore unknown".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  // Well-formed 40-hex-char (git SHA-1 length) id that doesn't correspond
  // to any commit ever written to this repo.
  let bogus_commit_id = "f".repeat(40);
  let err = treq_lib::core::restore_working_copy_snapshot(ws_dir_str, &bogus_commit_id)
    .expect_err("restore_working_copy_snapshot should reject an unknown commit id");
  assert!(
    err.contains("Snapshot is no longer available"),
    "expected 'Snapshot is no longer available' in error, got: {err}"
  );
}

/// Restoring a snapshot that already matches the current working copy
/// (nothing changed since the snapshot was taken) must be a harmless no-op.
#[test]
fn test_restore_snapshot_noop_when_nothing_changed() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/snapshot-restore-noop",
    Some("snapshot restore noop".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  let snapshot_id = treq_lib::core::snapshot_working_copy(ws_dir_str)
    .expect("snapshot_working_copy should succeed");

  treq_lib::core::restore_working_copy_snapshot(ws_dir_str, &snapshot_id)
    .expect("restoring an unchanged snapshot should succeed as a no-op");

  assert!(
    snapshot_disk_into_wc(ws_dir_str).is_empty(),
    "expected working copy to remain clean after a no-op restore"
  );
}
