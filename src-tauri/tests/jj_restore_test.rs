mod common;

use common::{JjVerifier, TestRepo};

/// After `jj_restore_file`, the file on disk must match the parent commit's
/// content AND the working copy must be clean.
#[test]
fn test_jj_restore_file_resets_working_copy_on_disk() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/restore-file",
    Some("restore file".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  // README.md in the parent commit is "# Test Repository\n".
  // Modify it on disk so it appears as a working-copy change.
  let original = "# Test Repository\n";
  TestRepo::write_workspace_file(ws_dir_str, "README.md", "# CHANGED CONTENT\n")
    .expect("Failed to overwrite README.md");

  // Sanity: jj sees the change.
  assert!(
    JjVerifier::has_changes(ws_dir_str).expect("jj diff failed"),
    "expected working copy to have changes before restore"
  );

  // Discard the change.
  treq_lib::jj::jj_restore_file(ws_dir_str, "README.md")
    .expect("jj_restore_file should succeed without panicking");

  // The file on disk must be restored to the parent content.
  let readme_path = ws_dir.join("README.md");
  let on_disk = std::fs::read_to_string(&readme_path).expect("README.md should exist on disk");
  assert_eq!(
    on_disk, original,
    "README.md on disk should match parent content after jj_restore_file"
  );

  // The app's own changed-files probe must show a clean working copy —
  // no divergence between the rewritten wc commit and disk.
  let changed = treq_lib::jj::jj_get_changed_files(ws_dir_str)
    .expect("jj_get_changed_files should not panic after restore");
  assert!(
    changed.is_empty(),
    "working copy should be clean after jj_restore_file, got: {:?}",
    changed.iter().map(|c| &c.path).collect::<Vec<_>>()
  );
}

/// Same contract for `jj_restore_all`: every modified file on disk must be
/// reverted to its parent content and the working copy must be clean.
#[test]
fn test_jj_restore_all_resets_working_copy_on_disk() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/restore-all",
    Some("restore all".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  let original_readme = "# Test Repository\n";
  TestRepo::write_workspace_file(ws_dir_str, "README.md", "# CHANGED\n")
    .expect("Failed to overwrite README.md");
  TestRepo::write_workspace_file(ws_dir_str, "newfile.txt", "brand new\n")
    .expect("Failed to create newfile.txt");

  assert!(
    JjVerifier::has_changes(ws_dir_str).expect("jj diff failed"),
    "expected working copy to have changes before restore"
  );

  treq_lib::jj::jj_restore_all(ws_dir_str)
    .expect("jj_restore_all should succeed without panicking");

  // README.md must be back to parent content.
  let readme_on_disk =
    std::fs::read_to_string(ws_dir.join("README.md")).expect("README.md should exist on disk");
  assert_eq!(
    readme_on_disk, original_readme,
    "README.md on disk should match parent content after jj_restore_all"
  );

  // The newly-added file must be gone from disk (it wasn't in the parent).
  assert!(
    !ws_dir.join("newfile.txt").exists(),
    "newfile.txt should be removed from disk after jj_restore_all"
  );

  let changed = treq_lib::jj::jj_get_changed_files(ws_dir_str)
    .expect("jj_get_changed_files should not panic after restore");
  assert!(
    changed.is_empty(),
    "working copy should be clean after jj_restore_all, got: {:?}",
    changed.iter().map(|c| &c.path).collect::<Vec<_>>()
  );
}

/// Discard must snapshot dirty on-disk files before restoring. Without that,
/// a clean WC commit with dirty disk early-returns as a no-op and Review-tab
/// "Discard" appears to succeed while leaving changes in place.
#[test]
fn test_jj_restore_all_without_prior_snapshot_resets_disk() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/restore-nosnap",
    Some("restore nosnap".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  let original_readme = "# Test Repository\n";
  TestRepo::write_workspace_file(ws_dir_str, "README.md", "# CHANGED NOSNAP\n")
    .expect("Failed to overwrite README.md");
  TestRepo::write_workspace_file(ws_dir_str, "newfile.txt", "brand new\n")
    .expect("Failed to create newfile.txt");

  // Intentionally do NOT call jj/CLI snapshot (JjVerifier::has_changes).
  treq_lib::jj::jj_restore_all(ws_dir_str)
    .expect("jj_restore_all should succeed without panicking");

  let readme_on_disk =
    std::fs::read_to_string(ws_dir.join("README.md")).expect("README.md should exist on disk");
  assert_eq!(
    readme_on_disk, original_readme,
    "README.md on disk should match parent content after jj_restore_all without prior snapshot"
  );

  assert!(
    !ws_dir.join("newfile.txt").exists(),
    "newfile.txt should be removed from disk after jj_restore_all without prior snapshot"
  );

  let changed = treq_lib::jj::jj_get_changed_files(ws_dir_str)
    .expect("jj_get_changed_files should not panic after restore");
  assert!(
    changed.is_empty(),
    "working copy should be clean after jj_restore_all without prior snapshot, got: {:?}",
    changed.iter().map(|c| &c.path).collect::<Vec<_>>()
  );
}

/// Same no-prior-snapshot contract for single-file restore.
#[test]
fn test_jj_restore_file_without_prior_snapshot_resets_disk() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/restore-file-nosnap",
    Some("restore file nosnap".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  let original = "# Test Repository\n";
  TestRepo::write_workspace_file(ws_dir_str, "README.md", "# CHANGED FILE NOSNAP\n")
    .expect("Failed to overwrite README.md");

  treq_lib::jj::jj_restore_file(ws_dir_str, "README.md")
    .expect("jj_restore_file should succeed without prior snapshot");

  let on_disk =
    std::fs::read_to_string(ws_dir.join("README.md")).expect("README.md should exist on disk");
  assert_eq!(
    on_disk, original,
    "README.md on disk should match parent after jj_restore_file without prior snapshot"
  );

  let changed = treq_lib::jj::jj_get_changed_files(ws_dir_str)
    .expect("jj_get_changed_files should not panic after restore");
  assert!(
    changed.is_empty(),
    "working copy should be clean after jj_restore_file without prior snapshot, got: {:?}",
    changed.iter().map(|c| &c.path).collect::<Vec<_>>()
  );
}
